//! Planted feet that step and walk: each foot holds its place in the world while the body moves and
//! turns above it. Pure, in world space.
//!
//! Standing, a foot steps to its wanted place once it has fallen too far behind: one foot at a
//! time, the worse one first, over a fixed duration on an eased arc. Moving, the feet walk
//! ([`GaitTuning`]): a phase advances by the distance travelled over the stride, each foot lifts
//! when its share of the phase comes round, and it lands where its wanted place will be half a
//! stance after it lands, so that it passes under the hip at mid-stance. Changing speed changes the
//! cadence and the stride, never where the body is.
//!
//! The ground under a foothold comes from a query the caller supplies (a scene ray in the game, a
//! function in the tests). Each step asks it for the foothold the step is heading to, every tick
//! the step is in the air, since the foothold moves on with the body; a planted foot keeps the
//! height and the slope it landed on.

use glam::{Quat, Vec3};

/// A foot's place and orientation in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FootPose {
    pub position: Vec3,
    pub rotation: Quat,
}

/// The ground under a foothold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ground {
    /// The ground's height, metres.
    pub height: f32,
    /// The ground's upward normal.
    pub normal: Vec3,
}

/// The standing steps' tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct StepTuning {
    /// How far, horizontally, a foot may be from its wanted place before it steps, metres.
    pub threshold_m: f32,
    /// How far a foot may be turned from its wanted orientation before it steps, radians.
    pub threshold_rad: f32,
    /// How long a step takes, seconds.
    pub duration_s: f32,
    /// How high a step lifts the foot at its middle, metres.
    pub height_m: f32,
    /// How far a foot tilts to the ground's slope, at most, radians.
    pub tilt_limit_rad: f32,
}

/// The walk's tuning at the current speed.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GaitTuning {
    /// The distance the body travels in one full cycle (a step of each foot), metres.
    pub stride_m: f32,
    /// The share of a cycle each foot spends on the ground: above a half both feet are down
    /// together for a moment, below it both are in the air.
    pub duty: f32,
    /// How high a step lifts the foot at its middle, metres.
    pub height_m: f32,
}

/// One tick's input to the feet.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FeetInput {
    /// Where each foot would stand under the body now, left then right: the neutral stance carried
    /// by the root, at the root's height.
    pub wanted: [FootPose; 2],
    /// The root's height: the wanted places' ground.
    pub root_height: f32,
    /// The body's horizontal velocity, metres per second: the root's own, which paces the walk.
    pub velocity: Vec3,
    /// The velocity the body is told to move at, metres per second: where a foot in the air is
    /// carried ahead to. It leads the root's own while the body speeds up or turns.
    pub commanded: Vec3,
    /// The walk's tuning while the body moves fast enough to walk, `None` standing.
    pub gait: Option<GaitTuning>,
    /// The tick's step, seconds.
    pub dt: f32,
}

/// The two feet, left then right.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Feet {
    feet: [Option<Foot>; 2],
    /// The walk's phase, `0..1`, while walking.
    phase: Option<f32>,
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
        height: f32,
        kind: StepKind,
    },
}

/// What paces a step: its own clock standing, the walk's phase walking.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum StepKind {
    Standing,
    Walking,
}

impl Feet {
    pub const fn new() -> Self {
        Self {
            feet: [None; 2],
            phase: None,
        }
    }

    /// Lift both feet out of the world: the next tick plants them where they are wanted.
    pub fn reset(&mut self) {
        *self = Self::new();
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

    /// Whether the feet are in the world, rather than lifted out by [`reset`](Self::reset).
    pub fn placed(&self) -> bool {
        self.feet.iter().all(Option::is_some)
    }

    /// Plant both feet at `poses` (world space): the feet carry on from where something else, the
    /// animation, left them, and step to their wanted places from there.
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
        input: &FeetInput,
        tuning: &StepTuning,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) -> [FootPose; 2] {
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        for (i, foot) in self.feet.iter_mut().enumerate() {
            // A foot left far behind (a teleport, a respawn) is planted afresh rather than stepped.
            let lost = foot.is_none_or(|f| {
                !f.is_finite() || f.pose().position.distance(input.wanted[i].position) > LOST_M
            });
            if lost {
                let (pose, normal) = grounded(&input.wanted[i], input.root_height, ground);
                *foot = Some(Foot::Planted { pose, normal });
            }
        }
        let gait = input
            .gait
            .filter(|g| g.stride_m > 0.0 && g.duty > 0.0 && g.duty < 1.0)
            .filter(|_| input.velocity.length() > 1e-3 && input.velocity.is_finite());
        match gait {
            Some(gait) => self.walk(input, &gait, dt, ground),
            None => self.stand(input, tuning, dt, ground),
        }
        [0, 1].map(|i| match self.feet[i] {
            Some(Foot::Planted { pose, normal }) => tilt(&pose, normal, tuning.tilt_limit_rad),
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
                tilt(&pose, normal, tuning.tilt_limit_rad)
            }
            None => input.wanted[i],
        })
    }

    /// Standing: finish the steps in the air on their own clock, and step the worse foot once it
    /// is too far from its wanted place.
    fn stand(
        &mut self,
        input: &FeetInput,
        tuning: &StepTuning,
        dt: f32,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        self.phase = None;
        let advance = if tuning.duration_s > 0.0 {
            dt / tuning.duration_s
        } else {
            1.0
        };
        let mut stepping = false;
        for (i, foot) in self.feet.iter_mut().enumerate() {
            if let Some(Foot::Stepping {
                to,
                to_normal,
                t,
                kind,
                ..
            }) = foot
            {
                // A walking step that the stop leaves in the air finishes as a standing one.
                *kind = StepKind::Standing;
                // The wanted place moves on while the foot is in the air; the step lands there.
                (*to, *to_normal) = grounded(&input.wanted[i], input.root_height, ground);
                *t += advance;
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
        let errors = [0, 1].map(|i| match self.feet[i] {
            Some(Foot::Planted { pose, .. }) => error(&pose, &input.wanted[i], tuning),
            _ => 0.0,
        });
        let worse = if errors[0] >= errors[1] { 0 } else { 1 };
        if errors[worse] > 1.0 {
            self.lift(worse, input, tuning.height_m, StepKind::Standing, ground);
        }
    }

    /// Walking: advance the phase, lift each foot as its share of the phase comes round, and carry
    /// the feet in the air toward the footholds they land on.
    fn walk(
        &mut self,
        input: &FeetInput,
        gait: &GaitTuning,
        dt: f32,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        let speed = input.velocity.length();
        let period = gait.stride_m / speed;
        let swing = 1.0 - gait.duty;
        let previous = match self.phase {
            Some(phase) => phase,
            None => {
                // The walk starts with the foot furthest from where it is wanted, which lifts
                // straight away: its phase starts just short of its lift.
                let leader = self.leader(input);
                (-PHASE_OFFSETS[leader] - LIFT_EPSILON).rem_euclid(1.0)
            }
        };
        let phase = (previous + dt / period).rem_euclid(1.0);
        self.phase = Some(phase);
        for (i, offset) in PHASE_OFFSETS.into_iter().enumerate() {
            let before = (previous + offset).rem_euclid(1.0);
            let now = (phase + offset).rem_euclid(1.0);
            let lifted = now < before;
            let lead = |t: f32| {
                // The foot lands at the end of its swing and should pass under the hip half a
                // stance later.
                let remaining = (1.0 - t).max(0.0) * swing * period;
                input.commanded * (remaining + gait.duty * period * 0.5)
            };
            match &mut self.feet[i] {
                Some(Foot::Planted { .. }) if lifted => {
                    self.lift(i, input, gait.height_m, StepKind::Walking, ground);
                    if let Some(Foot::Stepping {
                        t, to, to_normal, ..
                    }) = &mut self.feet[i]
                    {
                        *t = (now / swing).clamp(0.0, 1.0);
                        let ahead = with_offset(&input.wanted[i], lead(*t));
                        (*to, *to_normal) = grounded(&ahead, input.root_height, ground);
                    }
                }
                Some(Foot::Stepping {
                    to,
                    to_normal,
                    t,
                    kind,
                    height,
                    ..
                }) => {
                    match kind {
                        StepKind::Walking => {
                            if lifted || now >= swing {
                                let landed = Foot::Planted {
                                    pose: *to,
                                    normal: *to_normal,
                                };
                                self.feet[i] = Some(landed);
                                continue;
                            }
                            *t = (now / swing).clamp(0.0, 1.0);
                        }
                        // A standing step under way when the walk starts finishes on its own
                        // clock, at the walk's pace.
                        StepKind::Standing => {
                            *t += dt / (swing * period).max(MIN_SWING_S);
                            *height = height.max(gait.height_m);
                            if *t >= 1.0 {
                                let landed = Foot::Planted {
                                    pose: *to,
                                    normal: *to_normal,
                                };
                                self.feet[i] = Some(landed);
                                continue;
                            }
                        }
                    }
                    let ahead = with_offset(&input.wanted[i], lead(*t));
                    (*to, *to_normal) = grounded(&ahead, input.root_height, ground);
                }
                _ => {}
            }
        }
    }

    /// The foot to lead a walk with: the one furthest from its wanted place, the left on a tie.
    fn leader(&self, input: &FeetInput) -> usize {
        let distance = |i: usize| match self.feet[i] {
            Some(foot) => {
                let offset = input.wanted[i].position - foot.pose().position;
                Vec3::new(offset.x, 0.0, offset.z).length()
            }
            None => 0.0,
        };
        if distance(1) > distance(0) { 1 } else { 0 }
    }

    /// Lift a planted foot toward its wanted place.
    fn lift(
        &mut self,
        i: usize,
        input: &FeetInput,
        height: f32,
        kind: StepKind,
        ground: &mut impl FnMut(Vec3) -> Option<Ground>,
    ) {
        if let Some(Foot::Planted { pose, normal }) = self.feet[i] {
            let (to, to_normal) = grounded(&input.wanted[i], input.root_height, ground);
            self.feet[i] = Some(Foot::Stepping {
                from: pose,
                from_normal: normal,
                to,
                to_normal,
                t: 0.0,
                height,
                kind,
            });
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

    fn is_finite(&self) -> bool {
        let pose = self.pose();
        pose.position.is_finite() && pose.rotation.is_finite()
    }
}

/// How far a foot may be from its wanted place before it counts as lost, metres.
const LOST_M: f32 = 4.0;

/// Each foot's offset into the walk's phase: the feet alternate.
const PHASE_OFFSETS: [f32; 2] = [0.0, 0.5];

/// How far short of its lift a leading foot's phase starts, so that the first tick lifts it.
const LIFT_EPSILON: f32 = 1e-4;

/// The shortest swing a standing step finishing at the walk's pace takes, seconds.
const MIN_SWING_S: f32 = 0.05;

/// `pose` moved horizontally by `offset`.
fn with_offset(pose: &FootPose, offset: Vec3) -> FootPose {
    FootPose {
        position: pose.position + Vec3::new(offset.x, 0.0, offset.z),
        rotation: pose.rotation,
    }
}

/// `wanted` stood on the ground under it: raised or lowered by the ground's height against the
/// root's, so the ankle keeps its height above the ground, with the ground's normal. Without
/// ground, the wanted place on a level floor.
fn grounded(
    wanted: &FootPose,
    root_height: f32,
    ground: &mut impl FnMut(Vec3) -> Option<Ground>,
) -> (FootPose, Vec3) {
    let found = ground(Vec3::new(wanted.position.x, root_height, wanted.position.z))
        .filter(|g| g.height.is_finite() && g.normal.is_finite() && g.normal.y > 0.0);
    match found {
        Some(g) => (
            FootPose {
                position: wanted.position + Vec3::Y * (g.height - root_height),
                rotation: wanted.rotation,
            },
            g.normal.normalize(),
        ),
        None => (*wanted, Vec3::Y),
    }
}

/// How far a planted foot is from its wanted pose, as a share of the step thresholds: above one,
/// it steps.
fn error(planted: &FootPose, wanted: &FootPose, tuning: &StepTuning) -> f32 {
    let offset = wanted.position - planted.position;
    let distance = Vec3::new(offset.x, 0.0, offset.z).length();
    let turn = planted.rotation.angle_between(wanted.rotation);
    let by_distance = distance / tuning.threshold_m.max(1e-3);
    let by_turn = turn / tuning.threshold_rad.max(1e-3);
    by_distance.max(by_turn)
}

/// The foot's pose `t` of the way through a step: eased along the line, lifted on a sine arch that
/// also clears the rise of a step up.
fn arc(from: &FootPose, to: &FootPose, t: f32, height: f32) -> FootPose {
    let t = t.clamp(0.0, 1.0);
    let eased = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let rise = (to.position.y - from.position.y).abs() * 0.5;
    FootPose {
        position: from.position.lerp(to.position, eased)
            + Vec3::Y * ((height + rise) * (std::f32::consts::PI * t).sin()),
        rotation: from.rotation.slerp(to.rotation, eased),
    }
}

/// `pose` tilted so the foot lies on ground with `normal`, at most `limit` radians.
fn tilt(pose: &FootPose, normal: Vec3, limit: f32) -> FootPose {
    let normal = normal.normalize_or(Vec3::Y);
    let angle = normal.angle_between(Vec3::Y);
    if angle.is_nan() || angle <= 1e-4 {
        return *pose;
    }
    let axis = Vec3::Y.cross(normal).normalize_or_zero();
    if axis == Vec3::ZERO {
        return *pose;
    }
    let turn = Quat::from_axis_angle(axis, angle.min(limit.max(0.0)));
    FootPose {
        position: pose.position,
        rotation: (turn * pose.rotation).normalize(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn tuning() -> StepTuning {
        StepTuning {
            threshold_m: 0.12,
            threshold_rad: 35f32.to_radians(),
            duration_s: 0.3,
            height_m: 0.06,
            tilt_limit_rad: 25f32.to_radians(),
        }
    }

    fn walk() -> GaitTuning {
        GaitTuning {
            stride_m: 1.4,
            duty: 0.6,
            height_m: 0.08,
        }
    }

    fn at(x: f32, z: f32) -> FootPose {
        FootPose {
            position: Vec3::new(x, 0.0, z),
            rotation: Quat::IDENTITY,
        }
    }

    fn standing(wanted: [FootPose; 2]) -> FeetInput {
        FeetInput {
            wanted,
            root_height: 0.0,
            velocity: Vec3::ZERO,
            commanded: Vec3::ZERO,
            gait: None,
            dt: DT,
        }
    }

    fn flat(_: Vec3) -> Option<Ground> {
        None
    }

    #[test]
    fn small_motion_leaves_the_feet_planted() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let placed = feet.step(
            &standing([at(-0.05, 0.05), at(0.15, 0.05)]),
            &tuning(),
            &mut flat,
        );
        assert_eq!(placed, [at(-0.1, 0.0), at(0.1, 0.0)]);
    }

    #[test]
    fn a_far_body_steps_one_foot_then_the_other() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let wanted = [at(-0.1, -0.3), at(0.1, -0.3)];
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            let placed = feet.step(&standing(wanted), &tuning(), &mut flat);
            peak = peak.max(placed[0].position.y.max(placed[1].position.y));
            // Never both in the air.
            assert!(placed[0].position.y < 1e-6 || placed[1].position.y < 1e-6);
            let landed = [0, 1].map(|i| placed[i].position.abs_diff_eq(wanted[i].position, 1e-5));
            if landed[0] && landed[1] {
                assert!(peak > 0.05);
                assert!(!feet.stepping());
                return;
            }
        }
        panic!("the feet never both arrived");
    }

    #[test]
    fn a_turn_in_place_steps_the_feet() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let turned = Quat::from_rotation_y(1.0);
        let wanted = [at(-0.1, 0.0), at(0.1, 0.0)].map(|p| FootPose {
            position: turned * p.position,
            rotation: turned,
        });
        feet.step(&standing(wanted), &tuning(), &mut flat);
        assert!(feet.stepping());
    }

    #[test]
    fn a_reset_plants_where_wanted() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(0.0, 0.0), at(0.0, 0.0)]),
            &tuning(),
            &mut flat,
        );
        feet.reset();
        assert_eq!(
            feet.step(
                &standing([at(1.0, 1.0), at(2.0, 2.0)]),
                &tuning(),
                &mut flat
            ),
            [at(1.0, 1.0), at(2.0, 2.0)]
        );
    }

    /// Walk the body forward (-Z) at `speed` for `seconds`, returning each tick's feet and the
    /// body's position.
    fn walk_for(feet: &mut Feet, speed: f32, seconds: f32) -> Vec<([FootPose; 2], Vec3)> {
        let velocity = Vec3::new(0.0, 0.0, -speed);
        let mut body = Vec3::ZERO;
        let mut out = vec![];
        let ticks = (seconds / DT) as usize;
        for _ in 0..ticks {
            body += velocity * DT;
            let input = FeetInput {
                wanted: [at(-0.1, 0.0), at(0.1, 0.0)].map(|p| with_offset(&p, body)),
                root_height: 0.0,
                velocity,
                commanded: velocity,
                gait: Some(walk()),
                dt: DT,
            };
            out.push((feet.step(&input, &tuning(), &mut flat), body));
        }
        out
    }

    #[test]
    fn walking_alternates_the_feet_and_keeps_up_with_the_body() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let ticks = walk_for(&mut feet, 1.5, 4.0);
        let mut lifts = [0usize; 2];
        let mut was_up = [false; 2];
        for (placed, body) in &ticks {
            for i in 0..2 {
                let up = placed[i].position.y > 1e-4;
                if up && !was_up[i] {
                    lifts[i] += 1;
                }
                was_up[i] = up;
            }
            // With a duty above a half, one foot is always down.
            assert!(placed[0].position.y < 1e-4 || placed[1].position.y < 1e-4);
            // The feet stay around the body: never more than a stride from it.
            for foot in placed {
                assert!((foot.position.z - body.z).abs() < walk().stride_m);
            }
        }
        // Four seconds at 1.5 m/s over a 1.4 m stride is a little over four cycles.
        for count in lifts {
            assert!((4..=5).contains(&count), "{lifts:?}");
        }
    }

    #[test]
    fn a_body_held_still_does_not_walk_whatever_it_is_told() {
        let mut feet = Feet::new();
        let wanted = [at(-0.1, 0.0), at(0.1, 0.0)];
        feet.step(&standing(wanted), &tuning(), &mut flat);
        let pushing_a_wall = FeetInput {
            commanded: Vec3::new(0.0, 0.0, -3.0),
            gait: Some(walk()),
            ..standing(wanted)
        };
        for _ in 0..30 {
            let placed = feet.step(&pushing_a_wall, &tuning(), &mut flat);
            assert_eq!(placed, wanted);
        }
        assert!(feet.phase().is_none());
    }

    #[test]
    fn a_planted_foot_does_not_slide_while_walking() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let ticks = walk_for(&mut feet, 3.0, 3.0);
        for pair in ticks.windows(2) {
            for i in 0..2 {
                let (a, b) = (pair[0].0[i].position, pair[1].0[i].position);
                if a.y < 1e-6 && b.y < 1e-6 {
                    assert!(a.abs_diff_eq(b, 1e-5), "foot {i} slid from {a:?} to {b:?}");
                }
            }
        }
    }

    #[test]
    fn stopping_settles_the_feet_under_the_body() {
        let mut feet = Feet::new();
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut flat,
        );
        let ticks = walk_for(&mut feet, 1.5, 2.0);
        let body = ticks.last().unwrap().1;
        let wanted = [at(-0.1, 0.0), at(0.1, 0.0)].map(|p| with_offset(&p, body));
        let mut placed = [at(0.0, 0.0); 2];
        for _ in 0..90 {
            placed = feet.step(&standing(wanted), &tuning(), &mut flat);
        }
        assert!(!feet.stepping());
        assert!(feet.phase().is_none());
        for i in 0..2 {
            let offset = placed[i].position - wanted[i].position;
            assert!(
                Vec3::new(offset.x, 0.0, offset.z).length() <= tuning().threshold_m,
                "foot {i} at {:?}",
                placed[i].position
            );
        }
    }

    #[test]
    fn a_step_lands_on_the_ground_it_finds_and_tilts_to_it() {
        let mut feet = Feet::new();
        let slope = Vec3::new(0.0, 1.0, 0.3).normalize();
        let mut ground = |p: Vec3| {
            (p.z < -0.2).then_some(Ground {
                height: 0.15,
                normal: slope,
            })
        };
        feet.step(
            &standing([at(-0.1, 0.0), at(0.1, 0.0)]),
            &tuning(),
            &mut ground,
        );
        let wanted = [at(-0.1, -0.4), at(0.1, -0.4)];
        let mut placed = [at(0.0, 0.0); 2];
        for _ in 0..60 {
            placed = feet.step(&standing(wanted), &tuning(), &mut ground);
        }
        for foot in placed {
            assert!((foot.position.y - 0.15).abs() < 1e-4, "{foot:?}");
            let up = foot.rotation * Vec3::Y;
            assert!(up.angle_between(slope) < 1e-3);
        }
    }

    #[test]
    fn a_steep_slope_tilts_the_foot_only_to_the_limit() {
        let steep = Vec3::new(0.0, 1.0, 1.5).normalize();
        let tilted = tilt(&at(0.0, 0.0), steep, tuning().tilt_limit_rad);
        let up = tilted.rotation * Vec3::Y;
        assert!((up.angle_between(Vec3::Y) - tuning().tilt_limit_rad).abs() < 1e-4);
    }
}
