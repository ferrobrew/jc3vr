//! VR runtime configuration. See [`crate::vr`] and `docs/mod/vr-runtime.md`.

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

/// Which depth convention the per-eye off-axis projection is written in, and where in the
/// `SetupRenderCamera` sequence it lands (`docs/engine/rendering/rendering.md` §2.7, `docs/mod/vr-runtime.md` blocker 1).
/// The headset path always writes [`EnginePreReverseZ`](Self::EnginePreReverseZ); the other form
/// describes per-eye parameters built outside the camera hook (the overlay's own views).
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectionConvention {
    /// **Preferred and verified-correct.** Write a standard (non-reverse-Z) off-axis projection into
    /// `m_Projection` *before* the engine's `SetupRenderCamera`, so the engine applies its reverse-Z
    /// remap and TAA jitter to it exactly once, matching every other camera. `SetupRenderCamera`
    /// consumes the pre-written `m_Projection` in place rather than rebuilding it from FOV/near/far
    /// (settled against the engine, `docs/engine/rendering/rendering.md` §2.9), so this write reaches the GPU.
    #[default]
    EnginePreReverseZ,
    /// **Fallback / escape hatch.** Write an already-reverse-Z'd off-axis projection *after*
    /// `SetupRenderCamera` (so the engine does not re-reverse it), then rebuild the view-projections
    /// manually. TAA jitter is not applied on this path.
    ///
    /// The consume-vs-rebuild question this guards against is settled against the engine
    /// (`docs/engine/rendering/rendering.md` §2.9): `Camera::SetupRenderCamera` *consumes* whatever is in
    /// `m_Projection`, applying `z' = w - z` to it in place — it never rebuilds from FOV/near/far —
    /// so the pre-call [`EnginePreReverseZ`](Self::EnginePreReverseZ) write flows through correctly
    /// and is the verified-correct default. This variant exists only as a runtime escape hatch
    /// for a headset playtest, in case the depth still reads wrong for a reason not visible from the
    /// desktop.
    ManualReverseZ,
}

/// Which pose the freeze diagnostic holds still (see [`crate::vr::pose_control`]). The two frozen
/// modes are two answers to the same question -- "what do you want held still?" -- so they are
/// mutually exclusive: freezing the head's contribution and freezing the whole camera are not
/// composable, and holding both would only mean the second one.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum FreezeMode {
    /// Nothing is frozen; the camera follows the head, the body, and the game as usual.
    #[default]
    Off,
    /// Hold the **cockpit-frame HMD pose**: the head pose relative to the recenter baseline, plus the
    /// sim-driven body frame and head-bone anchor captured with it. The rendered camera is then
    /// bit-identical frame to frame *as long as nothing else moves it*, which isolates artifacts
    /// driven by the HMD's per-frame pose sensor noise (present even sitting on a desk) from ones
    /// intrinsic to the render. The body frame and anchor are captured too, so on-foot idle motion
    /// does not leak in; a camera the *game* moves (a vehicle, a cutscene) still does.
    CockpitPose,
    /// Hold the **final render camera** in world space: the scene camera's world transform is pinned
    /// at the last point before the engine consumes it, so nothing upstream -- the head, the body, the
    /// animated head-bone anchor, or the game's own camera -- can move the view. The per-eye offsets
    /// and projections are held with it, so the two eyes are static too. This is the mode for
    /// measuring content that mis-renders only *in motion*: the view is a fixed world-space pose that
    /// can be stepped by an exact amount and returned to.
    FullCamera,
}

/// What the desktop mirror draws into the game window.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum MirrorView {
    /// The left eye alone, filling the window.
    #[default]
    Left,
    /// The right eye alone, filling the window.
    Right,
    /// Both eyes side by side, letterboxed as a pair so neither is cropped, in cross-eyed order: the
    /// right eye's image on the left and the left eye's on the right, so the pair fuses by crossing
    /// the eyes.
    StereoPair,
}

impl MirrorView {
    /// Whether this view reads eye `eye`'s capture.
    pub fn shows_eye(self, eye: usize) -> bool {
        match self {
            Self::Left => eye == 0,
            Self::Right => eye == 1,
            Self::StereoPair => eye <= 1,
        }
    }

    /// The F10 toggle: a single eye switches to the stereo pair, and the pair switches back to
    /// `last_mono`. Returns the new view and the single eye to return to next time, which is the
    /// eye being left when that is a single eye, so the toggle round-trips.
    pub fn toggled(self, last_mono: Self) -> (Self, Self) {
        match self {
            Self::StereoPair => {
                let back = match last_mono {
                    Self::StereoPair => Self::Left,
                    eye => eye,
                };
                (back, back)
            }
            eye => (Self::StereoPair, eye),
        }
    }
}

/// The VR runtime settings, at `vr` in the settings file; the payload binds the slot at startup.
pub static VR_CONFIG: ConfigSlot<VrConfig> = ConfigSlot::new("vr");

/// OpenXR runtime settings. `Clone` (not `Copy`) because [`loader_path`](VrConfig::loader_path) owns
/// a `String`.
#[derive(Clone, Serialize, Deserialize)]
pub struct VrConfig {
    /// Master switch: bring up the OpenXR session and render to the HMD. Off leaves the mod in
    /// flatscreen stereo (and tears any live runtime down). While on, a failed bring-up is retried
    /// every ten seconds until it succeeds.
    pub enabled: bool,
    /// Per-eye swapchain resolution scale, applied to the runtime's recommended width/height. `1.0`
    /// is the runtime's recommendation; lower trades sharpness for fill rate. Clamped to a small
    /// positive minimum at swapchain creation.
    pub resolution_scale: f32,
    /// A per-eye size, `[width, height]`, to use in place of the runtime's recommended one before
    /// [`resolution_scale`](VrConfig::resolution_scale) applies. For running against a runtime whose
    /// recommended size is not the headset's being studied (the visual harness's simulated headset);
    /// `None`, the default, takes the runtime's.
    #[serde(default)]
    pub(crate) recommended_size_override: Option<[u32; 2]>,
    /// World scale: metres of head/IPD motion per engine unit (`1.0` = 1:1). Kept here so the render wiring and the camera path share one knob.
    pub world_scale: f32,
    /// Override path to the OpenXR loader DLL. `None` loads `openxr_loader.dll` next to the payload
    /// DLL, falling back to the platform default search path.
    pub loader_path: Option<String>,
    /// Render each eye at the HMD-recommended per-eye resolution (× [`resolution_scale`]) rather than
    /// the desktop display size, by driving the engine's own deferred resize (see
    /// [`crate::vr::resolution`]). On by default; disabled automatically at runtime if the resize
    /// path faults or returns the wrong size, falling back to the desktop resolution.
    ///
    /// [`resolution_scale`]: VrConfig::resolution_scale
    #[serde(default = "default_true")]
    pub native_resolution: bool,
    /// Mirror the eyes to the game's own desktop window while a session is running. The engine's
    /// present stays blocked (`BLOCK_FLIP`); the mirror draws the configured view's captures into the
    /// game swapchain's back buffer, framed to the window aspect, and presents it unsynced (see
    /// [`crate::vr::mirror`]). On by default; disabled automatically at runtime on any draw/present
    /// fault, after which the game window simply shows the last mirrored (or stale) frame.
    #[serde(default = "default_true")]
    pub mirror: bool,
    /// What the desktop [`mirror`](VrConfig::mirror) shows: one eye, or both side by side (see
    /// [`MirrorView`]). F10 toggles between the stereo pair and the last single eye.
    #[serde(default)]
    pub mirror_view: MirrorView,
    /// Substitute a mod-owned render target for the engine's swapchain-derived back buffer, so the
    /// render resolution and the DXGI swapchain size stop being the same number (see
    /// [`crate::vr::back_buffer`] and `docs/mod/stereo/swapchain-ownership.md`).
    ///
    /// The engine builds its final composite target as a format alias of DXGI back buffer 0, so
    /// driving the scene to the per-eye render resolution resizes the swapchain with it, and the
    /// desktop present then rescales every frame onto a window of a different size and shape. Owning
    /// the back buffer breaks that link: the swapchain stays at the window size, the mirror's present
    /// stops rescaling, and single-pass double-wide stops forcing a 2x-width swapchain.
    ///
    /// On by default; it only ever engages while an XR session is running, and is released again
    /// (with the engine's own objects rebuilt over the live swapchain) on session end and on eject.
    #[serde(default = "default_true")]
    pub own_back_buffer: bool,
    /// Overlap the next frame's runtime wait, view location, and render update with the previous
    /// frame's draw task and frame tail, instead of starting them once the tail has submitted (see
    /// `docs/mod/threading.md`). The next frame then blocks on the tail only where it begins its
    /// OpenXR frame, just before it dispatches. On by default; off restores the serial ordering, for
    /// an A/B in the benchmark or the headset.
    #[serde(default = "default_true")]
    pub overlap_render_update: bool,
    /// Keep the game's threads on the cores that share the largest L3 cache, on a processor whose
    /// L3 caches differ in size (see [`jc3vr_core::cpu_placement`]). The draw task, which bounds the
    /// heaviest scenes, runs far faster there. A processor with one size of L3 is left alone. On by
    /// default; off puts the affinity it found back.
    #[serde(default = "default_true")]
    pub large_cache_cores: bool,
    /// Diagnostic: which pose to hold still (see [`FreezeMode`] and [`crate::vr::pose_control`]).
    /// Whichever pose the mode holds is captured on the first frame it is on and reused every frame
    /// after, and is hand-editable while held -- so a motion-dependent artifact can be driven by an
    /// exact, repeatable step instead of an eyeballed head movement. Not for gameplay: the view locks
    /// in place. **Default off**, and not saved, so a launch never starts with the view locked.
    #[serde(skip)]
    pub freeze_mode: FreezeMode,
}

/// The serde default for [`VrConfig::native_resolution`] (the manual [`Default`] via
/// [`VrConfig::new`] is not consulted per-field when a field is absent from the serialized form).
fn default_true() -> bool {
    true
}

impl VrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            resolution_scale: 1.0,
            recommended_size_override: None,
            world_scale: 1.0,
            loader_path: None,
            native_resolution: true,
            mirror: true,
            mirror_view: MirrorView::Left,
            own_back_buffer: true,
            overlap_render_update: true,
            large_cache_cores: true,
            freeze_mode: FreezeMode::Off,
        }
    }
}

impl Default for VrConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The foveation settings, at `foveation` in the settings file; the payload binds the slot at
/// startup.
pub static FOVEATION_CONFIG: ConfigSlot<FoveationConfig> = ConfigSlot::new("foveation");

/// Static foveated rendering (issue #29): a radial stencil mask drops a dithered fraction of the
/// peripheral pixels before the expensive scene passes shade them, and a fill-in pass reconstructs them
/// from their neighbours. On by default (validated in-headset); costs the mask + fill passes but saves
/// peripheral shading. See `docs/mod/stereo/foveation.md`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FoveationConfig {
    /// Master toggle.
    pub enabled: bool,
    /// Diagnostic: paint the dropped peripheral pixels magenta (in the fill-in pass) instead of
    /// reconstructing them, so the mask is directly visible. Off by default.
    pub debug_show_mask: bool,
}
impl FoveationConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            debug_show_mask: false,
        }
    }
}
impl Default for FoveationConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_view_toggle_round_trips_through_the_pair() {
        for eye in [MirrorView::Left, MirrorView::Right] {
            let (pair, remembered) = eye.toggled(MirrorView::Left);
            assert_eq!(pair, MirrorView::StereoPair);
            assert_eq!(remembered, eye);
            assert_eq!(pair.toggled(remembered).0, eye);
        }
    }

    #[test]
    fn mirror_view_toggle_from_the_pair_defaults_to_left() {
        // A pair chosen in the debug UI with no F10 press before it returns to the left eye.
        assert_eq!(
            MirrorView::StereoPair.toggled(MirrorView::StereoPair),
            (MirrorView::Left, MirrorView::Left)
        );
    }

    #[test]
    fn mirror_view_reads_the_eyes_it_draws() {
        assert!(MirrorView::Left.shows_eye(0) && !MirrorView::Left.shows_eye(1));
        assert!(!MirrorView::Right.shows_eye(0) && MirrorView::Right.shows_eye(1));
        assert!(MirrorView::StereoPair.shows_eye(0) && MirrorView::StereoPair.shows_eye(1));
    }
}
