//! The lower body's poses: standing, crouching, and kneeling on one knee once the head goes below
//! the floor of a squat. Pure, in Rico's model space, with the places a pose holds kept in the
//! world so the root moving under them does not slide them.
//!
//! [`LowerBody`] picks the pose from the player's head and what the body is doing, and eases each
//! held pose in and out. Standing and crouching are the solver's own: the hips follow the head down
//! to `min_hips_height`, and the crouch's depth only scales the walk. The kneel is a pose of its own
//! ([`LowerTargets`]): it pins the hips, places both feet, and points both knees, and the solver
//! mixes them over its own by the pose's weight.
//!
//! - Kneeling: the head below the squat's floor, still on the ground and not moving, for a moment.
//!   One knee goes down: the trailing foot's, or the off hand's side when neither foot trails. The
//!   hips are held over the kneeling knee where they were when the kneel began, at the height the
//!   head asks for between an upright kneel and one sat back toward the heel; the kneeling shin
//!   lies on the ground behind the knee with the toes tucked under, and the other foot stands
//!   ahead with its knee up. The head rising past the floor, the body moving, or leaving the
//!   ground ends it.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::controllers::body::procedural::{
    config::ProceduralBodyConfig,
    rig::{Rig, RigBone},
    solver::{arms::Side, feet::FootPose},
};

/// The lower body's pose.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LowerPose {
    #[default]
    Stand,
    Crouch,
    Kneel,
}

impl LowerPose {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stand => "stand",
            Self::Crouch => "crouch",
            Self::Kneel => "kneel",
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
        let (left, right) = (at(RigBone::LeftUpLeg), at(RigBone::RightUpLeg));
        Self {
            standing_hips: hips.y,
            lowest_hips: hips.y * config.min_hips_height.clamp(0.1, 1.0),
            head_over_hips: at(RigBone::Head).y - hips.y,
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
        KNEE_GROUND_M + self.thigh + self.hip_joint_drop
    }

    /// The kneeling hips' lowest: sat back toward the heel.
    pub fn kneel_low(&self) -> f32 {
        KNEE_GROUND_M + KNEEL_LOW_THIGH * self.thigh + self.hip_joint_drop
    }
}

/// What the poses are allowed, from the settings.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LowerTuning {
    /// Kneel below the squat's floor.
    pub kneel: bool,
    /// How long the kneel takes to ease in or out, seconds.
    pub kneel_blend_s: f32,
}

impl LowerTuning {
    pub fn of(config: &ProceduralBodyConfig) -> Self {
        Self {
            kneel: config.kneel,
            kneel_blend_s: config.kneel_blend_s,
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
        }
    }
}

/// A change of pose, for the log.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PoseChange {
    pub from: LowerPose,
    pub to: LowerPose,
}

/// The lower body's pose, the places a held pose keeps, and the weights it eases in and out by.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LowerBody {
    pose: LowerPose,
    kneel: Option<Kneel>,
    kneel_weight: Ramp,
    low_s: f32,
    status: LowerStatus,
}

impl LowerBody {
    pub const fn new() -> Self {
        Self {
            pose: LowerPose::Stand,
            kneel: None,
            kneel_weight: Ramp::new(),
            low_s: 0.0,
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
        let wanted = rig.wanted_hips(input.head.y);
        let wanted = if wanted.is_finite() {
            wanted
        } else {
            rig.standing_hips
        };
        let below_floor = wanted < rig.lowest_hips - KNEEL_ENTER_M;
        let above_floor = wanted > rig.lowest_hips + KNEEL_EXIT_M;

        let kneeling = self.kneel.is_some_and(|k| k.active);
        if kneeling {
            if (!tuning.kneel || !input.grounded || input.moving || above_floor)
                && let Some(kneel) = &mut self.kneel
            {
                kneel.active = false;
            }
        } else {
            self.low_s = if tuning.kneel && input.grounded && !input.moving && below_floor {
                self.low_s + dt
            } else {
                0.0
            };
            if self.low_s >= KNEEL_AFTER_S {
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

        let previous = self.pose;
        let depth = input.crouch;
        self.pose = if active {
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
            kneel_weight: weight,
            kneel_side: self.kneel.filter(|k| k.active).map(|k| k.side),
            moving: input.moving,
        };
        (self.pose != previous).then_some(PoseChange {
            from: previous,
            to: self.pose,
        })
    }

    /// The held pose's targets this tick, `None` while no pose is held.
    pub fn targets(&self, rig: &LowerRig, input: &LowerInput) -> Option<LowerTargets> {
        let kneel = self.kneel?;
        let weight = self.kneel_weight.eased();
        if weight <= 0.0 {
            return None;
        }
        let targets = kneel.targets(rig, input, weight);
        targets.is_finite().then_some(targets)
    }
}

/// How far below the squat's floor the head must ask the hips to go before the body kneels,
/// metres.
const KNEEL_ENTER_M: f32 = 0.05;

/// How far above the squat's floor the head must ask the hips to come for the kneel to end,
/// metres.
const KNEEL_EXIT_M: f32 = 0.03;

/// How long the head stays low before the body kneels, seconds: a dip through the floor on the
/// way somewhere else does not kneel.
const KNEEL_AFTER_S: f32 = 0.25;

/// The crouch's depth past which the pose reads as a crouch, and below which it reads as standing
/// again.
const CROUCH_ENTER: f32 = 0.15;
const CROUCH_EXIT: f32 = 0.1;

/// How high the kneeling knee's joint sits above the ground, metres: the knee's own thickness.
const KNEE_GROUND_M: f32 = 0.06;

/// The kneeling hips' lowest, as the share of the thigh's length the hip joint stands above the
/// kneeling knee.
const KNEEL_LOW_THIGH: f32 = 0.55;

/// How far toward the kneeling side the hips move, as a share of half the hip width: the weight
/// goes over the knee that is down.
const KNEEL_SHIFT: f32 = 0.4;

/// How far the standing foot of a kneel is ahead of the hips, as a share of the thigh.
const FRONT_REACH: f32 = 0.85;

/// How far the kneeling foot is pitched toes-down, radians: the toes tucked under.
const KNEEL_TOE_PITCH: f32 = 1.0;

/// How far one foot must be behind the other, along the hips' facing, to be the trailing one,
/// metres.
const TRAIL_M: f32 = 0.06;

/// A kneel: which knee is down, and where the hips are held, in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Kneel {
    side: Side,
    /// The hips' place on the ground, world space.
    hips: Vec3,
    /// The hips' facing when the kneel began, world space, on the ground plane.
    forward: Vec3,
    active: bool,
}

impl Kneel {
    /// Begin a kneel from where the body stands now: the trailing foot's knee goes down, or the off
    /// hand's when neither trails, and the hips move toward it.
    fn begin(rig: &LowerRig, input: &LowerInput) -> Self {
        let forward = Quat::from_rotation_y(input.hips_yaw) * Vec3::NEG_Z;
        let right = forward.cross(Vec3::Y);
        let along = input.feet.map(|f| f.position.dot(forward));
        let side = if (along[0] - along[1]).abs() < TRAIL_M {
            input.off_side
        } else if along[0] < along[1] {
            Side::Left
        } else {
            Side::Right
        };
        let hips = Vec3::new(input.hips_xz.x, 0.0, input.hips_xz.y)
            + right * (side.sign() * rig.hip_half_width * KNEEL_SHIFT);
        Self {
            side,
            hips: input.world.transform_point3(hips),
            forward: flat(input.world.transform_vector3(forward)),
            active: true,
        }
    }

    /// The kneel's targets this tick, model space.
    fn targets(&self, rig: &LowerRig, input: &LowerInput, weight: f32) -> LowerTargets {
        let to_model = input.world.inverse();
        let ground = to_model.transform_point3(self.hips);
        let forward = flat(to_model.transform_vector3(self.forward));
        let right = forward.cross(Vec3::Y);
        let height = rig
            .wanted_hips(input.head.y)
            .clamp(rig.kneel_low(), rig.kneel_high());
        let hips = Vec3::new(ground.x, height, ground.z);
        let down = self.side;
        let up = down.other();
        let mut feet = input.stance;

        // The kneeling knee rests on the ground ahead of its hip joint by as far as the thigh
        // reaches past straight down, and the shin lies back from it to the tucked foot.
        let hip_joint =
            hips + right * (down.sign() * rig.hip_half_width) - Vec3::Y * rig.hip_joint_drop;
        let rise = (hip_joint.y - KNEE_GROUND_M).max(0.0);
        let ahead = (rig.thigh * rig.thigh - rise * rise).max(0.0).sqrt();
        let knee = Vec3::new(hip_joint.x, KNEE_GROUND_M, hip_joint.z) + forward * ahead;
        let toe_height = (rig.toe_length * KNEEL_TOE_PITCH.sin()).max(KNEE_GROUND_M);
        let lift = toe_height - KNEE_GROUND_M;
        let back = (rig.shin * rig.shin - lift * lift).max(0.0).sqrt();
        feet[down.index()] = FootPose {
            position: knee - forward * back + Vec3::Y * lift,
            rotation: (Quat::from_axis_angle(right, -KNEEL_TOE_PITCH)
                * input.stance[down.index()].rotation)
                .normalize(),
        };

        // The other foot stands ahead of its hip joint, its knee up.
        let ankle = input.stance[up.index()].position.y;
        let standing =
            hips + right * (up.sign() * rig.hip_half_width) + forward * (FRONT_REACH * rig.thigh);
        feet[up.index()] = FootPose {
            position: Vec3::new(standing.x, ankle, standing.z),
            rotation: input.stance[up.index()].rotation,
        };
        LowerTargets {
            weight,
            hips,
            feet,
            poles: [forward; 2],
        }
    }
}

impl LowerTargets {
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
mod tests {
    use super::*;
    use crate::controllers::body::procedural::{
        rig::tests::skeleton,
        solver::{SolveInput, pose::RigPose, solve},
    };

    const DT: f32 = 1.0 / 30.0;

    /// The test rig, its measures, a standing input, and the default tuning.
    fn setup() -> (LowerRig, LowerInput, LowerTuning) {
        let (_, rig, input, tuning) = setup_with_rig();
        (rig, input, tuning)
    }

    fn setup_with_rig() -> (Rig, LowerRig, LowerInput, LowerTuning) {
        let rig = Rig::from_skeleton(&skeleton()).unwrap();
        let config = ProceduralBodyConfig::new();
        let stance = [RigBone::LeftFoot, RigBone::RightFoot].map(|bone| FootPose {
            position: rig.reference(bone).translation,
            rotation: rig.reference(bone).rotation,
        });
        let input = LowerInput {
            world: Mat4::from_translation(Vec3::new(3.0, 1.0, -2.0)),
            head: rig.reference(RigBone::Head).translation,
            crouch: 0.0,
            hips_xz: Vec2::ZERO,
            hips_yaw: 0.0,
            feet: stance,
            stance,
            grounded: true,
            moving: false,
            off_side: Side::Left,
            dt: DT,
        };
        let lower = LowerRig::measure(&rig, &config);
        (rig, lower, input, LowerTuning::of(&config))
    }

    fn hold(
        body: &mut LowerBody,
        rig: &LowerRig,
        input: &LowerInput,
        tuning: &LowerTuning,
        s: f32,
    ) {
        for _ in 0..(s / DT).round() as usize {
            body.update(rig, input, tuning);
        }
    }

    /// Put the head where it asks for the hips at `hips`, with the crouch's depth that follows.
    fn lower_to(rig: &LowerRig, input: &mut LowerInput, hips: f32) {
        input.head.y = hips + rig.head_over_hips;
        let floored = hips.clamp(rig.lowest_hips, rig.standing_hips);
        input.crouch = (rig.standing_hips - floored) / (rig.standing_hips - rig.lowest_hips);
    }

    #[test]
    fn the_head_picks_standing_and_crouching() {
        let (rig, mut input, tuning) = setup();
        let mut body = LowerBody::new();
        assert_eq!(body.update(&rig, &input, &tuning), None);
        assert_eq!(body.status().pose, LowerPose::Stand);
        lower_to(&rig, &mut input, 0.8);
        assert_eq!(
            body.update(&rig, &input, &tuning),
            Some(PoseChange {
                from: LowerPose::Stand,
                to: LowerPose::Crouch
            })
        );
        // Within the hysteresis, a crouch stays a crouch.
        lower_to(&rig, &mut input, rig.standing_hips - 0.06);
        assert_eq!(body.update(&rig, &input, &tuning), None);
        assert_eq!(body.status().pose, LowerPose::Crouch);
        lower_to(&rig, &mut input, rig.standing_hips);
        body.update(&rig, &input, &tuning);
        assert_eq!(body.status().pose, LowerPose::Stand);
    }

    #[test]
    fn a_head_below_the_squats_floor_kneels_after_a_moment_on_the_off_hands_side() {
        let (rig, mut input, tuning) = setup();
        let mut body = LowerBody::new();
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        body.update(&rig, &input, &tuning);
        assert_eq!(body.status().pose, LowerPose::Crouch);
        hold(&mut body, &rig, &input, &tuning, KNEEL_AFTER_S + 0.05);
        let status = body.status();
        assert_eq!(status.pose, LowerPose::Kneel);
        assert_eq!(status.kneel_side, Some(Side::Left));
        input.off_side = Side::Right;
        let mut other = LowerBody::new();
        hold(&mut other, &rig, &input, &tuning, KNEEL_AFTER_S + 0.1);
        assert_eq!(other.status().kneel_side, Some(Side::Right));
    }

    #[test]
    fn the_trailing_foot_kneels() {
        let (rig, mut input, tuning) = setup();
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        // The right foot behind (+Z is back).
        input.feet[1].position.z += 0.2;
        let mut body = LowerBody::new();
        hold(&mut body, &rig, &input, &tuning, 0.5);
        assert_eq!(body.status().kneel_side, Some(Side::Right));
        // Turned half round, the same feet put the left one behind.
        input.hips_yaw = std::f32::consts::PI;
        let mut turned = LowerBody::new();
        hold(&mut turned, &rig, &input, &tuning, 0.5);
        assert_eq!(turned.status().kneel_side, Some(Side::Left));
    }

    #[test]
    fn moving_or_rising_ends_the_kneel_and_moving_never_starts_one() {
        let (rig, mut input, tuning) = setup();
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        let mut body = LowerBody::new();
        hold(&mut body, &rig, &input, &tuning, 0.5);
        assert_eq!(body.status().pose, LowerPose::Kneel);
        input.moving = true;
        body.update(&rig, &input, &tuning);
        assert_eq!(body.status().pose, LowerPose::Crouch);
        hold(&mut body, &rig, &input, &tuning, 2.0);
        assert_eq!(body.status().pose, LowerPose::Crouch);
        input.moving = false;
        hold(&mut body, &rig, &input, &tuning, 0.5);
        assert_eq!(body.status().pose, LowerPose::Kneel);
        // Just above the floor holds the kneel; further up ends it.
        lower_to(&rig, &mut input, rig.lowest_hips + 0.01);
        body.update(&rig, &input, &tuning);
        assert_eq!(body.status().pose, LowerPose::Kneel);
        lower_to(&rig, &mut input, rig.lowest_hips + 0.1);
        body.update(&rig, &input, &tuning);
        assert_eq!(body.status().pose, LowerPose::Crouch);
        // Off the ground, and switched off, it does not start.
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        input.grounded = false;
        hold(&mut body, &rig, &input, &tuning, 1.0);
        assert_ne!(body.status().pose, LowerPose::Kneel);
        input.grounded = true;
        let off = LowerTuning {
            kneel: false,
            ..tuning
        };
        hold(&mut body, &rig, &input, &off, 1.0);
        assert_ne!(body.status().pose, LowerPose::Kneel);
    }

    #[test]
    fn the_kneel_eases_in_and_out() {
        let (rig, mut input, tuning) = setup();
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        let mut body = LowerBody::new();
        let mut last = 0.0;
        let mut weights = vec![];
        for _ in 0..60 {
            body.update(&rig, &input, &tuning);
            let w = body.status().kneel_weight;
            assert!(w >= last - 1e-6, "the weight fell on the way in");
            last = w;
            weights.push(w);
        }
        assert_eq!(last, 1.0);
        // Eased: the first steps of the blend are small.
        let first = weights.iter().copied().find(|w| *w > 0.0).unwrap();
        assert!(first < 0.05, "{first}");
        assert!(body.targets(&rig, &input).is_some());
        lower_to(&rig, &mut input, rig.standing_hips);
        for _ in 0..60 {
            body.update(&rig, &input, &tuning);
            let w = body.status().kneel_weight;
            assert!(w <= last + 1e-6, "the weight rose on the way out");
            last = w;
        }
        assert_eq!(last, 0.0);
        assert!(body.targets(&rig, &input).is_none());
    }

    #[test]
    fn the_kneeling_knee_is_on_the_ground_and_the_hips_stay_over_it_as_the_root_moves() {
        let (rig, mut input, tuning) = setup();
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        let mut body = LowerBody::new();
        hold(&mut body, &rig, &input, &tuning, 1.0);
        let targets = body.targets(&rig, &input).unwrap();
        assert_eq!(targets.weight, 1.0);
        // The hips are at the height the head asks for, between the kneel's limits.
        assert!((targets.hips.y - (rig.lowest_hips - 0.1)).abs() < 1e-4);
        // The kneeling (left) foot is behind the hips with its toes down, the other ahead.
        let [left, right] = targets.feet;
        assert!(left.position.z > targets.hips.z && right.position.z < targets.hips.z);
        assert!((left.rotation * Vec3::NEG_Z).y < -0.5);
        assert!((right.position.y - input.stance[1].position.y).abs() < 1e-5);
        // The kneeling leg's lengths put the knee on the ground: hip joint, knee, ankle.
        let hip = targets.hips - Vec3::X * rig.hip_half_width - Vec3::Y * rig.hip_joint_drop;
        let span = hip.distance(left.position);
        assert!(span <= rig.thigh + rig.shin + 1e-4);
        // The root moves a little under the held hips: the targets stay put in the world.
        let world = input.world;
        input.world = world * Mat4::from_translation(Vec3::new(0.05, 0.0, 0.03));
        let moved = body.targets(&rig, &input).unwrap();
        let in_world = |w: &Mat4, p: Vec3| w.transform_point3(p);
        assert!(
            in_world(&input.world, moved.hips).abs_diff_eq(in_world(&world, targets.hips), 1e-4)
        );
        // A head lower still sits back no further than the kneel's lowest.
        lower_to(&rig, &mut input, 0.0);
        body.update(&rig, &input, &tuning);
        let low = body.targets(&rig, &input).unwrap();
        assert!((low.hips.y - rig.kneel_low()).abs() < 1e-4);
    }

    /// The solve's input for a standing reference pose with the feet at the stance.
    fn standing_solve_input(full: &Rig, input: &LowerInput) -> SolveInput {
        let animated = RigPose {
            bones: full.reference,
        };
        SolveInput {
            animated,
            head: input.head,
            head_rotation: Quat::IDENTITY,
            torso_yaw: Some(0.0),
            feet: input.stance.map(|f| f.position),
            foot_rotations: input.stance.map(|f| f.rotation),
            support: (input.stance[0].position + input.stance[1].position) * 0.5,
            planted: [true; 2],
            heel_lifts: [0.0; 2],
            hips_xz: None,
            hips_drop: 0.0,
            lower: None,
        }
    }

    #[test]
    fn the_solve_puts_the_kneeling_knee_on_the_ground_and_the_other_up() {
        let (full, rig, mut input, tuning) = setup_with_rig();
        let config = ProceduralBodyConfig::new();
        input.world = Mat4::IDENTITY;
        lower_to(&rig, &mut input, rig.lowest_hips - 0.1);
        let mut body = LowerBody::new();
        hold(&mut body, &rig, &input, &tuning, 1.0);
        let targets = body.targets(&rig, &input).unwrap();
        let mut solve_input = standing_solve_input(&full, &input);
        solve_input.hold(targets);
        let pose = solve(&full, &solve_input, &config);
        assert!(pose.position(RigBone::Hips).abs_diff_eq(targets.hips, 1e-3));
        let knee = pose.position(RigBone::LeftLeg);
        assert!(
            (knee.y - KNEE_GROUND_M).abs() < 0.01,
            "the knee is at {knee}"
        );
        assert!(
            pose.position(RigBone::LeftFoot)
                .abs_diff_eq(targets.feet[0].position, 2e-3)
        );
        // The standing leg's knee is up, ahead of its hip and above its ankle.
        let front = pose.position(RigBone::RightLeg);
        assert!(
            front.y > pose.position(RigBone::RightFoot).y + 0.2,
            "{front}"
        );
        assert!(front.z < pose.position(RigBone::RightUpLeg).z);
        // Half the weight is half the way from standing.
        let mut half = standing_solve_input(&full, &input);
        half.hold(LowerTargets {
            weight: 0.5,
            ..targets
        });
        let between = solve(&full, &half, &config);
        let standing = pose_without(&full, &input, &config);
        let mid = (standing.position(RigBone::Hips) + targets.hips) * 0.5;
        assert!(between.position(RigBone::Hips).abs_diff_eq(mid, 1e-3));
    }

    fn pose_without(full: &Rig, input: &LowerInput, config: &ProceduralBodyConfig) -> RigPose {
        solve(full, &standing_solve_input(full, input), config)
    }
}
