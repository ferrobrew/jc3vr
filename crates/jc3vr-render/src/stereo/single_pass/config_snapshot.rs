//! The single-pass config snapshot: the flags the render path reads, sampled off the mutex-guarded
//! configuration and held in an atomic bitfield.
//!
//! Two snapshots, for one reason. Every gate in the feature is consulted from viewport, draw, and
//! render-block detours, so reading the configuration behind its mutex would cost a lock per draw
//! submission -- the exact cost single-pass exists to remove. And with the frame tail deferred, the
//! game thread is a frame ahead of the draw thread, so a flag that moved between an arm and its
//! restore would leave that state raised for the rest of the frame. The game thread therefore
//! refreshes a process-wide snapshot per frame, and the draw thread pins its own per dispatch.
//!
//! The master switch is taken together with whether a VR session is running, latched at the same
//! frame refresh: single-pass renders both eyes into the halves of a double-wide target sized from
//! the headset's views, so without a session it stays inert and the game renders as it ships.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::stereo::config::STEREO_CONFIG;

/// One flag in the per-frame single-pass config snapshot.
#[derive(Clone, Copy)]
pub(super) enum Flag {
    SinglePass,
    DryRun,
    DualEye,
    Collapse,
    DoubleWide,
    Reproject,
    ReprojectCameraOnly,
    Terrain,
    TerrainPatchPerEye,
    TerrainEyeRows,
    FullViewProjectionRows,
    TreeImpostors,
    TreeImpostorPixelSize,
    StaticLights,
    Moon,
    WaterPlane,
    Clouds,
    Stars,
    AoVolumes,
    Bark,
    Foliage,
    VegetationOnce,
    Occluder,
    WaterScreenUv,
    NvWater,
    InstancedPerEye,
    HwInstancedOnce,
    IndirectPerEye,
    VertexGlobalsPerEye,
    UniformViewportSlots,
    ViewportFollowsTarget,
    Slot13PerEye,
    NonIndexedGeometryOnce,
    TerrainDetailEyeRows,
    TerrainCullBothEyes,
    ViewportBindCache,
    SkipUnreadEffectDepthCopy,
}

/// The single-pass config flags, sampled once per frame and pinned for the duration of a dispatch.
#[derive(Clone, Copy)]
pub(super) struct ConfigFlags(u64);

impl ConfigFlags {
    pub(super) fn has(self, flag: Flag) -> bool {
        self.0 & (1 << flag as u64) != 0
    }
}

/// The flags this draw thread's dispatch was opened with, else the game thread's frame snapshot, else
/// a fresh read if neither has been taken yet.
///
/// Every gate in this module is consulted from the render thread on paths that run per draw -- the
/// viewport, indexed-draw and render-block detours -- so reading them through `Config`'s mutex made
/// the feature pay a lock acquisition per draw submission, in a feature whose purpose is to reduce
/// per-draw cost.
///
/// There are two snapshots because the two consumers want different things, and one snapshot cannot
/// give both. The **draw thread** wants a value that does not move for the whole dispatch: nearly
/// every gate here brackets something (a pass range, an eye-half viewport, an armed reprojection,
/// a per-eye re-issue loop), and a flag that changes between the arm and the restore leaks that state
/// into the rest of the frame -- so it pins its own copy in [`DISPATCH_FLAGS`] at
/// [`begin_dispatch`]. Everything else -- the game thread's resolution and camera decisions, and the
/// shader-creation hooks, which must see a toggle in the *same* frame the debug UI turns it on,
/// because that frame's shader reload is what applies it -- reads [`CONFIG_FLAGS`], resampled at frame
/// start ([`refresh_config_flags`]).
///
/// The pin ends with the dispatch ([`end_dispatch`]): the draw thread is a job-pool worker that runs
/// game-side jobs between dispatches, and those read the frame snapshot like everything else.
///
/// The frame snapshot deliberately does **not** reach the draw thread mid-dispatch. It is written on
/// the game thread, which with the frame tail deferred runs concurrently with the previous
/// frame's still-walking dispatch -- the same interleaving that made the game-thread G-buffer range
/// clear tear a live range (see [`begin_dispatch`]). A flag changing there would land between an arm
/// and its restore, which is why the draw thread reads a pinned copy instead.
pub(super) fn config_flags() -> ConfigFlags {
    if let Some(flags) = DISPATCH_FLAGS.get() {
        return flags;
    }
    let snapshot = CONFIG_FLAGS.load(Ordering::Relaxed);
    if snapshot & CONFIG_FLAGS_VALID != 0 {
        return ConfigFlags(snapshot);
    }
    ConfigFlags(store_config_flags())
}

/// Re-read the single-pass config flags into the frame snapshot. Called at frame start, on the game
/// thread, and read by everything that is not inside a draw-thread dispatch -- see [`config_flags`]
/// for why the draw thread pins its own copy instead of reading this one.
///
/// It also latches whether a VR session is running, which the master switch is taken together
/// with. The session state is the one the previous frame's runtime pump published, so a session
/// that starts or stops takes effect from the next frame's refresh, for every reader of that frame
/// at once.
pub fn refresh_config_flags() {
    SESSION_LIVE.store(crate::vr::is_running_published(), Ordering::Relaxed);
    store_config_flags();
}

/// Pin the config flags for the dispatch this thread is opening, so every gate it consults answers
/// the same for the whole of it. Called from [`begin_dispatch`].
pub(super) fn pin_dispatch_config_flags() {
    DISPATCH_FLAGS.set(Some(ConfigFlags(store_config_flags())));
}

/// The end of the dispatch this thread opened: drops the pinned flags when it goes out of scope, so
/// whatever this thread runs between dispatches reads the frame snapshot like any other thread.
///
/// The engine runs its draw dispatch on a worker of its job pool, and the pool's workers run
/// game-side jobs between dispatches too. A pin left in place outlived its dispatch on that worker:
/// a job it later ran read the flags of whatever dispatch the worker last drew, so the
/// water's reflection camera update, which runs as such a job, saw the collapse off on the frames it
/// landed on a worker last used for a multipass dispatch, and sized the reflection to the double-wide
/// target.
pub struct DispatchEnd(());

/// Open the scope that ends this thread's dispatch pin; held for the dispatch's last seam.
pub fn end_dispatch() -> DispatchEnd {
    DispatchEnd(())
}

impl Drop for DispatchEnd {
    fn drop(&mut self) {
        DISPATCH_FLAGS.set(None);
    }
}

/// Whether a VR session was running at the last [`refresh_config_flags`]. Off until the first
/// refresh, so nothing single-pass runs before the frame driver has seen a session.
static SESSION_LIVE: AtomicBool = AtomicBool::new(false);

fn store_config_flags() -> u64 {
    let bit = |on: bool, flag: Flag| u64::from(on) << flag as u64;
    let session_live = SESSION_LIVE.load(Ordering::Relaxed);
    let snapshot = STEREO_CONFIG.query(|c| {
        let s = &c.single_pass;
        bit(s.enabled && session_live, Flag::SinglePass)
            | bit(s.patch_dryrun, Flag::DryRun)
            | bit(s.dual_eye, Flag::DualEye)
            | bit(s.collapse, Flag::Collapse)
            | bit(s.double_wide, Flag::DoubleWide)
            | bit(s.reproject, Flag::Reproject)
            | bit(s.reproject_camera_only, Flag::ReprojectCameraOnly)
            | bit(s.terrain, Flag::Terrain)
            | bit(s.terrain_patch_per_eye, Flag::TerrainPatchPerEye)
            | bit(s.terrain_eye_rows, Flag::TerrainEyeRows)
            | bit(s.full_view_projection_rows, Flag::FullViewProjectionRows)
            | bit(s.tree_impostors, Flag::TreeImpostors)
            | bit(
                s.tree_impostor_pixel_size_per_eye,
                Flag::TreeImpostorPixelSize,
            )
            | bit(s.static_lights_per_eye, Flag::StaticLights)
            | bit(s.moon_per_eye, Flag::Moon)
            | bit(s.water_plane_per_eye, Flag::WaterPlane)
            | bit(s.clouds_per_eye, Flag::Clouds)
            | bit(s.stars_per_eye, Flag::Stars)
            | bit(s.ao_volumes_per_eye, Flag::AoVolumes)
            | bit(s.bark, Flag::Bark)
            | bit(s.foliage, Flag::Foliage)
            | bit(s.vegetation_once, Flag::VegetationOnce)
            | bit(s.occluder, Flag::Occluder)
            | bit(s.water_uv_per_eye, Flag::WaterScreenUv)
            | bit(s.nvwater_per_eye, Flag::NvWater)
            | bit(s.instanced_per_eye, Flag::InstancedPerEye)
            | bit(s.hw_instanced_once, Flag::HwInstancedOnce)
            | bit(s.indirect_per_eye, Flag::IndirectPerEye)
            | bit(s.vertex_globals_per_eye, Flag::VertexGlobalsPerEye)
            | bit(s.uniform_viewport_slots, Flag::UniformViewportSlots)
            | bit(
                s.collapse_viewport_follows_target,
                Flag::ViewportFollowsTarget,
            )
            | bit(s.slot13_per_eye, Flag::Slot13PerEye)
            | bit(s.non_indexed_geometry_once, Flag::NonIndexedGeometryOnce)
            | bit(s.terrain_detail_eye_rows, Flag::TerrainDetailEyeRows)
            | bit(s.terrain_cull_both_eyes, Flag::TerrainCullBothEyes)
            | bit(s.viewport_bind_cache, Flag::ViewportBindCache)
            | bit(
                s.skip_unread_effect_depth_copy,
                Flag::SkipUnreadEffectDepthCopy,
            )
    }) | CONFIG_FLAGS_VALID;
    CONFIG_FLAGS.store(snapshot, Ordering::Relaxed);
    snapshot
}

/// Marks [`CONFIG_FLAGS`] as having been sampled at least once, so a zero snapshot before the first
/// frame is not mistaken for "everything off".
const CONFIG_FLAGS_VALID: u64 = 1 << 63;
static CONFIG_FLAGS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The flags [`begin_dispatch`] pinned for the dispatch in flight on this thread, or `None` on a
    /// thread that has never opened one. Thread-local rather than global because the point is to keep
    /// the draw thread's view of the config still while the game thread is free to resample its own.
    static DISPATCH_FLAGS: Cell<Option<ConfigFlags>> = const { Cell::new(None) };
}
