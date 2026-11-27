//! Render-thread detours for single-pass stereo (see `docs/mod/stereo/single-pass-stereo.md` and
//! [`crate::stereo::single_pass`]).
//!
//! `SetAllGlobalShaderProgramConstants` is the per-pass seam. After the engine refreshes and uploads
//! the global vertex constants (`m_VPGlobalConstData` to `cb0`), the detour uploads them again with
//! the centre camera in the terrain patch prep passes ([`crate::stereo::terrain_prep`]), mirrors the
//! current view's per-eye rows into the mod-owned `cb13` and binds it at `b13`, and rewrites the eye
//! camera buffer from the rows the engine just staged. It runs at the cadence of the engine's own
//! `cb0` upload, so `cb13` always matches whatever view is current: the G-buffer eye view, but also
//! the shadow and reflection views that reuse the same model shaders. [`upload_global_constants`]
//! repeats the engine's upload for a caller that changes staged rows mid-pass.
//!
//! The render-block detours below cover the blocks that bake their view-projection into a constant
//! buffer inside their own `Draw` (bark, foliage, occluder), across draw kinds that can't be
//! instance-doubled. When the collapse is active, the block's `Draw` is re-issued once per eye with
//! that baked constant reprojected by the eye's `M_eye`
//! ([`reproject_baked_cb_per_eye`](crate::stereo::single_pass::reproject_baked_cb_per_eye)); otherwise
//! it draws once, unchanged. The bark and foliage blocks are
//! first offered to [`vegetation_once`](crate::stereo::single_pass::vegetation_once), which draws
//! them once for both eyes when it can, and fall back to the re-issue when it cannot.
//!
//! The two vegetation families' vertex shaders are also declined by the `cb0` remap at creation
//! ([`baked_cb_block_owns_vs`](crate::stereo::single_pass::baked_cb_block_owns_vs)), because the remap
//! would claim them on a `cb0[4]` reference that is not their position path and then leave both eyes
//! on the collapsed centre view. The decline and the intercept only make sense together.
//!
//! Two per-pass setups stage a width from the render context that is double-wide under the collapse,
//! and the detours hold it at one eye's for the call: the tree impostors' pixel size
//! ([`EyeDisplayWidth`]) and the static light sprites' inverse screen width ([`EyeLightWidth`]). Both
//! stay one instance-doubled draw. The water reflection plane's `Draw` runs inside a capture probe
//! block scope.

use std::sync::atomic::{AtomicPtr, Ordering};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    render_block::{
        RBIInfo, RenderBlockBark, RenderBlockFoliage, RenderBlockOccluder, RenderBlockTypeLights,
        RenderBlockWaterReflectionPlane, TreeImpostorRBType,
    },
    render_engine::RenderEngine,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    probes,
    probes::{BlockScope, PipelinePoint},
    stereo::single_pass::{BoundVsGate, EyeDisplayWidth, EyeLightWidth},
};

/// Where a render block's baked view-projection lives, and how its intercept is gated: the values
/// that vary between the render-block detours below, pinned in one place so a block's buffer layout is
/// stated once and stays greppable.
struct BakedCbBlock {
    /// The vertex constant buffer the block bakes its view-projection into.
    cb_index: i32,
    /// The first of the four consecutive registers the view-projection occupies within `cb_index`.
    reg_offset: u32,
    /// Whether the re-issue must check for a patched vertex shader before running (see
    /// [`BoundVsGate`]).
    gate: BoundVsGate,
    /// Whether the block may be drawn once for both eyes instead
    /// ([`vegetation_once`](crate::stereo::single_pass::vegetation_once)), falling back to the
    /// re-issue.
    once: bool,
}

/// Runs the shared re-issue dance for a render-block detour: draw it once for both eyes when the block
/// allows it and the single draw's preconditions hold, else hand the original `Draw` off to
/// [`reproject_baked_cb_per_eye`] so it runs once per eye against a reprojected constant; if the
/// re-issue declines, fall back to a single unmodified `Draw`.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the engine passed to the detour, and `draw` must invoke that
/// detour's original trampoline with the same arguments the engine passed in.
unsafe fn intercept_render_block(
    block: &BakedCbBlock,
    rc: *mut RenderContext,
    mut draw: impl FnMut(),
) {
    // SAFETY: forwarded from this function's own contract.
    if block.once
        && unsafe { crate::stereo::single_pass::vegetation_once::draw_block(rc, &mut draw) }
    {
        return;
    }
    // SAFETY: forwarded from this function's own contract.
    let handled = unsafe {
        crate::stereo::single_pass::reproject_baked_cb_per_eye(
            rc,
            block.cb_index,
            block.reg_offset,
            block.gate,
            &mut draw,
        )
    };
    if !handled {
        draw();
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS_BINDER)
        .with_static_binder(&PROXY_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_BARK_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_BARK_DRAW_Z_BINDER)
        .with_static_binder(&RENDER_BLOCK_FOLIAGE_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_FOLIAGE_DRAW_Z_BINDER)
        .with_static_binder(&RENDER_BLOCK_OCCLUDER_DRAW_Z_BINDER)
        .with_static_binder(&TREE_IMPOSTOR_TYPE_SETUP_BINDER)
        .with_static_binder(&LIGHTS_TYPE_SETUP_BINDER)
}

/// The tree impostors' per-pass setup stages their `PixelSize` from the render context's display
/// width; under the double-wide collapse that is held at one eye's for the call
/// ([`EyeDisplayWidth`]), so the card footprints match an eye-sized render. The impostors themselves
/// stay one instance-doubled draw.
#[detour(address = jc3gi::graphics_engine::render_block::TreeImpostorRBType::Setup_ADDRESS)]
fn tree_impostor_type_setup(this: *mut TreeImpostorRBType, rc: *mut RenderContext) {
    // SAFETY: `rc` is the live render context the engine passed in, and the guard drops before this
    // detour returns.
    let _width = unsafe { EyeDisplayWidth::stage(rc) };
    TREE_IMPOSTOR_TYPE_SETUP.get().unwrap().call(this, rc);
}

/// The static light sprites' per-pass setup stages their inverse screen width from the back buffer's
/// width; under the double-wide collapse its upload is held at one eye's width for the call
/// ([`EyeLightWidth`]), so the sprites routed to each eye half keep an eye-sized render's width. The
/// sprites themselves stay one instance-doubled draw.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockTypeLights::Setup_ADDRESS)]
fn lights_type_setup(this: *const RenderBlockTypeLights, rc: *mut RenderContext) {
    let _width = EyeLightWidth::stage();
    LIGHTS_TYPE_SETUP.get().unwrap().call(this, rc);
}

#[detour(address=jc3gi::graphics_engine::render_block::RenderBlockWaterReflectionPlane::Draw_ADDRESS)]
fn proxy_draw(
    this: *const RenderBlockWaterReflectionPlane,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let proxy = BlockScope::WaterReflectionProxy {
        block: this,
        rc: rc.cast_const(),
    };
    // SAFETY: `this` and `rc` are the live block and render context the engine passed into `Draw`.
    unsafe { probes::block_scope(proxy, || PROXY_DRAW.get().unwrap().call(this, rc, info)) };
}

/// The tree-trunk/branch block bakes its world-view-projection into vertex `cb1` registers 0..3.
const BARK: BakedCbBlock = BakedCbBlock {
    cb_index: 1,
    reg_offset: 0,
    gate: BoundVsGate::Owned,
    once: true,
};

/// The grass/foliage block bakes its view-projection into vertex `cb2` registers 4..7 (register 0 is
/// the per-draw world matrix).
const FOLIAGE: BakedCbBlock = BakedCbBlock {
    cb_index: 2,
    reg_offset: 4,
    gate: BoundVsGate::Owned,
    once: true,
};

/// The occluder depth-prime block's non-instanced path bakes its world-view-projection into vertex
/// `cb1` registers 0..3.
///
/// Its shaders read no `cb0[4]`, so the remap never claims them and nothing declines them at creation;
/// the bound-shader check stays as the guard against a patched neighbour's geometry being re-issued.
const OCCLUDER: BakedCbBlock = BakedCbBlock {
    cb_index: 1,
    reg_offset: 0,
    gate: BoundVsGate::Checked,
    once: false,
};

/// Reprojects the tree-trunk/branch block's baked `cb1` view-projection ([`BARK`]) per eye. Covers
/// all three of its draw kinds (plain, CPU-instanced, GPU-indirect) via the whole-`Draw` re-issue.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockBark::Draw_ADDRESS)]
fn render_block_bark_draw(
    this: *const RenderBlockBark,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_BARK_DRAW.get().unwrap();
    // SAFETY: `this`/`rc`/`info` are the live pointers the engine passed in; the closure re-invokes the
    // original `Draw` trampoline.
    unsafe { intercept_render_block(&BARK, rc, || detour.call(this, rc, info)) };
}

/// The tree-trunk/branch depth-and-velocity pass bakes the same `cb1` view-projection; reproject it too.
///
/// Only the current-frame matrix at `cb1[0..3]` is reprojected. The pass also bakes the
/// *previous* frame's world-view-projection at `cb1[5..8]`, which the shader differences against the
/// current one to produce a velocity vector, and that copy is left centred -- so bark's motion
/// vectors carry the centre view's parallax rather than the eye's, and SMAA and FSR reproject bark
/// slightly wrongly. Correcting it needs the previous frame's `M_eye` retained and a second armed
/// range; the error is a temporal-sharpening artefact rather than a geometric one, and it belongs to
/// the per-eye motion-vector path as a whole rather than to this block.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockBark::DrawZ_ADDRESS)]
fn render_block_bark_draw_z(
    this: *const RenderBlockBark,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_BARK_DRAW_Z.get().unwrap();
    // SAFETY: as above.
    unsafe { intercept_render_block(&BARK, rc, || detour.call(this, rc, info)) };
}

/// Reprojects the grass/foliage block's baked `cb2` view-projection ([`FOLIAGE`]) per eye.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockFoliage::Draw_ADDRESS)]
fn render_block_foliage_draw(
    this: *const RenderBlockFoliage,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_FOLIAGE_DRAW.get().unwrap();
    // SAFETY: as above.
    unsafe { intercept_render_block(&FOLIAGE, rc, || detour.call(this, rc, info)) };
}

/// The grass/foliage depth, shadow, and velocity pass stages the same `cb2[4..7]` view-projection as
/// [`render_block_foliage_draw`], so it takes the same reprojection.
///
/// It has to: the depth prepass and the colour pass must agree on where a blade of grass is, and the
/// colour pass is per-eye. A prepass left at the centre view primes depth for geometry neither eye
/// draws there, and the colour fragments that follow are rejected against it.
///
/// The velocity permutation additionally stages the previous frame's offset view-projection at
/// `cb2[11..14]`, which is **not** reprojected -- the same limitation as bark's `DrawZ`, for the
/// same reason (it would need the previous frame's `M_eye` retained and a second armed range). The
/// shadow and reflective-shadow permutations render from a light view outside the G-buffer range,
/// where the intercept stands down and the draw is issued once, as it must be.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockFoliage::DrawZ_ADDRESS)]
fn render_block_foliage_draw_z(
    this: *const RenderBlockFoliage,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_FOLIAGE_DRAW_Z.get().unwrap();
    // SAFETY: as above.
    unsafe { intercept_render_block(&FOLIAGE, rc, || detour.call(this, rc, info)) };
}

/// The occluder depth-prime block's non-instanced path bakes its world-view-projection into vertex
/// `cb1` registers 0..3; reproject it per eye so each eye's depth is primed with its own projection.
///
/// This is conditional on the block taking that path: its instanced path (selected by the engine's
/// `gfx.occluders.use_instancing` cvar) reads `cb0`'s `m_VPGlobals` with per-instance world rows and
/// bakes no `cb1` matrix, so there is nothing to reproject and both eyes would be primed from the
/// centred view. Nothing in the mod writes that cvar, so the precondition is unenforced; the
/// re-issue logs when it finds nothing to reproject (see `REPROJECT_FIRED`), which is what surfaces
/// the condition.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockOccluder::DrawZ_ADDRESS)]
fn render_block_occluder_draw_z(
    this: *const RenderBlockOccluder,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_OCCLUDER_DRAW_Z.get().unwrap();
    // SAFETY: as above.
    unsafe { intercept_render_block(&OCCLUDER, rc, || detour.call(this, rc, info)) };
}

/// Upload the render engine's global constant staging blocks again, into the context the engine
/// last uploaded them to on this dispatch, without the per-pass work the detour hangs off the
/// engine's own upload. For a caller that has changed staged rows mid-pass. `false` when the engine
/// has not uploaded yet.
pub(crate) fn upload_global_constants(engine: &mut RenderEngine) -> bool {
    let ctx = GLOBALS_CONTEXT.load(Ordering::Relaxed);
    let Some(upload) = SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS.get() else {
        return false;
    };
    if ctx.is_null() {
        return false;
    }
    upload.call(engine, ctx);
    true
}

/// The render context of the engine's most recent global constant upload. The engine's render
/// contexts live for the process, and only the render thread uploads to them.
static GLOBALS_CONTEXT: AtomicPtr<RenderContext> = AtomicPtr::new(std::ptr::null_mut());

#[detour(
    address = jc3gi::graphics_engine::render_engine::RenderEngine::SetAllGlobalShaderProgramConstants_ADDRESS
)]
fn set_all_global_shader_program_constants(this: *mut RenderEngine, ctx: *const RenderContext) {
    GLOBALS_CONTEXT.store(ctx.cast_mut(), Ordering::Relaxed);
    SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(this, ctx);
    // The terrain patch prep takes the centre camera in an eye-at-a-time dispatch; the upload above
    // carried the eye's, so upload again with the centre's and put the eye's back.
    if let (Some(engine), Some(rc)) = unsafe { (this.as_mut(), ctx.as_ref()) }
        && let Some(eye) = crate::stereo::terrain_prep::stage_centre_camera(engine, rc)
    {
        upload_global_constants(engine);
        crate::stereo::terrain_prep::restore(engine, eye);
    }
    let cb13_uploaded = crate::stereo::single_pass::active()
        && unsafe { this.as_ref() }.is_some_and(crate::stereo::single_pass::mirror_and_bind_cb13);
    if let Some(engine) = unsafe { this.as_ref() } {
        crate::stereo::single_pass::eye_camera::sync(engine);
    }
    if let Some(probes) = probes::armed()
        && let (Some(engine), Some(ctx)) = unsafe { (this.as_ref(), ctx.as_ref()) }
    {
        probes.point(PipelinePoint::GlobalConstants {
            engine,
            ctx,
            cb13_uploaded,
        });
    }
}
