//! Single-pass stereo, the default rendering mode while a VR session runs: render the G-buffer
//! geometry once, emitting both eyes via instancing + `SV_ViewportArrayIndex` routing into a
//! double-wide render target, instead of the double-draw (two full `game.Draw` walks, one per eye).
//! See `docs/mod/stereo/single-pass-stereo.md` for the design.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set;
//! - which **transform** each vertex shader gets ([`decide_vs_transform`]), decided from its engine
//!   name at `CreateVertexProgram` time and remembered against its bytecode ([`remember_vs_transform`])
//!   so the D3D-level re-create path, which sees no name, applies the same decision instead of
//!   guessing a different one.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) runs under [`jc3vr_view::stereo::config::StereoConfig::single_pass`] and the per-step flags
//! beside it.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};

use dxbc_stereo::DxbcError;
use jc3gi::{
    graphics_engine::{
        draw::SetVertexProgramConstants,
        graphics_engine::{GraphicsEngine, HContext_t, RenderContext},
        render_block::RenderBlockTerrainDetail,
        render_engine::{RenderEngine, RenderPassId},
    },
    types::math::{Matrix4, Vector4},
};
use jc3vr_core::{hooks::suspend, util::bump};
use parking_lot::Mutex;
use windows::{
    Win32::{
        Graphics::Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE,
            D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA,
            D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::{IUnknown, Interface},
};

// The ambient-occlusion generation pass's rewrite and its per-eye rows; reached by the shader
// creation detour and the SSAO block.
pub(crate) mod ambient_occlusion;

mod cb13;
mod config_snapshot;
mod detour_lifecycle;
// The draw detours' own cost, which a benchmark run arms.
pub mod detour_timing;
mod draw_detours;
pub mod eye_camera;
pub(crate) mod eye_globals;
mod frame_diagnostics;
pub mod hw_instanced;
mod impostor_pixel_size;
mod instanced_exposure;
mod per_eye_reissue;
mod pinned_eye;
pub mod post_filters;
// The capture harness's probes into the draw path.
pub mod probes;
// The ambient-occlusion volumes' pixel-shader rewrite and its per-eye rows; reached by the shader
// creation detour and the volume pass's draw walk.
pub(crate) mod ao_volumes;
// The screen-space reflection ray march's rewrite and its per-eye rows; reached by the shader creation
// detour and the reflection block.
pub(crate) mod screen_space_reflection;
mod shader_detours;
pub mod shader_identity;
mod shader_policy;
mod static_lights;
mod sun;
pub mod vegetation_once;
mod viewport;
pub(crate) mod viewport_record;
mod water_fragment_state;
pub(crate) mod water_pixel_variants;

pub use cb13::*;
pub use config_snapshot::*;
use detour_lifecycle::{DETOUR_INSTALL, DetourSlot, PIXEL_SHADER_EVER_ENABLED};
pub use detour_lifecycle::{
    com_detours_installed, ensure_pixel_shader_detour, ensure_viewport_detours,
    vertex_identity_detour_ready,
};
use draw_detours::*;
pub use frame_diagnostics::*;
pub use impostor_pixel_size::EyeDisplayWidth;
pub use instanced_exposure::*;
pub(crate) use jc3vr_view::stereo::engine_context::EngineContext;
use jc3vr_view::stereo::layout::{Capability, capability, collapse_active};
pub use per_eye_reissue::*;
pub(crate) use pinned_eye::tag_pinned_eye;
pub use probes::RawDrawKind;
pub(crate) use probes::{WaterGpuProbe, begin_water_input_capture, reflection_producer};
pub use shader_detours::*;
pub use shader_policy::*;
pub use static_lights::EyeLightWidth;
pub use sun::*;
pub use viewport::*;
pub(crate) use water_fragment_state::begin_water_surface;

/// The live `cb13` rows the remap and reprojection blocks occupy (the clip-origin rows after them
/// are left out).
pub fn live_cb13_rows() -> Option<[Vector4; dxbc_stereo::STEREO_REPROJ_CB_ROWS as usize]> {
    let rows = CB13.lock().live_rows()?;
    Some(std::array::from_fn(|row| rows[row]))
}

/// Whether single-pass rendering should actually run this frame: the master switch is on, a VR
/// session is running, the census-only dry-run is off, the device supports viewport routing, and the
/// draw-path COM detours did not fail to install.
/// The VS-substitution and cb13 paths gate on this; when it is false the double-draw path is left
/// untouched. The shader set follows it: the frame driver re-creates the shaders whenever it
/// changes ([`crate::hooks::graphics_engine::shader::reconcile_single_pass`]).
pub fn active() -> bool {
    // Go inert the instant eject begins: the render thread keeps running through the whole teardown,
    // so an ungated single-pass path would race the hook uninstall and the D3D-resource release (the
    // same crash-on-uninject class already fixed for `vr::update` -- see `jc3vr_core::is_shutting_down`).
    if jc3vr_core::is_shutting_down() {
        return false;
    }
    let flags = config_flags();
    flags.has(Flag::SinglePass)
        && capability() == Capability::Supported
        && !detour_lifecycle::com_detours_failed()
}

/// Whether the engine's per-dispatch copy of the reflection-proxy depth into the effect
/// depth-history slots (`GraphicsEngine::m_EffectInfo`) is skipped: whenever single-pass is
/// [`active`]. Nothing in the game reads those textures (see the pyxis def), so the copy is a
/// whole-surface `CopyResource` at the render size spent on nothing. Only the copy is skipped; the
/// slots' lifecycle state advances as the engine drives it.
pub(crate) fn skip_unread_effect_depth_copy() -> bool {
    active()
}

/// The terrain block types whose hull programs cull against the baked offset view-projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerrainCullBlock {
    /// The base terrain tiles, drawn once for both eyes on the eye-lane path.
    Base,
    /// The terrain patch system's patches, re-issued per eye on the pinned-eye path.
    Patch,
}

/// Whether a terrain bake made now should cull against the projection bounding both eyes: under the
/// collapse, inside the G-buffer range, with `single_pass.terrain_cull_both_eyes` on and the domain
/// programs of `block` reading each eye's own rows, so that only the hull's cull reads the baked
/// offset view-projection. See
/// [`SinglePassConfig::terrain_cull_both_eyes`](jc3vr_view::stereo::config::SinglePassConfig::terrain_cull_both_eyes).
pub(crate) fn terrain_cull_both_eyes(block: TerrainCullBlock) -> bool {
    let flags = config_flags();
    let domain_reads_eye_rows = flags.has(Flag::Terrain)
        && match block {
            TerrainCullBlock::Base => true,
            TerrainCullBlock::Patch => flags.has(Flag::TerrainPatchPerEye),
        };
    collapse_active()
        && in_gbuffer_range()
        && flags.has(Flag::TerrainCullBothEyes)
        && domain_reads_eye_rows
}

/// Whether the WaveWorks water blocks take their per-eye re-issue
/// ([`nvwater_per_eye`](jc3vr_view::stereo::config::SinglePassConfig::nvwater_per_eye)). Read from
/// the frame's config snapshot, so the block `Draw` detour, which fires for every draw of its type
/// whether or not single-pass is on, costs a relaxed load rather than a mutex acquisition.
pub fn nv_water_per_eye() -> bool {
    config_flags().has(Flag::NvWater)
}

/// Vertex-shader name prefixes whose draws a baked-view-projection block intercept owns end to end,
/// and whether the intercept is the WaveWorks water's, which [`nv_water_per_eye`] gates. Names come from `CreateVertexProgramParams.m_Name`; matched by
/// prefix to cover each family's permutations, all of which are issued by the block's `Draw`/`DrawZ`.
///
/// The occluder is deliberately absent: its shader has no `cb0[4]` reference, so the remap never
/// claims it and there is nothing to decline.
const BAKED_CB_VS_NAME_PREFIXES: &[(&str, bool)] = &[
    ("vegetationbark", false),
    ("vegetationfoliage", false),
    ("nvwater", true),
];

/// Whether a baked-view-projection block intercept owns this vertex shader, so the `cb0` remap must
/// leave it pristine.
///
/// Every vegetation vertex shader that reads `cb0` reads **only** `cb0[4]`, the camera world position
/// -- the foliage family as the world-space origin of its wind-noise lookup, the bark family as the
/// offset paired with a view-projection baked into `cb1`. Neither takes its clip position from `cb0`
/// (`dcl_constantbuffer CB0[5]` cannot even address the view-projection rows), so the remap gives them
/// no per-eye clip: both eyes keep the collapsed centre view, and near geometry rendered at zero
/// disparity reads as swimming against the parallaxed world around it. Being remapped also costs them
/// the fix: the intercept that *does* reproject their baked matrix stands down for a patched shader
/// ([`reproject_baked_cb_per_eye`]), because a patched shader is supposed to be producing both eyes
/// already.
///
/// So the two go together: the block's `Draw`/`DrawZ` is re-issued per eye with the baked matrix
/// reprojected, and its shaders are declined here. The WaveWorks water's pair follows its switch:
/// with it off both halves revert and the family is remapped.
///
/// The bytecode is the final say, not the name: a permutation that really does read the global
/// view-projection is left to the remap, so a future bundle that moves one of these families onto
/// `cb0` does not silently lose its position path.
///
/// The decline is recorded as [`VsTransform::None`] against the blob, so the D3D-level re-create path
/// honours it too -- that path has no name to decline by, and left to itself it would remap the very
/// shaders this declined.
pub fn baked_cb_block_owns_vs(name: Option<&str>, code: &[u8]) -> bool {
    let reads = dxbc_stereo::reads_global_view_projection(code);
    baked_cb_vs_owned(name, reads.is_ok_and(|reads| !reads), nv_water_per_eye())
}

fn baked_cb_vs_owned(
    name: Option<&str>,
    no_global_view_projection: bool,
    nv_water_per_eye: bool,
) -> bool {
    let Some(name) = name else {
        return false;
    };
    BAKED_CB_VS_NAME_PREFIXES
        .iter()
        .any(|&(prefix, nv_water)| name.starts_with(prefix) && (!nv_water || nv_water_per_eye))
        && no_global_view_projection
}

/// The eye whose viewport + view-projection the collapse UI overlays (HUD panel, egui panel) should
/// currently draw with, so a head/world-locked quad lands at the correct 3D spot in each eye instead
/// of being drawn once, stretched, across the double-wide target. Set around each eye's overlay draw
/// by `render_engine_post_draw`; [`NO_UI_EYE`] means "not drawing a collapse overlay".
static COLLAPSE_UI_EYE: AtomicUsize = AtomicUsize::new(NO_UI_EYE);
const NO_UI_EYE: usize = usize::MAX;

/// Select the eye for the collapse UI overlay draws (`Some(0)`/`Some(1)`), or clear it (`None`).
pub fn set_collapse_ui_eye(eye: Option<usize>) {
    COLLAPSE_UI_EYE.store(eye.unwrap_or(NO_UI_EYE), Ordering::Relaxed);
}

/// The eye-half viewport and per-eye **full** view-projection for the current collapse UI overlay
/// draw, or `None` when not drawing one (or not collapsed). The HUD/egui-panel quad renderer uses
/// this to draw each overlay into one eye's half with that eye's own VP.
pub fn collapse_ui_eye_override() -> Option<(D3D11_VIEWPORT, Matrix4)> {
    let eye = COLLAPSE_UI_EYE.load(Ordering::Relaxed);
    if eye == NO_UI_EYE || !collapse_active() {
        return None;
    }
    let full = viewport_record::scene_viewport()?;
    let half = full.Width / 2.0;
    let viewport = D3D11_VIEWPORT {
        TopLeftX: full.TopLeftX + eye as f32 * half,
        Width: half,
        ..full
    };
    // The UI draws after FSR has resolved the eyes, so its view-projection leaves out FSR's jitter.
    let center_transform = jc3vr_view::stereo::STEREO_STATE.lock().center_transform?;
    let params = jc3vr_view::vr::render_params(eye)?;
    let eye_world = params.resolve(glam::Mat4::from(center_transform));
    let view_projection = params.unjittered_projection_reverse_z() * eye_world.inverse();
    Some((viewport, Matrix4::from(view_projection)))
}

/// Set the immediate-context viewport via the original (un-detoured) `RSSetViewports`, so a mod
/// overlay can bind one eye's half of the double-wide target without the collapse viewport detour
/// dup'ing it to full width. `context` is the immediate context.
pub fn set_ui_viewport_raw(context: &ID3D11DeviceContext, viewport: &D3D11_VIEWPORT) {
    if let Some(detour) = RS_SET_VIEWPORTS.get() {
        // One slot: slot 1 is left unbound, so the slots are no longer uniform.
        set_viewport_slots_uniform(false);
        // SAFETY: `context` is the live immediate context; the trampoline is the original function.
        unsafe { detour.call(context.as_raw(), 1, std::slice::from_ref(viewport).as_ptr()) };
        viewport_record::forget_bound();
    }
}

/// The per-eye **full** (translation-carrying) world→clip view-projection, matching the render
/// camera's `m_ViewProjectionF` for that eye, FSR's sub-pixel jitter included -- for the scene's own
/// per-eye passes in the collapse, where the render camera stays centered. What draws after FSR
/// takes [`collapse_ui_eye_override`]'s, which leaves the jitter out. `None` if the centre transform
/// or per-eye params are unavailable.
pub fn full_eye_view_projection(eye: usize) -> Option<Matrix4> {
    let center_transform = jc3vr_view::stereo::STEREO_STATE.lock().center_transform?;
    let params = jc3vr_view::vr::render_params(eye)?;
    let eye_world = params.resolve(glam::Mat4::from(center_transform));
    let vp = glam::Mat4::from(params.projection_reverse_z) * eye_world.inverse();
    Some(Matrix4::from(vp))
}

/// Marks the render thread as inside the G-buffer geometry pass range
/// (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`) until the returned guard drops. The dual-eye viewport split and
/// instance doubling apply only here -- so shadow/lighting/post passes, which reuse the same patched
/// shaders but are not double-wide, keep the identical-viewport behaviour.
///
/// A guard rather than a matching pair of calls: the range wraps a re-entrant engine call, and any
/// non-local exit from it (a panic unwinding through the detour, an early return added later) that
/// skipped the clear would leave the flag raised for the rest of the session -- after which *every*
/// shadow and reflection draw is instance-doubled and eye-split.
#[must_use = "the G-buffer range ends when the guard is dropped"]
pub fn enter_gbuffer_range() -> GBufferRange {
    IN_GBUFFER_RANGE.store(true, Ordering::Relaxed);
    GBufferRange(())
}

/// Holds [`in_gbuffer_range`] true; see [`enter_gbuffer_range`].
pub struct GBufferRange(());

impl Drop for GBufferRange {
    fn drop(&mut self) {
        // The guard is the only writer that should ever lower the flag. Finding it already down means
        // something else cleared it while the range was still running -- every draw between that point
        // and here was treated as out-of-range -- so count it rather than losing it in the clear.
        if !IN_GBUFFER_RANGE.swap(false, Ordering::Relaxed) {
            RANGE_TORN.fetch_add(1, Ordering::Relaxed);
        }
        // The per-eye matrices belong to the range that just ended; see [`clear_gbuffer_range`].
        *CURRENT_M_EYE.lock() = None;
        // The collapse's per-draw split ([`ensure_collapse_viewport`]) leaves the two slots holding
        // different eye halves, and that state outlives the range: everything drawn between here and
        // the next engine viewport bind would route its odd-parity instances into the other eye's half.
        // Put the slots back to a single region now, which also covers the draws nothing detours (the
        // GPU-indirect ones) -- the per-draw repair cannot see those.
        if collapse_active() {
            unify_viewport_slots();
        }
        // The pixel shaders' per-eye camera rows belong to the range too.
        eye_camera::range_ended();
    }
}

/// Force the range closed, whether or not a guard is live, so a range left open by a torn-down or
/// interrupted dispatch cannot bleed into the next one. See [`begin_frame`] for the caller.
fn clear_gbuffer_range() {
    IN_GBUFFER_RANGE.store(false, Ordering::Relaxed);
    eye_camera::range_ended();
    // The per-eye matrices belong to the range that just ended. Dropping them means a re-issue that
    // somehow runs outside a range -- or in a later frame where `compute_dual_eye_rows` declined to
    // publish -- reprojects with nothing rather than with a stale head pose.
    *CURRENT_M_EYE.lock() = None;
}

fn in_gbuffer_range() -> bool {
    IN_GBUFFER_RANGE.load(Ordering::Relaxed)
}

/// The render pass currently being walked, published by the `RenderPass::DoDraw` detour so the draw
/// detours -- which see only a D3D context -- can tell which pass a draw belongs to. [`NO_PASS`]
/// stands for "outside any pass"; no real id collides with it (`m_Index` is a byte and the engine's
/// highest real pass is [`RenderPassId::RP_POSTEFFECTS`]).
static CURRENT_PASS: AtomicU8 = AtomicU8::new(NO_PASS);

/// A pass id outside any real pass: the sentinel [`CURRENT_PASS`] holds before the first `DoDraw`
/// and while a draw belongs to no walkable pass. Deliberately larger than any byte-range pass id
/// the engine can publish.
const NO_PASS: u8 = 0xFF;

/// Publish the pass being drawn for the duration of one `DoDraw`. Returns the previous value so the
/// caller restores it rather than clearing, since a block-level re-issue can nest one pass inside
/// another.
///
/// Takes the engine's `m_Index` in its own `i16` form; an id outside the byte range is not a pass this
/// module can classify, so it reads as [`NO_PASS`] rather than truncating into a real pass's slot.
pub fn set_current_pass(pass: i16) -> u8 {
    let pass = u8::try_from(pass).unwrap_or(NO_PASS);
    // A pass boundary: whatever the slots held, the next routed draw binds its viewports afresh.
    viewport_record::forget_bound();
    CURRENT_PASS.swap(pass, Ordering::Relaxed)
}

/// Restore a pass id previously returned by [`set_current_pass`].
pub fn restore_current_pass(pass: u8) {
    CURRENT_PASS.store(pass, Ordering::Relaxed);
}

pub fn current_pass_id() -> u8 {
    CURRENT_PASS.load(Ordering::Relaxed)
}

/// Per-pass tally of non-indexed (`Draw`, slot 13) submissions seen inside a range while collapsed.
///
/// This exists to replace inference with measurement. Which passes actually reach slot 13 is the fact
/// that decides whether a family is being rasterised across the whole double-wide target instead of
/// one eye's half, and it is otherwise only obtainable from a frame capture. Indexed by pass id.
static SLOT13_BY_PASS: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];

/// Drain the per-pass slot-13 census as `(pass_id, count)` for the passes that saw any, most frequent
/// first. Reported in the per-range diagnostic line.
fn drain_slot13_census() -> Vec<(u8, u32)> {
    let mut seen: Vec<(u8, u32)> = SLOT13_BY_PASS
        .iter()
        .enumerate()
        .filter_map(|(pass, count)| {
            let count = count.swap(0, Ordering::Relaxed);
            (count > 0).then_some((pass as u8, count))
        })
        .collect();
    seen.sort_unstable_by_key(|&(_, count)| std::cmp::Reverse(count));
    seen
}

/// Open a new real frame: advance the frame ordinal the diagnostics are keyed to, and fold the
/// previous frame's already-instanced exposure into the history.
///
/// The exposure counters are per *frame*, but the G-buffer range is entered once per
/// `DrawRenderPassRange` call -- three times per dispatch under the collapse (`DrawGBuffer`
/// [`RenderPassId::RP_Z_OCCLUDERS`]..[`RenderPassId::RP_LAST_GBUFFER`], `Draw`
/// [`RenderPassId::RP_REFLECTIVE_WATER_PLANES`]..[`RenderPassId::RP_POSTEFFECTS`],
/// `DrawPosteffects` [`RenderPassId::RP_POSTEFFECTS`]..[`RenderPassId::RP_LAST_MAIN`]; see
/// `docs/mod/stereo/single-pass-stereo.md`). Folding them at a range boundary therefore cut the frame into
/// three unequal windows and reported each as if it were a frame; the fold belongs here, where a
/// frame actually begins.
pub fn begin_frame() {
    // The leaked-range clear deliberately does *not* happen here: it belongs to the thread that
    // raises and lowers the flag, and [`begin_dispatch`] does it there. With the frame tail deferred
    // this thread runs concurrently with the previous frame's still-walking dispatch, so clearing
    // from here tears a live range out from under it.
    //
    // The frame that just ended owns both the exposure fold and, if it was a diagnostic frame, the
    // trailing exposure line -- so it carries the same ordinal as its own range lines, which were
    // emitted before the ordinal advanced.
    let logged = diagnostic_frame();
    let exposure = accumulate_instanced_exposure();
    if logged {
        log_instanced_exposure(exposure);
    }
    FRAME_ORDINAL.fetch_add(1, Ordering::Relaxed);
}

/// Open a dispatch on the draw thread: pin this dispatch's config flags, and close a G-buffer range
/// left open by an interrupted dispatch, before this one's first range is entered.
///
/// This is the only place the range flag may be forced down from outside its guard. The flag is
/// written and read exclusively on the draw thread, and the dispatch prologue is the one point in
/// that thread's sequence where no range can be live -- so a clear here cannot interleave with a range
/// in progress, as a frame-start clear on the game thread could while a deferred frame tail is still
/// drawing.
///
/// The config flags are pinned here for the same reason and at the same point. They gate state this
/// thread arms and restores in pairs -- the eye-half viewport, the armed constant reprojection, the
/// per-eye re-issue loops, the range guard's own viewport repair -- and a flag that moved between an
/// arm and its restore would leave that state raised for the rest of the frame. Sampled per dispatch
/// rather than per frame because a dispatch is the unit the draw thread actually walks: under the
/// collapse the game thread is already a frame ahead of it.
pub fn begin_dispatch() {
    pin_dispatch_config_flags();
    clear_gbuffer_range();
    hw_instanced::maintain();
    vegetation_once::begin_dispatch();
}

/// Whether this frame is one the per-frame single-pass diagnostics log on.
fn diagnostic_frame() -> bool {
    FRAME_ORDINAL
        .load(Ordering::Relaxed)
        .is_multiple_of(DIAGNOSTIC_FRAME_CADENCE)
}

/// How often the single-pass bring-up diagnostics log, in real frames. Every range of a logged frame
/// reports, so the frame's whole pass-range sequence appears together and can be read as one frame
/// rather than as unrelated samples.
const DIAGNOSTIC_FRAME_CADENCE: usize = 120;

/// Log and reset the patched/unpatched draw counts of one G-buffer range -- called at each range's
/// exit, tagged with the `[first, last)` pass-index window the range covered so the frame's several
/// ranges can be told apart in the log.
///
/// `torn` counts the ranges whose guard found the flag already lowered: a non-zero value means the
/// range was closed from outside while it was still running, and every draw after that point was
/// mis-classified as out-of-range.
pub fn log_draw_split(first: u32, last: u32) {
    let patched = PATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let unpatched = UNPATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let split = VIEWPORT_SPLIT.swap(0, Ordering::Relaxed);
    let dup = VIEWPORT_DUP.swap(0, Ordering::Relaxed);
    let skipped = VIEWPORT_BINDS_SKIPPED.swap(0, Ordering::Relaxed);
    let in_patched = INSTANCED_RANGE_PATCHED.swap(0, Ordering::Relaxed);
    let out_patched = INSTANCED_RANGE_OUT_PATCHED.swap(0, Ordering::Relaxed);
    let indirect_reissued = INDIRECT_REISSUED.swap(0, Ordering::Relaxed);
    let indirect_forwarded = INDIRECT_FORWARDED.swap(0, Ordering::Relaxed);
    let slot13 = drain_slot13_census();
    let (hw_once, hw_untwinned) = hw_instanced::take_counts();
    let torn = RANGE_TORN.swap(0, Ordering::Relaxed);
    let torn_total = RANGE_TORN_TOTAL.fetch_add(torn, Ordering::Relaxed) + torn;
    if torn > 0 {
        tracing::warn!(
            target: "single_pass",
            "pass range [{first:#x}, {last:#x}) was closed from outside while it ran: the draws after \
             that point were treated as out-of-range ({torn_total} so far this session)"
        );
    }
    if !diagnostic_frame() {
        return;
    }
    let s = substitution_stats();
    // Named per pass, since the whole point is to say *which* passes submit geometry this way.
    let slot13 = slot13
        .iter()
        .map(|(pass, count)| {
            let kind = if is_geometry_slot13_pass(*pass) {
                "geom"
            } else {
                "full"
            };
            format!("{pass:#04x}:{count}:{kind}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    tracing::info!(
        target: "single_pass",
        "pass range [{first:#x}, {last:#x}): {patched} patched, {unpatched} unpatched draws | \
         instanced while it ran: {in_patched} patched in-range, {out_patched} patched out-of-range | \
         indirect: {indirect_reissued} re-issued per eye, {indirect_forwarded} forwarded | \
         hardware-instanced: {hw_once} once through a twin, {hw_untwinned} untwinned | \
         torn {torn} ({torn_total} this session) | viewports: {split} split, {dup} identical-dup, {skipped} binds \
         skipped as already bound | \
         recorded VS={} | CreateVertexShader: pending={} reacq[patched={} cb13={} no-refs={} err={}] | \
         census[patched={} no-refs={} deferred={} errored={}] | slot-13 by pass: [{slot13}]",
        s.recorded_vs,
        s.cvs_pending,
        s.cvs_reacq_patched,
        s.cvs_reacq_cb13,
        s.cvs_reacq_no_refs,
        s.cvs_reacq_err,
        s.census_patched,
        s.census_no_refs,
        s.census_deferred,
        s.census_errored,
    );
}

/// Disable all the single-pass COM-vtable detours, restoring the original D3D functions. Must run on
/// eject **before** the payload unloads: the detours inline-patch the DXVK functions to jump into
/// payload code, so leaving them enabled while the DLL unmaps dangles those jumps and the next D3D
/// call crashes. Runs under a thread suspender and the install lock, like install -- with the same
/// caveat that suspension narrows rather than closes the in-prologue window.
pub fn uninstall_com_detours() -> bool {
    let _install = DETOUR_INSTALL.lock();
    if !allow_com_reclamation(PIXEL_SHADER_EVER_ENABLED.load(Ordering::Acquire), true, 0) {
        tracing::error!(
            subsystem = "shader_identity",
            restart_required = true,
            "COM detour removal: shader identity detour was enabled; the payload must remain mapped and the game must be restarted"
        );
        return false;
    }
    if RS_SET_VIEWPORTS.get().is_none() && shader_identity::CREATE_PIXEL_SHADER.get().is_none() {
        // No draw detour was ever installed, so no draw can be holding a twin.
        hw_instanced::release();
        vegetation_once::release();
        return true;
    }

    // Ordering here has to satisfy two constraints that pull against each other.
    //
    // The slots must stay populated for as long as the functions are still patched: a detour that
    // fires with its slot already emptied finds no trampoline and aborts (it runs in a `nounwind`
    // context, so the panic is fatal rather than recoverable).
    //
    // And the trampolines must be freed *outside* the thread suspender: dropping a detour frees its
    // trampoline, a heap free takes the process heap lock, and if a suspended thread holds that lock
    // the free waits on a thread only we can resume -- an unrecoverable wedge, seen as a hang with a
    // thread spinning in the game's `PlatformAllocHook`.
    //
    // So: disable under suspension with the slots intact, resume, and only then reclaim and drop.
    // Between the two, the functions are unpatched, so nothing can enter a detour at all.
    // Fixed-size and not a `Vec`: growing it would allocate, and allocation is exactly what must
    // not happen while other threads are suspended below. `disable_detour!` writes through a
    // checked accessor, so a slot count that falls behind the number of call sites drops the
    // overflowing name from the log instead of indexing out of bounds in this `nounwind` context.
    let mut failed: [Option<&'static str>; 11] = [None; 11];
    let mut failures = 0usize;

    let suspension = suspend::for_block(|| {
        // A detour left enabled here is a relay still pointing into the about-to-be-freed payload
        // image, so a swallowed failure would be an undiagnosable crash -- record it. Recorded
        // rather than logged because formatting a `tracing` event allocates, which is the very
        // thing that must not happen under suspension.
        macro_rules! disable_detour {
            ($slot:expr, $name:literal) => {
                if let Some(detour) = $slot.get() {
                    // SAFETY: patching the function back runs with all other threads suspended.
                    let bad = match unsafe { detour.disable() } {
                        Err(_) => true,
                        Ok(()) => detour.is_enabled(),
                    };
                    if bad {
                        // `failures` still increments past the end of the array so the count
                        // stays truthful even when a name gets dropped for lack of a slot.
                        if let Some(slot) = failed.get_mut(failures) {
                            *slot = Some($name);
                        }
                        failures += 1;
                    }
                }
            };
        }
        disable_detour!(RS_SET_VIEWPORTS, "RSSetViewports");
        disable_detour!(RS_SET_SCISSOR_RECTS, "RSSetScissorRects");
        disable_detour!(DRAW_INDEXED, "DrawIndexed");
        disable_detour!(DRAW, "Draw");
        disable_detour!(DRAW_INDEXED_INSTANCED, "DrawIndexedInstanced");
        disable_detour!(
            DRAW_INDEXED_INSTANCED_INDIRECT,
            "DrawIndexedInstancedIndirect"
        );
        disable_detour!(DRAW_INSTANCED_INDIRECT, "DrawInstancedIndirect");
        disable_detour!(VS_SET_SHADER, "VSSetShader");
        disable_detour!(shader_identity::CREATE_PIXEL_SHADER, "CreatePixelShader");
        disable_detour!(CREATE_VERTEX_SHADER, "CreateVertexShader");
        disable_detour!(SET_VERTEX_PROGRAM_CONSTANTS, "SetVertexProgramConstants");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    if let Err(error) = &suspension {
        tracing::error!(%error, restart_required = true,
            "COM detour removal: thread suspension failed; the payload must remain mapped and the game must be restarted");
    }
    for name in failed.iter().flatten() {
        tracing::error!(
            detour = name,
            restart_required = true,
            "COM detour removal: detour did not disable; the payload must remain mapped and the game must be restarted"
        );
    }
    if !allow_com_reclamation(false, suspension.is_ok(), failures) {
        return false;
    }

    // Threads are running again and the functions are unpatched, so these drops are unreachable by
    // any detour and their frees cannot deadlock against a suspended lock holder.
    drop(RS_SET_VIEWPORTS.take());
    drop(RS_SET_SCISSOR_RECTS.take());
    drop(DRAW_INDEXED.take());
    drop(DRAW.take());
    drop(DRAW_INDEXED_INSTANCED.take());
    drop(DRAW_INDEXED_INSTANCED_INDIRECT.take());
    drop(DRAW_INSTANCED_INDIRECT.take());
    drop(VS_SET_SHADER.take());
    drop(shader_identity::CREATE_PIXEL_SHADER.take());
    shader_identity::disarm();
    drop(CREATE_VERTEX_SHADER.take());
    drop(SET_VERTEX_PROGRAM_CONSTANTS.take());
    release_cb13();
    ambient_occlusion::release();
    ao_volumes::release();
    screen_space_reflection::release();
    eye_camera::release();
    for shader in std::mem::take(&mut *PATCHED_VS.lock()) {
        // SAFETY: every entry was `com_add_ref`'d exactly once when it was recorded.
        unsafe { com_release(shader as *mut c_void) };
    }
    PATCHED_VS_NAMES.lock().clear();
    // The transform decisions go with them. They are normally cleared by the eject's shader bounce,
    // but a session that patched nothing never bounces, and a `static` outlives the payload -- so a
    // re-inject with different flags could otherwise consult the previous session's decisions for
    // any blob the engine still has cached.
    VS_TRANSFORM_CACHE.lock().clear();
    // After the draw detours are gone: a draw still finishing holds its own reference to the twin it
    // bound, so destroying the engine handles here cannot pull a layout out from under it.
    hw_instanced::release();
    vegetation_once::release();
    reset_instanced_exposure();
    tracing::info!("single-pass: COM detours uninstalled");
    true
}

fn allow_com_reclamation(
    identity_ever_enabled: bool,
    suspension_ok: bool,
    failures: usize,
) -> bool {
    !identity_ever_enabled && suspension_ok && failures == 0
}

static IN_GBUFFER_RANGE: AtomicBool = AtomicBool::new(false);
/// Real frames since injection, advanced by [`begin_frame`]; the diagnostics' cadence and grouping.
static FRAME_ORDINAL: AtomicUsize = AtomicUsize::new(0);
/// Ranges closed from outside their guard since the last [`log_draw_split`], and the session total --
/// see [`GBufferRange::drop`]. The tear is intermittent, so the total is never reset: a diagnostic
/// frame that reports zero of its own still shows whether it has ever happened.
static RANGE_TORN: AtomicUsize = AtomicUsize::new(0);
static RANGE_TORN_TOTAL: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Set by the `CreateVertexProgram` hook right before the engine creates the D3D shader from a
    /// substituted (patched) blob, so [`create_vertex_shader_detour`] knows the next shader is patched.
    ///
    /// Thread-local, not a global: `ID3D11Device` is free-threaded and JC3 streams resources off the
    /// render thread, so a loader thread's flag would otherwise tag whatever shader a *different*
    /// thread happened to create next -- instance-doubling and eye-splitting an unrelated shader while
    /// the genuinely patched one went unrecorded. The hook sets it and the device detour consumes it
    /// within the same synchronous call, so the flag never needs to cross a thread.
    static PATCH_PENDING: Cell<bool> = const { Cell::new(false) };

    /// The engine name of the shader [`PATCH_PENDING`] refers to, carried alongside it so
    /// [`create_vertex_shader_detour`] can record the name against the `ID3D11VertexShader` it gets
    /// back. The D3D layer never sees the name, and the shader pointer is the only identity the
    /// draw-time paths have, so this is the one point where the two can be joined.
    static PATCH_PENDING_NAME: Cell<Option<String>> = const { Cell::new(None) };

    /// Set by the `CreateVertexProgram` hook right before the engine creates the D3D shader from a
    /// pinned-eye rewrite, so [`create_vertex_shader_detour`] tags it instead of recording it as
    /// patched: its draws must keep their per-eye submission. Thread-local for the same reason as
    /// [`PATCH_PENDING`].
    static PIN_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Set (or clear) this thread's [`PIN_PENDING`] flag. Called by the `CreateVertexProgram` hook around
/// the engine's shader creation.
pub fn set_pin_pending(pending: bool) {
    PIN_PENDING.with(|flag| flag.set(pending));
}

/// Set (or clear) this thread's [`PATCH_PENDING`] flag and the pending shader's engine name. Called
/// by the `CreateVertexProgram` hook around the engine's shader creation.
pub fn set_patch_pending(pending: bool, name: Option<&str>) {
    PATCH_PENDING.with(|flag| flag.set(pending));
    PATCH_PENDING_NAME.with(|slot| slot.set(pending.then(|| name.map(str::to_owned)).flatten()));
}

/// The engine name of each recorded patched vertex shader, where one was available (the
/// `CreateVertexProgram` path carries it; the re-acquire path does not). Read only by the diagnostic
/// readouts, never on a draw path.
static PATCHED_VS_NAMES: Mutex<BTreeMap<usize, String>> = Mutex::new(BTreeMap::new());
/// The `ID3D11VertexShader`s created from patched blobs, keyed by their raw pointer.
///
/// The set *owns* a reference to each shader: [`com_add_ref`] on record, [`com_release`] on
/// [`reset_patched_vs`]. Without that reference a shader the game releases could have its address
/// recycled by an unpatched shader, which would then match on `VSSetShader` and be instance-doubled --
/// the recycled draw appears in one eye only. An ordered set rather than a linear scan because the
/// lookup is on the hottest path in the codebase: every `VSSetShader` of every frame, in a feature
/// whose whole purpose is cutting draw-submission cost. (The raw pointer is stored rather than an
/// owned `IUnknown` only because `IUnknown` is not `Send`.)
static PATCHED_VS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// Take a reference of our own on a COM object, so the pointer we record cannot be freed (and its
/// address recycled) while we still consider it live. Paired with [`com_release`].
///
/// # Safety
///
/// `object` must be a live COM object pointer.
unsafe fn com_add_ref(object: *mut c_void) {
    // SAFETY: the caller guarantees a live object; cloning the borrowed interface calls `AddRef`, and
    // forgetting the clone is what keeps that reference outstanding.
    if let Some(unknown) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        std::mem::forget(unknown.clone());
    }
}

/// Drop the reference [`com_add_ref`] took.
///
/// # Safety
///
/// `object` must be a pointer this module previously passed to [`com_add_ref`], not yet released.
unsafe fn com_release(object: *mut c_void) {
    // SAFETY: `from_raw` adopts the outstanding reference; dropping it calls `Release`.
    drop(unsafe { IUnknown::from_raw(object) });
}
/// Whether the currently-bound vertex shader is a patched one (updated on `VSSetShader`).
static BOUND_VS_PATCHED: AtomicBool = AtomicBool::new(false);

/// Whether the vertex shader bound on the immediate context is one the single-pass rewrite patched.
pub fn bound_vertex_shader_patched() -> bool {
    BOUND_VS_PATCHED.load(Ordering::Relaxed)
}
/// The currently-bound `ID3D11VertexShader` pointer (updated on `VSSetShader`), so an exposed
/// already-instanced draw can be attributed to a shader.
static BOUND_VS: AtomicUsize = AtomicUsize::new(0);
static PATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
static UNPATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
/// Already-instanced draws with a patched vertex shader bound, split by whether the G-buffer range
/// was up, accumulated per *range* rather than per frame ([`log_draw_split`] resets them). The
/// per-frame `INSTANCED_*` buckets carry the same events; these say *when in the frame* they landed.
static INSTANCED_RANGE_PATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_RANGE_OUT_PATCHED: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_SPLIT: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_DUP: AtomicUsize = AtomicUsize::new(0);
/// Routed draws whose viewport slots already held what they wanted, so `ensure_collapse_viewport`
/// bound nothing.
static VIEWPORT_BINDS_SKIPPED: AtomicUsize = AtomicUsize::new(0);
/// How often [`unify_viewport_slots`] had to put a split slot pair back to one region: the number of
/// windows in which an out-of-range patched draw would otherwise have lost its odd-parity instances.
static VIEWPORT_UNIFIED: AtomicUsize = AtomicUsize::new(0);

/// `CreateVertexShader`-detour outcome tallies (cumulative since injection), to diagnose what the
/// shader re-create path -- which bypasses `CreateVertexProgram` -- feeds through the D3D-level
/// substitution: `pending` came pre-substituted from `CreateVertexProgram`; the two `decided_*` buckets
/// re-applied the decision the hook recorded for that blob; the four `reacq_*` buckets are what the
/// detour's own rewrite found for a blob the hook never saw.
static CVS_PENDING: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided a transform for, re-applied here.
static CVS_DECIDED_TRANSFORMED: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided to leave pristine -- overwhelmingly the families a render-block
/// intercept owns. A count here is the intercepts and the rewrite staying out of each other's way; the
/// same shaders showing up under `reacq_patched` instead would mean the decline was being undone.
static CVS_DECIDED_PRISTINE: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_PATCHED: AtomicUsize = AtomicUsize::new(0);
/// The incoming bytecode already declared `cb13`, so the rewriter refused it as already-patched.
///
/// No pristine game vertex shader declares `cb13` -- the offline corpus run over all 455 of them
/// reports zero `Cb13AlreadyDeclared`, which is why the register was chosen -- so a non-zero count is
/// unambiguous: something is presenting the mod's *own* rewritten bytecode back to
/// `CreateVertexShader`, from a store the substitution paths do not own (both of them repoint the
/// engine's code pointer only for the duration of the create call and restore it afterwards).
///
/// That matters for eject. The restore bounce re-creates every shader from whatever bytecode its
/// resource holds, with the substitution inert; a resource holding patched bytecode therefore
/// re-creates a patched shader, and there is no inverse rewrite to undo it. Which store that is has
/// not been identified -- it needs a live session with a non-zero count and a breakpoint on the
/// caller. [`warn_if_shaders_hold_patched_bytecode`] makes it visible at eject rather than silent.
static CVS_REACQ_CB13: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_NOREFS: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_ERR: AtomicUsize = AtomicUsize::new(0);

// The collapse's viewport records live in `viewport_record`. The scene's (the last scene-sized
// bind, the full width the eye halves are cut from) is deliberately *not* cleared at the end of the
// G-buffer range, unlike [`CURRENT_M_EYE`]: the post-draw UI overlay ([`collapse_ui_eye_override`])
// reads it to place each eye's HUD quad, and that runs long after the range. Every consumer that must
// not see a stale value gates on [`in_gbuffer_range`] instead.
//
// The engine's (its latest bind, whatever its size) exists alongside the scene's because several
// passes redirect their draws to a reduced-resolution off-screen target -- the shared
// quarter-resolution buffer the low-resolution clouds, the low-resolution particles, and the
// volumetric spot-light cones all render into, and the downsampled depth buffer -- and a draw into a
// `W x H/2` target handed a `2W x H` viewport is magnified 2x about the target's origin and cropped,
// which is a 2x motion gain as well. It is always the live bind, so the halves derived from it are
// the halves of whatever target is actually bound.

pub fn patch_camera_sources(eye: usize) -> Option<(D3D11_VIEWPORT, usize)> {
    let full = viewport_record::scene_viewport()?;
    Some((eye_half_viewport(full, eye), CB13.lock().buffer_ptr()?))
}
/// The two per-eye reprojection matrices `M_eye` (`clip_eye = M_eye · clip_center`), published each
/// view by [`compute_dual_eye_rows`]. The terrain-detail render-block intercept reads them to build a
/// per-eye `cb1` on the CPU (the detail draw is GPU-indirect, so it cannot be instance-doubled).
static CURRENT_M_EYE: Mutex<Option<[glam::Mat4; 2]>> = Mutex::new(None);

static PATCHED: AtomicUsize = AtomicUsize::new(0);
static NO_REFS: AtomicUsize = AtomicUsize::new(0);
static DEFERRED: AtomicUsize = AtomicUsize::new(0);
static ERRORED: AtomicUsize = AtomicUsize::new(0);
/// Terrain tessellation shaders substituted for single-pass since injection: hull shaders whose eye
/// lane was forwarded, and domain shaders reprojected. Surfaced in the debug UI so it is clear whether
/// the terrain path is catching anything.
static TERRAIN_HS_FORWARDED: AtomicUsize = AtomicUsize::new(0);
static TERRAIN_DS_REPROJECTED: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn com_reclamation_fails_closed() {
        assert!(allow_com_reclamation(false, true, 0));
        assert!(!allow_com_reclamation(true, true, 0));
        assert!(!allow_com_reclamation(false, false, 0));
        assert!(!allow_com_reclamation(false, true, 1));
        assert!(!allow_com_reclamation(true, false, 1));
    }

    const CAPTURE_VS: &[u8] = jc3vr_view::CAPTURE_VS;

    #[test]
    fn waveworks_permutations_are_owned_when_the_intercept_is_on() {
        for name in ["nvwaterbox", "nvwaterbox_tess"] {
            assert!(baked_cb_vs_owned(Some(name), true, true), "{name}");
            assert!(!baked_cb_vs_owned(Some(name), true, false), "{name}");
        }
    }

    #[test]
    fn global_view_projection_reads_are_left_to_the_remap() {
        for name in ["nvwaterbox", "nvwaterbox_tess"] {
            assert!(!baked_cb_vs_owned(Some(name), false, true), "{name}");
        }
    }

    #[test]
    fn legacy_water_permutations_and_unnamed_shaders_are_not_owned() {
        for name in [
            "waterbox",
            "waterboxsurface",
            "waterhighend",
            "watershader_lod0",
        ] {
            assert!(!baked_cb_vs_owned(Some(name), true, true), "{name}");
        }
        assert!(!baked_cb_vs_owned(None, true, true));
    }

    #[test]
    fn baked_cb_block_owns_vs_agrees_with_the_intercept_flag_on_real_bytecode() {
        crate::test_config::register();
        for name in ["nvwaterbox", "nvwaterbox_tess"] {
            assert_eq!(
                baked_cb_block_owns_vs(Some(name), CAPTURE_VS),
                nv_water_per_eye(),
                "{name}"
            );
        }
        assert!(!baked_cb_block_owns_vs(Some("waterbox"), CAPTURE_VS));
        assert!(!baked_cb_block_owns_vs(None, CAPTURE_VS));
    }

    #[test]
    fn malformed_bytecode_is_never_owned() {
        crate::test_config::register();
        assert!(!baked_cb_block_owns_vs(
            Some("nvwaterbox"),
            b"not a dxbc container"
        ));
        assert!(!baked_cb_block_owns_vs(Some("nvwaterbox_tess"), &[]));
    }
}
