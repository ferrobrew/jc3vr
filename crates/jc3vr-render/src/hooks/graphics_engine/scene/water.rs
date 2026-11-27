//! The legacy (non-WaveWorks) water render blocks' screen-space reflection/refraction lookup, biased
//! into each eye's half of the double-wide target under the single-pass collapse.
//!
//! These blocks sample `ReflectionMap`, `RefractionMap`, and `DepthMap` through a *projective*
//! coordinate rather than `SV_Position`: their block type stages a world→screen-UV matrix on vertex
//! `cb1` once per pass, the vertex shader transforms the water vertex by it and passes the result on
//! as `TEXCOORD1`, and the pixel shader divides by `w`. The NDC→UV half-scale is already folded into
//! the CPU-side matrix, so the UV it yields is normalized over the **viewport** -- one eye's half --
//! while every buffer it indexes is the whole double-wide target. Each eye therefore reads the entire
//! two-eye image stretched across its water surface, and because the error is a fixed 2x scale it is a
//! 2x motion gain too: the reflections slide over the water as the camera moves.
//!
//! The fix is four rows of arithmetic on the matrix the type already staged --
//! `u' = (u + eye) · 0.5` -- applied around a per-eye re-issue of the block's `Draw`
//! ([`screen_uv_cb_per_eye`](crate::stereo::single_pass::screen_uv_cb_per_eye)), which is also what
//! makes the eye known. No shader is touched: the water vertex shaders take their clip position from
//! the global `cb0`, which the collapse already handles, and the projective coordinate is entirely a
//! CPU-side constant.
//!
//! The WaveWorks family (`NvWater*`, [`NvWaterHighEndRenderBlock`]) does not have that defect -- its
//! shaders build the screen UV as `SV_Position × (1/2W, 1/H)`, which is already self-consistent under
//! double-wide -- but it has the other one, and the second half of this module fixes it: the whole
//! family takes its clip position from a model-view-projection the block bakes into its own constant
//! buffer, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed centre
//! view of the water surface. See [`nv_water_per_eye`].
//!
//! The WaveWorks planar reflection is rendered from the water patch manager's reflection camera,
//! which the engine sizes to the active camera every frame: under the double-wide collapse, twice an
//! eye's width. [`update_reflection_camera`] puts it back to one eye.
//!
//! Not covered: the water-box *surface* geometry ([`WaterBoxRenderBlock::DrawSurface`], and the
//! [`DrawWaterBoxSurface`](jc3gi::graphics_engine::render_block::DrawWaterBoxSurface) loop
//! [`NvWaterHighEndRenderBlock::Draw`] runs over every registered box). Neither mechanism above
//! reaches it, and neither is its defect: its vertex shader builds clip from the full,
//! translation-bearing view-projection in the global `cb0`, which the collapse's per-eye register
//! remap does not cover. See `docs/mod/stereo/single-pass-stereo.md`.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    camera::camera_manager::CameraManager,
    graphics_engine::{
        graphics_engine::RenderContext,
        render_block::{
            NvWaterHighEndRenderBlock, RBIInfo, WaterBoxRenderBlock, WaterHighEndRenderBlock,
        },
        render_engine::RenderPassId,
    },
    types::math::Matrix4,
    water_patch_manager::WaterPatchManager,
};
use jc3vr_view::stereo::{draw_index, layout::double_wide_active};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::{
    probes,
    probes::{BlockScope, PipelinePoint},
    stereo::single_pass::{
        begin_water_surface, draw_per_eye_half_ignoring_bound_vs, eye_globals,
        screen_uv_cb_per_eye, with_wave_simulation_viewports,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WATER_BOX_DRAW_BINDER)
        .with_static_binder(&NV_WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WAVE_WORKS_SIMULATION_STEP_BINDER)
        .with_static_binder(&UPDATE_REFLECTION_CAMERA_BINDER)
}

/// The high-end water family (`waterhighend`, `waterbelow`, `watershader_lod0/1/2`) reads the
/// screen-UV matrix
/// [`WaterHighEndRenderBlockType::Setup`](jc3gi::graphics_engine::render_block::WaterHighEndRenderBlockType::Setup)
/// stages from the render context's world→clip view-projection.
#[detour(address = jc3gi::graphics_engine::render_block::WaterHighEndRenderBlock::Draw_ADDRESS)]
fn water_high_end_draw(
    this: *const WaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `rc` is the live render context the engine passed into `Draw`, read only for its
    // matrices; the closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        per_eye(
            rc,
            HIGH_END_REGISTER,
            |rc| rc.m_ViewProjectionF.data,
            || detour.call(this, rc, info),
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The water-box family (`waterbox`, `waterboxbelow`, `waterboxclear`) reads the same kind of matrix,
/// which [`WaterBoxRenderBlockType::Setup`](jc3gi::graphics_engine::render_block::WaterBoxRenderBlockType::Setup)
/// stages from the translation-free view-projection instead -- its geometry is camera-relative.
#[detour(address = jc3gi::graphics_engine::render_block::WaterBoxRenderBlock::Draw_ADDRESS)]
fn water_box_draw(this: *const WaterBoxRenderBlock, rc: *mut RenderContext, info: *const RBIInfo) {
    let detour = WATER_BOX_DRAW.get().unwrap();
    // SAFETY: as above.
    let handled = unsafe {
        per_eye(
            rc,
            BOX_REGISTER,
            |rc| rc.m_OffsetViewProjection.data,
            || detour.call(this, rc, info),
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The WaveWorks family, re-issued once per eye with that eye's camera substituted into the render
/// context the block bakes its matrices from. See [`nv_water_per_eye`].
#[detour(address = jc3gi::graphics_engine::render_block::NvWaterHighEndRenderBlock::Draw_ADDRESS)]
fn nv_water_high_end_draw(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = NV_WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `this` and `rc` are the live block and render context the engine passed into `Draw`; the
    // closure re-invokes the original `Draw` trampoline.
    let handled = unsafe { nv_water_per_eye(this, rc, || detour.call(this, rc, info)) };
    if !handled {
        // The per-eye re-issue brackets the water surface with the `water_*` stages and the GPU probe;
        // bracket the plain path's single draw the same way so the two paths stay comparable.
        // SAFETY: `rc` is the live render context the engine passed into `Draw`.
        let pass = unsafe { (*rc).m_ActiveRenderPass };
        let water_surface = pass == RenderPassId::RP_WATER as i32;
        if water_surface {
            probes::point(PipelinePoint::BeforeWater);
            let surface = BlockScope::WaterSurface {
                pass,
                eye: draw_index(),
            };
            // SAFETY: the scope carries no pointer.
            unsafe { probes::block_scope(surface, || detour.call(this, rc, info)) };
            probes::point(PipelinePoint::AfterWater(draw_index()));
        } else {
            detour.call(this, rc, info);
        }
    }
}

/// The WaveWorks simulation step, suppressed on the second eye of an [`nv_water_per_eye`] re-issue.
///
/// Re-issuing the block's `Draw` re-drives everything in it, and this one call is the only part that
/// is not idempotent: it advances the simulation clock, blocks on the readback staging cursor, and
/// archives another displacement snapshot into the ring the CPU-side wave-height and buoyancy queries
/// read. Running it twice per frame would halve that ring's time span and pay the stall twice, for a
/// second kick at the same simulation time that produces the same water.
#[detour(address = jc3gi::graphics_engine::render_block::WaveWorksSimulationStep_ADDRESS)]
extern "system" fn wave_works_simulation_step(
    render_time: f64,
    gfx_context: *mut c_void,
    kick_id: *mut u64,
    simulation: *mut c_void,
    savestate: *mut c_void,
) {
    if SUPPRESS_SIMULATION_STEP.load(Ordering::Relaxed) {
        return;
    }
    with_wave_simulation_viewports(|| {
        WAVE_WORKS_SIMULATION_STEP.get().unwrap().call(
            render_time,
            gfx_context,
            kick_id,
            simulation,
            savestate,
        )
    });
}

/// The water reflection camera's projection parameters, sized to one eye under the double-wide
/// collapse.
///
/// [`WaterPatchManager::UpdateReflectionCamera`] copies the active camera's dimensions, field of view,
/// and clip distances, and the camera manager's aspect ratio, to the reflection camera, and its caller
/// rebuilds the projection from them right after. Under the double-wide collapse the dimensions and
/// the aspect ratio are the double-wide target's, so the reflection passes render twice an eye's
/// horizontal field into the reflection texture, while each eye's water run looks the texture up as
/// one eye's frame (the eye-local planar lookup). In its place this sets the same
/// parameters with the width and the aspect ratio halved, which is the camera an eye rendered on its
/// own reflects from.
///
/// It writes the final values directly rather than correcting the engine's after the call. The
/// reflection camera is live, and with the frame tail deferred the draw thread can be rendering the
/// previous frame's reflection passes from it while this runs; a correction after the call leaves a
/// window in which the camera holds the double-wide parameters with its projection marked dirty, and
/// with one the reflection kept the double-wide field for several seconds at a time.
#[detour(address = jc3gi::water_patch_manager::WaterPatchManager::UpdateReflectionCamera_ADDRESS)]
fn update_reflection_camera(this: *mut WaterPatchManager) {
    let eye_size = double_wide_active();
    // SAFETY: `this` is the live manager the engine passed in, whose caller has checked the
    // reflection camera pointer, and the camera manager is the live singleton.
    if !eye_size || !unsafe { set_eye_sized_reflection_camera(this) } {
        UPDATE_REFLECTION_CAMERA.get().unwrap().call(this);
    }
    // SAFETY: as above.
    unsafe { log_reflection_camera_change(this, eye_size) };
}

/// Log the reflection camera's projection parameters whenever they change, with the active camera's
/// dimensions they were derived from, so a reflection rendered at the wrong size can be traced to the
/// frame that sized it.
///
/// # Safety
///
/// `this` must be the live water patch manager, called from its update.
unsafe fn log_reflection_camera_change(this: *mut WaterPatchManager, eye_size: bool) {
    // SAFETY: per this function's contract; the camera manager is the live singleton.
    let (Some(camera), Some(active)) = (unsafe {
        (
            this.as_ref().and_then(|m| m.m_ReflectionCamera.as_ref()),
            CameraManager::get().and_then(|m| m.m_ActiveCamera.as_ref()),
        )
    }) else {
        return;
    };
    let applied = ReflectionCameraSize {
        width: camera.m_Width,
        height: camera.m_Height,
        aspect: camera.m_AspectRatio,
        eye_size,
    };
    let mut last = LAST_REFLECTION_CAMERA_SIZE.lock();
    if last.as_ref() != Some(&applied) {
        tracing::info!(
            target: "single_pass",
            width = applied.width,
            height = applied.height,
            aspect = applied.aspect,
            eye_size,
            active_width = active.m_Width,
            active_height = active.m_Height,
            // SAFETY: reads only the calling thread's own id.
            thread_id = unsafe { GetCurrentThreadId() },
            "water: reflection camera resized"
        );
        *last = Some(applied);
    }
}

/// The reflection camera's projection parameters as last logged.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ReflectionCameraSize {
    width: i32,
    height: i32,
    aspect: f32,
    eye_size: bool,
}

static LAST_REFLECTION_CAMERA_SIZE: Mutex<Option<ReflectionCameraSize>> = Mutex::new(None);

/// Give the reflection camera the active camera's parameters at one eye's width, or return `false`
/// without touching it when the manager, either camera, or the parameters are unusable.
///
/// # Safety
///
/// `this` must be the live water patch manager, called from its update.
unsafe fn set_eye_sized_reflection_camera(this: *mut WaterPatchManager) -> bool {
    // SAFETY: per this function's contract.
    let Some(camera) =
        (unsafe { this.as_ref() }).and_then(|m| unsafe { m.m_ReflectionCamera.as_mut() })
    else {
        return false;
    };
    // SAFETY: the camera manager is a process-lifetime singleton; its active camera is live.
    let Some(manager) = (unsafe { CameraManager::get() }) else {
        return false;
    };
    let Some(active) = (unsafe { manager.m_ActiveCamera.as_ref() }) else {
        return false;
    };
    let (width, height, aspect) = (
        active.m_Width / 2,
        active.m_Height,
        manager.m_AspectRatio * 0.5,
    );
    if width <= 0 || height <= 0 || !aspect.is_finite() || aspect <= 0.0 {
        return false;
    }
    // SAFETY: the setters only store their arguments and mark the projection dirty, which the
    // caller's `UpdateRender` rebuilds from. The field of view and the clip distances are the fields
    // the engine's own setters store, and the aspect-ratio setter marks the projection dirty after
    // them.
    unsafe {
        if (camera.m_Width, camera.m_Height) != (width, height) {
            camera.SetWidthHeight(width, height);
        }
        camera.m_FOVT1 = active.m_FOV;
        camera.m_Near = active.m_Near;
        camera.m_Far = active.m_Far;
        camera.SetAspectRatio(aspect);
    }
    true
}

/// The vertex constant buffer the water block types stage their screen-UV matrix into.
const CONSTANT_BUFFER: i32 = 1;

/// The first register of the high-end family's screen-UV matrix (`TypeConstants.ReflectionViewProj`).
const HIGH_END_REGISTER: u32 = 1;

/// The first register of the water-box family's screen-UV matrix (`cbWaterConsts.WaterConsts[4..7]`).
const BOX_REGISTER: u32 = 4;

/// Re-issue `draw` once per eye with the eye-half-biased screen-UV matrix, or return `false` when the
/// render context is unreadable, or the collapse intercept declines.
///
/// `view_projection` picks the render-context matrix the block type bakes the matrix from; this
/// recomputes the type's staged rows rather than intercepting them, because the type stages them once
/// per pass, well before the `Draw` being re-issued.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn per_eye(
    rc: *mut RenderContext,
    reg_offset: u32,
    view_projection: impl Fn(&RenderContext) -> [f32; 16],
    draw: impl FnMut(),
) -> bool {
    // SAFETY: `rc` is live per the caller contract.
    let Some(view_projection) = (unsafe { rc.as_ref() }).map(view_projection) else {
        return false;
    };
    // SAFETY: as above; `draw` is the block's original `Draw`.
    unsafe {
        screen_uv_cb_per_eye(
            rc,
            CONSTANT_BUFFER,
            reg_offset,
            screen_uv_matrix(view_projection),
            draw,
        )
    }
}

/// The rows the block type stages: `view_projection · TEX_BIAS`, in the engine's row-major storage.
///
/// Loading a row-major matrix into glam's column-major reading yields its transpose, and the
/// row-vector product `A · B` transposes to `Bᵀ · Aᵀ` -- so the factors swap and the result reads back
/// row-major unchanged.
fn screen_uv_matrix(view_projection: [f32; 16]) -> [f32; 16] {
    (glam::Mat4::from_cols_array(&TEX_BIAS) * glam::Mat4::from_cols_array(&view_projection))
        .to_cols_array()
}

/// The NDC→texture bias the water block types post-multiply their view-projection by: the standard
/// `xy · 0.5 + w · 0.5` with the depth row left alone, folded in on the CPU so the shader can divide
/// the interpolated result by `w` and sample directly. Row-major, row-vector convention (the
/// translation is the last row).
#[rustfmt::skip]
const TEX_BIAS: [f32; 16] = [
    0.5, 0.0, 0.0, 0.0,
    0.0, 0.5, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.5, 0.5, 0.0, 1.0,
];

/// Re-issue the WaveWorks block's `Draw` once per eye with that eye's camera, or return `false` when
/// the flag is off, the pass is one of the block's auxiliary passes, the per-eye camera is
/// unavailable, or the collapse intercept declines.
///
/// Every `NvWater*` permutation writes clip position from the model-view-projection
/// [`NvWaterHighEndRenderBlock::Setup`] bakes into the block type's *own* vertex/domain constant
/// buffer, and its hull and domain shaders carry the same epilogue -- none of them touch the global
/// `cb0` the collapse's shader rewrite works on, so no amount of `cb13`/viewport routing reaches
/// them. `Setup` builds that matrix from exactly two render-context fields, `m_View` and
/// `m_ProjectionF`, and those same two matrices are also handed to WaveWorks itself as the view and
/// projection its quadtree culls and picks patch LODs against.
///
/// So rather than reproject a constant in flight, this substitutes the *inputs*: it writes the eye's
/// view and projection into the render context, calls the block's own `Setup` to rebuild and re-upload
/// everything downstream of them, and re-issues `Draw` -- once per eye, each into that eye's half of
/// the double-wide target. The render context is restored afterwards.
///
/// What that does *not* restore is the block's own cached matrices and the shared constant buffer,
/// which are left holding the right eye's. Nothing reads them before the next `Setup`: the draw-list
/// walk calls `Setup` before the first `Draw` of a block-type run and again whenever the sort id
/// changes, and every `Draw` in between comes back through here and restages per eye anyway. The
/// pyxis definitions for [`NvWaterHighEndRenderBlock::Setup`] (neither buffer is written anywhere
/// else) and [`NvWaterHighEndRenderBlock::Draw`] (which restages nothing) record why.
///
/// The bound-shader gate is skipped ([`draw_per_eye_half_ignoring_bound_vs`]): the block binds its
/// programs inside `Draw`, so the shader bound when the gate would run is whatever drew before the
/// water.
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn nv_water_per_eye(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    mut draw: impl FnMut(),
) -> bool {
    if !crate::stereo::single_pass::nv_water_per_eye() {
        return false;
    }
    // SAFETY: `rc` is the live render context per the caller contract.
    let (pass, center_view) = unsafe { ((*rc).m_ActiveRenderPass, (*rc).m_View) };
    if NV_WATER_AUXILIARY_PASSES.contains(&pass) {
        return false;
    }
    let water_surface = pass == RenderPassId::RP_WATER as i32;
    let Some(eyes) = eye_cameras(center_view) else {
        return false;
    };
    // SAFETY: as above.
    let saved_projection = unsafe { (*rc).m_ProjectionF };

    let _restore = RenderContextRestore {
        rc,
        center_view,
        saved_projection,
    };

    if water_surface {
        probes::point(PipelinePoint::BeforeWater);
    }
    let replay = water_surface.then(begin_water_surface);

    draw_per_eye_half_ignoring_bound_vs(|eye| {
        if let Some(replay) = &replay {
            replay.begin_eye(eye);
        }
        // SAFETY: `rc` is live, and `this` is the live block whose `Setup` reads it. The two trailing
        // arguments are the draw-list sort ids, which this block's `Setup` override does not read.
        unsafe {
            (*rc).m_View = eyes[eye].view;
            (*rc).m_ProjectionF = eyes[eye].projection;
            SUPPRESS_SIMULATION_STEP.store(eye != 0, Ordering::Relaxed);
            (*this).Setup(rc, 0, 0);
        }
        let _camera = EyeGlobals::stage(eye);
        if water_surface {
            // SAFETY: the scope carries no pointer.
            unsafe { probes::block_scope(BlockScope::WaterSurface { pass, eye }, &mut draw) };
            probes::point(PipelinePoint::AfterWater(eye));
        } else {
            draw();
        }
    })
}

/// The passes whose [`NvWaterHighEndRenderBlock::Draw`] body is not the water surface: the compute
/// foam sub-pass, the wake prepass, and the painted-foam prepass. They render into the block's own
/// simulation and foam targets from their own viewports rather than into the scene, so the eye split
/// does not apply to them and `Setup` stages no view matrix for them either.
const NV_WATER_AUXILIARY_PASSES: [i32; 3] = [
    RenderPassId::PRE_RP_WATER_CS_PRE as i32,
    RenderPassId::PRE_RP_WATER_WAKES_PRE as i32,
    RenderPassId::PRE_RP_WATER_FOAM_PRE as i32,
];

/// Raised for the duration of the second eye's re-issue; see [`wave_works_simulation_step`].
static SUPPRESS_SIMULATION_STEP: AtomicBool = AtomicBool::new(false);

/// Restores the render context's view/projection and clears the simulation-suppression flag on drop,
/// so engine state is consistent even if the per-eye closure in [`nv_water_per_eye`] unwinds.
struct RenderContextRestore {
    rc: *mut RenderContext,
    center_view: Matrix4,
    saved_projection: Matrix4,
}

impl Drop for RenderContextRestore {
    fn drop(&mut self) {
        SUPPRESS_SIMULATION_STEP.store(false, Ordering::Relaxed);
        // SAFETY: `rc` is the live render context that was valid when the guard was constructed, and
        // the render thread still owns it during drop on the same unwind path.
        unsafe {
            (*self.rc).m_View = self.center_view;
            (*self.rc).m_ProjectionF = self.saved_projection;
        }
    }
}

/// The fragment `GlobalConstants` camera rows moved to one eye for one eye's draw, put back on drop.
/// The water pixel shaders measure the view vector of their reflection and specular terms from
/// them, and the engine stages them once per pass from the centred camera.
struct EyeGlobals;

impl EyeGlobals {
    fn stage(eye: usize) -> Self {
        eye_globals::stage(eye);
        Self
    }
}

impl Drop for EyeGlobals {
    fn drop(&mut self) {
        eye_globals::restore();
    }
}

/// One eye's substitute for the render context's camera matrices.
struct EyeCamera {
    view: Matrix4,
    projection: Matrix4,
}

/// The two eyes' view and projection matrices, derived from the collapse's centre view the same way
/// the `SetupRenderCamera` hook derives the double-draw path's per-eye camera
/// ([`EyeRenderParams::resolve`](jc3vr_view::vr::EyeRenderParams::resolve)). `None` when no VR frame is in
/// flight.
fn eye_cameras(center_view: Matrix4) -> Option<[EyeCamera; 2]> {
    let center_world = glam::Mat4::from(center_view).inverse();
    let eye = |eye| {
        jc3vr_view::vr::render_params(eye).map(|params| EyeCamera {
            view: Matrix4::from(params.resolve(center_world).inverse()),
            // The reverse-Z form, which is what the render camera's `m_ProjectionF` holds by the
            // time a render context is filled from it, under either projection convention.
            projection: params.projection_reverse_z,
        })
    };
    Some([eye(0)?, eye(1)?])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stereo::single_pass::eye_half_screen_uv_rows;

    /// The composed transform maps a clip position into the eye's half of the double-wide target: the
    /// screen-UV matrix's own NDC→UV bias, then the eye-half bias the per-eye re-issue applies.
    #[test]
    fn eye_half_bias_maps_ndc_into_the_eyes_half() {
        // A clip position with a non-unit `w`, to catch a bias applied to the post-divide `u` instead
        // of to the projective components.
        let clip = glam::Vec4::new(0.5, -0.25, 0.75, 2.0);
        let base = screen_uv_matrix(glam::Mat4::IDENTITY.to_cols_array());

        for eye in 0..2 {
            let rows = eye_half_screen_uv_rows(base, eye);
            let row = |k: usize| glam::Vec4::from_slice(&rows[k * 4..k * 4 + 4]);
            // The row-vector product the vertex shader's multiply-add chain computes.
            let projective: glam::Vec4 = (0..4).map(|k| clip[k] * row(k)).sum();
            // The vertex shader packs `(x, y, w)` into `TEXCOORD1` and the pixel shader divides the
            // first two by the third.
            let uv = glam::Vec2::new(projective.x, projective.y) / projective.w;

            let ndc = glam::Vec2::new(clip.x, clip.y) / clip.w;
            let expected_u = ((ndc.x * 0.5 + 0.5) + eye as f32) * 0.5;
            assert!((uv.x - expected_u).abs() < 1e-6, "eye {eye}: {uv:?}");
            assert!(
                (uv.y - (ndc.y * 0.5 + 0.5)).abs() < 1e-6,
                "eye {eye}: {uv:?}"
            );
        }
    }

    /// The screen-UV matrix of a real view-projection maps a world point to the same UV as projecting
    /// it and then applying the NDC→UV bias, so the row-major factor swap in [`screen_uv_matrix`]
    /// composes in the order the block type stages.
    #[test]
    fn screen_uv_matrix_projects_then_biases() {
        // glam's column-major reading of the engine's row-major, row-vector storage is the
        // column-vector matrix of the same transform, so `from_cols_array(storage) * world` is the
        // product the vertex shader computes.
        let view_projection = (glam::Mat4::perspective_rh(1.2, 1.5, 0.1, 1000.0)
            * glam::Mat4::look_at_rh(
                glam::Vec3::new(3.0, 2.0, 1.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            ))
        .to_cols_array();
        let world = glam::Vec4::new(0.4, -0.3, -0.2, 1.0);

        let clip = glam::Mat4::from_cols_array(&view_projection) * world;
        let ndc = glam::Vec2::new(clip.x, clip.y) / clip.w;

        let rows = screen_uv_matrix(view_projection);
        let projective = glam::Mat4::from_cols_array(&rows) * world;
        let uv = glam::Vec2::new(projective.x, projective.y) / projective.w;

        assert!(
            (uv - (ndc * 0.5 + 0.5)).length() < 1e-5,
            "{uv:?} vs {ndc:?}"
        );
    }
}
