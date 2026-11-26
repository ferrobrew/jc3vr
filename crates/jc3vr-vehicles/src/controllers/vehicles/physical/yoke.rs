//! The yoke's motion: a control wheel on a column, turned for the roll and pushed or pulled along
//! its column for the pitch, as the planes whose cockpits model one fly. Pure.
//!
//! Either hand or both take the yoke by its horns. The grab is relative: each hand's angle about
//! the column and its place along it are taken when it grabs, so a hand that takes the yoke while
//! it is turned or springing back moves it only by moving. With both hands on it, the yoke takes
//! the mean of what each hand would make it. Let go, a centring spring returns it, still flying the
//! plane on its way, as the stick's does. The held yoke cues its hands at the centre and at the
//! ends of each axis, so the hands find centre and the limits without looking.

use glam::{Quat, Vec2, Vec3};
use jc3vr_input::frame::Hand;

use crate::controllers::vehicles::{
    physical::{
        ControlState, Holding,
        cues::{self, ControlCue, TravelCues},
    },
    tuning::YokeSpec,
};

/// The yoke at rest in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YokeFrame {
    /// Where the column meets the wheel.
    pub hub: Vec3,
    /// Along the column, toward the pilot: the way a pull moves it. Unit.
    pub axis: Vec3,
    /// Across the wheel, to the pilot's right, square to the axis. Unit.
    pub right: Vec3,
    /// From the hub to each horn's grip, across the wheel, metres.
    pub half_span: f32,
    /// From the hub up to the horns' grips, metres.
    pub grip_height: f32,
}

impl YokeFrame {
    /// The frame for a hub, its column toward the pilot, and the wheel's right, or `None` when they
    /// are degenerate.
    pub fn new(
        hub: Vec3,
        axis: Vec3,
        right: Vec3,
        half_span: f32,
        grip_height: f32,
    ) -> Option<Self> {
        let axis = axis.normalize_or_zero();
        let right = (right - axis * right.dot(axis)).normalize_or_zero();
        let usable = hub.is_finite()
            && axis != Vec3::ZERO
            && right != Vec3::ZERO
            && half_span.is_finite()
            && half_span > 0.0
            && grip_height.is_finite();
        usable.then_some(Self {
            hub,
            axis,
            right,
            half_span,
            grip_height,
        })
    }

    /// The wheel's up, square to the axis and the right.
    pub fn up(&self) -> Vec3 {
        self.axis.cross(self.right)
    }

    /// The hub with the yoke pulled `pitch` metres toward the pilot.
    pub fn hub_at(&self, pitch: f32) -> Vec3 {
        self.hub + self.axis * pitch
    }

    /// The yoke's turn by `roll` radians, positive clockwise as the pilot sees it.
    pub fn turn(&self, roll: f32) -> Quat {
        // The axis points at the pilot, so a turn clockwise to them is a negative one about it.
        Quat::from_axis_angle(self.axis, -roll)
    }

    /// The grip of `hand`'s horn with the yoke turned by `roll` and pulled by `pitch`.
    pub fn horn(&self, hand: Hand, roll: f32, pitch: f32) -> Vec3 {
        let side = match hand {
            Hand::Left => -1.0,
            Hand::Right => 1.0,
        };
        let rest = self.right * (side * self.half_span) + self.up() * self.grip_height;
        self.hub_at(pitch) + self.turn(roll) * rest
    }

    /// The angle of `point` about the column, radians anticlockwise from the right as the pilot
    /// sees it, `None` for a point on the column.
    fn angle(&self, point: Vec3, pitch: f32) -> Option<f32> {
        let v = point - self.hub_at(pitch);
        let (x, y) = (v.dot(self.right), v.dot(self.up()));
        (x * x + y * y > MIN_RADIUS_M * MIN_RADIUS_M).then(|| y.atan2(x))
    }
}

/// A hand's hold on the yoke: where it was about and along the column when it grabbed, and the
/// yoke's turn and pull then.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YokeGrip {
    angle: Option<f32>,
    along: f32,
    roll: f32,
    pitch: f32,
    /// The hand's grip in the yoke's own frame, at rest, so it is drawn riding the yoke.
    local: Vec3,
}

impl YokeGrip {
    pub fn new(frame: &YokeFrame, point: Vec3, roll: f32, pitch: f32) -> Self {
        let local = frame.turn(roll).inverse() * (point - frame.hub_at(pitch));
        Self {
            angle: frame.angle(point, pitch),
            along: point.dot(frame.axis),
            roll,
            pitch,
            local,
        }
    }

    /// The turn and the pull this hold makes for the hand at `point`.
    fn wants(&self, frame: &YokeFrame, point: Vec3) -> (f32, f32) {
        let pitch = self.pitch + point.dot(frame.axis) - self.along;
        let roll = match (self.angle, frame.angle(point, pitch)) {
            (Some(from), Some(to)) => self.roll - wrap(to - from),
            _ => self.roll,
        };
        (roll, pitch)
    }

    /// Where the hand is drawn on the yoke turned by `roll` and pulled by `pitch`.
    pub fn anchor(&self, frame: &YokeFrame, roll: f32, pitch: f32) -> Vec3 {
        frame.hub_at(pitch) + frame.turn(roll) * self.local
    }
}

/// The yoke's moving parts.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct YokeState {
    /// The turn, radians, positive clockwise as the pilot sees it.
    pub roll: f32,
    /// The pull toward the pilot, metres.
    pub pitch: f32,
    pub(super) grips: [Option<YokeGrip>; 2],
    roll_cues: TravelCues,
    pitch_cues: TravelCues,
}

impl YokeState {
    pub const fn new() -> Self {
        Self {
            roll: 0.0,
            pitch: 0.0,
            grips: [None; 2],
            roll_cues: TravelCues::new(),
            pitch_cues: TravelCues::new(),
        }
    }
}

/// What the yoke turned by `roll` and pulled by `pitch` outputs, as the flight stick does (+X rolls
/// right, +Y pushes forward): each axis's share of its range past its deadzone, rescaled so that
/// it still reaches one.
pub fn yoke_value(spec: &YokeSpec, roll: f32, pitch: f32) -> Vec2 {
    let roll_share = roll / spec.max_roll_deg.to_radians().max(1e-3);
    let pitch_share = pitch / spec.travel_m.max(1e-3);
    Vec2::new(
        past_deadzone(roll_share, spec.deadzone),
        -past_deadzone(pitch_share, spec.deadzone),
    )
}

/// What the yoke produced this tick.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(super) struct YokeStep {
    pub(super) value: Option<Vec2>,
    pub(super) cue: Option<ControlCue>,
}

/// Move the yoke with the hands holding it, or let go, spring it back to centre, still writing its
/// value on the way. A tick on which the tables write the stick's axes (`tables_move`) snaps a
/// returning yoke to centre, so the thumbstick flies at once.
pub(super) fn step_yoke(
    state: &mut ControlState,
    frame: &YokeFrame,
    spec: &YokeSpec,
    holding: Holding,
    tables_move: bool,
    dt: f32,
) -> YokeStep {
    let yoke = &mut state.yoke;
    for hand in Hand::BOTH {
        if holding[hand.index()].is_none() {
            yoke.grips[hand.index()] = None;
        }
    }
    let wants: Vec<(f32, f32)> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| {
            let (_, point) = holding[hand.index()]?;
            let grip = yoke.grips[hand.index()]?;
            Some(grip.wants(frame, point))
        })
        .collect();
    let max_roll = spec.max_roll_deg.to_radians().max(1e-3);
    let travel = spec.travel_m.max(1e-3);
    if wants.is_empty() {
        if yoke.roll == 0.0 && yoke.pitch == 0.0 {
            return YokeStep::default();
        }
        if tables_move {
            *yoke = YokeState::new();
            return YokeStep::default();
        }
        let decay = if spec.return_s > 0.0 {
            (-dt.max(0.0) / spec.return_s).exp()
        } else {
            0.0
        };
        yoke.roll *= decay;
        yoke.pitch *= decay;
        if yoke.roll.abs() < SETTLED_RAD && yoke.pitch.abs() < SETTLED_M {
            yoke.roll = 0.0;
            yoke.pitch = 0.0;
        }
        yoke.roll_cues = TravelCues::new();
        yoke.pitch_cues = TravelCues::new();
        return YokeStep {
            value: Some(yoke_value(spec, yoke.roll, yoke.pitch)),
            cue: None,
        };
    }
    let count = wants.len() as f32;
    let (roll, pitch) = wants.iter().fold((0.0, 0.0), |(r, p), (wr, wp)| {
        (r + wr / count, p + wp / count)
    });
    if roll.is_finite() && pitch.is_finite() {
        yoke.roll = roll.clamp(-max_roll, max_roll);
        yoke.pitch = pitch.clamp(-travel, travel);
    }
    let roll_cue = cues::stick_cue(&mut yoke.roll_cues, yoke.roll.abs() / max_roll);
    let pitch_cue = cues::stick_cue(&mut yoke.pitch_cues, yoke.pitch.abs() / travel);
    YokeStep {
        value: Some(yoke_value(spec, yoke.roll, yoke.pitch)),
        cue: roll_cue.or(pitch_cue),
    }
}

/// Take hold of the yoke with `hand` at `point`.
pub(super) fn grab_yoke(state: &mut ControlState, frame: &YokeFrame, hand: Hand, point: Vec3) {
    state.yoke.grips[hand.index()] = Some(YokeGrip::new(
        frame,
        point,
        state.yoke.roll,
        state.yoke.pitch,
    ));
}

/// The horn nearest `point`: the grab point offered to a hand there.
pub(super) fn nearest_horn(frame: &YokeFrame, state: &YokeState, point: Vec3) -> Vec3 {
    let [left, right] = Hand::BOTH.map(|hand| frame.horn(hand, state.roll, state.pitch));
    if point.distance_squared(left) <= point.distance_squared(right) {
        left
    } else {
        right
    }
}

fn past_deadzone(share: f32, deadzone: f32) -> f32 {
    let deadzone = deadzone.clamp(0.0, 0.9);
    if !share.is_finite() || share.abs() <= deadzone {
        return 0.0;
    }
    share.signum() * ((share.abs() - deadzone) / (1.0 - deadzone)).min(1.0)
}

/// `angle` wrapped into `-π..=π`.
fn wrap(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    angle - tau * ((angle + std::f32::consts::PI) / tau).floor()
}

/// A hand nearer the column than this, metres, has no angle about it to turn the yoke by.
const MIN_RADIUS_M: f32 = 0.03;
/// A returning yoke within these of centre is at centre.
const SETTLED_RAD: f32 = 0.005;
const SETTLED_M: f32 = 0.001;

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::grab::cockpit::ControlKind;

    use super::*;
    use crate::controllers::vehicles::{physical::Hold, tuning::PHYSICAL};

    fn frame() -> YokeFrame {
        YokeFrame::new(
            Vec3::new(0.0, -0.3, -0.5),
            Vec3::new(0.0, 0.26, 0.97),
            Vec3::X,
            0.14,
            0.07,
        )
        .unwrap()
    }

    fn holding(points: [Option<Vec3>; 2]) -> Holding {
        points.map(|p| {
            p.map(|p| {
                (
                    Hold {
                        kind: ControlKind::Yoke,
                        offset: Vec3::ZERO,
                        orientation: None,
                    },
                    p,
                )
            })
        })
    }

    fn step(state: &mut ControlState, points: [Option<Vec3>; 2]) -> YokeStep {
        step_yoke(
            state,
            &frame(),
            &PHYSICAL.yoke,
            holding(points),
            false,
            1.0 / 30.0,
        )
    }

    #[test]
    fn turning_the_horns_clockwise_rolls_right_and_pulling_climbs() {
        let f = frame();
        let mut state = ControlState::default();
        let [left, right] = Hand::BOTH.map(|h| f.horn(h, 0.0, 0.0));
        grab_yoke(&mut state, &f, Hand::Left, left);
        grab_yoke(&mut state, &f, Hand::Right, right);
        let turn = 0.5;
        let pull = 0.04;
        let [left_now, right_now] = Hand::BOTH.map(|h| f.horn(h, turn, pull));
        // Clockwise as the pilot sees it: the right horn goes down.
        assert!(right_now.y < right.y);
        let out = step(&mut state, [Some(left_now), Some(right_now)]);
        assert!((state.yoke.roll - turn).abs() < 1e-4, "{}", state.yoke.roll);
        assert!((state.yoke.pitch - pull).abs() < 1e-4);
        let value = out.value.unwrap();
        assert!(value.x > 0.3, "{value}");
        // A pull is the stick pulled back: nose up.
        assert!(value.y < -0.3, "{value}");
    }

    #[test]
    fn one_hand_turns_it_about_the_column_and_a_late_grab_does_not_jump() {
        let f = frame();
        let mut state = ControlState::default();
        let right = f.horn(Hand::Right, 0.0, 0.0);
        grab_yoke(&mut state, &f, Hand::Right, right);
        step(&mut state, [None, Some(f.horn(Hand::Right, -0.4, 0.0))]);
        assert!((state.yoke.roll + 0.4).abs() < 1e-4);
        // The left hand takes it where the yoke now is.
        let left = f.horn(Hand::Left, -0.4, 0.0);
        grab_yoke(&mut state, &f, Hand::Left, left);
        step(
            &mut state,
            [Some(left), Some(f.horn(Hand::Right, -0.4, 0.0))],
        );
        assert!((state.yoke.roll + 0.4).abs() < 1e-4);
    }

    #[test]
    fn the_yoke_stops_at_its_limits_and_cues_the_ends_and_centre() {
        let f = frame();
        let mut state = ControlState::default();
        let right = f.horn(Hand::Right, 0.0, 0.0);
        grab_yoke(&mut state, &f, Hand::Right, right);
        let out = step(&mut state, [None, Some(f.horn(Hand::Right, 2.0, 0.0))]);
        let max = PHYSICAL.yoke.max_roll_deg.to_radians();
        assert!((state.yoke.roll - max).abs() < 1e-4);
        assert_eq!(out.cue, Some(ControlCue::Stop));
        let out = step(&mut state, [None, Some(right)]);
        assert_eq!(out.cue, Some(ControlCue::Centre));
        let travel = PHYSICAL.yoke.travel_m;
        let out = step(&mut state, [None, Some(right + f.axis * travel * 2.0)]);
        assert!((state.yoke.pitch - travel).abs() < 1e-5);
        assert_eq!(out.cue, Some(ControlCue::Stop));
    }

    #[test]
    fn let_go_it_springs_back_flying_on_the_way() {
        let f = frame();
        let mut state = ControlState::default();
        let right = f.horn(Hand::Right, 0.0, 0.0);
        grab_yoke(&mut state, &f, Hand::Right, right);
        step(&mut state, [None, Some(f.horn(Hand::Right, 0.6, 0.05))]);
        let out = step(&mut state, [None, None]);
        assert!(out.value.is_some_and(|v| v.x > 0.0));
        assert!(state.yoke.roll < 0.6);
        for _ in 0..60 {
            step(&mut state, [None, None]);
        }
        assert_eq!((state.yoke.roll, state.yoke.pitch), (0.0, 0.0));
        assert_eq!(step(&mut state, [None, None]).value, None);
    }

    #[test]
    fn the_drawn_hand_rides_the_turned_yoke() {
        let f = frame();
        let point = f.horn(Hand::Left, 0.0, 0.0) + Vec3::new(0.01, 0.0, 0.0);
        let grip = YokeGrip::new(&f, point, 0.0, 0.0);
        assert!(grip.anchor(&f, 0.0, 0.0).distance(point) < 1e-5);
        let turned = grip.anchor(&f, 0.3, 0.02);
        let horn = f.horn(Hand::Left, 0.3, 0.02);
        assert!((turned.distance(horn) - 0.01).abs() < 1e-4);
    }
}
