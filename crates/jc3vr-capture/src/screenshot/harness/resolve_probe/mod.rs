mod cascade;
mod samples;
use std::{cell::Cell, ffi::c_void};

use anyhow::Context as _;
use cascade::{CascadeGuard, CascadeTrial};
use jc3vr_render::stereo::single_pass::{shader_identity, shader_identity::IdentityResult};
use jc3vr_view::{
    stereo::{dispatch, draw_index},
    vr,
};
use parking_lot::Mutex;
use serde::Serialize;
use windows::{
    Win32::Graphics::Direct3D11::{
        D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_TEXTURE2D_DESC, D3D11_TEXTURE3D_DESC,
        D3D11_USAGE_STAGING, ID3D11Buffer, ID3D11DeviceContext, ID3D11DeviceContext1,
        ID3D11PixelShader, ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D,
        ID3D11Texture3D,
    },
    core::Interface,
};

use crate::{
    screenshot::harness::{diagnostic_capture_id, resolve_inputs_requested},
    testing::visual,
};

const PS_SRV_SLOTS: [u32; 28] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28,
];
const PS_EXTRA_SRV_SLOTS: [u32; 5] = [29, 30, 31, 44, 45];
const MAX_CB_BYTES: u32 = 65536;
const PS_CB_SLOTS: u32 = 4;

#[derive(Clone, Serialize)]
pub(crate) struct Capture {
    frame: i64,
    generation: u64,
    eyes: [EyeResult; 2],
}

impl Capture {
    pub(crate) fn write_artifacts(&mut self, folder: &std::path::Path, id: &str) {
        for (eye, result) in self.eyes.iter_mut().enumerate() {
            let EyeResult::Observed { observation } = result else {
                continue;
            };
            for buffer in &mut observation.constant_buffers {
                let Some(BufferContent::Captured { bytes, raw }) = &mut buffer.content else {
                    continue;
                };
                let path = folder.join(format!("{id}.resolve_cb{}.eye{eye}.raw", buffer.slot));
                match std::fs::write(&path, bytes) {
                    Ok(()) => *raw = Some(path.to_string_lossy().into_owned()),
                    Err(error) => {
                        buffer.content = Some(BufferContent::ReadError {
                            error: format!(
                                "resolve constants: writing {}: {error}",
                                path.display()
                            ),
                        })
                    }
                }
            }
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum EyeResult {
    NotRequired,
    Missing,
    Ambiguous { candidates: u32 },
    Observed { observation: Box<Observation> },
}

#[derive(Clone, Serialize)]
struct Observation {
    #[serde(skip_serializing_if = "Option::is_none")]
    cascade: Option<CascadeTrial>,
    #[serde(skip_serializing_if = "Option::is_none")]
    camera: Option<CascadeTrial>,
    frame_counters: FrameCounters,
    pixel_shader: ShaderIdentity,
    constant_buffers: Vec<ConstantBuffer>,
    resources: Vec<Resource>,
}

#[derive(Clone, Serialize)]
struct FrameCounters {
    counter: u32,
    frame_index: u32,
    ring_index: u32,
}

#[derive(Clone, Serialize)]
struct ShaderIdentity {
    pointer: usize,
    identity: Option<IdentityResult>,
}

#[derive(Clone, Serialize)]
struct ConstantBuffer {
    slot: u32,
    pointer: usize,
    byte_width: u32,
    first_constant: Option<u32>,
    num_constants: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<BufferContent>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum BufferContent {
    Captured {
        #[serde(skip)]
        bytes: Vec<u8>,
        raw: Option<String>,
    },
    Unbound,
    UnknownRange,
    EmptyRange,
    Oversize,
    InvalidRange,
    ReadError {
        error: String,
    },
}

#[derive(Clone, Serialize)]
struct Resource {
    slot: u32,
    view: usize,
    resource: usize,
    format: i32,
    dimension: i32,
    texture: Option<TextureDesc>,
}

#[derive(Clone, Serialize)]
struct TextureDesc {
    width: u32,
    height: u32,
    depth: u32,
    mip_levels: u32,
    array_size: u32,
    format: i32,
}

struct State {
    frame: Option<i64>,
    generation: u64,
    candidates: [u32; 2],
    observations: [Option<Observation>; 2],
}

impl State {
    const fn new() -> Self {
        Self {
            frame: None,
            generation: 0,
            candidates: [0; 2],
            observations: [None, None],
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
thread_local! { static DEPTH: Cell<u32> = const { Cell::new(0) }; }

pub(crate) struct Scope(bool);

pub(crate) fn begin(active: bool) -> Scope {
    if active {
        DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
    }
    Scope(active)
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.0 {
            DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        }
    }
}

pub(crate) fn reset() {
    *STATE.lock() = State::new();
}

fn eye_result(
    required: bool,
    same: bool,
    candidates: u32,
    observation: Option<Observation>,
) -> EyeResult {
    if !required {
        return EyeResult::NotRequired;
    }
    if !same {
        return EyeResult::Missing;
    }
    match candidates {
        0 => EyeResult::Missing,
        1 => observation.map_or(EyeResult::Missing, |observation| EyeResult::Observed {
            observation: Box::new(observation),
        }),
        candidates => EyeResult::Ambiguous { candidates },
    }
}

fn result_for_eye(
    state: &mut State,
    frame: i64,
    generation: u64,
    eye: usize,
    required: bool,
) -> EyeResult {
    let same = state.frame == Some(frame) && state.generation == generation;
    eye_result(
        required,
        same,
        state.candidates[eye],
        state.observations[eye].take(),
    )
}

pub(crate) fn take(frame: i64, generation: u64, required: [bool; 2]) -> Capture {
    let mut state = STATE.lock();
    let eyes = std::array::from_fn(|eye| {
        result_for_eye(&mut state, frame, generation, eye, required[eye])
    });
    *state = State::new();
    Capture {
        frame,
        generation,
        eyes,
    }
}

pub(crate) unsafe fn on_draw(
    context: *mut c_void,
    vertex_count: u32,
    start_vertex: u32,
) -> Option<(Option<CascadeGuard>, Option<samples::Guard>)> {
    if vertex_count != 3
        || start_vertex != 0
        || !DEPTH.with(|depth| depth.get() == 1)
        || !resolve_inputs_requested()
        || diagnostic_capture_id().is_none()
    {
        return None;
    }
    let metadata = visual::metadata();
    let frame = vr::capture_views()?.predicted_display_time_ns;
    let eye = if dispatch::mode() == dispatch::RenderMode::SinglePass {
        jc3vr_render::probes::state::masked_eye()?
    } else {
        draw_index()
    };
    if !metadata.ready || eye >= 2 || dispatch::mode() == dispatch::RenderMode::Native {
        return None;
    }
    let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }?;
    let camera = crate::screenshot::harness::resolve_eye_camera(frame);
    let selection = if let Some(treated) = camera {
        let id = diagnostic_capture_id()?;
        let source =
            crate::screenshot::harness::patch_draw::resolve_camera_source(frame, eye, &id)?;
        let (w, h) = jc3vr_view::stereo::render_size()?;
        let mut rects = [windows::Win32::Foundation::RECT::default(); 2];
        let mut n = 2;
        let mut desc = windows::Win32::Graphics::Direct3D11::D3D11_RASTERIZER_DESC::default();
        unsafe {
            ctx.RSGetScissorRects(&mut n, Some(rects.as_mut_ptr()));
            ctx.RSGetState().ok()?.GetDesc(&mut desc);
        }
        if n != 2
            || !desc.ScissorEnable.as_bool()
            || !rects.iter().all(|r| {
                (r.left, r.top, r.right, r.bottom)
                    == (
                        (w as i32 / 2) * (eye as i32),
                        0,
                        (w as i32 / 2) * (eye as i32 + 1),
                        h as i32,
                    )
            })
        {
            return None;
        }
        Some(cascade::Change::Camera(source, treated))
    } else {
        crate::screenshot::harness::cascade_scale(frame).map(cascade::Change::Cascade)
    };
    let mut state = STATE.lock();
    if state.frame != Some(frame) || state.generation != metadata.generation {
        *state = State::new();
        state.frame = Some(frame);
        state.generation = metadata.generation;
    }
    state.candidates[eye] = state.candidates[eye].saturating_add(1);
    if state.candidates[eye] == 1 {
        let mut obs = observe(ctx);
        let guard =
            selection.and_then(
                |selection| match cascade::bind(ctx, &obs, selection, eye, frame) {
                    Ok((g, trial)) => {
                        if camera.is_some() {
                            obs.camera = Some(trial);
                        } else {
                            obs.cascade = Some(trial);
                        }
                        Some(g)
                    }
                    Err(error) => {
                        tracing::warn!(%error,"cascade treatment declined");
                        None
                    }
                },
            );
        let samples = crate::screenshot::harness::resolve_samples_requested(frame)
            .then(|| samples::Guard::bind(ctx, &obs, frame, eye))
            .and_then(|r| match r {
                Ok(guard) => Some(guard),
                Err(e) => {
                    tracing::warn!("resolve samples declined: {e:#}");
                    None
                }
            });
        state.observations[eye] = Some(obs);
        return Some((guard, samples));
    }
    None
}

fn checked_range(
    byte_width: u32,
    range: Option<(u32, u32)>,
) -> Result<std::ops::Range<usize>, BufferContent> {
    if byte_width == 0 {
        return Err(BufferContent::EmptyRange);
    }
    let Some((first, count)) = range else {
        return Err(BufferContent::UnknownRange);
    };
    if count == 0 {
        return Err(BufferContent::EmptyRange);
    }
    let Some(start) = first.checked_mul(16) else {
        return Err(BufferContent::InvalidRange);
    };
    let Some(size) = count.checked_mul(16) else {
        return Err(BufferContent::InvalidRange);
    };
    let Some(end) = start.checked_add(size) else {
        return Err(BufferContent::InvalidRange);
    };
    if byte_width > MAX_CB_BYTES || size > MAX_CB_BYTES {
        return Err(BufferContent::Oversize);
    }
    if end > byte_width {
        return Err(BufferContent::InvalidRange);
    }
    Ok(start as usize..end as usize)
}

fn unbound_or_range(
    bound: bool,
    byte_width: u32,
    range: Option<(u32, u32)>,
) -> Result<std::ops::Range<usize>, BufferContent> {
    if !bound {
        return Err(BufferContent::Unbound);
    }
    checked_range(byte_width, range)
}

fn read_content(
    ctx: &ID3D11DeviceContext,
    buffer: Option<&ID3D11Buffer>,
    byte_width: u32,
    range: Option<(u32, u32)>,
) -> BufferContent {
    let range = match unbound_or_range(buffer.is_some(), byte_width, range) {
        Ok(range) => range,
        Err(error) => return error,
    };
    let Some(buffer) = buffer else {
        return BufferContent::Unbound;
    };
    match copy_content(ctx, buffer, byte_width, range) {
        Ok(bytes) => BufferContent::Captured { bytes, raw: None },
        Err(error) => BufferContent::ReadError {
            error: format!("{error:#}"),
        },
    }
}

pub(super) fn copy_content(
    ctx: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    byte_width: u32,
    range: std::ops::Range<usize>,
) -> anyhow::Result<Vec<u8>> {
    let device = unsafe { ctx.GetDevice() }.context("resolve constants: device")?;
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: byte_width,
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut staging = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut staging)) }
        .context("resolve constants: staging buffer")?;
    let staging =
        staging.ok_or_else(|| anyhow::anyhow!("resolve constants: staging buffer missing"))?;
    let source: ID3D11Resource = buffer
        .cast()
        .context("resolve constants: source resource")?;
    let destination: ID3D11Resource = staging
        .cast()
        .context("resolve constants: staging resource")?;
    unsafe { ctx.CopyResource(&destination, &source) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .context("resolve constants: map")?;
    if mapped.pData.is_null() {
        unsafe { ctx.Unmap(&staging, 0) };
        anyhow::bail!("resolve constants: null mapped data");
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), byte_width as usize) }
            [range]
            .to_vec();
    unsafe { ctx.Unmap(&staging, 0) };
    Ok(bytes)
}

fn observe(ctx: &ID3D11DeviceContext) -> Observation {
    let counters = unsafe { jc3gi::graphics_engine::graphics_engine::get_render_frame_counters() };
    let frame_counters = FrameCounters {
        counter: counters.m_Counter,
        frame_index: counters.m_FrameIndex,
        ring_index: counters.m_RingIndex,
    };
    let mut shader: Option<ID3D11PixelShader> = None;
    unsafe { ctx.PSGetShader(&mut shader, None, None) };
    let pixel_shader = ShaderIdentity {
        pointer: shader.as_ref().map_or(0, |shader| shader.as_raw() as usize),
        identity: shader.as_ref().map(shader_identity::lookup),
    };
    let constant_buffers = (0..PS_CB_SLOTS)
        .map(|slot| {
            let mut buffers: [Option<ID3D11Buffer>; 1] = [None];
            let mut first = 0;
            let mut count = 0;
            let ranged = if let Ok(ctx1) = ctx.cast::<ID3D11DeviceContext1>() {
                unsafe {
                    ctx1.PSGetConstantBuffers1(
                        slot,
                        1,
                        Some(buffers.as_mut_ptr()),
                        Some(&mut first),
                        Some(&mut count),
                    )
                };
                true
            } else {
                unsafe { ctx.PSGetConstantBuffers(slot, Some(&mut buffers)) };
                false
            };
            let mut desc = D3D11_BUFFER_DESC::default();
            if let Some(buffer) = buffers[0].as_ref() {
                unsafe { buffer.GetDesc(&mut desc) };
            }
            ConstantBuffer {
                slot,
                pointer: buffers[0]
                    .as_ref()
                    .map_or(0, |buffer| buffer.as_raw() as usize),
                byte_width: desc.ByteWidth,
                first_constant: ranged.then_some(first),
                num_constants: ranged.then_some(count),
                content: matches!(slot, 0 | 3).then(|| {
                    read_content(
                        ctx,
                        buffers[0].as_ref(),
                        desc.ByteWidth,
                        ranged.then_some((first, count)),
                    )
                }),
            }
        })
        .collect();
    let resources = PS_SRV_SLOTS
        .into_iter()
        .chain(PS_EXTRA_SRV_SLOTS)
        .map(|slot| {
            let mut view: Option<ID3D11ShaderResourceView> = None;
            unsafe { ctx.PSGetShaderResources(slot, Some(std::slice::from_mut(&mut view))) };
            let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
            if let Some(view) = view.as_ref() {
                unsafe { view.GetDesc(&mut desc) };
            }
            let resource = view
                .as_ref()
                .and_then(|view| unsafe { view.GetResource().ok() });
            let texture = resource.as_ref().and_then(|resource| {
                if let Ok(texture) = resource.cast::<ID3D11Texture2D>() {
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    unsafe { texture.GetDesc(&mut desc) };
                    Some(TextureDesc {
                        width: desc.Width,
                        height: desc.Height,
                        depth: 1,
                        mip_levels: desc.MipLevels,
                        array_size: desc.ArraySize,
                        format: desc.Format.0,
                    })
                } else if let Ok(texture) = resource.cast::<ID3D11Texture3D>() {
                    let mut desc = D3D11_TEXTURE3D_DESC::default();
                    unsafe { texture.GetDesc(&mut desc) };
                    Some(TextureDesc {
                        width: desc.Width,
                        height: desc.Height,
                        depth: desc.Depth,
                        mip_levels: desc.MipLevels,
                        array_size: 1,
                        format: desc.Format.0,
                    })
                } else {
                    None
                }
            });
            Resource {
                slot,
                view: view.as_ref().map_or(0, |view| view.as_raw() as usize),
                resource: resource
                    .as_ref()
                    .map_or(0, |resource| resource.as_raw() as usize),
                format: desc.Format.0,
                dimension: desc.ViewDimension.0,
                texture,
            }
        })
        .collect();
    Observation {
        cascade: None,
        camera: None,
        frame_counters,
        pixel_shader,
        constant_buffers,
        resources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_results_fail_closed_on_missing_or_multiple_draws() {
        assert!(matches!(
            eye_result(false, false, 5, None),
            EyeResult::NotRequired
        ));
        assert!(matches!(
            eye_result(true, false, 1, None),
            EyeResult::Missing
        ));
        assert!(matches!(
            eye_result(true, true, 0, None),
            EyeResult::Missing
        ));
        assert!(matches!(
            eye_result(true, true, 1, None),
            EyeResult::Missing
        ));
        assert!(matches!(
            eye_result(true, true, 2, None),
            EyeResult::Ambiguous { candidates: 2 }
        ));
        let observation = Observation {
            cascade: None,
            camera: None,
            frame_counters: FrameCounters {
                counter: 0,
                frame_index: 0,
                ring_index: 0,
            },
            pixel_shader: ShaderIdentity {
                pointer: 0,
                identity: None,
            },
            constant_buffers: vec![],
            resources: vec![],
        };
        assert!(matches!(
            eye_result(true, true, 1, Some(observation)),
            EyeResult::Observed { .. }
        ));
    }

    #[test]
    fn published_observation_preserves_bindings_and_capture_identity() {
        let mut state = State {
            frame: Some(1234),
            generation: 9,
            candidates: [1, 0],
            observations: [
                Some(Observation {
                    cascade: None,
                    camera: None,
                    frame_counters: FrameCounters {
                        counter: 15,
                        frame_index: 14,
                        ring_index: 2,
                    },
                    pixel_shader: ShaderIdentity {
                        pointer: 7,
                        identity: Some(IdentityResult::Unmapped),
                    },
                    constant_buffers: vec![ConstantBuffer {
                        slot: 3,
                        pointer: 11,
                        byte_width: 512,
                        first_constant: Some(4),
                        num_constants: Some(12),
                        content: None,
                    }],
                    resources: vec![Resource {
                        slot: 31,
                        view: 12,
                        resource: 13,
                        format: 14,
                        dimension: 15,
                        texture: Some(TextureDesc {
                            width: 5,
                            height: 6,
                            depth: 7,
                            mip_levels: 1,
                            array_size: 1,
                            format: 16,
                        }),
                    }],
                }),
                None,
            ],
        };
        let capture = Capture {
            frame: 1234,
            generation: 9,
            eyes: [
                result_for_eye(&mut state, 1234, 9, 0, true),
                result_for_eye(&mut state, 1234, 9, 1, false),
            ],
        };
        let wire = serde_json::to_value(capture).unwrap();
        assert_eq!(wire["frame"], 1234);
        assert_eq!(wire["generation"], 9);
        assert_eq!(wire["eyes"][0]["status"], "observed");
        assert_eq!(wire["eyes"][1]["status"], "not_required");
        let observation = &wire["eyes"][0]["observation"];
        assert_eq!(observation["frame_counters"]["ring_index"], 2);
        assert_eq!(
            observation["pixel_shader"]["identity"]["status"],
            "unmapped"
        );
        assert_eq!(
            observation["constant_buffers"][0],
            serde_json::json!({"slot": 3, "pointer": 11, "byte_width": 512, "first_constant": 4, "num_constants": 12})
        );
        assert_eq!(
            observation["resources"][0],
            serde_json::json!({"slot": 31, "view": 12, "resource": 13, "format": 14, "dimension": 15, "texture": {"width": 5, "height": 6, "depth": 7, "mip_levels": 1, "array_size": 1, "format": 16}})
        );
    }

    #[test]
    fn published_failure_status_does_not_expose_stale_or_ambiguous_observation() {
        let observation = Observation {
            cascade: None,
            camera: None,
            frame_counters: FrameCounters {
                counter: 1,
                frame_index: 0,
                ring_index: 0,
            },
            pixel_shader: ShaderIdentity {
                pointer: 0,
                identity: None,
            },
            constant_buffers: vec![],
            resources: vec![],
        };
        let mut state = State {
            frame: Some(10),
            generation: 3,
            candidates: [1, 2],
            observations: [Some(observation.clone()), Some(observation)],
        };
        let missing = result_for_eye(&mut state, 11, 3, 0, true);
        let ambiguous = result_for_eye(&mut state, 10, 3, 1, true);
        assert_eq!(
            serde_json::to_value(missing).unwrap(),
            serde_json::json!({"status": "missing"})
        );
        assert_eq!(
            serde_json::to_value(ambiguous).unwrap(),
            serde_json::json!({"status": "ambiguous", "candidates": 2})
        );
    }

    #[test]
    fn bound_ranges_reject_empty_oversize_and_invalid_without_truncation() {
        assert!(matches!(
            unbound_or_range(false, 0, None),
            Err(BufferContent::Unbound)
        ));
        assert!(matches!(
            checked_range(0, None),
            Err(BufferContent::EmptyRange)
        ));
        assert!(matches!(
            checked_range(64, None),
            Err(BufferContent::UnknownRange)
        ));
        assert!(matches!(
            checked_range(64, Some((0, 0))),
            Err(BufferContent::EmptyRange)
        ));
        assert!(matches!(
            checked_range(MAX_CB_BYTES + 16, Some((0, 1))),
            Err(BufferContent::Oversize)
        ));
        assert!(matches!(
            checked_range(64, Some((3, 2))),
            Err(BufferContent::InvalidRange)
        ));
        assert!(matches!(
            checked_range(64, Some((u32::MAX, 1))),
            Err(BufferContent::InvalidRange)
        ));
        assert_eq!(checked_range(64, Some((1, 2))).ok(), Some(16..48));
        assert_eq!(checked_range(8192, Some((128, 256))).ok(), Some(2048..6144));
        assert_eq!(
            checked_range(MAX_CB_BYTES, Some((0, 4096))).ok(),
            Some(0..65536)
        );
    }

    fn artifact_observation(bytes: Vec<u8>) -> Observation {
        Observation {
            cascade: None,
            camera: None,
            frame_counters: FrameCounters {
                counter: 7,
                frame_index: 6,
                ring_index: 1,
            },
            pixel_shader: ShaderIdentity {
                pointer: 1,
                identity: None,
            },
            constant_buffers: vec![ConstantBuffer {
                slot: 0,
                pointer: 2,
                byte_width: 64,
                first_constant: Some(1),
                num_constants: Some(2),
                content: Some(BufferContent::Captured { bytes, raw: None }),
            }],
            resources: vec![],
        }
    }

    #[test]
    fn artifact_writer_publishes_exact_bytes_and_failures() {
        let folder = std::env::temp_dir().join(format!(
            "jc3vr-resolve-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let bytes: Vec<u8> = (0..32).collect();
        let mut capture = Capture {
            frame: 10,
            generation: 4,
            eyes: [
                EyeResult::Observed {
                    observation: Box::new(artifact_observation(bytes.clone())),
                },
                EyeResult::NotRequired,
            ],
        };
        capture.write_artifacts(&folder, "test");
        let wire = serde_json::to_value(&capture).unwrap();
        let content = &wire["eyes"][0]["observation"]["constant_buffers"][0]["content"];
        assert_eq!(content["status"], "captured");
        assert_eq!(
            std::fs::read(content["raw"].as_str().unwrap()).unwrap(),
            bytes
        );
        std::fs::remove_dir_all(&folder).unwrap();
        let mut failed = Capture {
            frame: 10,
            generation: 4,
            eyes: [
                EyeResult::Observed {
                    observation: Box::new(artifact_observation(vec![0; 32])),
                },
                EyeResult::NotRequired,
            ],
        };
        failed.write_artifacts(&folder, "test");
        let wire = serde_json::to_value(failed).unwrap();
        let content = &wire["eyes"][0]["observation"]["constant_buffers"][0]["content"];
        assert_eq!(content["status"], "read_error");
        assert!(
            content["error"]
                .as_str()
                .unwrap()
                .contains("resolve constants: writing")
        );
        assert!(content.get("raw").is_none());
    }

    #[test]
    fn bound_window_includes_every_requested_byte() {
        let bytes: Vec<u8> = (0..8192).map(|i| (i % 251) as u8).collect();
        let range = checked_range(bytes.len() as u32, Some((128, 256)))
            .ok()
            .unwrap();
        let selected = &bytes[range];
        assert_eq!(selected.len(), 4096);
        assert_eq!(selected, &bytes[2048..6144]);
    }

    #[test]
    fn bracket_is_scoped_and_inactive_brackets_do_not_arm() {
        DEPTH.with(|depth| assert_eq!(depth.get(), 0));
        let inactive = begin(false);
        DEPTH.with(|depth| assert_eq!(depth.get(), 0));
        let active = begin(true);
        DEPTH.with(|depth| assert_eq!(depth.get(), 1));
        let nested = begin(true);
        DEPTH.with(|depth| assert_eq!(depth.get(), 2));
        drop(nested);
        drop(active);
        drop(inactive);
        DEPTH.with(|depth| assert_eq!(depth.get(), 0));
    }
}
