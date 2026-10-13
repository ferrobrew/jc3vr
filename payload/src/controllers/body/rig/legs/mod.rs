//! The legs: planted feet that step standing and walk moving, each holding its place in the world
//! while the body moves and turns above it. Pure, in world space.
//!
//! - [`regime`]: moving or standing, from the measured speed with hysteresis;
//! - [`gait`]: the moving regime's stride, duty, lift, and landing;
//! - [`stepping`]: the standing regime's support, capture point, and step triggers;
//! - [`swing`]: a foot through its step and on the ground;
//! - [`stance`]: the neutral stance the rest places come from.
//!
//! Moving, a phase advances by the root's measured distance over the stride, each foot lifts when
//! its share of the phase comes round, and it lands where its hip will be, half a stance ahead
//! along the measured velocity, with the capture term on the velocity's change, never further
//! from the hip than the leg reaches. Standing, planted feet hold until the capture point leaves
//! the support or a foot is too far from its rest place or turned from it, and then one foot steps
//! at a time. Stopping, a foot in the air finishes its swing to its standing place and plants.
//!
//! Walking, each foot also rolls onto its toes ([`Legs::heel_lifts`]). The ground under a foothold
//! comes from a query the caller supplies (a scene ray in the game, a function in the tests): each
//! step asks it every tick it is in the air, since the foothold moves on with the body, and a
//! planted foot keeps the height and the slope it landed on.

pub mod gait;
pub mod regime;
pub mod stance;
pub mod stepping;
pub mod swing;

#[cfg(test)]
mod tests;

use glam::Vec3;

use crate::controllers::body::rig::{
    config::LegsConfig,
    legs::{
        gait::{CrouchGait, GaitTuning, LandingInput, capture_time, landing_lead},
        regime::{Regime, blend},
        stepping::{StepTuning, choose},
        swing::{
            FootPose, Ground, arc, beside, grounded, heels_down, push_off, tilt, toe_off,
            with_offset,
        },
    },
};

/// One tick's input to the legs, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LegsInput {
    /// Where each foot rests under the body now, left then right: the neutral stance carried by the
    /// root, at the root's height.
    pub rest: [FootPose; 2],
    /// The root's height: the rest places' ground.
    pub root_height: f32,
    /// The root's measured horizontal velocity, metres per second: what paces the walk and where
    /// the feet land. The stick's commanded velocity never feeds it.
    pub velocity: Vec3,
    /// How far the hips have dropped toward their lowest, `0..=1`.
    pub crouch: f32,
    /// The hips' height over the ground, metres.
    pub hip_height_m: f32,
    /// The leg's length, metres.
    pub leg_length_m: f32,
    /// The tick's step, seconds.
    pub dt: f32,
}

/// The legs' tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LegsTuning {
    /// Whether the feet walk; off, they only step standing.
    pub gait: bool,
    pub crouch: CrouchGait,
    pub step: StepTuning,
    /// The measured speed above which the feet start walking, metres per second.
    pub moving_mps: f32,
    /// The measured speed below which they stand again, metres per second.
    pub standing_mps: f32,
    /// The capture term's weight in a walking landing.
    pub capture_gain: f32,
    /// The furthest a foot lands from its hip, as a share of the leg's length.
    pub reach_share: f32,
}

impl LegsTuning {
    /// The tuning the settings ask for, with the crouch's gait and the ankle's tilt limit.
    pub fn of(config: &LegsConfig, crouch: CrouchGait, tilt_limit_rad: f32) -> Self {
        Self {
            gait: config.gait,
            crouch,
            step: StepTuning {
                rest_m: config.rest_error_m,
                turn_rad: config.turn_error_deg.to_radians(),
                duration_s: config.step_s,
                height_m: config.step_height_m,
                margin_m: config.support_margin_m,
                tilt_limit_rad,
            },
            moving_mps: config.moving_mps,
            standing_mps: config.standing_mps,
            capture_gain: config.capture_gain,
            reach_share: config.reach_share,
        }
    }
}

/// The two feet, left then right.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Legs {
    feet: [Option<Foot>; 2],
    /// The walk's phase, `0..1`, while walking.
    phase: Option<f32>,
    /// How far each heel is lifted about its toes, radians.
    lifts: [f32; 2],
    regime: Regime,
    /// The measured velocity, smoothed for the capture term.
    mean_velocity: Vec3,
    /// The last walking touchdown: the foot, and how far ahead of its hip it landed, metres.
    touchdown: Option<(usize, f32)>,
    /// Steps begun since the legs were made.
    steps: u64,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Foot {
    Planted {
        pose: FootPose,
        normal: Vec3,
    },
    Stepping {
        from: FootPose,
        from_normal: Vec3,
        to: FootPose,
        to_normal: Vec3,
        /// The progress through the step, `0..1`.
        t: f32,
        /// How long the whole step takes, seconds: fixed when the foot lifts, so the foot keeps
        /// its own pace through the swing whatever the speed does meanwhile.
        duration: f32,
        height: f32,
        kind: StepKind,
    },
}

/// What started a step: the standing steps, or the walk's phase.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum StepKind {
    Standing,
    Walking,
}

impl Legs {
    pub const fn new() -> Self {
        Self {
            feet: [None; 2],
            phase: None,
            lifts: [0.0; 2],
            regime: Regime::new(),
            mean_velocity: Vec3::ZERO,
            touchdown: None,
            steps: 0,
        }
    }

    /// Lift both feet out of the world: the next tick plants them where they rest.
    pub fn reset(&mut self) {
        let steps = self.steps;
        *self = Self::new();
        self.steps = steps;
    }

    /// Whether either foot is mid-step.
    pub fn stepping(&self) -> bool {
        self.feet
            .iter()
            .any(|f| matches!(f, Some(Foot::Stepping { .. })))
    }

    /// The walk's phase, `0..1`, while walking.
    pub fn phase(&self) -> Option<f32> {
        self.phase
    }

    /// Whether the feet walk.
    pub fn moving(&self) -> bool {
        self.regime.moving()
    }

    /// The last walking touchdown: the foot, and how far ahead of its hip's ground point, along
    /// the velocity, it landed, metres.
    pub fn touchdown(&self) -> Option<(usize, f32)> {
        self.touchdown
    }

    /// Steps begun since the legs were made.
    #[cfg(test)]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// How far each heel is lifted about its toes, radians, left then right: the push-off at the end
    /// of a walking stance and the toe-off into the swing. Zero standing.
    pub fn heel_lifts(&self) -> [f32; 2] {
        self.lifts
    }

    /// Which feet are planted on the ground, left then right.
    pub fn planted(&self) -> [bool; 2] {
        self.feet.map(|f| matches!(f, Some(Foot::Planted { .. })))
    }

    /// Whether the feet are in the world, rather than lifted out by [`reset`](Self::reset).
    pub fn placed(&self) -> bool {
        self.feet.iter().all(Option::is_some)
    }

    /// Plant both feet at `poses` (world space): the feet carry on from where something else, the
    /// animation, left them, and step to their rest places from there.
    pub fn plant(&mut self, poses: [FootPose; 2]) {
        self.phase = None;
        self.feet = poses.map(|pose| {
            Some(Foot::Planted {
                pose,
                normal: Vec3::Y,
            })
        });
    }

    /// Advance by one tick and return where the feet are. `ground` gives the ground under a point,
    /// or `None` where there is none to find.
    pub fn step(
        &mut self,
        input: &LegsInput,
        tuning: &LegsTuning,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) -> [FootPose; 2] {
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let velocity = flat(input.velocity);
        let velocity = if velocity.is_finite() {
            velocity
        } else {
            Vec3::ZERO
        };
        let alpha = if dt > 0.0 {
            1.0 - (-dt / MEAN_VELOCITY_S).exp()
        } else {
            0.0
        };
        self.mean_velocity = self.mean_velocity.lerp(velocity, alpha);
        for (i, foot) in self.feet.iter_mut().enumerate() {
            // A foot left far behind (a teleport, a respawn) is planted afresh rather than stepped.
            let lost = foot.is_none_or(|f| {
                !f.is_finite() || f.pose().position.distance(input.rest[i].position) > LOST_M
            });
            if lost {
                let (pose, normal) = grounded(&input.rest[i], input.root_height, ground);
                *foot = Some(Foot::Planted { pose, normal });
            }
        }
        let speed = velocity.length();
        let moving = self
            .regime
            .update(speed, tuning.gait, tuning.moving_mps, tuning.standing_mps);
        let gait = gait::tuning(speed, input.crouch, &tuning.crouch);
        let input = LegsInput { velocity, ..*input };
        if moving {
            self.walk(&input, tuning, &gait, dt, ground);
        } else {
            self.stand(&input, tuning, &gait, dt, ground);
        }
        let limit = tuning.step.tilt_limit_rad;
        [0, 1].map(|i| match self.feet[i] {
            Some(Foot::Planted { pose, normal }) => tilt(&pose, normal, limit),
            Some(Foot::Stepping {
                from,
                from_normal,
                to,
                to_normal,
                t,
                height,
                ..
            }) => {
                let pose = arc(&from, &to, t, height);
                let normal = from_normal.lerp(to_normal, t).normalize_or(Vec3::Y);
                tilt(&pose, normal, limit)
            }
            None => input.rest[i],
        })
    }

    /// Standing: finish the steps in the air on their own clock toward the standing place, and step
    /// the foot the support or its rest calls for, one at a time.
    fn stand(
        &mut self,
        input: &LegsInput,
        tuning: &LegsTuning,
        gait: &GaitTuning,
        dt: f32,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        self.phase = None;
        self.lifts = heels_down(self.lifts, dt);
        let capture = input.velocity * capture_time(input.hip_height_m);
        let reach = (input.leg_length_m * tuning.reach_share).max(0.0);
        let capture = capture.clamp_length_max(reach);
        let landing = |i: usize| with_offset(&input.rest[i], capture);
        let mut stepping = false;
        for (i, foot) in self.feet.iter_mut().enumerate() {
            if let Some(Foot::Stepping {
                to,
                to_normal,
                t,
                duration,
                kind,
                ..
            }) = foot
            {
                // A walking step the stop leaves in the air finishes as a standing one, toward the
                // standing place, which moves on with the body while the foot is in the air.
                *kind = StepKind::Standing;
                (*to, *to_normal) = grounded(&landing(i), input.root_height, ground);
                *t += advance(dt, *duration);
                if *t >= 1.0 {
                    *foot = Some(Foot::Planted {
                        pose: *to,
                        normal: *to_normal,
                    });
                } else {
                    stepping = true;
                }
            }
        }
        if stepping {
            return;
        }
        let (Some(Foot::Planted { pose: left, .. }), Some(Foot::Planted { pose: right, .. })) =
            (self.feet[0], self.feet[1])
        else {
            return;
        };
        let pelvis = (input.rest[0].position + input.rest[1].position) * 0.5;
        let Some(choice) = choose(
            &[left, right],
            &input.rest,
            pelvis,
            pelvis + capture,
            &tuning.step,
        ) else {
            return;
        };
        // A slow shuffle's steps are between a standing step's and the walk's.
        let weight = self.regime.weight();
        let walk_swing = (1.0 - gait.duty) * gait.period(input.velocity.length());
        let duration = if walk_swing.is_finite() {
            blend(tuning.step.duration_s, walk_swing, weight)
        } else {
            tuning.step.duration_s
        };
        let height = blend(
            tuning.step.height_m * tuning.crouch.step_lift(input.crouch),
            gait.height_m,
            weight,
        );
        self.lift(
            choice.foot,
            landing(choice.foot),
            input.root_height,
            Lift {
                height,
                duration,
                kind: StepKind::Standing,
            },
            ground,
        );
    }

    /// Walking: advance the phase by the distance travelled, lift each foot as its share of the
    /// phase comes round, and carry the feet in the air toward the footholds they land on.
    fn walk(
        &mut self,
        input: &LegsInput,
        tuning: &LegsTuning,
        gait: &GaitTuning,
        dt: f32,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        let speed = input.velocity.length();
        let period = gait.period(speed);
        let swing = 1.0 - gait.duty;
        let previous = match self.phase {
            Some(phase) => phase,
            None => {
                // The walk starts with the foot furthest from its rest place, which lifts straight
                // away: its phase starts just short of its lift.
                let leader = self.leader(input);
                (-PHASE_OFFSETS[leader] - LIFT_EPSILON).rem_euclid(1.0)
            }
        };
        let travelled = speed * dt;
        let phase = (previous + travelled / gait.stride_m.max(1e-3)).rem_euclid(1.0);
        self.phase = Some(phase);
        let landing = LandingInput {
            velocity: input.velocity,
            mean_velocity: self.mean_velocity,
            hip_height_m: input.hip_height_m,
            leg_length_m: input.leg_length_m,
            capture_gain: tuning.capture_gain,
            reach_share: tuning.reach_share,
        };
        let lead = landing_lead(gait, &landing);
        for (i, offset) in PHASE_OFFSETS.into_iter().enumerate() {
            let before = (previous + offset).rem_euclid(1.0);
            let now = (phase + offset).rem_euclid(1.0);
            let lifted = now < before;
            // The foot lands at the end of its swing, `remaining` seconds away, where its hip will
            // be then, plus the lead.
            let ahead = |t: f32, duration: f32| {
                let remaining = (1.0 - t).max(0.0) * duration;
                input.velocity * remaining + lead
            };
            let other = self.feet[1 - i].map(|f| f.foothold().position);
            let place = |target: FootPose| match other {
                Some(other) => beside(target, i, other, &input.rest),
                None => target,
            };

            match &mut self.feet[i] {
                Some(Foot::Planted { .. }) if lifted => {
                    let duration = swing * period;
                    let target = place(with_offset(&input.rest[i], ahead(0.0, duration)));
                    self.lift(
                        i,
                        target,
                        input.root_height,
                        Lift {
                            height: gait.height_m,
                            duration,
                            kind: StepKind::Walking,
                        },
                        ground,
                    );
                    if let Some(Foot::Stepping {
                        t,
                        duration,
                        to,
                        to_normal,
                        ..
                    }) = &mut self.feet[i]
                    {
                        // The tick that lifts it is already part of the way into the swing.
                        *t = (now / swing).clamp(0.0, 1.0);
                        let target = place(with_offset(&input.rest[i], ahead(*t, *duration)));
                        (*to, *to_normal) = grounded(&target, input.root_height, ground);
                    }
                }
                Some(Foot::Stepping {
                    to,
                    to_normal,
                    t,
                    duration,
                    kind,
                    height,
                    ..
                }) => {
                    if *kind == StepKind::Standing {
                        // A standing step under way when the walk starts finishes at the walk's
                        // pace, if that is quicker.
                        *duration = duration.min((swing * period).max(MIN_SWING_S));
                        *height = height.max(gait.height_m);
                    }
                    *t += advance(dt, *duration);
                    // A walking foot whose turn to lift comes round again before it lands (the
                    // walk sped up a lot) lands at once.
                    let overtaken = *kind == StepKind::Walking && lifted;
                    if *t >= 1.0 || overtaken {
                        let walked = *kind == StepKind::Walking;
                        let landed = Foot::Planted {
                            pose: *to,
                            normal: *to_normal,
                        };
                        if walked && speed > 1e-3 {
                            let along = input.velocity / speed;
                            let lead = flat(to.position - input.rest[i].position).dot(along);
                            self.touchdown = Some((i, lead));
                        }
                        self.feet[i] = Some(landed);
                        continue;
                    }
                    let target = place(with_offset(&input.rest[i], ahead(*t, *duration)));
                    (*to, *to_normal) = grounded(&target, input.root_height, ground);
                }
                _ => {}
            }
        }
        // Each foot's roll, from where it is in its cycle after this tick's lifts and landings.
        for (i, offset) in PHASE_OFFSETS.into_iter().enumerate() {
            let now = (phase + offset).rem_euclid(1.0);
            self.lifts[i] = match self.feet[i] {
                Some(Foot::Planted { .. }) => push_off(now, swing),
                Some(Foot::Stepping {
                    t,
                    kind: StepKind::Walking,
                    ..
                }) => toe_off(t),
                _ => 0.0,
            };
        }
    }

    /// The foot to lead a walk with: the one furthest from its rest place, the left on a tie.
    fn leader(&self, input: &LegsInput) -> usize {
        let distance = |i: usize| match self.feet[i] {
            Some(foot) => flat(input.rest[i].position - foot.pose().position).length(),
            None => 0.0,
        };
        if distance(1) > distance(0) { 1 } else { 0 }
    }

    /// Lift a planted foot toward `target`.
    fn lift(
        &mut self,
        i: usize,
        target: FootPose,
        root_height: f32,
        lift: Lift,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        if let Some(Foot::Planted { pose, normal }) = self.feet[i] {
            let (to, to_normal) = grounded(&target, root_height, ground);
            self.feet[i] = Some(Foot::Stepping {
                from: pose,
                from_normal: normal,
                to,
                to_normal,
                t: 0.0,
                duration: lift.duration,
                height: lift.height,
                kind: lift.kind,
            });
            self.steps += 1;
        }
    }
}

impl Foot {
    fn pose(&self) -> FootPose {
        match *self {
            Foot::Planted { pose, .. } => pose,
            Foot::Stepping { from, .. } => from,
        }
    }

    /// Where the foot stands or is about to: its plant, or the end of its step.
    fn foothold(&self) -> FootPose {
        match *self {
            Foot::Planted { pose, .. } => pose,
            Foot::Stepping { to, .. } => to,
        }
    }

    fn is_finite(&self) -> bool {
        let pose = self.pose();
        pose.position.is_finite() && pose.rotation.is_finite()
    }
}

/// How a foot lifts: how high, over how long, and for what.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Lift {
    height: f32,
    duration: f32,
    kind: StepKind,
}

/// The share of a step of `duration` seconds that `dt` seconds cover: all of it for a step with no
/// length.
fn advance(dt: f32, duration: f32) -> f32 {
    if duration > 0.0 && duration.is_finite() {
        dt / duration
    } else {
        1.0
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// How far a foot may be from its rest place before it counts as lost, metres.
const LOST_M: f32 = 4.0;

/// Each foot's offset into the walk's phase: the feet alternate.
const PHASE_OFFSETS: [f32; 2] = [0.0, 0.5];

/// How far short of its lift a leading foot's phase starts, so that the first tick lifts it.
const LIFT_EPSILON: f32 = 1e-4;

/// The shortest swing a standing step finishing at the walk's pace takes, seconds.
const MIN_SWING_S: f32 = 0.05;

/// How long the measured velocity is smoothed over for the capture term, seconds.
const MEAN_VELOCITY_S: f32 = 0.3;
