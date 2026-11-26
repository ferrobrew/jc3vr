//! Per-rendered-frame VR wiring: turn the located views into the headpose the camera follows and
//! the per-eye render parameters (off-axis projection and head-local eye displacement) the
//! `SetupRenderCamera` hook applies.
//!
//! This is the bridge between [`crate::vr`]'s frame API and the render hooks (which run during the
//! eye Draws and must not touch the runtime's locks). The parameters are extracted once at the top
//! of the frame into per-frame slots ([`RENDER_PARAMS`] and its siblings, each a
//! [`RenderFrameSlot`]), so the draw task keeps reading the frame it is recording while the game
//! thread prepares the next one. The head pose is published through the [`crate::headpose::xr`]
//! source.

use glam::{Quat, Vec2, Vec3};
use jc3gi::{camera::camera::Camera, types::math::Matrix4};
use jc3vr_core::render_frame::RenderFrameSlot;
use parking_lot::Mutex;

use crate::{
    headpose,
    headpose::{FrameTelemetry, config::HEADPOSE_CONFIG, participants::head_participants},
    tuning::VIEW,
    vr::{
        EyeView, Fov, FrameInputs, FreezeMode, LocatedFrame, OffAxisProjection, VrConfig,
        config::ProjectionConvention,
        convert::{quat, vec3},
        harness,
        harness::HeadsetSubstitute,
        participants::frame_participants,
        pose_control,
    },
};

/// The per-eye render parameters the `SetupRenderCamera` hook applies while a VR frame is in flight.
#[derive(Copy, Clone)]
pub struct EyeRenderParams {
    /// The standard-depth off-axis projection, to write into `m_Projection` *before*
    /// `SetupRenderCamera` under [`ProjectionConvention::EnginePreReverseZ`].
    pub projection_standard: Matrix4,
    /// The reverse-Z off-axis projection, to write *after* `SetupRenderCamera` under
    /// [`ProjectionConvention::ManualReverseZ`].
    pub projection_reverse_z: Matrix4,
    /// The eye displacement in the located center head's axes, scaled to game units.
    pub local_offset: Vec3,
    /// This eye's orientation relative to the center head pose (the display canting from
    /// `locate_views`), as a head-local rotation. The camera hook applies it to the render camera's
    /// basis, about the already-offset eye position, so the rendered content matches the per-eye pose
    /// submitted to the compositor. Identity on parallel-panel HMDs; the Valve Index cants each eye
    /// ~5°, and dropping it leaves the two eyes rotationally mismatched, so the stereo will not fuse.
    pub orientation_delta: Quat,
    /// Which depth convention to write the projection in.
    pub convention: ProjectionConvention,
    /// The frame's FSR sub-pixel jitter, already applied to both projections above, with the
    /// projections it was applied to; `None` when the jitter is off. See [`crate::fsr::jitter`].
    pub jitter: Option<AppliedJitter>,
}

/// The FSR jitter an eye's projections carry this frame.
#[derive(Copy, Clone)]
pub struct AppliedJitter {
    /// The clip-space offset (the NDC translation times `w`).
    pub ndc: Vec2,
    /// The offset's index in the sequence.
    pub phase: u32,
    /// The sequence's length.
    pub phase_count: u32,
    /// [`EyeRenderParams::projection_standard`] before the jitter.
    standard: Matrix4,
    /// [`EyeRenderParams::projection_reverse_z`] before the jitter.
    reverse_z: Matrix4,
}

impl EyeRenderParams {
    /// These parameters with both projections jittered by the frame's `jitter`.
    pub fn jittered(self, frame: crate::fsr::jitter::FrameJitter) -> Self {
        let jitter = crate::fsr::jitter::translation(frame.ndc);
        Self {
            projection_standard: Matrix4::from(jitter * glam::Mat4::from(self.projection_standard)),
            projection_reverse_z: Matrix4::from(
                jitter * glam::Mat4::from(self.projection_reverse_z),
            ),
            jitter: Some(AppliedJitter {
                ndc: frame.ndc,
                phase: frame.phase,
                phase_count: frame.phase_count,
                standard: self.projection_standard,
                reverse_z: self.projection_reverse_z,
            }),
            ..self
        }
    }

    /// The jitter's clip-space offset in this frame's projections (zero without one).
    #[inline]
    pub fn jitter_ndc(self) -> Vec2 {
        self.jitter.map_or(Vec2::ZERO, |jitter| jitter.ndc)
    }

    /// The reverse-Z projection without the frame's jitter, bit for bit as it was before the jitter
    /// went on: for what is drawn after FSR has resolved the eye (the HUD, the overlays, the scope
    /// lens) and must hold still while the scene jitters, and for the motion FSR is handed, which
    /// must carry none. Undoing the jitter in `f32` instead would leave a rounding in the
    /// view-projection's world-scale translation that changes from frame to frame.
    #[inline]
    pub fn unjittered_projection_reverse_z(self) -> glam::Mat4 {
        glam::Mat4::from(
            self.jitter
                .map_or(self.projection_reverse_z, |jitter| jitter.reverse_z),
        )
    }

    /// The standard-depth projection without the frame's jitter, bit for bit.
    #[inline]
    pub fn unjittered_projection_standard(self) -> glam::Mat4 {
        glam::Mat4::from(
            self.jitter
                .map_or(self.projection_standard, |jitter| jitter.standard),
        )
    }

    /// The reverse-Z projection the render camera holds after `SetupRenderCamera` under this frame's
    /// [`convention`](Self::convention), without the jitter: the engine's own reverse-Z remap of the
    /// standard-depth projection
    /// ([`Camera::SetupRenderCamera`](jc3gi::camera::camera::Camera::SetupRenderCamera)) under
    /// [`ProjectionConvention::EnginePreReverseZ`], so it matches the camera's bit for bit but for
    /// the jitter.
    pub fn unjittered_camera_projection(self) -> Matrix4 {
        match self.convention {
            ProjectionConvention::EnginePreReverseZ => {
                engine_reverse_z(Matrix4::from(self.unjittered_projection_standard()))
            }
            ProjectionConvention::ManualReverseZ => {
                Matrix4::from(self.unjittered_projection_reverse_z())
            }
        }
    }

    /// Resolve displacement and cant from the final selected, interpolated center camera.
    pub fn resolve(self, center: glam::Mat4) -> glam::Mat4 {
        let mut eye = center;
        eye.w_axis += self.world_offset(center).extend(0.0);
        eye * glam::Mat4::from_quat(self.orientation_delta)
    }

    #[inline]
    pub fn world_offset(self, center: glam::Mat4) -> Vec3 {
        center.transform_vector3(self.local_offset)
    }
}

/// The engine's reverse-Z remap of a standard-depth projection in its row-major layout, as
/// `Camera::SetupRenderCamera` applies it: each row's depth column becomes its `w` column less
/// itself, so the clip depth becomes `w - z`.
fn engine_reverse_z(standard: Matrix4) -> Matrix4 {
    let mut data = standard.data;
    for row in data.as_chunks_mut::<4>().0 {
        row[2] = row[3] - row[2];
    }
    Matrix4 { data }
}

/// The per-eye render parameters for the frame in flight, or `None` when no VR frame is rendering
/// (flatscreen, or a VR frame the runtime asked to skip). Kept per frame, apart from the OpenXR
/// runtime state, so the camera hook and the draw task read it without the runtime's locks, and
/// the draw task reads the frame it records (see [`jc3vr_core::render_frame`]).
static RENDER_PARAMS: RenderFrameSlot<Option<[EyeRenderParams; 2]>> = RenderFrameSlot::new();

/// The per-eye render parameters captured on the first frame of a [`FreezeMode::FullCamera`] freeze
/// and reused until it ends, so the eyes hold still around the pinned camera. `None` whenever that
/// mode is not engaged.
static FROZEN_EYE_PARAMS: Mutex<Option<FrozenEyeParams>> = Mutex::new(None);

/// The per-eye render parameters a full-camera freeze holds, with the visual harness's headset rig
/// they were captured under: a configuration that substitutes another rig recaptures them, since
/// the rig is what the parameters were derived from.
#[derive(Copy, Clone)]
struct FrozenEyeParams {
    rig: Option<HeadsetSubstitute>,
    params: [EyeRenderParams; 2],
}

/// The standard-depth, symmetric, union-FOV projection that bounds *both* eyes' off-axis frusta, for
/// widening the scene cull frustum (see [`cull_projection_standard`]). `None` when no VR frame is
/// rendering.
static CULL_PROJECTION: RenderFrameSlot<Option<Matrix4>> = RenderFrameSlot::new();

/// The standard-depth, symmetric projection that bounds both eyes' off-axis frusta exactly, without
/// the cull's padding ([`FrameTuning`](crate::tuning::FrameTuning)), for the sun-shadow cascade fit
/// (see [`shadow_fit_projection_standard`]). `None` when no VR frame is rendering.
static SHADOW_FIT_PROJECTION: RenderFrameSlot<Option<Matrix4>> = RenderFrameSlot::new();

/// The standard-depth, symmetric projection that bounds both eyes' off-axis frusta exactly this
/// frame, with neither the cull's padding nor the shadow fit's hold (see
/// [`eyes_union_projection_standard`]). `None` when no VR frame is rendering.
static EYES_UNION_PROJECTION: RenderFrameSlot<Option<Matrix4>> = RenderFrameSlot::new();

/// The field of view the sun-shadow cascade fit is currently held at (see [`hold_fit_fov`]). Kept
/// across frames that render no VR frame, so a skipped frame does not release the hold.
static SHADOW_FIT_FOV: Mutex<Option<Fov>> = Mutex::new(None);

/// The located views of the frame in flight, as the recorder's sidecar wants them: the raw eye
/// poses in LOCAL space, the eye fields of view, and the predicted display time. Published next to
/// [`RENDER_PARAMS`] so the render thread can read them at the eye-capture point without the
/// runtime lock. `None` when no VR frame is rendering.
static CAPTURE_VIEWS: RenderFrameSlot<Option<CaptureViews>> = RenderFrameSlot::new();

/// The per-frame view record the recorder stores per captured frame.
#[derive(Copy, Clone, Debug)]
pub struct CaptureViews {
    /// The runtime's predicted display time for the frame, in the runtime's nanosecond clock.
    pub predicted_display_time_ns: i64,
    /// Per eye: the raw located pose (position, then orientation as `x, y, z, w`) in LOCAL space,
    /// and the field of view as `left, right, up, down` angles in radians.
    pub eyes: [CaptureEyeView; 2],
}

/// One eye's located view for the recorder's sidecar.
#[derive(Copy, Clone, Debug)]
pub struct CaptureEyeView {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub fov: [f32; 4],
}

/// The located views for the frame in flight, or `None` when no VR frame is rendering.
#[inline]
pub fn capture_views() -> Option<CaptureViews> {
    CAPTURE_VIEWS.get().flatten()
}

/// The render parameters for `eye` (`0` = left, `1` = right) this frame, or `None` when no VR frame
/// is rendering. Read by the `SetupRenderCamera` hook.
#[inline]
pub fn render_params(eye: usize) -> Option<EyeRenderParams> {
    RENDER_PARAMS.get().flatten().map(|p| p[eye.min(1)])
}

/// The symmetric union-FOV projection covering both eyes' off-axis frusta, in the engine's
/// standard-depth `m_ProjectionF` layout, or `None` when no VR frame is rendering. The occluder-cull
/// hook writes this into the shared cull camera's `m_ProjectionF` so the visibility cull -- which the
/// engine runs once per frame against the narrower center camera -- covers everything either eye can
/// see, removing the black voids and pop-in at the outer edges.
#[inline]
pub fn cull_projection_standard() -> Option<Matrix4> {
    CULL_PROJECTION.get().flatten()
}

/// The symmetric projection covering both eyes' off-axis frusta with no padding beyond the eyes'
/// lateral shift, in the engine's standard-depth `m_ProjectionF` layout, or `None` when no VR frame
/// is rendering. The sun-shadow cascade fit sizes each cascade's sphere from its camera's field of
/// view; fitting to the padded cull projection instead stretched every cascade over the padding,
/// coarsening its texels for geometry neither eye sees.
#[inline]
pub fn shadow_fit_projection_standard() -> Option<Matrix4> {
    SHADOW_FIT_PROJECTION.get().flatten()
}

/// The symmetric projection that bounds both eyes' off-axis frusta exactly this frame (each eye's
/// extent taken in the head frame, so a canted eye's outer side is inside it, plus the lateral
/// margin for the eyes' offset from the centre), in the engine's standard-depth layout, or `None`
/// when no VR frame is rendering. For a cull that runs once from the centred camera on the GPU,
/// where the cull projection's padding would cost work for nothing.
#[inline]
pub fn eyes_union_projection_standard() -> Option<Matrix4> {
    EYES_UNION_PROJECTION.get().flatten()
}

/// Clear the per-eye render parameters (and the cull projection), so the camera hook falls back to
/// flatscreen stereo. Called when no VR frame renders this frame (session down, or `should_render`
/// false).
pub fn clear_render_params() {
    RENDER_PARAMS.set(None);
    CAPTURE_VIEWS.set(None);
    CULL_PROJECTION.set(None);
    SHADOW_FIT_PROJECTION.set(None);
    EYES_UNION_PROJECTION.set(None);
}

/// Drive one rendered VR frame's pose and per-eye camera parameters from the located views: reduce
/// the two eye poses to a cockpit-frame center pose, compose it into world space and publish it as
/// the headpose, and stash the per-eye off-axis projections and world offsets for the camera hook.
///
/// Only call when [`LocatedFrame::should_render`] is true; the located eye poses are meaningless
/// otherwise. Takes no OpenXR runtime lock; publishes through the headpose state and the
/// [`RENDER_PARAMS`] slot, both independently locked.
pub fn begin_render_frame(frame: &LocatedFrame, cfg: &VrConfig) {
    render_frame(
        [frame.eye_view(0), frame.eye_view(1)],
        frame.predicted_display_time(),
        cfg,
    );
}

/// The body of [`begin_render_frame`], over the frame's views. `display_time` is the VR frame's
/// predicted display time.
fn render_frame(views: [EyeView; 2], display_time: openxr::Time, cfg: &VrConfig) {
    // Cockpit-pose freeze diagnostic: reuse the first captured pose -- eye views plus the sim-driven
    // body frame and anchor -- so the head contributes the same thing every frame, isolating
    // per-frame-input-driven artifacts (pose noise, head-bone idle animation) from intrinsic render
    // ones. The captured centre pose is then hand-editable from the Camera tab, so a mis-scaled
    // screen-space pass can be driven by an exact, repeatable yaw or translation step. The
    // full-camera mode is not handled here at all: it pins the composed camera in the
    // `SetupRenderCamera` hook, downstream of everything this function publishes (see
    // `crate::vr::FreezeMode` and `crate::vr::pose_control`).
    let frozen = if cfg.freeze_mode == FreezeMode::CockpitPose {
        Some(pose_control::frozen_frame(|| {
            (
                views,
                headpose::xr::body_rotation(),
                headpose::anchor().unwrap_or(Vec3::ZERO),
            )
        }))
    } else {
        // The full-camera freeze owns the capture from the `SetupRenderCamera` hook, so only drop it
        // here when nothing is frozen at all.
        if cfg.freeze_mode == FreezeMode::Off {
            pose_control::clear();
        }
        None
    };
    let [eye0, eye1] = frozen.map_or(views, |f| f.eyes);

    // Advance the grapple reel-in filter at frame cadence, before the body frame and anchor below
    // are read through it: the engine rotates the body the instant a reel begins, and the ~33 Hz
    // input-tick advance alone left up to a tick of that rotation reaching the view before the
    // filter engaged (issue #36 telemetry). At frame cadence the held pre-reel frame is the one
    // the previous render composed with, so an instant engage is seamless.
    // The pose hangs from the view head, the neutral head while the view is held off the walking
    // animation, so the animation's bob stays on the body and out of the headset.
    let anchor_pair = headpose::anchor_pair();
    let anchor_raw = frozen.map_or_else(
        || anchor_pair.map_or(Vec3::ZERO, |pair| pair[1].view_head()),
        |f| f.anchor,
    );
    let participants = frame_participants();
    let head_participants = head_participants();
    let headpose_config = HEADPOSE_CONFIG.query(|c| *c);
    (head_participants.advance_grapple)(
        headpose::xr::body_rotation_followed(),
        frozen
            .is_none()
            .then(|| anchor_pair.map(|pair| anchor_raw - pair[1].root.w_axis.truncate()))
            .flatten(),
        headpose::sim::mode() == headpose::sim::HeadMode::OnFoot,
        &headpose_config.grapple,
    );

    let pos0 = vec3(eye0.pose.position);
    let pos1 = vec3(eye1.pose.position);
    let center_position = 0.5 * (pos0 + pos1);
    // The true mid orientation (slerp of the two eyes), so the cockpit/head frame sits between the
    // eyes rather than on one of them. On canted panels the eyes' orientations differ, so anchoring
    // the center on one eye would bias the head (and everything keyed to it -- aim, the recenter
    // baseline) toward that eye and split the per-eye canting asymmetrically. Matches the runtime
    // head-pose stand-in ([`crate::vr::recenter::mid_pose`]), which is likewise the slerp-mid.
    let center_orientation = quat(eye0.pose.orientation).slerp(quat(eye1.pose.orientation), 0.5);

    let body_rotation = frozen.map_or_else(headpose::xr::body_rotation, |f| f.body_rotation);
    // The anchor goes through the grapple filter's landing rate limit; the previous-tick anchor is
    // offset by the same amount so the pair keeps the true tick delta for the engine's `dtf` lerp.
    let anchor = (head_participants.filter_anchor)(anchor_raw);
    let anchor_prev_raw = frozen.map_or_else(
        || anchor_pair.map_or(anchor_raw, |pair| pair[0].view_head()),
        |f| f.anchor,
    );

    // Compose a tick-spaced pose pair sharing the fresh HMD cockpit delta but differing in the
    // sim-driven body frame and head anchor (T1 vs T0), so the engine's `dtf` lerp smooths the
    // per-tick body/anchor motion between rendered frames. The previous-tick anchor and body rotation
    // fall back to the current-tick values until they are available, degenerating to no interpolation
    // rather than a bad one. Under the freeze diagnostic, prev == cur so the lerp is constant too.
    let body_rotation_prev = frozen.map_or_else(
        || {
            anchor_pair.map_or_else(headpose::xr::body_rotation_prev, |pair| {
                pair[0].previous_endpoint(headpose::xr::body_rotation_prev(), body_rotation)
            })
        },
        |f| f.body_rotation,
    );
    let anchor_prev = anchor - (anchor_raw - anchor_prev_raw);

    // Publish the raw cockpit-frame HMD pose (the tracking delta before the body-frame composition
    // below), so the camera hook can compose it onto the engine's own camera when the game owns the
    // camera outside gameplay (loading screens, teleports) — keeping head-tracking through the
    // transition without pinning the camera to the body (issue #27).
    headpose::xr::set_cockpit_pose(center_position, center_orientation);

    // The posture's head height and the roomscale cockpit shift. The per-eye offsets below stay
    // relative to the located centre, so they are untouched by either.
    let cockpit = (participants.compose_cockpit)(&FrameInputs {
        cockpit: center_position,
        orientation: center_orientation,
        body: body_rotation,
        world_scale: cfg.world_scale,
    });

    let cur = headpose::xr::compose(
        cockpit.cur,
        center_orientation,
        body_rotation,
        anchor,
        cfg.world_scale,
    );
    let cockpit_prev = anchor_pair.map_or(cockpit.prev, |pair| {
        pair[0].previous_endpoint(cockpit.prev, cockpit.cur)
    });
    let prev = headpose::xr::compose(
        cockpit_prev,
        center_orientation,
        body_rotation_prev,
        anchor_prev,
        cfg.world_scale,
    );
    headpose::xr::publish_pair(prev, cur);

    (head_participants.log_grapple_frame)(&FrameTelemetry {
        cockpit_orientation: center_orientation,
        cockpit_position: center_position,
        body_raw: headpose::xr::body_rotation_raw(),
        body_filtered: body_rotation,
        composed: cur.orientation,
        position: cur.position,
        // The raw anchor, so a capture shows the landing snap against the eased `position`.
        anchor: anchor_raw,
    });

    // The symmetric union-FOV cull projection: a single centred frustum that contains both eyes'
    // off-axis frusta. Each eye is laterally offset by ~IPD/2 and has its own asymmetric FOV, so the
    // superset bounds the wider of the two on each side (in tangent space) plus a near-plane margin
    // `s = (IPD/2) / near` for the lateral eye shift. `s·z` covers the shift exactly at the near plane
    // and over-covers (harmlessly) with distance, so nothing either eye can see is culled. The engine
    // culls at a single interpolated pose, so the cull padding pads each side's tangent outward on
    // top of that to hide edge pop-in under fast motion; the eye-shift margin and the padding are both
    // applied on the vertical axis too (flying pitch shifts the eyes vertically). Written standard-depth
    // to match the cull camera's `m_ProjectionF`.
    let (near_clip, far_clip) =
        crate::hooks::camera::main_camera_planes_or(Camera::DEFAULT_FAR_PLANE);
    let ipd = (pos1 - pos0).length();
    let margin = 0.5 * ipd / near_clip.max(1e-3);
    let pad = 1.0 + VIEW.get().frame.cull_fov_padding;
    // Widen each side in tangent space: scale the half-extent outward by `pad`, then push out by the
    // eye-shift margin in the side's own direction (`copysign` keeps left/down negative, right/up
    // positive). Vertical uses tangents here (not raw angles) so it receives the same treatment.
    let expand = |t: f32| t * pad + margin.copysign(t);
    // Clamp each padded half-angle safely below 90 deg. The tangent widen grows without bound as a
    // side nears 90 deg (`tan -> inf`), so on a wide-FOV headset the padding could otherwise push the
    // cull frustum to a degenerate, near-hemispherical projection. 85 deg (a ~170 deg cull FOV) is far
    // wider than any real per-eye FOV, so this only guards the degenerate tail and never clips a sane
    // headset's coverage.
    let max_half = 85f32.to_radians();
    let widen = |t: f32| expand(t).atan().clamp(-max_half, max_half);
    // Each eye's extent is taken in the centre head frame, so a canted eye's outer side, which
    // reaches past its own half-angles there, stays inside both frusta.
    let union = union_tangents(
        head_frame_tangents(
            eye0.fov,
            center_orientation.inverse() * quat(eye0.pose.orientation),
        ),
        head_frame_tangents(
            eye1.fov,
            center_orientation.inverse() * quat(eye1.pose.orientation),
        ),
    );
    let union_fov = Fov {
        left: widen(union[0]),
        right: widen(union[1]),
        up: widen(union[2]),
        down: widen(union[3]),
    };
    CULL_PROJECTION.set(Some(
        OffAxisProjection::new(union_fov, near_clip, far_clip).standard_depth,
    ));
    let exact = |t: f32| (t + margin.copysign(t)).atan().clamp(-max_half, max_half);
    let exact_fov = Fov {
        left: exact(union[0]),
        right: exact(union[1]),
        up: exact(union[2]),
        down: exact(union[3]),
    };
    EYES_UNION_PROJECTION.set(Some(
        OffAxisProjection::new(exact_fov, near_clip, far_clip).standard_depth,
    ));
    let fit_fov = {
        let mut held = SHADOW_FIT_FOV.lock();
        let fit = hold_fit_fov(*held, exact_fov);
        *held = Some(fit);
        fit
    };
    SHADOW_FIT_PROJECTION.set(Some(
        OffAxisProjection::new(fit_fov, near_clip, far_clip).standard_depth,
    ));

    let eye_params = |eye: crate::vr::EyeView, eye_position: Vec3| {
        EyeRenderParams {
            projection_standard: eye.projection.standard_depth,
            projection_reverse_z: eye.projection.reverse_z,
            local_offset: center_orientation.inverse()
                * ((eye_position - center_position) * cfg.world_scale),
            // The eye's orientation relative to the center head pose, in the head-local frame -- the
            // display canting. `center_orientation` is the slerp-mid, so each eye carries half the
            // inter-eye cant (symmetric); the camera hook applies it locally so each eye renders at the
            // orientation it is submitted with.
            orientation_delta: center_orientation.inverse() * quat(eye.pose.orientation),
            convention: ProjectionConvention::EnginePreReverseZ,
            jitter: None,
        }
    };

    let eye0_params = eye_params(eye0, pos0);
    let eye1_params = eye_params(eye1, pos1);

    // Under the full-camera freeze the render camera itself is pinned in world space (see the
    // `SetupRenderCamera` hook), so the head-local eye geometry is pinned with it. Otherwise live
    // changes in the located eye pair would move the eyes around the frozen center.
    let params = {
        let mut frozen_params = FROZEN_EYE_PARAMS.lock();
        if cfg.freeze_mode == FreezeMode::FullCamera {
            let rig = harness::headset_substitute();
            match *frozen_params {
                Some(frozen) if frozen.rig == rig => frozen.params,
                _ => {
                    let params = [eye0_params, eye1_params];
                    *frozen_params = Some(FrozenEyeParams { rig, params });
                    params
                }
            }
        } else {
            *frozen_params = None;
            [eye0_params, eye1_params]
        }
    };
    // The jitter goes on after the freeze picks the parameters, so a frozen pose keeps cycling the
    // sequence rather than holding the phase it was captured on.
    let params = match crate::fsr::jitter::frame_offset() {
        Some(jitter) => params.map(|p| p.jittered(jitter)),
        None => params,
    };
    RENDER_PARAMS.set(Some(params));
    // The recorder's sidecar wants the raw located views, not the rebased or frozen ones: they are
    // what the submitted images were rendered for, so post can reproject against them exactly.
    let capture_eye = |view: &EyeView| CaptureEyeView {
        position: [
            view.raw_pose.position.x,
            view.raw_pose.position.y,
            view.raw_pose.position.z,
        ],
        orientation: [
            view.raw_pose.orientation.x,
            view.raw_pose.orientation.y,
            view.raw_pose.orientation.z,
            view.raw_pose.orientation.w,
        ],
        fov: [
            view.fov.angle_left,
            view.fov.angle_right,
            view.fov.angle_up,
            view.fov.angle_down,
        ],
    };
    CAPTURE_VIEWS.set(Some(CaptureViews {
        predicted_display_time_ns: display_time.as_nanos(),
        eyes: [capture_eye(&views[0]), capture_eye(&views[1])],
    }));
}

/// The tangent extents `[left, right, up, down]` of an eye's frustum in the centre head frame: the
/// frustum's corner rays turned by the eye's orientation relative to the head (`orientation_delta`)
/// and projected onto the head's unit-depth plane. A frustum is a convex cone, so its projection is
/// the quadrilateral of its projected corners, and their bounds bound it. For a parallel eye these
/// are the eye's own tangents; a canted eye reaches further on its outer side.
fn head_frame_tangents(fov: openxr::Fovf, orientation_delta: Quat) -> [f32; 4] {
    let (tl, tr) = (fov.angle_left.tan(), fov.angle_right.tan());
    let (tu, td) = (fov.angle_up.tan(), fov.angle_down.tan());
    let mut bounds = [
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
    ];
    for (x, y) in [(tl, tu), (tr, tu), (tl, td), (tr, td)] {
        let ray = orientation_delta * Vec3::new(x, y, -1.0);
        // A corner turned to or past the head's side plane has no finite tangent there; the
        // widening below clamps the bound short of a right angle in any case.
        let depth = (-ray.z).max(1e-3);
        let (x, y) = (ray.x / depth, ray.y / depth);
        bounds = [
            bounds[0].min(x),
            bounds[1].max(x),
            bounds[2].max(y),
            bounds[3].min(y),
        ];
    }
    bounds
}

/// The tangent extents bounding two eyes' [`head_frame_tangents`].
fn union_tangents(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].max(b[1]),
        a[2].max(b[2]),
        a[3].min(b[3]),
    ]
}

/// The relative change in any side's tangent that releases [`hold_fit_fov`]'s hold. Far above the
/// float noise of a tracked eye separation, and far below the step of an IPD adjustment.
const FIT_FOV_HOLD_TOLERANCE: f32 = 1e-3;

/// The field of view to fit the sun-shadow cascades to this frame: `held` while `fresh` stays
/// within [`FIT_FOV_HOLD_TOLERANCE`] of it on every side, and `fresh` otherwise.
///
/// The engine rounds each cascade's bounding-sphere radius up to a whole unit, and the fit's
/// lateral margin is the tracked eye separation divided by the near plane, so the float noise of
/// the located eye poses is amplified into the fitted frustum. A cascade whose radius sits on a
/// whole-unit boundary would otherwise flip its size, and with it its texel scale, on that noise.
fn hold_fit_fov(held: Option<Fov>, fresh: Fov) -> Fov {
    let within = |held: f32, fresh: f32| {
        (fresh.tan() - held.tan()).abs() <= FIT_FOV_HOLD_TOLERANCE * held.tan().abs()
    };
    match held {
        Some(held)
            if within(held.left, fresh.left)
                && within(held.right, fresh.right)
                && within(held.up, fresh.up)
                && within(held.down, fresh.down) =>
        {
            held
        }
        _ => fresh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jitter_comes_off_bit_for_bit_and_the_camera_projection_matches_the_engines() {
        let projection = OffAxisProjection::new(fov(0.4), 0.1, 38_400.0);
        let params = EyeRenderParams {
            projection_standard: projection.standard_depth,
            projection_reverse_z: projection.reverse_z,
            local_offset: Vec3::ZERO,
            orientation_delta: Quat::IDENTITY,
            convention: ProjectionConvention::EnginePreReverseZ,
            jitter: None,
        };
        let jittered = params.jittered(crate::fsr::jitter::FrameJitter {
            ndc: Vec2::new(1.4e-4, -0.9e-4),
            phase: 3,
            phase_count: 8,
        });
        assert_eq!(jittered.jitter_ndc(), Vec2::new(1.4e-4, -0.9e-4));
        assert_ne!(
            jittered.projection_reverse_z.data,
            params.projection_reverse_z.data
        );
        assert_eq!(
            Matrix4::from(jittered.unjittered_projection_reverse_z()).data,
            params.projection_reverse_z.data
        );
        assert_eq!(
            jittered.unjittered_camera_projection().data,
            engine_reverse_z(params.projection_standard).data
        );
        // The engine's remap of the standard projection is the reverse-Z one.
        let remapped = glam::Mat4::from(engine_reverse_z(params.projection_standard));
        assert!(remapped.abs_diff_eq(glam::Mat4::from(params.projection_reverse_z), 1e-5));
    }

    fn fov(scale: f32) -> Fov {
        Fov {
            left: (-2.5 * scale).atan(),
            right: (2.5 * scale).atan(),
            up: (2.4 * scale).atan(),
            down: (-2.4 * scale).atan(),
        }
    }

    fn xr_fov(left: f32, right: f32, up: f32, down: f32) -> openxr::Fovf {
        openxr::Fovf {
            angle_left: left.to_radians(),
            angle_right: right.to_radians(),
            angle_up: up.to_radians(),
            angle_down: down.to_radians(),
        }
    }

    #[test]
    fn a_parallel_eye_keeps_its_own_tangents_in_the_head_frame() {
        let fov = xr_fov(-54.0, 51.0, 55.8, -55.8);
        let [l, r, u, d] = head_frame_tangents(fov, Quat::IDENTITY);
        assert_eq!(
            [l, r, u, d],
            [
                fov.angle_left.tan(),
                fov.angle_right.tan(),
                fov.angle_up.tan(),
                fov.angle_down.tan()
            ]
        );
    }

    /// A left eye canted outward by five degrees reaches five degrees further left in the head frame
    /// and five degrees less far right, and its outer corners rise above its own vertical extent.
    #[test]
    fn a_canted_eye_reaches_further_on_its_outer_side() {
        let fov = xr_fov(-54.0, 51.0, 45.0, -45.0);
        let cant = Quat::from_rotation_y(5f32.to_radians());
        let [l, r, u, d] = head_frame_tangents(fov, cant);
        assert!(
            (l.atan().to_degrees() + 59.0).abs() < 1e-3,
            "{}",
            l.atan().to_degrees()
        );
        assert!((r.atan().to_degrees() - 46.0).abs() < 1e-3);
        assert!(
            u > 1.0 && d < -1.0,
            "the outer corners are nearer the head's side plane"
        );
        assert!((u + d).abs() < 1e-5);
        let mirrored = head_frame_tangents(xr_fov(-51.0, 54.0, 45.0, -45.0), cant.inverse());
        let [ul, ur, ..] = union_tangents([l, r, u, d], mirrored);
        assert!((ul + ur).abs() < 1e-5);
        assert!((ur.atan().to_degrees() - 59.0).abs() < 1e-3);
    }

    #[test]
    fn fit_fov_hold_ignores_tracking_noise() {
        let held = fov(1.0);
        assert_eq!(hold_fit_fov(Some(held), fov(1.0 + 1e-5)), held);
        assert_eq!(hold_fit_fov(Some(held), fov(1.0 - 1e-5)), held);
    }

    #[test]
    fn fit_fov_hold_releases_on_a_real_change() {
        let held = fov(1.0);
        assert_eq!(hold_fit_fov(Some(held), fov(1.01)), fov(1.01));
        assert_eq!(hold_fit_fov(None, held), held);
        let one_side = Fov {
            up: 2.5f32.atan(),
            ..held
        };
        assert_eq!(hold_fit_fov(Some(held), one_side), one_side);
    }

    #[test]
    fn final_center_transports_authored_rotation_and_interpolation_without_doubling_hmd_roll() {
        let hmd = Quat::from_rotation_z(0.4) * Quat::from_rotation_x(-0.2);
        let body = Quat::from_rotation_y(0.7);
        let authored0 = Quat::from_rotation_x(0.3);
        let authored1 = Quat::from_rotation_z(-0.6);
        let local = Vec3::new(0.032, 0.002, -0.003);
        let sampled = hmd * local;
        let cant = Quat::from_rotation_y(0.08);
        let params = EyeRenderParams {
            projection_standard: glam::Mat4::IDENTITY.into(),
            projection_reverse_z: glam::Mat4::IDENTITY.into(),
            local_offset: hmd.inverse() * sampled,
            orientation_delta: cant,
            convention: ProjectionConvention::ManualReverseZ,
            jitter: None,
        };
        for alpha in [0.0, 0.3, 1.0] {
            let rotation = (body * authored0 * hmd).slerp(body * authored1 * hmd, alpha);
            let position = Vec3::new(10.0, 2.0, -3.0).lerp(Vec3::new(11.0, 2.1, -4.0), alpha);
            let center = glam::Mat4::from_rotation_translation(rotation, position);
            let eye = params.resolve(center);
            assert!(
                eye.w_axis
                    .truncate()
                    .abs_diff_eq(position + rotation * local, 1.0e-5)
            );
            assert!(
                eye.transform_vector3(Vec3::NEG_Z)
                    .abs_diff_eq(rotation * cant * Vec3::NEG_Z, 1.0e-5)
            );
        }
        let ordinary = glam::Mat4::from_quat(body * hmd);
        assert!(
            params
                .world_offset(ordinary)
                .abs_diff_eq(body * sampled, 1.0e-6)
        );
    }
}
