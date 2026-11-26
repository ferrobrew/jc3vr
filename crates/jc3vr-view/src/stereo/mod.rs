//! Live stereo runtime state and the per-eye gating helpers shared across the render hooks.
//!
//! The Draw driver (the payload's `hooks::game`) marks every VR frame stereo
//! ([`mark_stereo_frame`]) and enters each of its dispatches ([`enter_dispatch`]); the camera hooks
//! in this crate record the per-eye camera state. The state itself is private to this crate: the
//! render hooks read it through [`is_second_eye`], [`draw_index`], [`active`], [`center_transform`],
//! [`shadow_anchor`], and the other accessors below, and the one value they publish into it, the
//! sun-shadow fit's camera position, goes through [`record_shadow_fit_position`]. This is *runtime*
//! state, distinct from the `stereo` config toggles in [`crate::stereo::config`].

use jc3gi::types::math::Matrix4;
use parking_lot::Mutex;

pub mod config;
pub mod dispatch;
pub mod engine_context;
pub mod layout;

/// The live stereo render state for the frame in flight.
pub(crate) struct StereoState {
    /// Whether the current frame is being rendered in stereo (the Draw driver double-Draws).
    pub(crate) active: bool,
    /// The physical eye currently being drawn: 0 = left, 1 = right.
    pub(crate) draw_index: usize,
    /// The current dispatch's ordinal within the frame (0-based). Once-per-frame work (the shared
    /// pre-passes) keys on `> 0`, independent of eye indexing.
    pub(crate) dispatch_ordinal: usize,
    /// Whether the current dispatch is the frame's last.
    pub(crate) last_dispatch: bool,
    /// The current eye's world-space camera offset from the center camera (`offset * right`, in
    /// metres), set by the `SetupRenderCamera` hook. The sun-shadow cascade correction adds
    /// `M * shadow_anchor_delta` to the cascade transform translation so the shadow lookup stays
    /// anchored at the (center-fit) shadow map instead of shifting with the per-eye camera. Zero when
    /// no per-eye offset is applied, making the correction a no-op.
    pub(crate) shadow_anchor_delta: glam::Vec3,
    /// Per-eye view-projection history for the FSR motion-vector correction, maintained by the
    /// `SetupRenderCamera` hook.
    pub(crate) vp_history: VpHistory,
    /// The render camera's center (un-offset) world transform (`m_TransformF`), snapshotted in
    /// `SetupRenderCamera` before the per-eye parallax offset is applied. The HUD panel pose reads
    /// this instead of the live `m_TransformF`, which by the time `draw_quad` runs on eye 0 already
    /// carries eye 0's half-IPD offset. Using the offset transform would shift the cached panel
    /// position toward eye 0, doubling the stereo disparity for eye 1.
    pub(crate) center_transform: Option<Matrix4>,
    /// The active camera's world position (`m_TransformF` translation) as the sun-shadow cascade fit
    /// read it this frame, captured by the `ShadowManager::UpdateRender` hook. The cascade transform
    /// is baked relative to it, and the material shaders anchor the shadow lookup at the render
    /// camera instead, so the cascade correction re-anchors the lookup to this point.
    pub(crate) shadow_fit_position: Option<glam::Vec3>,
}
impl StereoState {
    const fn new() -> Self {
        Self {
            active: false,
            draw_index: 0,
            dispatch_ordinal: 0,
            last_dispatch: true,
            shadow_anchor_delta: glam::Vec3::ZERO,
            vp_history: VpHistory::new(),
            center_transform: None,
            shadow_fit_position: None,
        }
    }
}

/// One real frame of view-projection snapshots, current and previous.
///
/// The engine's velocity pass computes `curUV - prevUV` with the *per-eye* current view-projection
/// but a single, sim-side *center* previous view-projection, so in stereo every static pixel carries
/// a spurious lateral motion vector: the eye-vs-center parallax, depth-dependent and of opposite sign
/// per eye. FSR then mis-reprojects each eye's temporal history, which shows as per-eye shimmer on
/// high-contrast edges (sun-shadow boundaries especially), worst under head motion. The FSR
/// velocity-decode pass uses these snapshots to re-anchor each vector at the eye's own previous pose
/// (`fsr::velocity_decode`).
pub struct VpHistory {
    /// The previous frame's center (un-offset) view-projection per eye -- the matrix the engine's
    /// velocity pass reprojects with. Per eye because the snapshot carries the projection of the
    /// dispatch it was taken in, and the eyes' projections differ on a headset with asymmetric
    /// frusta.
    pub prev_center: [Option<glam::Mat4>; 2],
    /// The previous frame's final per-eye view-projections (eye offset applied).
    pub prev_eye: [Option<glam::Mat4>; 2],
    /// This frame's center view-projection per eye, snapshotted before the per-eye patches in that
    /// eye's dispatch.
    pub cur_center: [Option<glam::Mat4>; 2],
    /// This frame's final per-eye view-projections, the matrices each dispatch rasterizes with but
    /// for the FSR jitter, which [`cur_jitter`](Self::cur_jitter) holds apart (see
    /// [`crate::fsr::jitter::jitter_free_reprojection`]).
    pub cur_eye: [Option<glam::Mat4>; 2],
    /// This frame's final per-eye view matrices (eye offset applied). Used by CPU-side
    /// projection paths such as the light-glow sprite, which cannot be fixed by shader constant remaps.
    pub cur_eye_view: [Option<glam::Mat4>; 2],
    /// This frame's final per-eye reverse-Z projections (eye offset applied). Paired with
    /// [`cur_eye_view`](Self::cur_eye_view) for rebuilding the per-eye offset view-projection.
    pub cur_eye_projection_f: [Option<glam::Mat4>; 2],
    /// The FSR jitter's clip-space offset each eye rasterizes with this frame, on top of
    /// [`cur_eye`](Self::cur_eye) (zero without one; see [`crate::fsr::jitter`]).
    pub cur_jitter: [glam::Vec2; 2],
}
impl VpHistory {
    const fn new() -> Self {
        Self {
            prev_center: [None, None],
            prev_eye: [None, None],
            cur_center: [None, None],
            cur_eye: [None, None],
            cur_eye_view: [None, None],
            cur_eye_projection_f: [None, None],
            cur_jitter: [glam::Vec2::ZERO; 2],
        }
    }

    /// Advance the history one real frame: this frame's snapshots become the previous frame's.
    /// Called at the start of the eye-0 render-camera rebuild.
    pub(crate) fn rotate(&mut self) {
        self.prev_center = [self.cur_center[0].take(), self.cur_center[1].take()];
        self.prev_eye = [self.cur_eye[0].take(), self.cur_eye[1].take()];
        self.cur_jitter = [glam::Vec2::ZERO; 2];
        self.cur_eye_view = [None, None];
        self.cur_eye_projection_f = [None, None];
    }

    /// The two clip-to-previous-clip reprojection matrices for `eye`: the center reprojection the
    /// engine's velocity pass encodes, and the per-eye reprojection FSR wants. `None` until a full
    /// frame of history exists. The products are formed in `f64`: the view-projections carry
    /// world-scale translations that only cancel between `prevVP` and `inv(curVP)` at
    /// higher-than-`f32` precision.
    ///
    /// Both take the current position from the jittered render; the eye's reprojection carries no
    /// jitter (see [`crate::fsr::jitter::jitter_free_reprojection`]).
    pub fn reprojection_matrices(&self, eye: usize) -> Option<(Matrix4, Matrix4)> {
        let cur = (*self.cur_eye.get(eye)?)?;
        let prev_center = (*self.prev_center.get(eye)?)?;
        let prev_eye = (*self.prev_eye.get(eye)?)?;
        let jitter = *self.cur_jitter.get(eye)?;
        Some((
            crate::fsr::jitter::reprojection_from_jittered(prev_center, cur, jitter),
            crate::fsr::jitter::jitter_free_reprojection(prev_eye, cur, jitter),
        ))
    }
}

/// Global live stereo state, written by the Draw driver and the camera hooks and read by the render
/// hooks through the accessors below.
pub(crate) static STEREO_STATE: Mutex<StereoState> = Mutex::new(StereoState::new());

/// Mark the frame in flight as rendered in stereo. Called by the Draw driver at the start of each
/// VR frame.
pub fn mark_stereo_frame() {
    STEREO_STATE.lock().active = true;
}

/// Enter one of the frame's dispatches: the physical eye it draws, its 0-based ordinal within the
/// frame, and whether it is the frame's last. Called by the Draw driver before each dispatch.
pub fn enter_dispatch(eye: usize, ordinal: usize, last: bool) {
    let mut state = STEREO_STATE.lock();
    state.draw_index = eye;
    state.dispatch_ordinal = ordinal;
    state.last_dispatch = last;
}

/// The render camera's centre (un-offset) world transform this frame (see
/// [`StereoState::center_transform`]), or `None` before the camera hook has recorded one.
#[inline]
pub fn center_transform() -> Option<Matrix4> {
    STEREO_STATE.lock().center_transform
}

/// Record the active camera's world position as the sun-shadow cascade fit reads it this frame (see
/// [`StereoState::shadow_fit_position`]).
pub fn record_shadow_fit_position(position: glam::Vec3) {
    STEREO_STATE.lock().shadow_fit_position = Some(position);
}

/// The positions the sun-shadow lookup is re-anchored between, read together.
#[derive(Clone, Copy)]
pub struct ShadowAnchor {
    /// The current eye's world-space offset from the centre camera (see
    /// [`StereoState::shadow_anchor_delta`]).
    pub eye_delta: glam::Vec3,
    /// The render camera's centre position, if the camera hook recorded one this frame.
    pub center: Option<glam::Vec3>,
    /// The camera position the cascade fit read, if the fit ran.
    pub fit: Option<glam::Vec3>,
}

/// The sun-shadow anchor positions for the dispatch in flight.
pub fn shadow_anchor() -> ShadowAnchor {
    let state = STEREO_STATE.lock();
    ShadowAnchor {
        eye_delta: state.shadow_anchor_delta,
        center: state
            .center_transform
            .map(|center| glam::Vec3::new(center.data[12], center.data[13], center.data[14])),
        fit: state.shadow_fit_position,
    }
}

/// Read the per-eye view-projection history under its lock.
pub fn with_vp_history<R>(read: impl FnOnce(&VpHistory) -> R) -> R {
    read(&STEREO_STATE.lock().vp_history)
}

/// The view and reverse-Z projection the eye being drawn rasterizes with this frame, offset
/// applied, or `None` before the camera hook has recorded them.
pub fn current_eye_view() -> Option<(glam::Mat4, glam::Mat4)> {
    let state = STEREO_STATE.lock();
    let eye = state.draw_index;
    let history = &state.vp_history;
    Some((
        (*history.cur_eye_view.get(eye)?)?,
        (*history.cur_eye_projection_f.get(eye)?)?,
    ))
}

/// Whether the current frame is being rendered in stereo.
#[inline]
pub fn active() -> bool {
    STEREO_STATE.lock().active
}

/// The physical eye currently being drawn (0 = left, 1 = right).
#[inline]
pub fn draw_index() -> usize {
    STEREO_STATE.lock().draw_index
}

/// True while the Draw driver is rendering the physical right eye of a stereo frame.
#[inline]
pub fn is_second_eye() -> bool {
    let state = STEREO_STATE.lock();
    state.active && state.draw_index == 1
}

/// True for a scene dispatch that follows another scene dispatch this frame.
#[inline]
pub fn later_scene() -> bool {
    let state = STEREO_STATE.lock();
    dispatch::later_scene_dispatch(state.active, state.dispatch_ordinal)
}

/// True on the frame's last dispatch, regardless of its physical eye.
#[inline]
pub fn last_dispatch() -> bool {
    STEREO_STATE.lock().last_dispatch
}

/// The current dispatch's 0-based ordinal within the frame (see [`StereoState::dispatch_ordinal`]).
#[inline]
pub fn dispatch_ordinal() -> usize {
    STEREO_STATE.lock().dispatch_ordinal
}

/// The engine's current render-target size in pixels, read from
/// [`GraphicsEngine::m_BackBufferLinear`], or `None` if the engine is not up yet.
///
/// This is the size the scene is rendered at, and it is deliberately *not* `Device::m_BackBuffer`,
/// the DXGI swapchain's buffer. Stock, the two are the same surface -- the engine builds
/// `m_BackBufferLinear` as a format alias of swapchain buffer 0 -- but they answer different
/// questions, and they diverge outright while the mod owns the back buffer, which is the default in
/// a session (`docs/mod/stereo/swapchain-ownership.md`). Anything sizing a resource *to the render* --
/// capture textures, HUD targets -- wants this; only code addressing the presented surface wants the
/// swapchain's.
#[inline]
pub fn render_size() -> Option<(u32, u32)> {
    // SAFETY: the engine singleton and its surface pointer are read under the same discipline as the
    // rest of the render hooks; both are null-checked.
    let texture = unsafe {
        jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()?
            .m_BackBufferLinear
            .as_ref()?
    };
    let size = (u32::from(texture.m_Width), u32::from(texture.m_Height));
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// One eye's slice of [`render_size`]. Under single-pass double-wide the render target holds both
/// eyes side by side, so the per-eye width is half of it.
#[inline]
pub fn per_eye_render_size() -> Option<(u32, u32)> {
    let (width, height) = render_size()?;
    let width = if layout::double_wide_active() {
        (width / 2).max(1)
    } else {
        width
    };
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each eye's correction reprojects through that eye's own previous centre snapshot: with
    /// asymmetric frusta the two dispatches' centre view-projections carry different projections,
    /// and the later dispatch's must not stand in for the earlier one's.
    #[test]
    fn each_eye_reprojects_through_its_own_centre_snapshot() {
        let left = glam::Mat4::from_cols_array(&[
            1.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -0.08, 0.0, 0.0, -1.0, 0.0, 0.0, 0.02, 0.0,
        ]);
        let right = glam::Mat4::from_cols_array(&[
            1.1, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.08, 0.0, 0.0, -1.0, 0.0, 0.0, 0.02, 0.0,
        ]);
        let mut history = VpHistory::new();
        for _ in 0..2 {
            history.rotate();
            history.cur_center = [Some(left), Some(right)];
            history.cur_eye = [Some(left), Some(right)];
        }
        for eye in 0..2 {
            let (center, own) = history.reprojection_matrices(eye).unwrap();
            assert!(
                glam::Mat4::from(center).abs_diff_eq(glam::Mat4::IDENTITY, 1e-5),
                "eye {eye}: the centre reprojection of a still camera is the identity"
            );
            assert!(glam::Mat4::from(own).abs_diff_eq(glam::Mat4::IDENTITY, 1e-5));
        }
    }
}
