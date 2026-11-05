//! The events a recording carries beside its frames, from version 5: what the mod decided, as
//! opposed to what the runtime gave it.
//!
//! An event is stamped on the recording's frame timeline ([`Event::time_ns`] shares the clock of
//! [`Frame::time_ns`](crate::Frame::time_ns)), and a writer stores it before the first frame
//! captured after it. Its payload is one of [`EventKind`]'s variants, each selected by a kind byte.
//!
//! The payload's own enums are recorded as their labels ([`Label`]), so that renaming or adding a
//! variant on the mod's side changes no layout here. Nested runtime values that have no stable
//! shape (a grab volume, a body state, a refusal) are recorded as their Rust `Debug` text, to be
//! read against the recording's build stamp; they are evidence, not a schema.
//!
//! Version 8 adds the body's poses and rig ([`body`]), the world around Rico ([`world`]), and the
//! diagnostic notes ([`notes`]).

mod body;
mod notes;
mod world;

use std::borrow::Cow;

use glam::{Quat, Vec2, Vec3};

pub use crate::event::{
    body::{
        BODY_BONE_PARENTS, BODY_BONES, BoneSet, IkGoal, IkTarget, PoseSample, PoseStage, RigConfig,
        RigInput, RigResult, RigSample,
    },
    notes::{Diagnostic, FrameTiming, Haptic, Injected, InjectedWrite, Settings, SlotTiming},
    world::{
        BodiesSample, BodyIntro, BodySample, CameraState, Force, ForceKind, FrameState,
        GrappleState, HeldMotion, HeldSample, HoldReport, LockState, SlingState, VehicleState,
        ViewState, WireEndSample,
    },
};
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
    /// The body's animation state changed, from version 7.
    BodyState(Box<BodyState>),
    /// An item left a hand with a requested motion, from version 7.
    Throw(Box<Throw>),
    /// A hand let go of what its grip held, from version 7.
    GripRelease(Box<GripRelease>),
    /// The wingsuit gesture recognizer's phase changed, from version 7.
    WingsuitGesture(Box<WingsuitGesture>),
    /// The grapple snatch's phase changed, from version 7.
    Snatch(Box<Snatch>),
    /// Rico's pose at one stage of a pose-finalization pass, from version 8.
    Pose(Box<PoseSample>),
    /// One grounded-rig frame, from version 8.
    Rig(Box<RigSample>),
    /// The rig's calibration and tuning, from version 8.
    RigConfig(Box<RigConfig>),
    /// The dynamic bodies near Rico after a physics step, from version 8.
    Bodies(Box<BodiesSample>),
    /// A body joined the near set, from version 8.
    BodyIntro(Box<BodyIntro>),
    /// What a hand holds after a simulation tick, from version 8.
    Held(Box<HeldSample>),
    /// A velocity write or an impulse the mod applied, from version 8.
    Force(Box<Force>),
    /// The actions the mod injected on an input tick, from version 8.
    Injected(Box<Injected>),
    /// A refusal or another diagnostic moment, from version 8.
    Diagnostic(Box<Diagnostic>),
    /// A haptic cue, from version 8.
    Haptic(Box<Haptic>),
    /// The settings in effect, from version 8.
    Settings(Box<Settings>),
    /// The frame-time breakdown over a reporting window, from version 8.
    FrameTiming(Box<FrameTiming>),
}

impl EventKind {
    /// The kind's name, as tools print it.
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::ControllerState(_) => "controller_state",
            EventKind::GripAcquisition(_) => "grip_acquisition",
            EventKind::GunRelease(_) => "gun_release",
            EventKind::TakeMarker(_) => "take_marker",
            EventKind::BodyState(_) => "body_state",
            EventKind::Throw(_) => "throw",
            EventKind::GripRelease(_) => "grip_release",
            EventKind::WingsuitGesture(_) => "wingsuit_gesture",
            EventKind::Snatch(_) => "snatch",
            EventKind::Pose(_) => "pose",
            EventKind::Rig(_) => "rig",
            EventKind::RigConfig(_) => "rig_config",
            EventKind::Bodies(_) => "bodies",
            EventKind::BodyIntro(_) => "body_intro",
            EventKind::Held(_) => "held",
            EventKind::Force(_) => "force",
            EventKind::Injected(_) => "injected",
            EventKind::Diagnostic(_) => "diagnostic",
            EventKind::Haptic(_) => "haptic",
            EventKind::Settings(_) => "settings",
            EventKind::FrameTiming(_) => "frame_timing",
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
    /// The root's latest on-foot tick, while the root's hook runs; absent before version 6.
    pub root: Option<RootTick>,
    /// The player character's measured motion and height over the ground, while there is a
    /// player character; absent before version 7.
    pub character: Option<CharacterMotion>,
    /// Where the cockpit and the camera sit in the world; absent before version 8.
    pub view: Option<ViewState>,
    /// The grapple's line, while a wire is out; absent before version 8.
    pub grapple: Option<GrappleState>,
    /// The vehicle, while the player rides one; absent before version 8.
    pub vehicle: Option<VehicleState>,
    /// The frame's place in the engine's sequence and its timing; absent before version 8.
    pub frame: Option<FrameState>,
}

/// The player character's motion as the game measures it, read once per game frame.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CharacterMotion {
    /// The character's measured world velocity, metres per second: what the animation graph's
    /// speed conditions compare.
    pub velocity_mps: Vec3,
    /// The height over the ground the game last computed, when the body's rule system was there.
    pub height: Option<GroundHeight>,
}

/// The character's height over the ground, metres, as the game's ground information last wrote
/// it: zero while the proxy stands or slides on the ground.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GroundHeight {
    /// Over the ground or the water surface, whichever is nearer.
    pub nearest_m: f32,
    /// Over the ground alone.
    pub ground_m: f32,
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

/// The root's latest on-foot tick: where it went, what the tick asked it to do, and the proxy it
/// moved through. A rendered frame samples the latest tick, so a frame without a tick repeats one
/// and two ticks in one frame show the second; [`tick`](Self::tick) tells them apart.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct RootTick {
    /// The on-foot ticks the root's hook has run since the payload loaded.
    pub tick: u64,
    /// The tick's step, seconds.
    pub dt_s: f32,
    /// The root's world position as the tick found it.
    pub position: Vec3,
    /// How far the root moved since the previous tick, world metres; zero after a gap.
    pub moved_m: Vec3,
    /// How far the horizontal move is from what the previous tick's wanted velocity and its
    /// ground's velocity explain, metres.
    pub unexplained_m: f32,
    /// The wanted horizontal velocity, world `(x, z)` metres per second, at each stage of the tick,
    /// in order: as the movement task wrote it (the clip's root speed), after the stick locomotion
    /// (the stick's velocity, or the task's while the animation moves the body), the palms' push
    /// added, and the chase added.
    pub task_mps: Vec2,
    pub stick_mps: Vec2,
    pub palms_mps: Vec2,
    pub chase_mps: Vec2,
    /// The wanted velocity the tick left, world metres per second.
    pub wanted_mps: Vec3,
    /// The proxy's own velocity as the tick read it, world metres per second.
    pub proxy_mps: Vec3,
    /// The velocity of the surface the proxy stands on, world metres per second.
    pub surface_mps: Vec3,
    /// The proxy's support: `unsupported`, `sliding`, or `supported`.
    pub support: Label,
    /// Whether the surface is a dynamic body.
    pub surface_dynamic: bool,
    /// The body the proxy stands on.
    pub ground_body: Option<u32>,
    /// The proxy's current, pending, and default states, as the game numbers them.
    pub proxy_state: i32,
    pub pending_proxy_state: i32,
    pub default_proxy_state: i32,
    /// Whether the animation carried the body this tick, so the task's velocity passed through.
    pub authored: bool,
    /// Whether the mod holds the crouch capsule, and whether its release waits for headroom.
    pub capsule_owned: bool,
    pub capsule_blocked: bool,
    /// The ticks whose move the wanted velocity did not explain, since the payload loaded.
    pub jumps: u64,
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
    /// The chosen target's world point, when the reach supplies one; absent before version 8.
    pub target_point: Option<Vec3>,
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

impl WorldMotion {
    /// No motion.
    pub const ZERO: Self = Self {
        linear: Vec3::ZERO,
        angular: Vec3::ZERO,
    };
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

/// The body's animation state, noted when it changes and repeated now and then, so that a stretch
/// of a recording, such as a lookback save, opens with the state it started in.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct BodyState {
    /// The current state of the body's animation rule system, as the game's state-name hash, when
    /// the chain to it was readable.
    pub rule_state: Option<u32>,
    /// The state's name, when the mod knows the name behind the hash.
    pub rule_state_name: Option<Label>,
    /// The procedural body's hand-over case: who poses the legs and the torso.
    pub case: Label,
    /// Whether nothing changed since the last note, and this one only repeats it.
    pub repeat: bool,
}

impl BodyState {
    /// The rule state as the tools print it: its name when known, else its hash, else a note that
    /// none was readable.
    pub fn rule_label(&self) -> String {
        match (&self.rule_state_name, self.rule_state) {
            (Some(name), _) => name.to_string(),
            (None, Some(hash)) => format!("{hash:#010x}"),
            (None, None) => "no rule state".to_owned(),
        }
    }
}

/// An item that left a hand with a requested motion: a gun let go, a grenade, a charge, or a carried
/// object thrown.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Throw {
    pub hand: Hand,
    /// What left the hand.
    pub item: Label,
    /// The motion the release requested of the item, in the world's units: metres and radians per
    /// game second. The same motion per real second is this times [`time_scale`](Self::time_scale).
    pub motion: WorldMotion,
    /// The game's time scale at the release.
    pub time_scale: f32,
    /// Where the hand's part of the estimate came from, when the release estimated one.
    pub source: Option<Label>,
    /// Whether the item left live (a grenade with its pin out, or a caught grenade), when the item
    /// has such a state.
    pub armed: Option<bool>,
    /// Where the item left the hand, world space, when the release knows; absent before version 8.
    pub release: Option<ReleasePose>,
}

/// Where a thrown item left the hand.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReleasePose {
    pub position: Vec3,
    /// Its orientation, when the release read one.
    pub orientation: Option<Quat>,
}

/// A hand that let go of what its grip held, whether the grip opened or the hold was lost.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct GripRelease {
    pub hand: Hand,
    /// What the hand held.
    pub held: Label,
    /// The held object's address, a correlation identity only, and its body.
    pub object: Option<u64>,
    pub body: Option<u32>,
    /// Why it let go: the grip opened, or the hold was lost and how.
    pub cause: Label,
    /// Whether the release threw what it held (a [`Throw`] records the motion).
    pub thrown: bool,
}

/// A change of the wingsuit gesture recognizer's phase, or of why it is unavailable.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct WingsuitGesture {
    /// The phase, absent while the player is neither falling nor in the suit.
    pub phase: Option<Label>,
    /// Why the gestures are unavailable, while they are.
    pub why: Option<Label>,
}

/// A change of the grapple snatch's phase.
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct Snatch {
    /// The hand the snatch belongs to, when one does.
    pub hand: Option<Hand>,
    /// The phase it left and the phase it entered.
    pub from: Label,
    pub to: Label,
    /// How the snatch ended, when it ended: `taken`, `failed`, `cancelled`, or `stood down`.
    pub outcome: Option<Label>,
    /// Why, for an ending that has a reason: the refusal, the cancellation, or what was taken.
    pub reason: Option<Label>,
    /// The offered target's kind and distance, metres, on arming and firing.
    pub target: Option<Label>,
    pub distance_m: Option<f32>,
    /// The yank that fired it: how far back the hand pulled, metres, and how fast, metres per
    /// second.
    pub yank_m: Option<f32>,
    pub yank_mps: Option<f32>,
}
