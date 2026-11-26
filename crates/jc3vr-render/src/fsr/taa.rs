//! The mod's own temporal anti-aliasing resolve, an alternative to FSR at the same hook.
//!
//! One compute pass per chain texture (`taa_resolve.hlsl`): the current colour's 3x3 neighbourhood
//! reconstructed at the pixel's unjittered centre, the history fetched at the pixel's previous
//! position (the closest-depth neighbour's motion, decoded from the engine's velocity buffer as the
//! FSR path decodes it) through a Catmull-Rom filter, rejected on leaving the eye or failing a depth
//! test, clipped to the neighbourhood's variance box in YCoCg, and blended exponentially. It reads
//! the same inputs FSR does (the jittered post-tonemap colour, the depth, the velocity, and the
//! per-eye reprojection matrices of [`EyeMotion`]) and writes the result back into the chain's
//! working slot. See `docs/mod/rendering/taa.md`.
//!
//! Under the single-pass collapse it resolves the double-wide texture in place, both eyes in one
//! dispatch, each pixel's neighbourhood and history taps clamped to its own eye's half, so no copy
//! into eye-sized textures is needed. Each layout lane (an eye rendered on its own, or the
//! side-by-side pair) keeps its own pair of history textures, ping-ponged each frame.

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::{
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
};
use jc3vr_view::{
    fsr::{FSR_CONFIG, TaaConfig, jitter},
    stereo::config::STEREO_CONFIG,
};
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
        Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
            D3D11_BOX, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_WRITE,
            D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
            D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC_0,
            D3D11_TEX2D_SRV, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
            D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, ID3D11Buffer, ID3D11ComputeShader,
            ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11SamplerState,
            ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC},
    },
    core::Interface,
};

use crate::fsr::{
    Build, ChainTextures, EyeLayout, EyeMotion, EyeViewProjections, camera_cut,
    velocity_decode::depth_srv,
};

/// The live resolve state (render thread only).
pub struct TaaState {
    /// The shader, its constants, and its sampler, built on first use.
    pipeline: Option<Pipeline>,
    /// One lane per eye rendered on its own; lane 0 also serves the side-by-side pair.
    lanes: [Option<Lane>; 2],
    /// Each eye's view-projections from the last side-by-side resolve.
    side_by_side_history: [Option<EyeViewProjections>; 2],
}

impl TaaState {
    const fn new() -> Self {
        Self {
            pipeline: None,
            lanes: [None, None],
            side_by_side_history: [None, None],
        }
    }
}

/// Global resolve state. Locked briefly on the render thread at the AA hook.
pub static TAA_STATE: Mutex<TaaState> = Mutex::new(TaaState::new());

/// Release every texture and the pipeline, for eject; see [`crate::fsr::release_on_eject`].
pub(super) fn release_on_eject() {
    *TAA_STATE.lock() = TaaState::new();
}

/// Discard every eye's history at the next resolve. Called with FSR's own reset
/// ([`crate::fsr::request_history_reset`]), so the harness restarts either resolve alike.
pub(super) fn request_history_reset() {
    RESET_REQUESTED.store(true, Ordering::Relaxed);
}

/// Resolve the chain's working LDR texture in place. Returns `true` if the resolve ran; `false`
/// means it fell through and the engine AA should proceed as normal.
///
/// Runs on the render thread; holds the engine context mutex across the dispatch and the
/// copy-back.
pub fn dispatch(
    state: &mut TaaState,
    device: &Device,
    textures: &ChainTextures<'_>,
    layout: EyeLayout,
    tuning: TaaConfig,
) -> bool {
    if jc3vr_core::is_shutting_down() {
        return false;
    }
    let width = u32::from(textures.slot_color.m_Width);
    let height = u32::from(textures.slot_color.m_Height);
    let build = Build::for_layout(layout, width, height);
    if build.eye_width == 0 || height == 0 {
        return false;
    }
    let lane_build = LaneBuild {
        width,
        height,
        format: textures.slot_color.m_Format as i32,
    };
    let (lane_index, eyes): (usize, &[usize]) = match layout {
        EyeLayout::Single(eye) => {
            let eye = eye.min(1);
            (eye, &EYES[eye..=eye])
        }
        EyeLayout::SideBySide => (0, &EYES),
    };
    if build.side_by_side {
        // The other lane serves only an eye rendered on its own; free it while the pair runs.
        state.lanes[1] = None;
    } else {
        state.side_by_side_history = [None, None];
    }
    if state
        .lanes
        .get(lane_index)
        .and_then(Option::as_ref)
        .is_none_or(|lane| lane.build != lane_build)
    {
        state.lanes[lane_index] = Lane::new(&device.m_Device, lane_build);
    }
    if state.pipeline.is_none() {
        state.pipeline = Pipeline::new(&device.m_Device);
    }
    if RESET_REQUESTED.swap(false, Ordering::Relaxed) {
        for lane in state.lanes.iter_mut().flatten() {
            lane.fresh = [true, true];
        }
    }
    let (Some(pipeline), Some(lane)) = (state.pipeline.as_ref(), state.lanes[lane_index].as_mut())
    else {
        return false;
    };

    jitter::publish_eye_size(build.eye_width, build.height);
    let stereo = jc3vr_view::stereo::active();
    let cameras = STEREO_CONFIG.lock().cameras;
    let (correction, hybrid) = FSR_CONFIG.query(|fsr| {
        (
            cameras && fsr.stereo_mv_correction && stereo,
            fsr.hybrid_velocity && stereo,
        )
    });
    let mut params = Params {
        texture_size: [width, height],
        eye_width: build.eye_width,
        eye_count: eyes.len() as u32,
        history_weight: tuning.history_weight.clamp(0.0, 0.99),
        motion_history_weight: tuning.motion_history_weight.clamp(0.0, 0.99),
        clip_gamma: tuning.clip_gamma.max(0.0),
        clip_gamma_rest: tuning.clip_gamma_rest.max(0.0),
        current_sharpness: tuning.current_sharpness.max(0.0),
        depth_tolerance: tuning.depth_tolerance.max(0.0),
        padding: [0.0; 2],
        eyes: [EyeParams::ZERO; 2],
    };
    for (slot, &eye) in eyes.iter().enumerate() {
        let motion = EyeMotion::this_frame(
            eye,
            build.side_by_side,
            &mut state.side_by_side_history[eye],
        );
        let reprojection = motion.reprojection.filter(|_| correction || hybrid);
        // A camera cut leaves no history worth reprojecting, as for FSR.
        let cut = reprojection.is_some_and(|(_, eye_reprojection)| camera_cut(&eye_reprojection));
        let fresh = std::mem::take(&mut lane.fresh[slot]) || cut;
        let reprojection = reprojection.filter(|_| !cut);
        let (center, eye_matrix) = reprojection.unwrap_or((Matrix4::IDENTITY, Matrix4::IDENTITY));
        let mut flags = 0;
        if !fresh {
            flags |= FLAG_HISTORY;
        }
        if reprojection.is_some() {
            flags |= FLAG_REPROJECTION;
            if correction {
                flags |= FLAG_CORRECTION;
            }
            if hybrid {
                flags |= FLAG_HYBRID;
            }
        }
        params.eyes[slot] = EyeParams {
            reproj_center: center.data,
            reproj_eye: eye_matrix.data,
            jitter: jitter::pixels_from_ndc(motion.jitter_ndc, build.eye_width, build.height)
                .to_array(),
            flags,
            padding: 0,
        };
    }

    // SAFETY: the engine's context wrapper is live for the duration of this render-thread call.
    let Some(context) = (unsafe { device.m_Context.as_ref() }) else {
        return false;
    };
    // SAFETY: `context.m_Mutex` guards the immediate context; every resource is valid for the
    // calls, all of which `resolve` records under the mutex.
    unsafe {
        windows::Win32::System::Threading::EnterCriticalSection(context.m_Mutex);
        let ran = resolve(
            pipeline,
            lane,
            &context.m_Context,
            device,
            textures,
            &params,
        );
        windows::Win32::System::Threading::LeaveCriticalSection(context.m_Mutex);
        ran
    }
}

/// The committed, precompiled resolve compute shader (entry point `main`, `cs_5_0`).
const RESOLVE_DXBC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/shaders/taa_resolve.dxbc"
));

/// Compute-shader thread-group size (must match `[numthreads(8, 8, 1)]` in the HLSL).
const GROUP: u32 = 8;

/// The shader's `FLAG_*` bits.
const FLAG_HISTORY: u32 = 1;
const FLAG_REPROJECTION: u32 = 2;
const FLAG_CORRECTION: u32 = 4;
const FLAG_HYBRID: u32 = 8;

static RESET_REQUESTED: AtomicBool = AtomicBool::new(false);
static EYES: [usize; 2] = [0, 1];

/// Matches the HLSL `Params` cbuffer: three registers of sizes and tuning, then one
/// [`EyeParams`] per eye.
#[repr(C)]
struct Params {
    texture_size: [u32; 2],
    eye_width: u32,
    eye_count: u32,
    history_weight: f32,
    motion_history_weight: f32,
    clip_gamma: f32,
    clip_gamma_rest: f32,
    current_sharpness: f32,
    depth_tolerance: f32,
    padding: [f32; 2],
    eyes: [EyeParams; 2],
}

/// Matches the HLSL `EyeParams`: two column-major matrices, then one register of the jitter and
/// the flags.
#[repr(C)]
#[derive(Clone, Copy)]
struct EyeParams {
    reproj_center: [f32; 16],
    reproj_eye: [f32; 16],
    jitter: [f32; 2],
    flags: u32,
    padding: u32,
}

impl EyeParams {
    const ZERO: Self = Self {
        reproj_center: [0.0; 16],
        reproj_eye: [0.0; 16],
        jitter: [0.0; 2],
        flags: 0,
        padding: 0,
    };
}

const _: () = assert!(size_of::<EyeParams>() == 9 * 16);
const _: () = assert!(size_of::<Params>() == 3 * 16 + 2 * 9 * 16);

/// The shader, its constant buffer, and the history's bilinear sampler.
struct Pipeline {
    shader: ID3D11ComputeShader,
    constants: ID3D11Buffer,
    sampler: ID3D11SamplerState,
}

impl Pipeline {
    fn new(d3d: &ID3D11Device) -> Option<Self> {
        // SAFETY: `d3d` is the live engine device; the descriptors are valid for these calls.
        unsafe {
            let mut shader = None;
            if let Err(error) = d3d.CreateComputeShader(RESOLVE_DXBC, None, Some(&mut shader)) {
                tracing::error!("taa: could not create the resolve shader: {error}");
                return None;
            }
            let mut constants = None;
            d3d.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: size_of::<Params>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                },
                None,
                Some(&mut constants),
            )
            .inspect_err(|error| {
                tracing::error!("taa: could not create the constant buffer: {error}")
            })
            .ok()?;
            let mut sampler = None;
            d3d.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MipLODBias: 0.0,
                    MaxAnisotropy: 1,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    BorderColor: [0.0; 4],
                    MinLOD: 0.0,
                    MaxLOD: 0.0,
                },
                Some(&mut sampler),
            )
            .inspect_err(|error| tracing::error!("taa: could not create the sampler: {error}"))
            .ok()?;
            Some(Self {
                shader: shader?,
                constants: constants?,
                sampler: sampler?,
            })
        }
    }
}

/// What a lane's textures were built for.
#[derive(Clone, Copy, PartialEq, Eq)]
struct LaneBuild {
    /// The whole chain texture's size, both eyes under the collapse.
    width: u32,
    height: u32,
    /// The chain texture's format, which the output copy needs.
    format: i32,
}

/// One layout lane's textures: two histories, ping-ponged, and the output in the chain's format.
struct Lane {
    build: LaneBuild,
    histories: [Target; 2],
    /// The history the last resolve wrote, which the next one reads.
    latest: usize,
    output: Target,
    /// Per eye of the lane (by its slot in the dispatch): discard the history at the next resolve.
    fresh: [bool; 2],
}

/// A texture with the views the resolve reads and writes it through.
struct Target {
    texture: ID3D11Texture2D,
    srv: Option<ID3D11ShaderResourceView>,
    uav: ID3D11UnorderedAccessView,
}

impl Lane {
    fn new(d3d: &ID3D11Device, build: LaneBuild) -> Option<Self> {
        let history = || Target::new(d3d, build, DXGI_FORMAT_R16G16B16A16_FLOAT, true);
        let lane = Self {
            build,
            histories: [history()?, history()?],
            latest: 0,
            output: Target::new(d3d, build, DXGI_FORMAT(build.format), false)?,
            fresh: [true, true],
        };
        tracing::info!(
            width = build.width,
            height = build.height,
            format = build.format,
            "taa: built a lane"
        );
        Some(lane)
    }
}

impl Target {
    fn new(
        d3d: &ID3D11Device,
        build: LaneBuild,
        format: DXGI_FORMAT,
        sampled: bool,
    ) -> Option<Self> {
        let mut bind = D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        if sampled {
            bind |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
        }
        // SAFETY: `d3d` is the live engine device; the descriptor is valid for these calls.
        unsafe {
            let mut texture: Option<ID3D11Texture2D> = None;
            d3d.CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: build.width,
                    Height: build.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: format,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                },
                None,
                Some(&mut texture),
            )
            .inspect_err(|error| {
                tracing::error!(
                    format = format.0,
                    "taa: could not create a texture: {error}"
                )
            })
            .ok()?;
            let texture = texture?;
            let mut uav = None;
            d3d.CreateUnorderedAccessView(&texture, None, Some(&mut uav))
                .inspect_err(|error| {
                    tracing::error!(format = format.0, "taa: could not create a UAV: {error}")
                })
                .ok()?;
            let srv = if sampled {
                let mut srv = None;
                d3d.CreateShaderResourceView(&texture, None, Some(&mut srv))
                    .ok()?;
                srv
            } else {
                None
            };
            Some(Self {
                texture,
                srv,
                uav: uav?,
            })
        }
    }
}

/// Record the resolve and the copy-back onto the immediate context.
///
/// # Safety
/// `context` must be the engine's immediate context, held under its mutex.
unsafe fn resolve(
    pipeline: &Pipeline,
    lane: &mut Lane,
    context: &ID3D11DeviceContext,
    device: &Device,
    textures: &ChainTextures<'_>,
    params: &Params,
) -> bool {
    let d3d = &device.m_Device;
    let Some(color_srv) = texture_srv(d3d, textures.slot_color) else {
        return false;
    };
    let Some(velocity_srv) = texture_srv(d3d, textures.velocity) else {
        return false;
    };
    let Some(depth_srv) = depth_srv(device, textures.depth) else {
        return false;
    };
    let read = lane.latest;
    let write = 1 - read;
    // SAFETY: the caller holds the context mutex; every resource is live for these calls.
    unsafe {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        if context
            .Map(
                &pipeline.constants,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped),
            )
            .is_err()
        {
            return false;
        }
        std::ptr::copy_nonoverlapping(params, mapped.pData as *mut Params, 1);
        context.Unmap(&pipeline.constants, 0);

        context.CSSetShader(&pipeline.shader, None);
        context.CSSetShaderResources(
            0,
            Some(&[
                Some(color_srv),
                Some(velocity_srv),
                Some(depth_srv),
                lane.histories[read].srv.clone(),
            ]),
        );
        context.CSSetSamplers(0, Some(&[Some(pipeline.sampler.clone())]));
        let uavs = [
            Some(lane.histories[write].uav.clone()),
            Some(lane.output.uav.clone()),
        ];
        context.CSSetUnorderedAccessViews(0, 2, Some(uavs.as_ptr()), None);
        context.CSSetConstantBuffers(0, Some(&[Some(pipeline.constants.clone())]));
        context.Dispatch(
            lane.build.width.div_ceil(GROUP),
            lane.build.height.div_ceil(GROUP),
            1,
        );
        let unbound: [Option<ID3D11UnorderedAccessView>; 2] = [None, None];
        context.CSSetUnorderedAccessViews(0, 2, Some(unbound.as_ptr()), None);
        context.CSSetShaderResources(0, Some(&[None, None, None, None]));

        let output: ID3D11Resource = lane.output.texture.cast().expect("texture is a resource");
        context.CopySubresourceRegion(
            &textures.slot_color.m_Texture,
            0,
            0,
            0,
            0,
            &output,
            0,
            Some(&D3D11_BOX {
                left: 0,
                top: 0,
                front: 0,
                right: lane.build.width,
                bottom: lane.build.height,
                back: 1,
            }),
        );
    }
    lane.latest = write;
    true
}

/// A shader-resource view over an engine texture in the engine's own format for it (the resource
/// itself may be typeless, viewed both plain and sRGB). `None` on failure.
fn texture_srv(d3d: &ID3D11Device, texture: &Texture) -> Option<ID3D11ShaderResourceView> {
    let mut srv = None;
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT(texture.m_Format as i32),
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    // SAFETY: `texture.m_Texture` is a live engine texture; the view holds its own reference.
    unsafe { d3d.CreateShaderResourceView(&texture.m_Texture, Some(&desc), Some(&mut srv)) }
        .inspect_err(|error| {
            tracing::warn!(
                format = texture.m_Format as i32,
                "taa: could not view an engine texture: {error}"
            )
        })
        .ok()?;
    srv
}
