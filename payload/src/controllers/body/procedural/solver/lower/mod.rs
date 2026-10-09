//! The lower body's poses: standing, crouching, kneeling on one knee once the head goes below the
//! floor of a squat, and sitting on the ground. Pure, in Rico's model space, with the places a pose
//! holds kept in the world so the root moving under them does not slide them.
//!
//! [`LowerBody`] picks the pose from the player's head and what the body is doing, and eases each
//! held pose in and out. Standing and crouching are the solver's own: the hips follow the head down
//! to `min_hips_height`, and the crouch's depth only scales the walk. A kneel and a sit are poses of
//! their own ([`LowerTargets`]): each pins the hips, places both feet, and points both knees, and
//! the solver mixes them over its own by the pose's weight, a sit over a kneel.
//!
//! - [`kneel`]: the head below the squat's floor, still on the ground and not moving, for a moment.
//!   One knee goes down, the trailing foot's or the off hand's side, under hips held over it.
//! - [`sit`]: in standing play, the head low and still for a moment. The hips go to the ground
//!   under the player and the legs come out in front, crossed or with the knees up. Standing up,
//!   by raising the head or by pushing the stick, blends back through a crouch, and the body holds
//!   the stick's walk until it is up.

mod kneel;
mod sit;

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::{
        arms::Side,
        feet::FootPose,
        lower::{kneel::Kneel, sit::Sit},
    },
};

/// The lower body's pose.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LowerPose {
    #[default]
    Stand,
    Crouch,
    Kneel,
    Sit,
}

impl LowerPose {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stand => "stand",
            Self::Crouch => "crouch",
            Self::Kneel => "kneel",
            Self::Sit => "sit",
        }
    }
}

/// How the legs lie while sitting on the ground.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SitLegs {
    /// Crossed in front, the knees out to the sides: the head forward over them.
    Crossed,
    /// The feet flat in front, the knees up: the head upright or back.
    KneesUp,
}

impl SitLegs {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Crossed => "crossed",
            Self::KneesUp => "knees up",
        }
    }
}

/// The rig's measures the poses are built from, model space, metres.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerRig {
    /// The hips' standing height.
    pub standing_hips: f32,
    /// The hips' lowest in a squat: `min_hips_height` of standing.
    pub lowest_hips: f32,
    /// The reference head's height over the hips.
    pub head_over_hips: f32,
    /// The reference head's horizontal offset from the hips, model space `(x, z)`.
    pub head_ahead: Vec2,
    /// How far the hip joints sit below the hips.
    pub hip_joint_drop: f32,
    /// Half the distance between the hip joints.
    pub hip_half_width: f32,
    /// The hip joint to the knee.
    pub thigh: f32,
    /// The knee to the ankle.
    pub shin: f32,
    /// The ankle to the toes.
    pub toe_length: f32,
}

impl LowerRig {
    pub fn measure(rig: &Rig, config: &ProceduralBodyConfig) -> Self {
        let at = |bone| rig.reference(bone).translation;
        let hips = at(RigBone::Hips);
        let head = at(RigBone::Head);
        let (left, right) = (at(RigBone::LeftUpLeg), at(RigBone::RightUpLeg));
        Self {
            standing_hips: hips.y,
            lowest_hips: hips.y * config.min_hips_height.clamp(0.1, 1.0),
            head_over_hips: head.y - hips.y,
            head_ahead: Vec2::new(head.x - hips.x, head.z - hips.z),
            hip_joint_drop: hips.y - (left.y + right.y) * 0.5,
            hip_half_width: (right.x - left.x).abs() * 0.5,
            thigh: left.distance(at(RigBone::LeftLeg)),
            shin: at(RigBone::LeftLeg).distance(at(RigBone::LeftFoot)),
            toe_length: at(RigBone::LeftFoot).distance(at(RigBone::LeftToeBase)),
        }
    }

    /// The hips' height a head at `head_height` asks for, before any floor: the head's less the
    /// reference pose's head-over-hips height.
    pub fn wanted_hips(&self, head_height: f32) -> f32 {
        head_height - self.head_over_hips
    }

    /// The kneeling hips' highest: upright on the knee, the thigh straight above it.
    pub fn kneel_high(&self) -> f32 {
        kneel::KNEE_GROUND_M + self.thigh + self.hip_joint_drop
    }

    /// The kneeling hips' lowest: sat back toward the heel.
    pub fn kneel_low(&self) -> f32 {
        kneel::KNEE_GROUND_M + kneel::KNEEL_LOW_THIGH * self.thigh + self.hip_joint_drop
    }

    /// The sitting hips' height: the hip joints just above the ground.
    pub fn sit_hips(&self) -> f32 {
        sit::SIT_HIP_JOINT_M + self.hip_joint_drop
    }

    /// The wanted hips' height below which a still head sits: halfway between a sit and the
    /// kneel's lowest, so a kneel sat back on the heel stays a kneel.
    pub fn sit_below(&self) -> f32 {
        (self.sit_hips() + self.kneel_low()) * 0.5
    }
}

/// What the poses are allowed, from the settings.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerTuning {
    /// Kneel below the squat's floor.
    pub kneel: bool,
    /// How long the kneel takes to ease in or out, seconds.
    pub kneel_blend_s: f32,
    /// Sit on the ground in standing play.
    pub sit: bool,
    /// How long the sit takes to ease in or out, seconds.
    pub sit_blend_s: f32,
    /// How long the head stays low and still before the body sits, seconds.
    pub sit_after_s: f32,
}

impl LowerTuning {
    pub fn of(config: &ProceduralBodyConfig) -> Self {
        Self {
            kneel: config.kneel,
            kneel_blend_s: config.kneel_blend_s,
            sit: config.sit,
            sit_blend_s: config.sit_blend_s,
            sit_after_s: config.sit_after_s,
        }
    }
}

/// One tick's input to the pose selection.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerInput {
    /// The root's world transform: the held places are kept in the world.
    pub world: Mat4,
    /// The player's head, model space.
    pub head: Vec3,
    /// The head's height the hips follow, model space
    /// ([`SolveInput::hips_head`](crate::controllers::body::procedural::solver::SolveInput::hips_head)).
    pub hips_head_y: f32,
    /// The player's head, world space: how fast it moves says whether the player is still.
    pub head_world: Vec3,
    /// How far the head has taken the hips toward the squat's floor, `0..=1`
    /// ([`crouch_depth`](crate::controllers::body::procedural::solver::crouch_depth)).
    pub crouch: f32,
    /// Where the hips sit horizontally now, model space `(x, z)`.
    pub hips_xz: Vec2,
    /// The hips' facing about +Y, model space, radians.
    pub hips_yaw: f32,
    /// The feet as they stand now, model space, left then right: the one further back kneels.
    pub feet: [FootPose; 2],
    /// The neutral stance's feet, model space, left then right: their height is the ankle's.
    pub stance: [FootPose; 2],
    /// Whether the body is on foot on the ground with the procedural legs.
    pub grounded: bool,
    /// Whether the body is told to move: the stick, or the movement task.
    pub moving: bool,
    /// Whether the move stick is pushed: it stands a sitting body up.
    pub stick: bool,
    /// Whether the player plays standing (the standing or the roomscale posture): only a standing
    /// player's head goes to the floor by sitting on it.
    pub standing_play: bool,
    /// The off hand's side: the knee that goes down when neither foot trails.
    pub off_side: Side,
    /// The tick's step, seconds.
    pub dt: f32,
}

/// A held pose's targets, model space, for the solver to mix over its own by `weight`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerTargets {
    /// How much of the pose to take, `0..=1`.
    pub weight: f32,
    /// Where the hips go.
    pub hips: Vec3,
    /// Where each foot (the ankle) goes and how it turns, left then right.
    pub feet: [FootPose; 2],
    /// Where each knee points, left then right.
    pub poles: [Vec3; 2],
}

/// What the pose selection decided and from what, for the readouts and the log.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LowerStatus {
    pub pose: LowerPose,
    /// The hips' height the head asks for, model space, metres.
    pub wanted_hips_m: f32,
    /// The squat's floor, metres.
    pub floor_m: f32,
    /// How long the head has been low enough to kneel, seconds.
    pub low_s: f32,
    /// The kneel's weight, `0..=1`.
    pub kneel_weight: f32,
    /// The side of the knee that is down, while kneeling.
    pub kneel_side: Option<Side>,
    /// Whether the body is told to move.
    pub moving: bool,
    /// The head's speed, metres per second, smoothed.
    pub head_speed_mps: f32,
    /// How long the head has been low and still enough to sit, seconds.
    pub still_s: f32,
    /// The sit's weight, `0..=1`.
    pub sit_weight: f32,
    /// How the legs lie, while sitting.
    pub sit_legs: Option<SitLegs>,
    /// Whether a sit may begin: false after the stick stood the body up, until the head rises.
    pub sit_armed: bool,
    /// Whether the player plays standing.
    pub standing_play: bool,
}

impl LowerStatus {
    pub const fn new() -> Self {
        Self {
            pose: LowerPose::Stand,
            wanted_hips_m: 0.0,
            floor_m: 0.0,
            low_s: 0.0,
            kneel_weight: 0.0,
            kneel_side: None,
            moving: false,
            head_speed_mps: 0.0,
            still_s: 0.0,
            sit_weight: 0.0,
            sit_legs: None,
            sit_armed: true,
            standing_play: false,
        }
    }

    /// Whether the body sits on the ground or is still getting up from it: the stick stands it up
    /// rather than walking it.
    pub fn holds_stick(&self) -> bool {
        self.pose == LowerPose::Sit || self.sit_weight > 0.0
    }
}

/// A change of pose, for the log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PoseChange {
    pub from: LowerPose,
    pub to: LowerPose,
    /// Why a sit ended, when this change ended one.
    pub sit_end: Option<SitEnd>,
}

/// Why a sit ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SitEnd {
    /// The head rose.
    HeadRose,
    /// The move stick was pushed.
    Stick,
    /// The body left the ground, the posture stopped being standing, or sitting was switched off.
    Interrupted,
}

impl SitEnd {
    pub const fn label(self) -> &'static str {
        match self {
            Self::HeadRose => "the head rose",
            Self::Stick => "the stick",
            Self::Interrupted => "interrupted",
        }
    }
}

/// The lower body's pose, the places a held pose keeps, and the weights it eases in and out by.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LowerBody {
    pose: LowerPose,
    kneel: Option<Kneel>,
    kneel_weight: Ramp,
    low_s: f32,
    sit: Option<Sit>,
    sit_weight: Ramp,
    still_s: f32,
    /// Whether a sit may begin; the stick standing the body up disarms it until the head rises.
    sit_armed: bool,
    head: Option<Vec3>,
    head_speed: f32,
    status: LowerStatus,
}

impl LowerBody {
    pub const fn new() -> Self {
        Self {
            pose: LowerPose::Stand,
            kneel: None,
            kneel_weight: Ramp::new(),
            low_s: 0.0,
            sit: None,
            sit_weight: Ramp::new(),
            still_s: 0.0,
            sit_armed: true,
            head: None,
            head_speed: 0.0,
            status: LowerStatus::new(),
        }
    }

    /// Stand at once: the body has left the procedural legs.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn status(&self) -> LowerStatus {
        self.status
    }

    /// Advance by one tick and pick the pose. Returns the change of pose, if any.
    pub fn update(
        &mut self,
        rig: &LowerRig,
        input: &LowerInput,
        tuning: &LowerTuning,
    ) -> Option<PoseChange> {
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let wanted = rig.wanted_hips(input.hips_head_y);
        let wanted = if wanted.is_finite() {
            wanted
        } else {
            rig.standing_hips
        };
        self.track_head(input.head_world, dt);
        let sit_end = self.update_sit(rig, input, tuning, wanted, dt);
        let sitting = self.sit.is_some_and(|s| s.active);
        self.update_kneel(rig, input, tuning, wanted, dt, sitting);

        let kneeling = self.kneel.is_some_and(|k| k.active);
        let previous = self.pose;
        let depth = input.crouch;
        self.pose = if sitting {
            LowerPose::Sit
        } else if kneeling {
            LowerPose::Kneel
        } else if depth > CROUCH_ENTER || (previous == LowerPose::Crouch && depth > CROUCH_EXIT) {
            LowerPose::Crouch
        } else {
            LowerPose::Stand
        };
        self.status = LowerStatus {
            pose: self.pose,
            wanted_hips_m: wanted,
            floor_m: rig.lowest_hips,
            low_s: self.low_s,
            kneel_weight: self.kneel_weight.eased(),
            kneel_side: self.kneel.filter(|k| k.active).map(|k| k.side),
            moving: input.moving,
            head_speed_mps: self.head_speed,
            still_s: self.still_s,
            sit_weight: self.sit_weight.eased(),
            sit_legs: self.sit.filter(|s| s.active).map(|s| s.legs()),
            sit_armed: self.sit_armed,
            standing_play: input.standing_play,
        };
        (self.pose != previous).then_some(PoseChange {
            from: previous,
            to: self.pose,
            sit_end,
        })
    }

    /// The held poses' targets this tick, a sit's over a kneel's, `None` while no pose is held.
    pub fn targets(&self, rig: &LowerRig, input: &LowerInput) -> Option<LowerTargets> {
        let kneel = self
            .kneel
            .map(|k| (k, self.kneel_weight.eased()))
            .filter(|(_, w)| *w > 0.0)
            .map(|(k, w)| k.targets(rig, input, w));
        let sit = self
            .sit
            .map(|s| (s, self.sit_weight.eased()))
            .filter(|(_, w)| *w > 0.0)
            .map(|(s, w)| s.targets(rig, input, w));
        let targets = match (sit, kneel) {
            (Some(sit), Some(kneel)) => sit.over(&kneel),
            (sit, kneel) => sit.or(kneel)?,
        };
        targets.is_finite().then_some(targets)
    }

    /// Follow the head's speed, smoothed over [`HEAD_SPEED_S`]. A jump of the head too fast to be
    /// the player's own (a recenter, a teleport) is left out.
    fn track_head(&mut self, head: Vec3, dt: f32) {
        if !head.is_finite() {
            return;
        }
        if let Some(last) = self.head
            && dt > 0.0
        {
            let speed = head.distance(last) / dt;
            if speed.is_finite() && speed <= MAX_HEAD_SPEED_MPS {
                let k = 1.0 - (-dt / HEAD_SPEED_S).exp();
                self.head_speed += (speed - self.head_speed) * k;
            }
        }
        self.head = Some(head);
    }

    /// Begin, hold, or end the sit. Returns why it ended, on the tick it does.
    fn update_sit(
        &mut self,
        rig: &LowerRig,
        input: &LowerInput,
        tuning: &LowerTuning,
        wanted: f32,
        dt: f32,
    ) -> Option<SitEnd> {
        let below = rig.sit_below();
        if !self.sit_armed && wanted > below + sit::SIT_REARM_M {
            self.sit_armed = true;
        }
        let mut ended = None;
        match &mut self.sit {
            Some(sit) if sit.active => {
                ended = if !tuning.sit || !input.grounded || !input.standing_play {
                    Some(SitEnd::Interrupted)
                } else if input.stick {
                    Some(SitEnd::Stick)
                } else if wanted > below + sit::SIT_EXIT_M {
                    Some(SitEnd::HeadRose)
                } else {
                    None
                };
                match ended {
                    Some(end) => {
                        sit.active = false;
                        if end == SitEnd::Stick {
                            self.sit_armed = false;
                        }
                    }
                    None => sit.choose_legs(rig, input, dt),
                }
            }
            _ => {
                let can_sit = tuning.sit
                    && self.sit_armed
                    && input.grounded
                    && input.standing_play
                    && !input.moving
                    && !input.stick
                    && wanted < below
                    && self.head_speed < sit::SIT_STILL_MPS;
                self.still_s = if can_sit { self.still_s + dt } else { 0.0 };
                if self.still_s >= tuning.sit_after_s.max(0.0) {
                    self.still_s = 0.0;
                    self.sit = Some(Sit::begin(rig, input));
                    if let Some(kneel) = &mut self.kneel {
                        kneel.active = false;
                    }
                }
            }
        }
        let active = self.sit.is_some_and(|s| s.active);
        let weight = self
            .sit_weight
            .step(active, dt, tuning.sit_blend_s.max(0.0));
        if let Some(sit) = &mut self.sit {
            sit.ease_legs(dt);
        }
        if !active && weight <= 0.0 {
            self.sit = None;
        }
        ended
    }

    /// Begin, hold, or end the kneel. A sit under way keeps it from beginning.
    fn update_kneel(
        &mut self,
        rig: &LowerRig,
        input: &LowerInput,
        tuning: &LowerTuning,
        wanted: f32,
        dt: f32,
        sitting: bool,
    ) {
        let below_floor = wanted < rig.lowest_hips - kneel::KNEEL_ENTER_M;
        let above_floor = wanted > rig.lowest_hips + kneel::KNEEL_EXIT_M;
        let kneeling = self.kneel.is_some_and(|k| k.active);
        if kneeling {
            if (!tuning.kneel || !input.grounded || input.moving || above_floor || sitting)
                && let Some(kneel) = &mut self.kneel
            {
                kneel.active = false;
            }
        } else {
            let can_kneel =
                tuning.kneel && input.grounded && !input.moving && below_floor && !sitting;
            self.low_s = if can_kneel { self.low_s + dt } else { 0.0 };
            if self.low_s >= kneel::KNEEL_AFTER_S {
                self.low_s = 0.0;
                self.kneel = Some(Kneel::begin(rig, input));
            }
        }
        let active = self.kneel.is_some_and(|k| k.active);
        let weight = self
            .kneel_weight
            .step(active, dt, tuning.kneel_blend_s.max(0.0));
        if !active && weight <= 0.0 {
            self.kneel = None;
        }
    }
}

/// The crouch's depth past which the pose reads as a crouch, and below which it reads as standing
/// again: apart, so that a head bobbing about one threshold does not flip the pose.
const CROUCH_ENTER: f32 = 0.2;
const CROUCH_EXIT: f32 = 0.08;

/// The time constant of the head's speed, seconds.
const HEAD_SPEED_S: f32 = 0.2;

/// The fastest the head moves and still counts as the player's own motion, metres per second.
const MAX_HEAD_SPEED_MPS: f32 = 10.0;

impl LowerTargets {
    /// `self` held over `under`: the solver mixing the result over its own pose gives `under` by
    /// its weight, and `self` over that by its own.
    fn over(&self, under: &Self) -> Self {
        let top = self.weight.clamp(0.0, 1.0);
        let bottom = under.weight.clamp(0.0, 1.0);
        let weight = top + bottom - top * bottom;
        if weight <= 0.0 {
            return *self;
        }
        let share = top / weight;
        Self {
            weight,
            hips: under.hips.lerp(self.hips, share),
            feet: [0, 1].map(|i| FootPose {
                position: under.feet[i].position.lerp(self.feet[i].position, share),
                rotation: under.feet[i]
                    .rotation
                    .slerp(self.feet[i].rotation, share)
                    .normalize(),
            }),
            poles: [0, 1].map(|i| {
                under.poles[i]
                    .lerp(self.poles[i], share)
                    .try_normalize()
                    .unwrap_or(self.poles[i])
            }),
        }
    }

    fn is_finite(&self) -> bool {
        self.weight.is_finite()
            && self.hips.is_finite()
            && self
                .feet
                .iter()
                .all(|f| f.position.is_finite() && f.rotation.is_finite())
            && self.poles.iter().all(|p| p.is_finite())
    }
}

/// `v` on the ground plane, unit length; forward when it has no horizontal part.
fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
        .try_normalize()
        .unwrap_or(Vec3::NEG_Z)
}

/// The turn about +Y from the model's forward (-Z) to `forward`, on the ground plane: what turns a
/// stance foot to face along a held pose's facing.
fn facing(forward: Vec3) -> Quat {
    Quat::from_rotation_arc(Vec3::NEG_Z, flat(forward))
}

/// A weight that moves linearly toward on or off over a blend time, read eased.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Ramp {
    value: f32,
}

impl Ramp {
    const fn new() -> Self {
        Self { value: 0.0 }
    }

    /// Move toward one when `on` and zero otherwise over `time_s` seconds, and return the eased
    /// weight.
    fn step(&mut self, on: bool, dt: f32, time_s: f32) -> f32 {
        let target = if on { 1.0 } else { 0.0 };
        self.value = if time_s <= 0.0 {
            target
        } else {
            let step = dt / time_s;
            if on {
                (self.value + step).min(1.0)
            } else {
                (self.value - step).max(0.0)
            }
        };
        self.eased()
    }

    fn eased(&self) -> f32 {
        let t = self.value.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
}

#[cfg(test)]
mod tests;
