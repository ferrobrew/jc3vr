use std::{cell::Cell, ffi::c_void};

use anyhow::{Context, ensure};
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext, render_engine::RenderPassId, render_pass::RenderPass,
    },
    water_patch_manager::WaterPatchManager,
};
use jc3vr_render::probes::state::vertex_identity_lookup;
use jc3vr_view::stereo::draw_index;
use parking_lot::Mutex;
use serde::Serialize;
use windows::{Win32::Graphics::Direct3D11::ID3D11DeviceContext, core::Interface};

use crate::screenshot::harness::{
    bound_buffer::{ShaderStage, read_bound_constant_buffer},
    diagnostic_capture_id, frame_identity, lake_producer_requested, patch_draw,
    screenshot_wants_eyes, water_surface_inputs_requested,
};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LakeProjection {
    camera_view: [f32; 16],
    camera_projection_f: [f32; 16],
    camera_aspect: f32,
    camera_size: [i32; 2],
    screen_space_reflection: bool,
    backdrop_context: Option<Producer>,
    producers: Vec<Producer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ps_refraction_view: Option<[[f32; 4]; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ps_wave_coefficients: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wave_producer: Option<WaveProducer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ds_mvp_binding: Option<LakeMvpBinding>,
    ps_globals_screen: [f32; 4],
    ps_type_scale: [f32; 4],
    ps_type_reflection: [f32; 4],
}

#[derive(Clone, Debug, Serialize)]
struct LakeMvpBinding {
    vs_pointer: usize,
    ds_pointer: usize,
    original: Vec<u32>,
    actual: Vec<u32>,
    generated: [u32; 16],
}

#[derive(Clone, Debug, Serialize)]
struct WaveProducer {
    vs: String,
    hs: String,
    ds: String,
    vs_name: Option<String>,
}

fn wave_producer(ctx: &ID3D11DeviceContext) -> anyhow::Result<WaveProducer> {
    let (mut vs, mut hs, mut ds, mut gs) = (None, None, None, None);
    unsafe {
        ctx.VSGetShader(&mut vs, None, None);
        ctx.HSGetShader(&mut hs, None, None);
        ctx.DSGetShader(&mut ds, None, None);
        ctx.GSGetShader(&mut gs, None, None);
    }
    ensure!(gs.is_none(), "lake producer: unexpected geometry stage");
    let vs = vs.context("lake producer: missing vertex stage")?;
    let id = vertex_identity_lookup(vs.as_raw() as usize)
        .map_err(|e| anyhow::anyhow!("lake producer: VS census {e}"))?
        .context("lake producer: untagged vertex stage")?;
    Ok(WaveProducer {
        vs_name: id.name,
        vs: id
            .actual_sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        hs: hs
            .as_ref()
            .and_then(|s| patch_draw::identity(s))
            .context("lake producer: untagged hull stage")?,
        ds: ds
            .as_ref()
            .and_then(|s| patch_draw::identity(s))
            .context("lake producer: untagged domain stage")?,
    })
}

#[derive(Clone, Debug, Serialize)]
struct Producer {
    frame: i64,
    capture_id: String,
    dispatch_eye: usize,
    pass: i16,
    source_camera: usize,
    snapshot_camera: usize,
    manager_camera: usize,
    view: [f32; 16],
    projection_f: [f32; 16],
    target_resources: Vec<usize>,
    draws: u32,
}

static PRODUCERS: Mutex<Vec<Producer>> = Mutex::new(Vec::new());
thread_local! { static RECORDING: Cell<Option<usize>> = const { Cell::new(None) }; }

pub(crate) struct ProducerScope;

impl Drop for ProducerScope {
    fn drop(&mut self) {
        RECORDING.set(None);
    }
}

pub(crate) unsafe fn capture_producer_context(
    pass: *const RenderPass,
    ctx: *const RenderContext,
) -> Option<ProducerScope> {
    if !screenshot_wants_eyes() || !water_surface_inputs_requested() {
        return None;
    }
    let capture_id = diagnostic_capture_id()?;
    let frame = frame_identity()?;
    let ctx = unsafe { ctx.as_ref() }?;
    let pass = unsafe { pass.as_ref() }?;
    let manager = unsafe { WaterPatchManager::get() }?;
    let mut producers = PRODUCERS.lock();
    producers.retain(|p| p.frame == frame && p.capture_id == capture_id);
    if producers.len() >= 2 * 3 {
        return None;
    }
    RECORDING.set(Some(producers.len()));
    producers.push(Producer {
        frame,
        capture_id,
        dispatch_eye: draw_index(),
        pass: pass.m_Index,
        source_camera: pass.ExternalCamera as usize,
        snapshot_camera: pass.ExternalRenderCamera as usize,
        manager_camera: manager.m_ReflectionCamera as usize,
        view: ctx.m_View.data,
        projection_f: ctx.m_ProjectionF.data,
        target_resources: Vec::new(),
        draws: 0,
    });
    Some(ProducerScope)
}

pub(crate) unsafe fn observe_producer_draw(raw: *mut c_void) {
    let Some(index) = RECORDING.get() else {
        return;
    };
    let mut producers = PRODUCERS.lock();
    let Some(p) = producers.get_mut(index) else {
        return;
    };
    p.draws += 1;
    if p.draws > 32 {
        return;
    }
    let Some(ctx) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return;
    };
    let mut view = None;
    unsafe { ctx.OMGetRenderTargets(Some(std::slice::from_mut(&mut view)), None) };
    let resource = view.and_then(|v| unsafe { v.GetResource() }.ok());
    p.target_resources
        .push(resource.as_ref().map_or(0, |v| v.as_raw() as usize));
}

pub(super) fn capture(
    ctx: &ID3D11DeviceContext,
    id: &str,
    frame: i64,
    eye: usize,
) -> anyhow::Result<LakeProjection> {
    let manager = unsafe { WaterPatchManager::get() }
        .context("lake projection: missing water patch manager")?;
    let camera = unsafe { manager.m_ReflectionCamera.as_ref() }
        .context("lake projection: missing reflection camera")?;
    let row = |values: &[f32], index: usize| -> anyhow::Result<[f32; 4]> {
        Ok(values
            .get(4 * index..4 * index + 4)
            .context("lake projection: missing row")?
            .try_into()?)
    };
    let read = |slot, count| -> anyhow::Result<Vec<f32>> {
        let data = read_bound_constant_buffer(ctx, ShaderStage::Pixel, slot, count)?;
        ensure!(
            data.range_known && data.range_error.is_none(),
            "lake projection: unknown PS buffer range for slot {slot}"
        );
        data.values.context("lake projection: unbound PS buffer")
    };
    let globals = read(0, 9)?;
    let constants = read(1, 15)?;
    let producers: Vec<_> = PRODUCERS
        .lock()
        .iter()
        .filter(|p| p.capture_id == id && p.frame == frame)
        .cloned()
        .collect();
    Ok(LakeProjection {
        camera_view: camera.m_View.data,
        camera_projection_f: camera.m_ProjectionF.data,
        camera_aspect: camera.m_AspectRatio,
        camera_size: [camera.m_Width, camera.m_Height],
        screen_space_reflection: manager.m_EnableScreenSpaceWaterReflection,
        backdrop_context: producers
            .iter()
            .rev()
            .find(|p| p.pass == RenderPassId::PRE_RP_REFLECTION_DISTANT_BACKDROP as i16)
            .cloned(),
        producers,
        ps_refraction_view: if crate::screenshot::harness::lake_normals_requested(frame) {
            Some([row(&constants, 0)?, row(&constants, 1)?])
        } else {
            None
        },
        ps_wave_coefficients: if crate::screenshot::harness::lake_waves_requested(frame) {
            let cb = read(2, 3)?;
            Some([
                row(&cb, 0)?[0],
                row(&cb, 0)?[2],
                row(&cb, 1)?[1],
                row(&cb, 2)?[0],
            ])
        } else {
            None
        },
        wave_producer: lake_producer_requested(frame)
            .then(|| wave_producer(ctx))
            .transpose()?,
        ds_mvp_binding: crate::screenshot::harness::lake_ds_mvp(frame)
            .map(|treated| -> anyhow::Result<_> {
                let (vs_pointer, original) =
                    patch_draw::read_rows(ctx, ShaderStage::Vertex, 1, 10)?;
                let (ds_pointer, actual) = patch_draw::read_rows(ctx, ShaderStage::Domain, 1, 10)?;
                let generated = patch_draw::water_mvp(eye)?;
                ensure!(
                    vs_pointer != ds_pointer
                        && actual[16..] == original[16..]
                        && actual[..16]
                            == *if treated {
                                &generated[..]
                            } else {
                                &original[..16]
                            },
                    "lake MVP: selected draw binding mismatch"
                );
                Ok(LakeMvpBinding {
                    vs_pointer,
                    ds_pointer,
                    original,
                    actual,
                    generated,
                })
            })
            .transpose()?,
        ps_globals_screen: row(&globals, 8)?,
        ps_type_scale: row(&constants, 14)?,
        ps_type_reflection: row(&constants, 11)?,
    })
}
