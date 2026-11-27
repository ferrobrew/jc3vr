mod impostor;
mod projection;

use std::{cell::RefCell, ffi::c_void};

use anyhow::{Context, Result, ensure};
use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
#[cfg(feature = "profiler")]
use jc3gi::graphics_engine::render_engine::RenderBlockTypeBase;
use jc3vr_render::{
    probes::state::vertex_identity_lookup,
    readback::{read_mapped, staging_texture},
    stereo::single_pass::shader_identity,
};
use jc3vr_view::stereo::{dispatch::RenderMode, draw_index};
use serde::Serialize;
use windows::{
    Win32::Graphics::{
        Direct3D11::*,
        Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_TYPELESS, DXGI_FORMAT_R32G8X24_TYPELESS},
    },
    core::Interface,
};

use crate::{
    harness,
    harness::{
        bound_buffer::{ShaderStage, read_bound_constant_buffer},
        kept_shaders, patch_draw, rewrites,
        stages::{
            STATE, Stage, empty_stage_record,
            velocity::impostor::{ImpostorInputs, impostor_inputs},
        },
    },
};

const POINTS: [[u32; 2]; 6] = [
    [249, 351],
    [213, 290],
    [248, 336],
    [233, 325],
    [250, 352],
    [248, 358],
];
const INTERVAL_POINTS: [[u32; 2]; 6] = [
    [155, 232],
    [132, 233],
    [154, 230],
    [65, 225],
    [140, 231],
    [249, 351],
];
const RESIDUAL_POINTS: [[u32; 2]; 6] = [
    [215, 292],
    [250, 355],
    [239, 320],
    [218, 299],
    [249, 354],
    [263, 358],
];
const GROUP_POINTS: [[u32; 2]; 8] = [
    [132, 321],
    [134, 314],
    [130, 323],
    [125, 332],
    [137, 337],
    [97, 303],
    [138, 329],
    [147, 320],
];
const LIMIT: usize = 512;
thread_local! { static ACTIVE: RefCell<Option<Trace>> = const {RefCell::new(None)}; }

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Evidence {
    id: String,
    points: &'static [[u32; 2]],
    packed: bool,
    dither: Option<bool>,
    draws: Vec<Submission>,
}

#[derive(Clone, Debug, Serialize)]
struct Submission {
    kind: &'static str,
    local_dither: bool,
    card_atlas: Option<bool>,
    group_fade: Option<bool>,
    group_atlas_scale: bool,
    physical_atlas_scale: bool,
    left_atlas_scale: bool,
    pass: u8,
    rtv0_is_source: bool,
    depth_state: [i32; 3],
    args: [i64; 5],
    block_run: Option<&'static str>,
    dsv_is_main: bool,
    viewports: Vec<[f32; 6]>,
    vs: Option<String>,
    vs_name: Option<String>,
    ps: Option<shader_identity::IdentityResult>,
    before: Vec<u32>,
    after: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrain: Option<TerrainSubmission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    projection: Option<BaseProjection>,
    cb: Vec<BufferPrefix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    impostor: Option<ImpostorInputs>,
}

#[derive(Clone, Debug, Serialize)]
struct BaseProjection {
    applied: Option<bool>,
    eye: usize,
    hs: bool,
    ds: bool,
    gs: bool,
    generated: Option<[u32; 16]>,
    buffers: Vec<BufferPrefix>,
}

#[derive(Clone, Debug, Serialize)]
struct TerrainSubmission {
    #[serde(skip_serializing_if = "Option::is_none")]
    albedo: Option<[[u32; 2]; 2]>,
    hull_camera: Option<bool>,
    domain_camera: Option<bool>,
    camera: Option<bool>,
    ordinal: usize,
    indirect: Option<[u32; 4]>,
    gs: bool,
    hs: Option<String>,
    ds: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct BufferPrefix {
    stage: &'static str,
    slot: u32,
    first: u32,
    count: u32,
    range_known: bool,
    bits: Option<Vec<u32>>,
}

struct Trace {
    owner_four: bool,
    tess_writer: bool,
    base_sources: bool,
    group_sources: bool,
    terrain: bool,
    ordinal: usize,
    color: bool,
    interval: bool,
    residual_bark: bool,
    eye: usize,
    frame: i64,
    evidence: Evidence,
    resources: Option<(ID3D11Texture2D, ID3D11Texture2D)>,
    block: Option<&'static str>,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    Velocity,
    BarkColor,
    Interval,
    Terrain,
    Base,
}

pub(crate) fn scope(scope: Scope, draw: impl FnOnce()) {
    let base_sources = scope == Scope::Base;
    let color = scope == Scope::BarkColor;
    let interval = scope == Scope::Interval;
    let terrain = scope == Scope::Terrain;
    let selected = harness::frame_identity()
        .filter(|&frame| {
            if base_sources {
                harness::base_sources_requested(frame)
            } else if terrain {
                harness::terrain_sources_requested(frame)
            } else if interval {
                harness::interval_trace_requested(frame)
            } else if color {
                harness::bark_color_trace_requested(frame)
            } else {
                harness::velocity_trace_requested(frame)
            }
        })
        .and_then(|frame| {
            Some((
                frame,
                harness::diagnostic_capture_id()?,
                crate::testing::visual::metadata().mode?,
            ))
        });
    let Some((frame, id, mode)) = selected else {
        draw();
        return;
    };
    let packed = mode == RenderMode::SinglePass;
    let group_sources = interval && harness::group_sources_requested(frame);
    let tess_writer = terrain && harness::tess_writer_requested(frame);
    let eye = if group_sources || base_sources || tess_writer {
        if packed { 0 } else { draw_index() }
    } else {
        usize::from(!terrain && (!interval || !harness::left_interval_requested(frame)))
    };
    if !packed && draw_index() != eye {
        draw();
        return;
    }
    let residual_bark = !terrain && !interval && harness::residual_bark_requested(frame);
    let owner_four = tess_writer && harness::lake_owner_four_requested(frame);
    let previous = ACTIVE.replace(Some(Trace {
        owner_four,
        tess_writer,
        base_sources,
        group_sources,
        terrain,
        ordinal: 0,
        color,
        interval,
        residual_bark,
        eye,
        frame,
        evidence: Evidence {
            id,
            points: if base_sources {
                &[
                    [236, 103],
                    [188, 105],
                    [145, 80],
                    [99, 79],
                    [28, 46],
                    [29, 47],
                    [30, 48],
                    [144, 76],
                ]
            } else if group_sources && harness::group_residual_requested(frame) {
                &[
                    [145, 80],
                    [99, 79],
                    [268, 65],
                    [273, 63],
                    [289, 65],
                    [289, 64],
                    [97, 303],
                    [302, 65],
                ]
            } else if group_sources {
                &GROUP_POINTS
            } else if owner_four {
                &[
                    [9, 258],
                    [18, 354],
                    [3, 308],
                    [0, 282],
                    [0, 256],
                    [23, 306],
                    [186, 161],
                    [187, 162],
                ]
            } else if tess_writer {
                &[[9, 258], [18, 354], [3, 308], [0, 282], [0, 256], [23, 306]]
            } else if terrain {
                &[[204, 150], [307, 166]]
            } else if interval {
                &INTERVAL_POINTS
            } else if residual_bark {
                &RESIDUAL_POINTS
            } else {
                &POINTS
            },
            dither: if base_sources || interval || terrain {
                None
            } else if color {
                harness::color_dither(frame)
            } else {
                harness::bark_dither(frame)
            },
            packed,
            draws: Vec::new(),
        },
        resources: None,
        block: None,
        error: None,
    }));
    draw();
    if let Some(trace) = ACTIVE.replace(previous) {
        let mut record = empty_stage_record(
            trace.frame,
            trace.eye,
            if base_sources {
                Stage::BaseSources
            } else if terrain {
                Stage::TerrainSources
            } else if interval {
                Stage::IntervalTrace
            } else if color {
                Stage::BarkColorTrace
            } else {
                Stage::VelocityTrace
            },
        );
        record.error = trace.error.or_else(|| {
            trace
                .evidence
                .draws
                .is_empty()
                .then(|| "velocity trace: no observed submissions".into())
        });
        record.velocity = Some(trace.evidence);
        let mut state = STATE.lock();
        if state.frame == Some(frame) {
            state.records.push(record)
        }
    }
}

#[cfg(feature = "profiler")]
pub(crate) fn type_switch(next: Option<*mut RenderBlockTypeBase>) {
    ACTIVE.with_borrow_mut(|active| {
        if let Some(t) = active {
            t.block = next.and_then(|p| unsafe { p.as_ref()?.get_type_name_str() });
        }
    });
}

pub(crate) fn submit(
    context: *mut c_void,
    kind: &'static str,
    args: [i64; 5],
    draw: impl FnOnce(),
) {
    submit_impl(context, kind, args, None, draw);
}

pub(crate) fn submit_indirect(
    context: *mut c_void,
    buffer: *mut c_void,
    offset: u32,
    draw: impl FnOnce(),
) {
    submit_impl(
        context,
        "instanced_indirect",
        [offset.into(), 0, 0, 0, 0],
        Some(buffer),
        draw,
    );
}

fn submit_impl(
    context: *mut c_void,
    kind: &'static str,
    args: [i64; 5],
    buffer: Option<*mut c_void>,
    draw: impl FnOnce(),
) {
    harness::draw_census::on_submission(kind.ends_with("_promoted"));
    let mut draw = Some(draw);
    ACTIVE.with_borrow_mut(|active| {
        let Some(t) = active.as_mut().filter(|t| t.error.is_none()) else {
            return;
        };
        if t.group_sources && jc3vr_render::probes::state::current_pass_id()!=jc3gi::graphics_engine::render_engine::RenderPassId::RP_VEGETATIONGROUP2 as u8 {return;}
        if t.color
            && !(t.block == Some("VegetationBark")
                && kind == "indexed_instanced_indirect"
                && (matches!(args[0], 36660 | 37960 | 36860) || (t.residual_bark && jc3vr_render::probes::state::current_pass_id()==jc3gi::graphics_engine::render_engine::RenderPassId::RP_VEGETATION_OPAQUE as u8)))
        {
            return;
        }
        let result = (|| {
            ensure!(t.evidence.draws.len() < LIMIT, "submission limit exceeded");
            let ctx = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }
                .context("missing raw context")?;
            t.record(ctx, kind, args, buffer, || draw.take().unwrap()())
        })();
        if let Err(e) = result {
            t.error = Some(format!("velocity trace: {e:#}"))
        }
    });
    if let Some(draw) = draw {
        jc3vr_render::stereo::single_pass::detour_timing::forward(draw);
    }
}

impl Trace {
    fn record(
        &mut self,
        ctx: &ID3D11DeviceContext,
        kind: &'static str,
        args: [i64; 5],
        buffer: Option<*mut c_void>,
        draw: impl FnOnce(),
    ) -> Result<()> {
        let mut physical_eye = self.eye;
        if self.terrain {
            if kind != "instanced_indirect" {
                draw();
                return Ok(());
            }
            if self.evidence.packed {
                let mut viewports = [D3D11_VIEWPORT::default(); 2];
                let mut count = 2;
                unsafe { ctx.RSGetViewports(&mut count, Some(viewports.as_mut_ptr())) };
                ensure!(
                    count == 2
                        && viewports[0] == viewports[1]
                        && viewports[0].Width == 320.
                        && viewports[0].Height == 360.,
                    "terrain sources: unexpected viewport"
                );
                physical_eye = usize::from(viewports[0].TopLeftX == 320.);
                if physical_eye == 1 && !self.tess_writer {
                    draw();
                    return Ok(());
                }
                ensure!(
                    matches!(viewports[0].TopLeftX, 0. | 320.),
                    "terrain sources: unexpected eye origin"
                );
            }
            if !self.evidence.packed || physical_eye == 0 {
                self.ordinal += 1;
            }
            if if self.tess_writer {
                !(self.ordinal == 31 && args[0] == 2016
                    || self.owner_four && self.ordinal == 4 && args[0] == 480)
            } else {
                !matches!(self.ordinal, 2 | 4)
            } {
                draw();
                return Ok(());
            }
        }
        if self.resources.is_none() {
            let ge = unsafe { GraphicsEngine::get() }.context("engine unavailable")?;
            let depth = unsafe {
                (if self.color {
                    ge.m_GBufferTexture[0]
                } else {
                    ge.m_MainDepthTexture
                })
                .as_ref()
            }
            .context("missing main depth")?;
            let src: ID3D11Texture2D = depth.m_Texture.cast()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { src.GetDesc(&mut desc) };
            ensure!(
                desc.Format
                    == (if self.color {
                        DXGI_FORMAT_R8G8B8A8_TYPELESS
                    } else {
                        DXGI_FORMAT_R32G8X24_TYPELESS
                    })
                    && desc.Width == if self.evidence.packed { 640 } else { 320 }
                    && desc.Height == 360
                    && desc.SampleDesc.Count == 1
                    && desc.MipLevels == 1
                    && desc.ArraySize == 1,
                "unexpected main depth: {desc:?}"
            );
            let staging = unsafe { staging_texture(&ctx.GetDevice()?, &desc) }?;
            self.resources = Some((src, staging));
        }
        let (src, staging) = self.resources.as_ref().unwrap();
        let stride = if self.color { 4 } else { 8 };
        let depths = || -> Result<Vec<u32>> {
            // SAFETY: the context is locked, and every read lies within the depth's mapped rows,
            // whose pitch is checked first.
            unsafe {
                ctx.CopyResource(staging, src);
                read_mapped(ctx, staging, |m| {
                    let pitch = m.row_pitch() as u32;
                    let offset = if self.evidence.packed && physical_eye == 1 {
                        320
                    } else {
                        0
                    };
                    ensure!(pitch >= (320 + offset) * stride, "invalid mapped depth");
                    let physical = |base: u32| {
                        self.evidence.points.iter().copied().map(move |[x, y]| {
                            m.data()
                                .add((y * pitch + (x + base) * stride) as usize)
                                .cast::<u32>()
                                .read_unaligned()
                        })
                    };
                    let mut result: Vec<u32> = physical(offset).collect();
                    if (self.base_sources || self.group_sources) && self.evidence.packed {
                        ensure!(
                            pitch >= 640 * stride,
                            "group sources: short packed depth row"
                        );
                        result.extend(physical(320));
                    }
                    Ok(result)
                })?
            }
        };
        let mut rt = None;
        let mut ds = None;
        let (mut dsv, mut vs, mut ps) = (None, None, None);
        let mut n = 16;
        let mut v = [D3D11_VIEWPORT::default(); 16];
        unsafe {
            ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut rt)), Some(&mut dsv));
            ctx.OMGetDepthStencilState(Some(&mut ds), None);
            ctx.RSGetViewports(&mut n, Some(v.as_mut_ptr()));
            ctx.VSGetShader(&mut vs, None, None);
            ctx.PSGetShader(&mut ps, None, None)
        };
        ensure!(n <= 16, "viewport count exceeds API limit");
        let bound = dsv
            .map(|v| unsafe { v.GetResource()?.cast::<ID3D11Texture2D>() })
            .transpose()?;
        let vs = vs.and_then(|v| vertex_identity_lookup(v.as_raw() as usize).ok().flatten());
        if self.evidence.packed
            && ((self.color && v[0].TopLeftX == 0.)
                || (self.interval
                    && n > 0
                    && v[..n as usize].iter().all(|vp| {
                        if self.eye == 1 {
                            vp.TopLeftX + vp.Width <= 320.
                        } else {
                            vp.TopLeftX >= 320.
                        }
                    })))
        {
            draw();
            return Ok(());
        }
        let rt_src = rt
            .map(|v| unsafe { v.GetResource()?.cast::<ID3D11Texture2D>() })
            .transpose()?;
        let main = unsafe { GraphicsEngine::get().and_then(|ge| ge.m_MainDepthTexture.as_ref()) }
            .context("missing main depth")?
            .m_Texture
            .cast::<ID3D11Texture2D>()?;
        let mut ds_desc = D3D11_DEPTH_STENCIL_DESC::default();
        if let Some(ds) = ds {
            unsafe { ds.GetDesc(&mut ds_desc) }
        } else {
            ds_desc.DepthEnable = true.into();
            ds_desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ALL;
            ds_desc.DepthFunc = D3D11_COMPARISON_LESS;
        }
        let candidate = self.evidence.packed
            && self.evidence.dither == Some(true)
            && self.block == Some("VegetationBark")
            && kind == "indexed_instanced_indirect"
            && (matches!(args[0], 36660 | 37960 | 36860)
                || (self.residual_bark
                    && matches!(args[0], 36760 | 38060 | 38160)
                    && harness::extra_bark_dither(self.frame) == Some(true)))
            && v[0].TopLeftX != 0.;
        let impostor = if self.interval
            && self.block == Some("TreeImpostor")
            && ((args[0] == 3300 && harness::impostor_inputs_requested(self.frame))
                || (matches!(args[0], 4872 | 5946 | 6534 | 7542)
                    && self.group_sources
                    && harness::group_cards_requested(self.frame))
                || (args[0] == 144
                    && self.group_sources
                    && harness::group_residual_requested(self.frame))
                || (args[0] == 1392
                    && self.group_sources
                    && harness::group_inputs_requested(self.frame)))
        {
            let expected = if self.evidence.packed {
                "3aa1cbfcb79045d02114b9286e3a37a9ff1f98d4de610d458c6fbfbfb4c61d90"
            } else {
                "876a34b985389badefb0df0b5318799a4465e18c38c4e371fb552882ad9497b8"
            };
            ensure!(
                jc3vr_render::probes::state::current_pass_id()
                    == jc3gi::graphics_engine::render_engine::RenderPassId::RP_VEGETATIONGROUP2
                        as u8
                    && bound.as_ref() == Some(&main)
                    && (kind, args)
                        == if self.evidence.packed {
                            ("indexed_promoted", [args[0], 2, 0, 0, 0])
                        } else {
                            ("indexed", [args[0], 0, 0, 0, 0])
                        }
                    && vs.as_ref().is_some_and(|v| format!(
                        "{:x}",
                        sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256)
                    ) == expected)
                    && matches!(ps.as_ref().map(shader_identity::lookup),Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256==kept_shaders::IMPOSTOR_PS && identity.pristine_sha256==kept_shaders::IMPOSTOR_PS),
                "impostor inputs: unexpected draw"
            );
            Some(impostor_inputs(ctx, args[0] as u32)?)
        } else {
            None
        };
        let projection = if self.base_sources
            && matches!(args[0], 16 | 48 | 272 | 496 | 752)
            && harness::base_projection_requested(self.frame)
        {
            ensure!(
                self.block == Some("VolumetricTerrainPatch")
                    && kind == "instanced_indirect"
                    && bound.as_ref() == Some(&main)
                    && n == if self.evidence.packed { 2 } else { 1 }
                    && v[..n as usize].iter().all(|w| *w == v[0]
                        && w.Width == 320.
                        && w.Height == 360.
                        && w.TopLeftY == 0.)
                    && matches!(v[0].TopLeftX, 0. | 320.),
                "base projection: unexpected submission"
            );
            let eye = if self.evidence.packed {
                usize::from(v[0].TopLeftX == 320.)
            } else {
                self.eye
            };
            let (mut hs, mut ds, mut gs) = (None, None, None);
            unsafe {
                ctx.HSGetShader(&mut hs, None, None);
                ctx.DSGetShader(&mut ds, None, None);
                ctx.GSGetShader(&mut gs, None, None);
            }
            let generated = if self.evidence.packed {
                Some(
                    jc3vr_render::probes::state::full_eye_view_projection(eye)
                        .context("base projection: missing eye VP")?
                        .data
                        .map(f32::to_bits),
                )
            } else {
                None
            };
            let mut buffers = Vec::new();
            for (slot, rows) in [(0, 4), (1, 2), (2, 6), (3, 1)] {
                let b = read_bound_constant_buffer(ctx, ShaderStage::Vertex, slot, rows)?;
                ensure!(
                    b.range_known
                        && b.range_error.is_none()
                        && b.values
                            .as_ref()
                            .is_some_and(|v| v.len() == rows as usize * 4),
                    "base projection: incomplete VS b{slot}"
                );
                buffers.push(BufferPrefix {
                    stage: "vs",
                    slot,
                    first: b.first_constant,
                    count: b.num_constants,
                    range_known: b.range_known,
                    bits: b.values.map(|v| v.into_iter().map(f32::to_bits).collect()),
                });
            }
            Some(BaseProjection {
                applied: harness::base_eye_vp(self.frame),
                eye,
                hs: hs.is_some(),
                ds: ds.is_some(),
                gs: gs.is_some(),
                generated,
                buffers,
            })
        } else {
            None
        };
        let albedo = if self.owner_four && self.ordinal == 4 {
            let texture =
                unsafe { GraphicsEngine::get().and_then(|ge| ge.m_GBufferTexture[0].as_ref()) }
                    .context("terrain albedo: missing source")?
                    .m_Texture
                    .cast::<ID3D11Texture2D>()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut desc) };
            ensure!(
                rt_src.as_ref() == Some(&texture)
                    && desc.Format == DXGI_FORMAT_R8G8B8A8_TYPELESS
                    && desc.Width == if self.evidence.packed { 640 } else { 320 }
                    && desc.Height == 360
                    && desc.SampleDesc.Count == 1
                    && desc.ArraySize == 1
                    && desc.MipLevels == 1,
                "terrain albedo: wrong target"
            );
            Some((
                unsafe { staging_texture(&ctx.GetDevice()?, &desc) }?,
                texture,
            ))
        } else {
            None
        };
        let color_taps = || -> Result<Option<[u32; 2]>> {
            let Some((staging, texture)) = albedo.as_ref() else {
                return Ok(None);
            };
            let x = if self.evidence.packed && physical_eye == 1 {
                320
            } else {
                0
            };
            // SAFETY: the context is locked, and both taps lie within the albedo's mapped rows, whose
            // pitch is checked first.
            let result = unsafe {
                ctx.CopyResource(staging, texture);
                read_mapped(ctx, staging, |m| {
                    let pitch = m.row_pitch() as u32;
                    (pitch >= 4 * (x + 320)).then(|| {
                        [(186, 161), (187, 162)].map(|(tx, ty)| {
                            m.data()
                                .add((ty * pitch + 4 * (tx + x)) as usize)
                                .cast::<u32>()
                                .read_unaligned()
                        })
                    })
                })?
            };
            Ok(Some(result.context("terrain albedo: invalid mapped row")?))
        };
        let before_color = color_taps()?;
        let before = depths()?;
        let mut terrain = if self.terrain {
            let (mut hs, mut ds, mut gs) = (None, None, None);
            unsafe {
                ctx.GSGetShader(&mut gs, None, None);
                ctx.HSGetShader(&mut hs, None, None);
                ctx.DSGetShader(&mut ds, None, None)
            };
            Some(TerrainSubmission {
                albedo: None,
                hull_camera: if self.tess_writer && self.ordinal == 31 && physical_eye == 1 {
                    harness::patch63_hull(self.frame)
                } else {
                    None
                },
                domain_camera: if self.tess_writer && self.ordinal == 31 {
                    harness::writer_domain_camera(self.frame)
                } else if self.owner_four && self.ordinal == 4 && physical_eye == 1 {
                    harness::owner_four_domain(self.frame)
                } else {
                    None
                },
                camera: harness::terrain_camera(self.frame),
                ordinal: self.ordinal,
                indirect: if self.tess_writer {
                    let buffer = buffer
                        .as_ref()
                        .and_then(|p| unsafe { ID3D11Buffer::from_raw_borrowed(p) })
                        .context("terrain writer: missing indirect buffer")?;
                    let bytes = impostor::buffer_slice(ctx, buffer, args[0].try_into()?, 16)?;
                    let words: [u8; 16] = bytes
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("terrain writer: short argument buffer"))?;
                    Some(std::array::from_fn(|i| {
                        u32::from_le_bytes(words.as_chunks::<4>().0[i])
                    }))
                } else {
                    None
                },
                gs: gs.is_some(),
                hs: hs.as_ref().and_then(|s| patch_draw::identity(s)),
                ds: ds.as_ref().and_then(|s| patch_draw::identity(s)),
            })
        } else {
            None
        };
        let (domain_guard, hull_guard) = if let Some((t, treated)) =
            terrain.as_ref().and_then(|t| Some((t, t.domain_camera?)))
        {
            let selector = read_bound_constant_buffer(ctx, ShaderStage::Vertex, 3, 1)?;
            let (offset, vertices, patch) =
                if self.owner_four && t.ordinal == 4 && physical_eye == 1 {
                    (480, 750, 15)
                } else {
                    ensure!(t.ordinal == 31, "unexpected terrain ordinal");
                    (2016, 3513, 63)
                };
            ensure!(self.evidence.packed && self.tess_writer && t.camera.is_none() && !t.gs && args == [offset,0,0,0,0] && t.indirect == Some([vertices,1,0,0]) &&
                selector.range_known && selector.range_error.is_none() && selector.first_constant == 0 && selector.values.is_some_and(|v| v.into_iter().map(f32::to_bits).eq([patch,2,0,0])) &&
                self.block==Some("VolumetricTerrainPatch") && jc3vr_render::probes::state::current_pass_id()==jc3gi::graphics_engine::render_engine::RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR as u8 &&
                n==2 && v[0]==v[1] && bound.as_ref()==Some(&main) && ds_desc.DepthEnable.as_bool() && ds_desc.DepthFunc==D3D11_COMPARISON_GREATER_EQUAL && ds_desc.DepthWriteMask==D3D11_DEPTH_WRITE_MASK_ALL &&
                vs.as_ref().is_some_and(|v|format!("{:x}",sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256))=="f55b68229567994446a0f9c3f13074e125e3237e75b3c2956286b86f7265e43b") && t.hs.as_deref()==Some("50607f399ea4023cdc6ad3f942d52728dc622de7a454d393501a9ae83a5143a1") && t.ds.as_deref()==Some("a58152c0cdef733943e99b2d4f9ac82eacaa89484e5c093668962b5379f6a5ba") && matches!(ps.as_ref().map(shader_identity::lookup),Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256=="38987576f35ad34008eb3321ebe4e531a75f840d8b40eb25081d287c0cd19470"), "terrain writer: unexpected domain treatment pipeline");
            let domain = patch_draw::WriterDomainGuard::bind(
                ctx,
                &self.evidence.id,
                physical_eye,
                v[0],
                treated,
                ShaderStage::Domain,
            )?;
            let hull = if let Some(change) = t.hull_camera {
                ensure!(
                    t.ordinal == 31 && physical_eye == 1 && treated,
                    "terrain hull: unexpected draw"
                );
                Some(patch_draw::WriterDomainGuard::bind(
                    ctx,
                    &self.evidence.id,
                    physical_eye,
                    v[0],
                    change,
                    ShaderStage::Hull,
                )?)
            } else {
                None
            };
            (Some(domain), hull)
        } else {
            (None, None)
        };
        let camera_guard = if let Some(t) = terrain.as_ref().filter(|t| t.camera.is_some()) {
            ensure!(self.evidence.packed && self.eye == 0 && n == 2 && v[0] == v[1] && self.block == Some("VolumetricTerrainPatch") && jc3vr_render::probes::state::current_pass_id() == jc3gi::graphics_engine::render_engine::RenderPassId::RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR as u8 && matches!((t.ordinal,args), (2,[448,0,0,0,0])|(4,[480,0,0,0,0])) && bound.as_ref()==Some(&main) && ds_desc.DepthEnable.as_bool() && ds_desc.DepthFunc==D3D11_COMPARISON_GREATER_EQUAL && ds_desc.DepthWriteMask==D3D11_DEPTH_WRITE_MASK_ALL && vs.as_ref().is_some_and(|v| format!("{:x}", sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256)) == "f55b68229567994446a0f9c3f13074e125e3237e75b3c2956286b86f7265e43b") && t.hs.as_deref() == Some("50607f399ea4023cdc6ad3f942d52728dc622de7a454d393501a9ae83a5143a1") && t.ds.as_deref() == Some("a58152c0cdef733943e99b2d4f9ac82eacaa89484e5c093668962b5379f6a5ba") && matches!(ps.as_ref().map(shader_identity::lookup),Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256=="38987576f35ad34008eb3321ebe4e531a75f840d8b40eb25081d287c0cd19470"), "terrain camera: submission identity mismatch");
            Some(patch_draw::TerrainCameraGuard::bind(
                ctx,
                &self.evidence.id,
                t.camera == Some(true),
                v[0],
            )?)
        } else {
            None
        };
        let physical_atlas_scale = args[0] == 3300
            && impostor.is_some()
            && self.evidence.packed
            && harness::impostor_scale(self.frame) == Some(true);
        let left_atlas_scale = physical_atlas_scale
            && self.eye == 0
            && harness::left_atlas_scale(self.frame) == Some(true);
        let group_atlas_scale = self.group_sources
            && args[0] == 1392
            && self.eye == 0
            && self.evidence.packed
            && impostor.is_some()
            && harness::group_atlas_scale(self.frame) == Some(true);
        let card_atlas = (matches!(args[0], 4872 | 5946)
            && self.evidence.packed
            && self.eye == 0
            && self.group_sources
            && impostor.is_some())
        .then(|| harness::card_atlas(self.frame))
        .flatten();
        let group_fade = (matches!(args[0], 6534 | 7542)
            && self.evidence.packed
            && self.eye == 0
            && self.group_sources
            && impostor.is_some())
        .then(|| harness::group_fade(self.frame))
        .flatten();
        let replacement = if physical_atlas_scale
            || group_atlas_scale
            || group_fade.is_some()
            || card_atlas.is_some()
        {
            if group_atlas_scale || group_fade.is_some() || card_atlas.is_some() {
                let inputs = impostor.as_ref().context("group atlas: missing inputs")?;
                let row = inputs
                    .cb
                    .iter()
                    .find(|c| c.stage == "vs" && c.slot == 1)
                    .and_then(|c| c.bits.as_ref())
                    .and_then(|v| v.get(12..14));
                ensure!(
                    row == Some(&[(1.0f32 / 640.).to_bits(), (1.0f32 / 360.).to_bits()][..])
                        && inputs.raster.as_ref().is_some_and(|r| !r.scissor_enabled)
                        && ds_desc.DepthEnable.as_bool(),
                    "group atlas: unexpected dimensions or raster state"
                );
            }
            ensure!(
                n == 2
                    && v[0].TopLeftX == 0.
                    && v[1].TopLeftX == 320.
                    && v[..2]
                        .iter()
                        .all(|vp| vp.Width == 320. && vp.Height == 360. && vp.TopLeftY == 0.)
                    && ds_desc.DepthFunc == D3D11_COMPARISON_GREATER_EQUAL
                    && ds_desc.DepthWriteMask == D3D11_DEPTH_WRITE_MASK_ALL,
                "impostor scale: unexpected viewport or depth state"
            );
            let original =
                kept_shaders::impostor_bytecode().context("impostor trial: missing bytecode")?;
            let bytes = if let Some(physical) = card_atlas {
                if physical {
                    rewrites::impostor_atlas_scale(&original, v[1].TopLeftX, true)?
                } else {
                    original
                }
            } else if let Some(physical) = group_fade {
                let inputs = impostor.as_ref().context("impostor fade: missing inputs")?;
                let row = inputs
                    .cb
                    .iter()
                    .find(|c| c.stage == "ps" && c.slot == 0 && c.first == 0 && c.range_known)
                    .and_then(|c| c.bits.as_ref())
                    .and_then(|v| v.get(34..36));
                ensure!(
                    row == Some(&[(1.0f32 / 640.).to_bits(), (1.0f32 / 360.).to_bits()][..]),
                    "impostor fade: unexpected screen reciprocals"
                );
                if physical {
                    rewrites::physical_impostor_fade(&original)?
                } else {
                    original
                }
            } else {
                rewrites::impostor_atlas_scale(
                    &original,
                    v[1].TopLeftX,
                    left_atlas_scale || group_atlas_scale,
                )?
            };
            let mut shader = None;
            unsafe {
                ctx.GetDevice()?
                    .CreatePixelShader(&bytes, None, Some(&mut shader))?
            };
            Some(shader.context("impostor scale: missing shader")?)
        } else if candidate {
            let (expected_vs, expected_ps) = if self.color {
                (
                    "148aa95ac4cbcf214e6a4882959f6ec8ea133e2d0b2ddc5523c635acbddb0d85",
                    kept_shaders::BARK_COLOR_PS,
                )
            } else {
                (
                    "4a50f1e36c27de7b0d1e6da719a0bc159768c44b692b63853c45f4e7683d31ee",
                    kept_shaders::BARK_PS,
                )
            };
            ensure!(
                bound.as_ref()==Some(&main) && (!self.color || (rt_src.as_ref()==Some(src) && jc3vr_render::probes::state::current_pass_id()==jc3gi::graphics_engine::render_engine::RenderPassId::RP_VEGETATION_OPAQUE as u8 && ds_desc.DepthFunc==D3D11_COMPARISON_EQUAL))
                    && n == 2
                    && v[0] == v[1]
                    && v[0].TopLeftX == 320.
                    && v[0].Width == 320.
                    && v[0].Height == 360.
                    && vs.as_ref().is_some_and(|v| format!(
                        "{:x}",
                        sha2::digest::Output::<sha2::Sha256>::from(v.actual_sha256)
                    ) == expected_vs)
                    // The bound shader may carry `single_pass.screen_door_per_eye`'s rewrite; the
                    // trial replaces it with its own treatment of the pristine bytecode either way.
                    && matches!(ps.as_ref().map(shader_identity::lookup),Some(shader_identity::IdentityResult::Mapped{identity}) if identity.pristine_sha256==expected_ps),
                "bark trial: unexpected submission"
            );
            let bytes = rewrites::local_bark_dither(
                &kept_shaders::bark_bytecode(self.color).context("missing bark bytecode")?,
                v[0].TopLeftX,
                self.color,
            )?;
            let mut shader = None;
            unsafe {
                ctx.GetDevice()?
                    .CreatePixelShader(&bytes, None, Some(&mut shader))?
            };
            Some(shader.context("missing bark treatment shader")?)
        } else {
            None
        };
        if let Some(shader) = replacement.as_ref() {
            unsafe { ctx.PSSetShader(shader, None) };
            let mut actual = None;
            unsafe { ctx.PSGetShader(&mut actual, None, None) };
            if actual.as_ref() != Some(shader) {
                unsafe { ctx.PSSetShader(ps.as_ref(), None) };
                anyhow::bail!("bark trial: shader not bound")
            }
        }
        if let Some(p) = projection.as_ref().filter(|p| p.applied.is_some()) {
            ensure!(
                self.evidence.packed
                    && !p.hs
                    && !p.ds
                    && !p.gs
                    && replacement.is_none()
                    && ds_desc.DepthEnable.as_bool()
                    && ds_desc.DepthFunc == D3D11_COMPARISON_GREATER_EQUAL
                    && ds_desc.DepthWriteMask == D3D11_DEPTH_WRITE_MASK_ALL
                    && vs.as_ref().is_some_and(|s| format!(
                        "{:x}",
                        sha2::digest::Output::<sha2::Sha256>::from(s.actual_sha256)
                    ) == "b3e9b24bcddfed3ef1bbd85b5df419035b42b7ad6420084457dedc409f10d234")
                    && matches!(ps.as_ref().map(shader_identity::lookup), Some(shader_identity::IdentityResult::Mapped{identity}) if identity.actual_sha256=="38987576f35ad34008eb3321ebe4e531a75f840d8b40eb25081d287c0cd19470"),
                "basemesh VP: unexpected pipeline"
            );
            projection::submit(ctx, p, draw)?;
        } else {
            draw();
        }
        if replacement.is_some() {
            unsafe { ctx.PSSetShader(ps.as_ref(), None) };
            let mut actual = None;
            unsafe { ctx.PSGetShader(&mut actual, None, None) };
            ensure!(actual == ps, "bark trial: shader not restored");
        }
        let after = depths()?;
        if let Some(before) = before_color {
            terrain
                .as_mut()
                .context("terrain albedo: missing submission")?
                .albedo = Some([
                before,
                color_taps()?.context("terrain albedo: missing result")?,
            ]);
        }
        let mut cb = Vec::new();
        if self.terrain || before != after {
            let slots: &[(ShaderStage, u32, u32)] = if self.base_sources {
                &[
                    (ShaderStage::Vertex, 0, 18),
                    (ShaderStage::Vertex, 1, 16),
                    (ShaderStage::Vertex, 3, 1),
                    (ShaderStage::Vertex, 13, 18),
                    (ShaderStage::Domain, 1, 10),
                    (ShaderStage::Hull, 1, 10),
                ]
            } else if self.tess_writer {
                &[
                    (ShaderStage::Vertex, 1, 2),
                    (ShaderStage::Vertex, 2, 6),
                    (ShaderStage::Vertex, 3, 1),
                    (ShaderStage::Domain, 0, 2),
                    (ShaderStage::Domain, 1, 10),
                    (ShaderStage::Domain, 13, 18),
                    (ShaderStage::Hull, 0, 1),
                    (ShaderStage::Hull, 1, 10),
                ]
            } else if self.terrain {
                &[
                    (ShaderStage::Domain, 0, 2),
                    (ShaderStage::Domain, 1, 10),
                    (ShaderStage::Domain, 13, 18),
                    (ShaderStage::Hull, 0, 1),
                    (ShaderStage::Hull, 1, 10),
                    (ShaderStage::Vertex, 3, 1),
                ]
            } else {
                &[
                    (ShaderStage::Vertex, 0, 16),
                    (ShaderStage::Vertex, 1, 16),
                    (ShaderStage::Vertex, 2, 16),
                    (ShaderStage::Vertex, 13, 16),
                ]
            };
            for &(stage, slot, rows) in slots {
                let r = read_bound_constant_buffer(ctx, stage, slot, rows)?;
                cb.push(BufferPrefix {
                    stage: stage.name(),
                    slot,
                    first: r.first_constant,
                    count: r.num_constants,
                    range_known: r.range_known,
                    bits: r.values.map(|v| v.into_iter().map(f32::to_bits).collect()),
                });
            }
        }
        if let Some(guard) = hull_guard {
            guard.restore()?;
        }
        if let Some(guard) = domain_guard {
            guard.restore()?;
        }
        if let Some(guard) = camera_guard {
            guard.restore()?;
        }
        self.evidence.draws.push(Submission {
            kind,
            args,
            terrain,
            projection,
            impostor,
            card_atlas,
            group_fade,
            group_atlas_scale,
            physical_atlas_scale,
            left_atlas_scale,
            local_dither: candidate && replacement.is_some(),
            block_run: self.block,
            pass: jc3vr_render::probes::state::current_pass_id(),
            rtv0_is_source: rt_src.as_ref() == Some(src),
            depth_state: [
                ds_desc.DepthEnable.0,
                ds_desc.DepthWriteMask.0,
                ds_desc.DepthFunc.0,
            ],
            dsv_is_main: bound.as_ref() == Some(&main),
            viewports: v[..n as usize]
                .iter()
                .map(|v| {
                    [
                        v.TopLeftX, v.TopLeftY, v.Width, v.Height, v.MinDepth, v.MaxDepth,
                    ]
                })
                .collect(),
            vs: vs
                .as_ref()
                .map(|v| v.actual_sha256.iter().map(|b| format!("{b:02x}")).collect()),
            vs_name: vs.and_then(|v| v.name),
            ps: ps.as_ref().map(shader_identity::lookup),
            before,
            after,
            cb,
        });
        Ok(())
    }
}
