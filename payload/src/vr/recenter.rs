//! Recentering the cockpit: the baseline the per-eye poses are expressed relative to, the manual
//! [`recenter`], the deferred [`request_recenter`] and [`request_seat_recenter`], the seat lock's
//! yaw re-base ([`request_seat_yaw_rebase`]), and the gameplay-resume auto-recenter
//! ([`auto_recenter_tick`]).

use std::{
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
    time::{Duration, Instant},
};

use body_rig::{angle::head_heading, rotation::twist_about_y};
use openxr as xr;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{body::rig::origin_provenance, frame as controller_frame},
    headpose::xr::{bank, on_recenter, pending_bank, stepping},
    recorder::tracking::format::RecenterEvent,
    vr::{
        convert::{posef, quat, vec3},
        input::flight_pose::{self, EpochChange, GestureSample},
        is_running,
        participants::frame_participants,
        state::lock_state,
    },
};

/// Recenter the cockpit: re-base the stored baseline from the latest located VIEW-space pose, taking
/// its position and yaw only (the cockpit model). The frame loop consumes the baseline when
/// mapping per-eye poses (see [`crate::vr::frame`]). No-op until a frame has located a head pose.
///
/// Out of a seat, only the yaw and the height are re-based
/// ([`crate::controllers::roomscale::manual_recenter_kind`]), and the body rig then snaps the pelvis
/// under the neck by re-basing the cockpit shift ([`crate::controllers::body::rig::root::on_recenter`]):
/// the view moves to the body, so the recenter never puts the body through a wall. The seat
/// transitions ask for their own re-base through [`request_seat_recenter`].
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
    /// The follow bank ([`crate::headpose::follow`]): a frozen turn of the body under the view
    /// folded into the baseline's yaw. Nobody requests it; it is noted so that a recording carries
    /// it on the frame it took effect in.
    FollowBank,
}

/// Ask for a manual recenter of `kind` at the start of the next frame. [`recenter`] asks for the
/// kind the posture calls for; this is for a caller that has chosen one. A full request outranks a
/// yaw-and-height one.
pub fn request_recenter(kind: RecenterKind) {
    request(kind, true);
}

/// Ask for a seat transition's recenter at the start of the next frame, as [`request_recenter`]
/// does, without the manual recenter's re-base of the seated eye reference. A manual request in
/// the same frame makes the merged recenter manual.
pub fn request_seat_recenter(kind: RecenterKind) {
    request(kind, false);
}

/// Ask for the seat lock's yaw re-base at the start of the next frame: the cockpit's yaw turns by
/// the head's heading against the body when the body last stopped following it (the latest follow
/// bank), so the offset the head had from the body then does not carry into the seat as a look off
/// the vehicle's axis. Only the yaw changes; the position and the height stay. A bank older than
/// [`SEAT_YAW_BANK_WINDOW`], or one a recenter has since spent, is not used. While the view steps
/// toward the body through the entry, the re-base waits for the steps to settle and bank.
pub fn request_seat_yaw_rebase() {
    PENDING_SEAT_YAW.store(true, Ordering::Relaxed);
}

/// How recent the follow bank must be for the seat lock's yaw re-base to use it: a vehicle entry,
/// a hijack, or a stunt position into a seat takes a few seconds from the moment the body stops
/// following.
pub const SEAT_YAW_BANK_WINDOW: Duration = Duration::from_secs(15);

/// A yaw re-base smaller than this is not worth turning the view for, radians: about a degree.
const SEAT_YAW_MIN: f32 = 0.017;

fn request(kind: RecenterKind, manual: bool) {
    let kind_bit = match kind {
        RecenterKind::YawAndHeight => REQUEST_YAW_HEIGHT,
        RecenterKind::Full => REQUEST_FULL,
    };
    let cause_bit = if manual { REQUEST_MANUAL } else { 0 };
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
            let _origin_mutation = origin_provenance::changing();
            let fresh = Baseline::from_pose(pose);
            let previous = state.baseline;
            // The head's forward becomes the cockpit's, so a bank's offset is spent.
            *LAST_BANK.lock() = None;
            state.baseline = Some(match (kind, previous) {
                (RecenterKind::YawAndHeight, Some(previous)) => fresh.keeping_horizontal(previous),
                _ => fresh,
            });
            flight_pose::source_changed(EpochChange::Recenter);
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

/// What follows a re-base of the baseline: the cockpit frame turned by `turn`, new frame from old.
fn rebased(kind: RecenterKind, cause: RecenterCause, turn: glam::Quat) {
    (frame_participants().on_recenter)(kind, turn, cause);
    // The head's forward becomes the body's: any turn of the body under the view is spent, and
    // the torso estimate turns with the frame.
    on_recenter();
    RECENTERS.fetch_add(1, Ordering::Relaxed);
}

/// Service a recenter asked for through [`request_recenter`]. Called at the top of
/// [`auto_recenter_tick`], before the frame takes the runtime lock.
fn service_requested_recenter() {
    let bits = PENDING_RECENTER.swap(REQUEST_NONE, Ordering::Relaxed);
    let posture_kind = if bits & REQUEST_POSTURE != 0 {
        (frame_participants().manual_recenter_kind)()
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
    let Some(yaw) = pending_bank() else {
        return;
    };
    let _origin_mutation = origin_provenance::changing();
    let rotation = glam::Quat::from_rotation_y(yaw);
    {
        let mut state = lock_state();
        // No baseline is the identity baseline at the origin.
        let baseline = state.baseline.get_or_insert(Baseline {
            position: glam::Vec3::ZERO,
            yaw: glam::Quat::IDENTITY,
        });
        baseline.yaw = (baseline.yaw * rotation).normalize();
        let banked = *baseline;
        let head = state.latest_head_pose.map(|pose| banked.rebase(pose));
        flight_pose::source_changed(EpochChange::FollowBank);
        // The head's heading in the banked cockpit frame is its offset from the body.
        *LAST_BANK.lock() = head
            .and_then(|pose| {
                let o = pose.orientation;
                head_heading(glam::Quat::from_xyzw(o.x, o.y, o.z, o.w))
            })
            .map(|yaw| (yaw, Instant::now()));
    }
    // The cockpit frame turns by the inverse: new frame from old.
    let turn = rotation.conjugate();
    banked(yaw, turn);
    crate::recorder::tracking::note_recenter(RecenterEvent {
        kind: RecenterKind::YawAndHeight,
        cause: RecenterCause::FollowBank,
        turn,
    });
    tracing::debug!(
        target: "vr",
        yaw_deg = yaw.to_degrees(),
        "folded the body's turn under the view into the cockpit baseline"
    );
}

/// Service a [`request_seat_yaw_rebase`] and advance the re-base it starts: the baseline's yaw turns
/// by the latest follow bank's head offset, eased in over [`SEAT_YAW_BLEND_S`] rather than in one
/// frame, since the turn reaches the displayed view. Each frame's share is a small re-base of its
/// own, so every consumer of the cockpit frame turns with it. A recenter in the meantime ends the
/// blend where it is. Called at the top of the frame, after the follow bank, so a bank in the same
/// frame as the seat lock is used, and before the frame takes the runtime lock.
fn service_seat_yaw() {
    // The entry's steps own the view's yaw until they settle, and their settling banks what is
    // left: the re-base waits for that bank rather than turning the view under the steps with an
    // older one.
    if !stepping() && PENDING_SEAT_YAW.swap(false, Ordering::Relaxed) {
        let bank = LAST_BANK.lock().take();
        if let Some(yaw) = bank
            .filter(|(_, at)| at.elapsed() <= SEAT_YAW_BANK_WINDOW)
            .map(|(yaw, _)| yaw)
            .filter(|yaw| yaw.is_finite() && yaw.abs() >= SEAT_YAW_MIN)
        {
            *SEAT_YAW_BLEND.lock() = Some(SeatYawBlend {
                total: yaw,
                applied: 0.0,
                started: Instant::now(),
                recenters: RECENTERS.load(Ordering::Relaxed),
            });
            tracing::info!(
                target: "vr",
                yaw_deg = yaw.to_degrees(),
                "re-basing the cockpit's yaw at the seat lock by the head's offset from the body"
            );
        }
    }
    let mut blend_slot = SEAT_YAW_BLEND.lock();
    let Some(blend) = blend_slot.as_mut() else {
        return;
    };
    if RECENTERS.load(Ordering::Relaxed) != blend.recenters {
        *blend_slot = None;
        return;
    }
    let progress = seat_yaw_progress(blend.started.elapsed().as_secs_f32());
    let step = blend.total * progress - blend.applied;
    blend.applied += step;
    if progress >= 1.0 {
        *blend_slot = None;
    }
    drop(blend_slot);
    if step == 0.0 {
        return;
    }
    let _origin_mutation = origin_provenance::changing();
    let rotation = glam::Quat::from_rotation_y(step);
    {
        let mut state = lock_state();
        let Some(baseline) = state.baseline.as_mut() else {
            *SEAT_YAW_BLEND.lock() = None;
            return;
        };
        baseline.yaw = (baseline.yaw * rotation).normalize();
        flight_pose::source_changed(EpochChange::Recenter);
    }
    // The cockpit frame turns by the inverse: new frame from old. Each share is too small to break
    // the anchors' interpolation, so it does not count as a recenter of its own.
    let turn = rotation.conjugate();
    let (kind, cause) = (RecenterKind::YawAndHeight, RecenterCause::SeatTransition);
    (frame_participants().on_recenter)(kind, turn, cause);
    on_recenter();
    crate::recorder::tracking::note_recenter(RecenterEvent { kind, cause, turn });
}

/// The share of the seat lock's yaw re-base applied `elapsed_s` seconds in: eased in and out, so
/// the view's turn starts and ends without a jolt, and complete at [`SEAT_YAW_BLEND_S`].
fn seat_yaw_progress(elapsed_s: f32) -> f32 {
    if !elapsed_s.is_finite() {
        return 1.0;
    }
    body_rig::smooth::smoothstep((elapsed_s / SEAT_YAW_BLEND_S).clamp(0.0, 1.0))
}

/// How long the seat lock's yaw re-base takes to turn the view, seconds.
const SEAT_YAW_BLEND_S: f32 = 1.0;

/// A seat lock's yaw re-base in progress.
#[derive(Copy, Clone)]
struct SeatYawBlend {
    /// The whole turn, radians.
    total: f32,
    /// The turn applied so far, radians.
    applied: f32,
    started: Instant,
    /// [`RECENTERS`] when the blend started: a recenter since ends it.
    recenters: u32,
}

static SEAT_YAW_BLEND: Mutex<Option<SeatYawBlend>> = Mutex::new(None);

/// The latest follow bank's head offset from the body, radians, and when the bank ran.
static LAST_BANK: Mutex<Option<(f32, Instant)>> = Mutex::new(None);

/// Whether a [`request_seat_yaw_rebase`] waits for the next frame.
static PENDING_SEAT_YAW: AtomicBool = AtomicBool::new(false);

/// What follows a follow bank of `yaw` besides the baseline's own turn: the cockpit frame turned by
/// `turn`, new frame from old, and the composition gives the turn back.
fn banked(yaw: f32, turn: glam::Quat) {
    (frame_participants().on_recenter)(RecenterKind::YawAndHeight, turn, RecenterCause::FollowBank);
    bank(yaw);
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
/// Call once per frame on the game thread, before `frame_wait` locates the views. Arms while the
/// game is not in settled gameplay (frontend, loading, or no local player character), and fires a
/// single manual recenter once gameplay is running and the player's head anchor has stopped moving (the
/// entry animation has finished), so the neutral snaps to the player's real pose. In-session
/// transitions such as exiting a vehicle keep a live character, so they never re-arm and never fire.
/// See [`crate::vr::VrConfig::auto_recenter_on_gameplay`].
pub fn auto_recenter_tick() {
    if !is_running() {
        PENDING_RECENTER.store(REQUEST_NONE, Ordering::Relaxed);
        PENDING_SEAT_YAW.store(false, Ordering::Relaxed);
        *SEAT_YAW_BLEND.lock() = None;
        return;
    }
    service_requested_recenter();
    service_follow_bank();
    service_seat_yaw();
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
        s.settled_since = None;
        s.last_anchor = None;
        return;
    }
    if !s.armed {
        return;
    }
    if lock_state().baseline.is_some() {
        s.armed = false;
        s.settled_since = None;
        s.last_anchor = None;
        return;
    }
    let now = Instant::now();
    let sample = controller_frame::with_latest(|frame| {
        flight_pose::for_frame(frame, now).map(|packet| packet.sample())
    })
    .flatten();
    let Some(sample) = sample else {
        s.settled_since = None;
        s.last_anchor = None;
        return;
    };
    match (on_foot, anchor) {
        (true, Some(a)) => {
            if s.observe_anchor(a, sample, now) {
                s.armed = false;
                drop(s);
                // In place rather than requested: this is the top of the frame, before it takes
                // the runtime lock.
                recenter_as(
                    (frame_participants().manual_recenter_kind)(),
                    RecenterCause::Manual,
                );
                tracing::info!(target: "vr", "auto-recentered on gameplay resume");
            }
        }
        _ => {
            s.settled_since = None;
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
        Self {
            position: vec3(pose.position),
            yaw: twist_about_y(quat(pose.orientation)),
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
        let inv_yaw = self.yaw.conjugate();
        posef(
            inv_yaw * (vec3(pose.position) - self.position),
            inv_yaw * quat(pose.orientation),
        )
    }
}

/// The midpoint pose of the two eyes (position averaged, orientation slerped halfway): a stand-in
/// head pose for the recenter baseline. The slerp-mid keeps the head frame between the eyes rather
/// than on one of them, matching the render center in [`crate::vr::begin_render_frame`]; on canted
/// panels (e.g. the Valve Index) the eyes' orientations differ, so taking one eye's would bias the
/// baseline.
pub(super) fn mid_pose(a: xr::Posef, b: xr::Posef) -> xr::Posef {
    posef(
        0.5 * (vec3(a.position) + vec3(b.position)),
        quat(a.orientation).slerp(quat(b.orientation), 0.5),
    )
}

/// The gameplay-resume auto-recenter state (see [`auto_recenter_tick`]).
static AUTO_RECENTER: Mutex<AutoRecenterState> = Mutex::new(AutoRecenterState {
    armed: false,
    last_anchor: None,
    settled_since: None,
    last_sample: None,
});

struct AutoRecenterState {
    /// Armed to fire one recenter once gameplay settles. Set while not in settled gameplay.
    armed: bool,
    /// The head anchor last frame, for the settle test.
    last_anchor: Option<glam::Vec3>,
    settled_since: Option<Instant>,
    last_sample: Option<GestureSample>,
}

impl AutoRecenterState {
    fn observe_anchor(&mut self, anchor: glam::Vec3, sample: GestureSample, now: Instant) -> bool {
        if self.last_sample == Some(sample) {
            return false;
        }
        let continuous = self.last_sample.is_none_or(|last| {
            last.epoch == sample.epoch
                && sample.sequence > last.sequence
                && sample.time_ns > last.time_ns
                && sample.time_ns.saturating_sub(last.time_ns) <= 250_000_000
        });
        self.last_sample = Some(sample);
        let settled = continuous
            && self
                .last_anchor
                .is_some_and(|last| (anchor - last).length() < ANCHOR_SETTLE_EPS);
        if !settled {
            self.settled_since = None;
        }
        self.last_anchor = Some(anchor);
        let since = *self.settled_since.get_or_insert(now);
        now.saturating_duration_since(since) >= ANCHOR_SETTLE_TIME
    }
}

const ANCHOR_SETTLE_TIME: Duration = Duration::from_millis(400);
/// Per-frame head-anchor movement (metres) below which the pose counts as settled: above idle sway,
/// below any scripted or locomotion motion.
const ANCHOR_SETTLE_EPS: f32 = 0.01;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_settles_by_time_and_requires_continuing_tracking_samples() {
        let mut state = AutoRecenterState {
            armed: true,
            last_anchor: None,
            settled_since: None,
            last_sample: None,
        };
        let now = Instant::now();
        let sample = |sequence, time_ns| GestureSample {
            epoch: 1,
            sequence,
            time_ns,
        };
        let anchor = glam::Vec3::Y;
        assert!(!state.observe_anchor(anchor, sample(1, 0), now));
        assert!(!state.observe_anchor(anchor, sample(1, 0), now + Duration::from_secs(1)));
        assert!(!state.observe_anchor(
            anchor,
            sample(2, 200_000_000),
            now + Duration::from_millis(200)
        ));
        assert!(state.observe_anchor(
            anchor,
            sample(3, 400_000_000),
            now + Duration::from_millis(400)
        ));
        assert!(!state.observe_anchor(
            anchor,
            sample(4, 900_000_000),
            now + Duration::from_millis(900)
        ));
    }

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

    /// The seat lock's re-base starts and ends at rest and never jumps: no frame's share at a
    /// 90 Hz frame rate exceeds a small turn of the whole.
    #[test]
    fn the_seat_yaw_rebase_eases_in_and_out() {
        assert_eq!(seat_yaw_progress(0.0), 0.0);
        assert_eq!(seat_yaw_progress(SEAT_YAW_BLEND_S), 1.0);
        assert_eq!(seat_yaw_progress(SEAT_YAW_BLEND_S * 3.0), 1.0);
        assert_eq!(seat_yaw_progress(f32::NAN), 1.0);
        let frame = 1.0 / 90.0;
        let largest = (0..100)
            .map(|i| {
                seat_yaw_progress((i + 1) as f32 * frame) - seat_yaw_progress(i as f32 * frame)
            })
            .fold(0.0f32, f32::max);
        assert!(largest < 0.02, "{largest}");
    }
}
