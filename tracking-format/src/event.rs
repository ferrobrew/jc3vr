//! The events a version 5 recording carries beside its frames: what the mod decided, as opposed to
//! what the runtime gave it.
//!
//! An event is stamped on the recording's frame timeline ([`Event::time_ns`] shares the clock of
//! [`Frame::time_ns`](crate::Frame::time_ns)), and a writer stores it before the first frame
//! captured after it. Its payload is one of [`EventKind`]'s variants, each selected by a kind byte.
//!
//! The payload's own enums are recorded as their labels ([`Label`]), so that renaming or adding a
//! variant on the mod's side changes no layout here. Nested runtime values that have no stable
//! shape (a grab volume, a body state, a refusal) are recorded as their Rust `Debug` text, to be
//! read against the recording's build stamp; they are evidence, not a schema.

use std::borrow::Cow;

use glam::{Vec2, Vec3};

use crate::model::Hand;

/// A label or a piece of evidence text. Producers lend `'static` labels without allocating; a
/// decoded event owns its text.
pub type Label = Cow<'static, str>;

/// One event on the recording's frame timeline.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// Nanoseconds since the recording started, on the clock of the frames' `time_ns`.
    pub time_ns: u64,
    pub kind: EventKind,
}

/// What happened.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    /// The controller subsystems' derived state for one game frame.
    ControllerState(Box<ControllerState>),
    /// A stage of a grip acquisition: a press's resolution, a request's outcome, or the first
    /// drive of what the grip took.
    GripAcquisition(Box<GripAcquisition>),
    /// An attempt to let an inventory gun fall from the hand.
    GunRelease(Box<GunRelease>),
    /// A take marker the player dropped to find a moment again.
    TakeMarker(Box<TakeMarker>),
}

impl EventKind {
    /// The kind's name, as tools print it.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::ControllerState(_) => "controller_state",
            EventKind::GripAcquisition(_) => "grip_acquisition",
            EventKind::GunRelease(_) => "gun_release",
            EventKind::TakeMarker(_) => "take_marker",
        }
    }
}

/// The controller subsystems' derived state, read once per game frame after the engine's render
/// update. It holds only what the frames do not: the frames already carry the controllers' poses,
/// validity and tracking flags, focus, action set, and the STAGE information.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ControllerState {
    /// The controller publication the subsystems read, when one was available.
    pub frame_sequence: Option<u64>,
    /// How old that publication was when the state was read, microseconds.
    pub publication_age_us: Option<u64>,
    /// The controller scheme in effect.
    pub scheme: Label,
    /// The mode detector's result, and its traversal and hold refinements.
    pub mode: Label,
    pub traversal: Option<Label>,
    pub held: Option<Label>,
    /// The input subsystem's standing error, if any.
    pub input_error: Option<Label>,
    /// Indexed by [`Hand::index`].
    pub ik: [IkState; 2],
    pub tiers: TierState,
    pub late: LateState,
    pub aim: AimState,
    pub roomscale: RoomscaleState,
}

/// One hand's HumanIK result.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct IkState {
    pub queued: bool,
    pub skip: Option<Label>,
    pub weight: f32,
    pub reach_distance_m: f32,
    pub rotation_deg: f32,
    pub secondary_wrist_off: bool,
    pub secondary_grip_off: bool,
    pub overrode_live_target: bool,
}

/// The classified hand tiers and their blend.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct TierState {
    pub state: Option<Label>,
    /// Indexed by [`Hand::index`].
    pub hands: [HandTier; 2],
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HandTier {
    pub assigned: Label,
    pub effective: Label,
    pub controller_weight: f32,
    pub anchor_weight: f32,
}

/// The render-time late hand correction.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct LateState {
    /// Indexed by [`Hand::index`].
    pub hands: [HandLate; 2],
    /// Cumulative counters since the payload loaded.
    pub weapons_updated: u64,
    pub palettes_edited: u64,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HandLate {
    pub reason: Label,
    pub physical_displacement_m: Option<f32>,
    pub relative_rotation_deg: Option<f32>,
    pub requested_wrist: Option<Vec3>,
    pub reference_wrist: Option<Vec3>,
    pub latest_wrist: Option<Vec3>,
    /// In the character model's frame.
    pub achieved_wrist_model: Option<Vec3>,
}

/// The aim rays, their hits, and the weapon sight.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct AimState {
    pub idle: Option<Label>,
    /// Indexed by [`Hand::index`].
    pub hands: [Option<HandAim>; 2],
    pub grapple: Option<GrappleAim>,
    pub weapon: Option<WeaponAim>,
    pub raycast_error: Option<Label>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct HandAim {
    pub source: Label,
    pub gun_index: Option<u32>,
    /// The hit's distance, metres, when the ray hit anything.
    pub hit_distance_m: Option<f32>,
    pub hit_object: bool,
    pub target: Vec3,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GrappleAim {
    pub hit: bool,
    pub valid: bool,
    pub tetherable: bool,
    pub point: Vec3,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponAim {
    pub name_hash: u32,
    /// The hand holding each of the weapon's guns.
    pub gun_hands: [Option<Hand>; 4],
    pub sight_source: Label,
    pub sight_offset_m: Option<Vec3>,
    /// Yaw and pitch, degrees.
    pub sight_angles_deg: Vec2,
}

/// The posture, the body rig's root and chase, the seat, and the fade.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RoomscaleState {
    pub active: bool,
    pub posture: Label,
    pub eye_drop_m: f32,
    /// Who owned the root on the last on-foot tick.
    pub root_owner: Option<Label>,
    /// How far the ground point under the upright eyes is from the root, and how far it is outside
    /// the lean, world metres.
    pub gap_m: f32,
    pub residual_m: f32,
    /// How far the push-back has slid the view in its current episode, metres.
    pub pushback_m: f32,
    pub cockpit_shift_m: Vec3,
    pub chase_velocity_mps: Vec2,
    pub task_velocity_mps: Vec2,
    pub crouched: bool,
    pub seated_crouch: bool,
    pub seat: Label,
    pub probe: Option<Label>,
    pub fade_level: f32,
}

/// Which stage of an acquisition a [`GripAcquisition`] records.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GripStage {
    /// A grip press, or a queued request, with what the resolver offered and what the pickup
    /// consumer did with it.
    Input,
    /// What the simulation consumer did with a queued request.
    Request,
    /// The first drive of a body the request took.
    Drive,
}

impl GripStage {
    pub const ALL: [GripStage; 3] = [GripStage::Input, GripStage::Request, GripStage::Drive];

    pub fn name(self) -> &'static str {
        match self {
            GripStage::Input => "input",
            GripStage::Request => "request",
            GripStage::Drive => "drive",
        }
    }
}

/// A stage of one hand's grip acquisition, with the resolver's decision behind it.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripAcquisition {
    pub hand: Hand,
    pub stage: GripStage,
    /// The controller publication the input read.
    pub sequence: u64,
    /// The hand's input-read ordinal, including repeated reads of one publication.
    pub observation: u64,
    /// The hand's grip episode, advanced on a press or a newly held grip.
    pub episode: u64,
    /// The hand's request ordinal, when the input queued a request.
    pub request: Option<u64>,
    pub grip_down: bool,
    pub grip_pressed: bool,
    pub grip_released: bool,
    pub latch: Label,
    pub grab: Option<Label>,
    /// The candidate the resolver chose, and every candidate it offered, each with its score
    /// (distance and tier) and, for a loser, why it lost to the winner.
    pub chosen: Option<GripCandidate>,
    pub candidates: Vec<GripCandidate>,
    /// Candidates beyond the producer's bound, left out.
    pub candidates_omitted: u32,
    /// The reach paths the provider evaluated for this input.
    pub scan: Option<GripScan>,
    /// What the pickup input consumer did with the input.
    pub consumer: Label,
    /// The reach the queued request carries.
    pub target: Option<Label>,
    /// What the simulation consumer did with the request.
    pub result: Option<Label>,
    pub drive: Option<GripDrive>,
}

/// One resolver candidate.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripCandidate {
    /// Its index in the tick's targets.
    pub resolver_index: u32,
    pub distance_m: f32,
    pub tier: Label,
    pub volume: Label,
    /// The provider's own reach, when the provider supplies identity.
    pub observed: Option<Label>,
    /// Why it lost to the chosen candidate.
    pub lost: Option<Label>,
}

/// The branches a reach provider evaluated.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripScan {
    pub entries: Vec<GripScanEntry>,
    pub omitted: u32,
    pub ray_ran: bool,
    pub handles_ran: bool,
    pub ray_selected: Option<Label>,
    pub handle_selected: Option<Label>,
    pub selected: Option<Label>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripScanEntry {
    pub branch: Label,
    /// A correlation identity, never dereferenced.
    pub object: Option<u64>,
    pub body: Option<u32>,
    pub door: Option<Label>,
    pub point: Option<Vec3>,
    pub distance_m: Option<f32>,
    pub mass_kg: Option<f32>,
    pub result: Label,
}

/// The first carried-body write or joined-body report for an accepted request.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripDrive {
    pub object: u64,
    pub body: u32,
    pub before: Label,
    /// The requested velocities, not a post-step observation; absent for a joint-driven hold.
    pub commanded: Option<Label>,
    pub result: Option<Label>,
    pub report: Option<Label>,
}

/// One attempt to let an inventory gun fall from the hand.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GunRelease {
    /// The physical controller hand.
    pub hand: Hand,
    /// The attempt's ordinal since the payload loaded.
    pub attempt: u64,
    /// The loadout object's address while it was held, a correlation identity only.
    pub weapon: Option<u64>,
    pub route: Label,
    pub max_speed_mps: f32,
    pub refusal: Option<Label>,
    /// Which release calls returned; a return is not an observation of the weapon afterwards.
    pub before_disarm: bool,
    pub disarm_returned: bool,
    pub slot_clear_returned: bool,
    pub reconcile_returned: bool,
    /// The controller publication the estimate was validated against. Its poses and flags are
    /// the frame's of the same display time.
    pub controller: Option<ReleaseController>,
    pub drop: Option<ReleaseDrop>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseController {
    pub frame_sequence: u64,
    pub display_time_ns: i64,
    /// The publication's age at the attempt.
    pub publication_age_ns: u64,
}

/// The native drop: the gun motion read before it, and the velocity the release requested.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseDrop {
    /// The address read at the drop site, a correlation identity only.
    pub weapon: u64,
    pub slot: Option<Label>,
    pub sample_stage: Label,
    pub sample_source: Label,
    pub cached_motion: Option<WorldMotion>,
    pub native_body: Option<u32>,
    /// The capped world velocities passed to the native setter, when it was called.
    pub requested_motion: Option<WorldMotion>,
    pub release_source: Label,
    pub history_age_s: Option<f32>,
    pub write: Label,
    pub on_drop_returned: bool,
    pub slot_clear_returned: bool,
}

/// World-space linear (metres per second) and angular (radians per second) velocities.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WorldMotion {
    pub linear: Vec3,
    pub angular: Vec3,
}

/// A take marker: the same marker the mod writes to a video recording's sidecar or to the session's
/// marker log, so the two can be joined by `index` and `stamp`.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct TakeMarker {
    /// The marker's session-local ordinal, from 1.
    pub index: u32,
    /// The local wall-clock stamp the marker was made at.
    pub stamp: Label,
    /// Milliseconds since the Unix epoch.
    pub unix_ms: i64,
    /// The engine's game-run time in microseconds, when its clock was up.
    pub game_run_time_us: Option<u64>,
    /// The marker's label, empty for an unlabelled marker.
    pub label: Label,
}
