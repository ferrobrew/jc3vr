//! Recentering the cockpit: the baseline the per-eye poses are expressed relative to, the manual
//! [`recenter`], the deferred [`request_recenter`] and [`request_seat_recenter`], and the
//! gameplay-resume auto-recenter ([`auto_recenter_tick`]).

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use openxr as xr;
use parking_lot::Mutex;

use crate::{
    config::Config,
    recorder::tracking::format::RecenterEvent,
    vr::{is_running, state::lock_state},
};

/// Recenter the cockpit: re-base the stored baseline from the latest located VIEW-space pose, taking
/// its position and yaw only (the cockpit model). The frame loop consumes the baseline when
/// mapping per-eye poses (see [`crate::vr::frame`]). No-op until a frame has located a head pose.
///
/// While the roomscale chase keeps the body on the physical head, only the yaw and the height are
/// re-based ([`crate::controllers::roomscale::manual_recenter_kind`]): the chase already keeps the
/// position converged, and a position re-base could put the body through a wall the chase could
/// not cross. The seat transitions ask for their own re-base through [`request_seat_recenter`].
///
/// The re-base runs at the start of the next frame ([`auto_recenter_tick`]), like
/// [`request_recenter`]. It needs the runtime lock, which a frame holds while it waits on other
/// threads, and the callers are not all at a point where taking it is safe: the debug UI holds the
/// egui state lock the draw task takes, and the Camera tab also holds the config lock, which choosing
/// the kind takes. So the request is only an atomic store and the kind is chosen when the frame takes
/// it, which makes this safe from any thread and under any lock. See `docs/mod/threading.md`.
pub fn recenter() {
    PENDING_RECENTER.fetch_or(REQUEST_POSTURE | REQUEST_MANUAL, Ordering::Relaxed);
}

/// What a recenter re-bases.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RecenterKind {
    /// Position and yaw: the head's current pose becomes the cockpit's origin.
    Full,
    /// Yaw and height only: the horizontal position stays where it was, so that a body standing
    /// where the head is does not move relative to it.
    YawAndHeight,
}

/// Who asked for a recenter, which decides whether it also re-bases the seated eye reference
/// ([`crate::controllers::roomscale::on_recenter`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RecenterCause {
    /// The player: the F7 key, the debug UI's and the pause menu's buttons, the stick click chord,
    /// and the recenter on resuming gameplay.
    Manual,
    /// A seat transition's re-base, on the game's schedule rather than the player's.
    SeatTransition,
}

/// Ask for a manual recenter of `kind` at the start of the next frame. [`recenter`] asks for the
/// kind the posture calls for; this is for a caller that has chosen one. A full request outranks a
/// yaw-and-height one.
pub fn request_recenter(kind: RecenterKind) {
    request(kind, RecenterCause::Manual);
}

/// Ask for a seat transition's recenter at the start of the next frame, as [`request_recenter`]
/// does, without the manual recenter's re-base of the seated eye reference. A manual request in
/// the same frame makes the merged recenter manual.
pub fn request_seat_recenter(kind: RecenterKind) {
    request(kind, RecenterCause::SeatTransition);
}

fn request(kind: RecenterKind, cause: RecenterCause) {
    let kind_bit = match kind {
        RecenterKind::YawAndHeight => REQUEST_YAW_HEIGHT,
        RecenterKind::Full => REQUEST_FULL,
    };
    let cause_bit = match cause {
        RecenterCause::Manual => REQUEST_MANUAL,
        RecenterCause::SeatTransition => 0,
    };
    PENDING_RECENTER.fetch_or(kind_bit | cause_bit, Ordering::Relaxed);
}

/// How many recenters have re-based the baseline since the mod loaded, wrapping. A caller that
/// asked for one through [`request_recenter`] compares it with the count when it asked, to tell
/// that the request ran.
pub fn recenter_count() -> u32 {
    RECENTERS.load(Ordering::Relaxed)
}

/// Re-base the baseline now. Locks the runtime state, so it runs only at the top of the frame, from
/// [`auto_recenter_tick`].
fn recenter_as(kind: RecenterKind, cause: RecenterCause) {
    let mut state = lock_state();
    match state.latest_head_pose {
        Some(pose) => {
            let fresh = Baseline::from_pose(pose);
            let previous = state.baseline;
            state.baseline = Some(match (kind, previous) {
                (RecenterKind::YawAndHeight, Some(previous)) => fresh.keeping_horizontal(previous),
                _ => fresh,
            });
            drop(state);
            // The cockpit frame turns by the yaw change: new frame from old.
            let turn = previous.map_or(glam::Quat::IDENTITY, |p| fresh.yaw.conjugate() * p.yaw);
            rebased(kind, cause, turn);
            crate::recorder::tracking::note_recenter(RecenterEvent { kind, cause, turn });
            tracing::info!(target: "vr", ?kind, ?cause, "recentered the cockpit baseline");
        }
        None => {
            tracing::warn!(target: "vr", "recenter requested before any head pose was located");
        }
    }
}

/// Apply a recenter a replay recorded: everything a recenter does besides re-basing the baseline,
/// which the recorded poses already carry. Game thread, at the top of the frame, like a live
/// recenter.
pub(crate) fn apply_replayed_recenter(kind: RecenterKind, cause: RecenterCause, turn: glam::Quat) {
    rebased(kind, cause, turn);
    tracing::info!(target: "vr", ?kind, ?cause, "replayed a recenter of the cockpit baseline");
}

/// What follows a re-base of the baseline: the cockpit frame turned by `turn`, new frame from old.
fn rebased(kind: RecenterKind, cause: RecenterCause, turn: glam::Quat) {
    crate::controllers::roomscale::on_recenter(kind, turn, cause);
    // The head's forward becomes the body's: any turn of the body under the view is spent, and
    // the torso estimate turns with the frame.
    crate::headpose::xr::on_recenter();
    crate::controllers::body::alignment::on_recenter(turn);
    RECENTERS.fetch_add(1, Ordering::Relaxed);
}

/// Service a recenter asked for through [`request_recenter`]. Called at the top of
/// [`auto_recenter_tick`], before the frame takes the runtime lock.
fn service_requested_recenter() {
    let bits = PENDING_RECENTER.swap(REQUEST_NONE, Ordering::Relaxed);
    let posture_kind = if bits & REQUEST_POSTURE != 0 {
        crate::controllers::roomscale::manual_recenter_kind()
    } else {
        RecenterKind::Full
    };
    if let Some((kind, cause)) = decode_request(bits, posture_kind) {
        recenter_as(kind, cause);
    }
}

/// The recenter a set of `REQUEST_*` bits asks for, with `posture_kind` standing for
/// [`REQUEST_POSTURE`]: full over yaw-and-height, and manual when any request was. Taken in one
/// swap, so a request can never be split between two frames.
fn decode_request(bits: u8, posture_kind: RecenterKind) -> Option<(RecenterKind, RecenterCause)> {
    let posture = bits & REQUEST_POSTURE != 0;
    let kind = if bits & REQUEST_FULL != 0 || (posture && posture_kind == RecenterKind::Full) {
        RecenterKind::Full
    } else if bits & REQUEST_YAW_HEIGHT != 0 || posture {
        RecenterKind::YawAndHeight
    } else {
        return None;
    };
    let cause = if bits & REQUEST_MANUAL != 0 {
        RecenterCause::Manual
    } else {
        RecenterCause::SeatTransition
    };
    Some((kind, cause))
}

/// Fold a frozen turn of the body under the view into the baseline ([`crate::headpose::follow`]):
/// once the body stops following the torso, the cockpit frame turns with the body so that its
/// forward is the body's again, as every consumer off foot expects. The view is unchanged, because
/// the composition's turn is taken out in the same frame. Called at the top of the frame, after
/// the recenters, before the frame takes the runtime lock.
fn service_follow_bank() {
    let Some(yaw) = crate::headpose::xr::pending_bank() else {
        return;
    };
    let rotation = glam::Quat::from_rotation_y(yaw);
    {
        let mut state = lock_state();
        // No baseline is the identity baseline at the origin.
        let baseline = state.baseline.get_or_insert(Baseline {
            position: glam::Vec3::ZERO,
            yaw: glam::Quat::IDENTITY,
        });
        baseline.yaw = (baseline.yaw * rotation).normalize();
    }
    // The cockpit frame turns by the inverse: new frame from old.
    let turn = rotation.conjugate();
    crate::controllers::roomscale::on_recenter(
        RecenterKind::YawAndHeight,
        turn,
        RecenterCause::SeatTransition,
    );
    crate::headpose::xr::bank(yaw);
    crate::controllers::body::alignment::on_recenter(turn);
    tracing::debug!(
        target: "vr",
        yaw_deg = yaw.to_degrees(),
        "folded the body's turn under the view into the cockpit baseline"
    );
}

/// The recenters done, for [`recenter_count`].
static RECENTERS: AtomicU32 = AtomicU32::new(0);

/// The pending [`recenter`], [`request_recenter`], and [`request_seat_recenter`] calls, as a set of
/// `REQUEST_*` bits ([`decode_request`]).
static PENDING_RECENTER: AtomicU8 = AtomicU8::new(REQUEST_NONE);
const REQUEST_NONE: u8 = 0;
const REQUEST_YAW_HEIGHT: u8 = 1 << 0;
const REQUEST_FULL: u8 = 1 << 1;
const REQUEST_MANUAL: u8 = 1 << 2;
/// The kind [`crate::controllers::roomscale::manual_recenter_kind`] picks, chosen when the request
/// is serviced.
const REQUEST_POSTURE: u8 = 1 << 3;

/// Drive the gameplay-resume auto-recenter and service the recenters asked for since the last frame.
/// Call once per frame on the game thread, before `frame_begin` takes the runtime lock. Arms while the
/// game is not in settled gameplay (frontend, loading, or no local player character), and fires a
/// single manual recenter once gameplay is running and the player's head anchor has stopped moving (the
/// entry animation has finished), so the neutral snaps to the player's real pose. In-session
/// transitions such as exiting a vehicle keep a live character, so they never re-arm and never fire.
/// See [`crate::vr::VrConfig::auto_recenter_on_gameplay`].
pub fn auto_recenter_tick() {
    // A replay's poses are re-based already, and it applies the recenters it recorded; a live one
    // would turn the body and the torso estimate under them.
    if !is_running() || crate::testing::replay::is_driving() {
        PENDING_RECENTER.store(REQUEST_NONE, Ordering::Relaxed);
        return;
    }
    service_requested_recenter();
    service_follow_bank();
    if !Config::lock_query(|c| c.vr.auto_recenter_on_gameplay) {
        return;
    }
    let in_gameplay = crate::hooks::in_gameplay();
    let has_character =
        unsafe { jc3gi::character::character::Character::GetLocalPlayerCharacter().as_ref() }
            .is_some();
    let on_foot = crate::headpose::sim::mode() == crate::headpose::sim::HeadMode::OnFoot;
    let anchor = crate::headpose::anchor();

    let mut s = AUTO_RECENTER.lock();
    // Arm whenever we are not in settled gameplay, so the next resume fires exactly one recenter while
    // in-session transitions (which keep a live character) do not.
    if !in_gameplay || !has_character {
        s.armed = true;
        s.settled_frames = 0;
        s.last_anchor = None;
        return;
    }
    if !s.armed {
        return;
    }
    // Wait for the head anchor to hold still (the entry animation to end) before recentering.
    match (on_foot, anchor) {
        (true, Some(a)) => {
            let settled = s
                .last_anchor
                .is_some_and(|prev| (a - prev).length() < ANCHOR_SETTLE_EPS);
            s.settled_frames = if settled { s.settled_frames + 1 } else { 0 };
            s.last_anchor = Some(a);
            if s.settled_frames >= ANCHOR_SETTLE_FRAMES {
                s.armed = false;
                drop(s);
                // In place rather than requested: this is the top of the frame, before it takes
                // the runtime lock.
                recenter_as(
                    crate::controllers::roomscale::manual_recenter_kind(),
                    RecenterCause::Manual,
                );
                tracing::info!(target: "vr", "auto-recentered on gameplay resume");
            }
        }
        _ => {
            s.settled_frames = 0;
            s.last_anchor = anchor;
        }
    }
}

/// The recenter baseline: a position and a yaw-only orientation, re-based from the latest VIEW-space
/// pose. Per-eye poses are expressed relative to this transform (the cockpit model).
#[derive(Copy, Clone)]
pub(crate) struct Baseline {
    /// The world-from-baseline transform (position + yaw). Per-eye poses are re-based by its inverse.
    position: glam::Vec3,
    /// Yaw-only orientation (rotation about the up axis).
    yaw: glam::Quat,
}

impl Baseline {
    /// Extract the position and yaw-only orientation from a located VIEW-space pose.
    fn from_pose(pose: xr::Posef) -> Self {
        let orientation = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        // Yaw only: project the orientation onto rotation about the Y (up) axis. Zero the X/Z
        // components of the quaternion and renormalize; a degenerate (looking straight up/down)
        // quaternion falls back to identity yaw.
        let yaw = glam::Quat::from_xyzw(0.0, orientation.y, 0.0, orientation.w);
        let yaw = if yaw.length_squared() > 1e-6 {
            yaw.normalize()
        } else {
            glam::Quat::IDENTITY
        };
        Self {
            position: glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z),
            yaw,
        }
    }

    /// This baseline's yaw and height with `previous`'s horizontal position.
    fn keeping_horizontal(self, previous: Baseline) -> Self {
        Self {
            position: glam::Vec3::new(previous.position.x, self.position.y, previous.position.z),
            yaw: self.yaw,
        }
    }

    /// Rotate a LOCAL-space vector (a velocity) into the baseline (cockpit) frame. Directions
    /// ignore the baseline's position.
    pub(super) fn rotate(&self, v: glam::Vec3) -> glam::Vec3 {
        self.yaw.conjugate() * v
    }

    /// Re-base a located pose into the baseline (cockpit) frame: `baseline⁻¹ · pose`.
    pub(super) fn rebase(&self, pose: xr::Posef) -> xr::Posef {
        let pos = glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z);
        let rot = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        let inv_yaw = self.yaw.conjugate();
        let rel_pos = inv_yaw * (pos - self.position);
        let rel_rot = inv_yaw * rot;
        xr::Posef {
            orientation: xr::Quaternionf {
                x: rel_rot.x,
                y: rel_rot.y,
                z: rel_rot.z,
                w: rel_rot.w,
            },
            position: xr::Vector3f {
                x: rel_pos.x,
                y: rel_pos.y,
                z: rel_pos.z,
            },
        }
    }
}

/// The midpoint pose of the two eyes (position averaged, orientation slerped halfway): a stand-in
/// head pose for the recenter baseline. The slerp-mid keeps the head frame between the eyes rather
/// than on one of them, matching the render center in [`crate::vr::begin_render_frame`]; on canted
/// panels (e.g. the Valve Index) the eyes' orientations differ, so taking one eye's would bias the
/// baseline.
pub(super) fn mid_pose(a: xr::Posef, b: xr::Posef) -> xr::Posef {
    let qa = glam::Quat::from_xyzw(
        a.orientation.x,
        a.orientation.y,
        a.orientation.z,
        a.orientation.w,
    );
    let qb = glam::Quat::from_xyzw(
        b.orientation.x,
        b.orientation.y,
        b.orientation.z,
        b.orientation.w,
    );
    let mid = qa.slerp(qb, 0.5);
    xr::Posef {
        orientation: xr::Quaternionf {
            x: mid.x,
            y: mid.y,
            z: mid.z,
            w: mid.w,
        },
        position: xr::Vector3f {
            x: 0.5 * (a.position.x + b.position.x),
            y: 0.5 * (a.position.y + b.position.y),
            z: 0.5 * (a.position.z + b.position.z),
        },
    }
}

/// The gameplay-resume auto-recenter state (see [`auto_recenter_tick`]).
static AUTO_RECENTER: Mutex<AutoRecenterState> = Mutex::new(AutoRecenterState {
    armed: false,
    last_anchor: None,
    settled_frames: 0,
});

struct AutoRecenterState {
    /// Armed to fire one recenter once gameplay settles. Set while not in settled gameplay.
    armed: bool,
    /// The head anchor last frame, for the settle test.
    last_anchor: Option<glam::Vec3>,
    /// Consecutive frames the anchor has stayed within [`ANCHOR_SETTLE_EPS`].
    settled_frames: u32,
}

/// Frames the head anchor must hold within [`ANCHOR_SETTLE_EPS`] before an armed auto-recenter fires
/// -- long enough for the resume-from-menu entry animation (standing up from the car) to finish.
const ANCHOR_SETTLE_FRAMES: u32 = 24;
/// Per-frame head-anchor movement (metres) below which the pose counts as settled: above idle sway,
/// below any scripted or locomotion motion.
const ANCHOR_SETTLE_EPS: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_merge_full_over_yaw_and_manual_over_seat() {
        let decode = |bits| decode_request(bits, RecenterKind::Full);
        assert_eq!(decode(REQUEST_NONE), None);
        assert_eq!(decode(REQUEST_MANUAL), None);
        assert_eq!(
            decode(REQUEST_YAW_HEIGHT),
            Some((RecenterKind::YawAndHeight, RecenterCause::SeatTransition))
        );
        assert_eq!(
            decode(REQUEST_YAW_HEIGHT | REQUEST_FULL),
            Some((RecenterKind::Full, RecenterCause::SeatTransition))
        );
        assert_eq!(
            decode(REQUEST_YAW_HEIGHT | REQUEST_MANUAL),
            Some((RecenterKind::YawAndHeight, RecenterCause::Manual))
        );
        assert_eq!(
            decode(REQUEST_FULL | REQUEST_MANUAL),
            Some((RecenterKind::Full, RecenterCause::Manual))
        );
    }

    #[test]
    fn a_posture_request_takes_the_posture_kind_and_still_yields_to_full() {
        let posture = REQUEST_POSTURE | REQUEST_MANUAL;
        assert_eq!(
            decode_request(posture, RecenterKind::YawAndHeight),
            Some((RecenterKind::YawAndHeight, RecenterCause::Manual))
        );
        assert_eq!(
            decode_request(posture, RecenterKind::Full),
            Some((RecenterKind::Full, RecenterCause::Manual))
        );
        assert_eq!(
            decode_request(posture | REQUEST_FULL, RecenterKind::YawAndHeight),
            Some((RecenterKind::Full, RecenterCause::Manual))
        );
        // A seat transition's yaw-and-height re-base merges into a posture request that is full.
        assert_eq!(
            decode_request(posture | REQUEST_YAW_HEIGHT, RecenterKind::Full),
            Some((RecenterKind::Full, RecenterCause::Manual))
        );
    }
}
