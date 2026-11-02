//! The body's yaw follow: Rico's facing follows the player's head through a deadzone.
//!
//! A glance to the side twists the neck only. Turning the head past the deadzone starts a body
//! turn, which runs at a slew rate until the body faces the target again, within a small settle
//! angle: the turn is a discrete act, as a person turns the body once the head has gone far enough,
//! not a constant drag after every look. The hands bias the target: held out toward one side, they
//! pull it their way, so aiming to one side turns the torso before the head gets there.

use glam::{Vec2, Vec3};

use crate::angle::{wrap, yaw_of_flat};

/// The yaw follow's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YawTuning {
    /// How far the target may be from the body before the body turns, degrees.
    pub deadzone_deg: f32,
    /// The deadzone while the body moves, degrees: a walking body faces where it looks sooner.
    pub moving_deadzone_deg: f32,
    /// A turn stops once the body is this close to the target, degrees.
    pub settle_deg: f32,
    /// The fastest the body turns, degrees per second.
    pub slew_deg_s: f32,
    /// How far the hands, held out, pull the target from the head toward their own heading, as a
    /// share of the angle between them.
    pub hand_bias: f32,
    /// How far out from the neck a hand must be held to count at all, metres; it counts fully a
    /// further [`HAND_FULL_M`] out.
    pub hand_min_m: f32,
}

impl YawTuning {
    pub const fn new() -> Self {
        Self {
            deadzone_deg: 30.0,
            moving_deadzone_deg: 15.0,
            settle_deg: 5.0,
            slew_deg_s: 200.0,
            hand_bias: 0.5,
            hand_min_m: 0.2,
        }
    }
}

impl Default for YawTuning {
    fn default() -> Self {
        Self::new()
    }
}

/// How much further out than [`YawTuning::hand_min_m`] the hands must be to count fully, metres.
pub const HAND_FULL_M: f32 = 0.25;

/// The hands' pull on the target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandsHeading {
    /// The heading of the hands' mean horizontal offset from the neck.
    pub yaw: f32,
    /// How much they count, `0..=1`.
    pub weight: f32,
}

/// The hands' heading from the neck pivot at `neck`, for the tracked hands: `None` when no tracked
/// hand is held out far enough to count.
pub fn hands_heading(
    neck: Vec3,
    hands: &[Option<Vec3>; 2],
    tuning: &YawTuning,
) -> Option<HandsHeading> {
    let (sum, count) = hands
        .iter()
        .flatten()
        .map(|hand| Vec2::new(hand.x - neck.x, hand.z - neck.z))
        .filter(|offset| offset.is_finite())
        .fold((Vec2::ZERO, 0u32), |(sum, count), offset| {
            (sum + offset, count + 1)
        });
    if count == 0 {
        return None;
    }
    let mean = sum / count as f32;
    let out = mean.length();
    let weight = ((out - tuning.hand_min_m) / HAND_FULL_M).clamp(0.0, 1.0);
    let yaw = yaw_of_flat(mean)?;
    (weight > 0.0).then_some(HandsHeading { yaw, weight })
}

/// One frame's inputs to the yaw follow.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YawInput {
    /// The head's heading.
    pub head_yaw: f32,
    /// The hands' pull, when they have one.
    pub hands: Option<HandsHeading>,
    /// Whether the body moves (a step, or the stick).
    pub moving: bool,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// The yaw follow's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct YawFollow {
    /// The body's heading, once known.
    yaw: Option<f32>,
    /// Whether a turn is under way.
    turning: bool,
    /// The target on the last frame.
    target: f32,
}

impl YawFollow {
    pub const fn new() -> Self {
        Self {
            yaw: None,
            turning: false,
            target: 0.0,
        }
    }

    /// The body's heading, once known.
    pub fn yaw(&self) -> Option<f32> {
        self.yaw
    }

    /// The target the body turns toward.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether a turn is under way.
    pub fn turning(&self) -> bool {
        self.turning
    }

    /// Face `yaw` at once, as a recenter does.
    pub fn snap(&mut self, yaw: f32) {
        if yaw.is_finite() {
            self.yaw = Some(wrap(yaw));
            self.target = wrap(yaw);
        } else {
            self.yaw = None;
        }
        self.turning = false;
    }

    /// Turn the frame the headings are measured in by `turn` radians about +Y (new frame from old),
    /// leaving the body where it faces in the world.
    pub fn rotate(&mut self, turn: f32) {
        if !turn.is_finite() {
            return;
        }
        self.yaw = self.yaw.map(|yaw| wrap(yaw + turn));
        self.target = wrap(self.target + turn);
    }

    /// Advance one frame and return the body's heading.
    pub fn update(&mut self, input: &YawInput, tuning: &YawTuning) -> f32 {
        if !input.head_yaw.is_finite() {
            return self.yaw.unwrap_or(0.0);
        }
        let target = match input.hands {
            Some(hands) if hands.yaw.is_finite() && hands.weight.is_finite() => wrap(
                input.head_yaw
                    + wrap(hands.yaw - input.head_yaw)
                        * tuning.hand_bias.clamp(0.0, 1.0)
                        * hands.weight.clamp(0.0, 1.0),
            ),
            _ => input.head_yaw,
        };
        self.target = target;
        let Some(yaw) = self.yaw else {
            self.yaw = Some(target);
            return target;
        };
        let error = wrap(target - yaw);
        let deadzone = if input.moving {
            tuning.moving_deadzone_deg
        } else {
            tuning.deadzone_deg
        }
        .max(0.0)
        .to_radians();
        if error.abs() > deadzone {
            self.turning = true;
        }
        if !self.turning {
            return yaw;
        }
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let step = tuning.slew_deg_s.max(0.0).to_radians() * dt;
        let yaw = wrap(yaw + error.clamp(-step, step));
        if wrap(target - yaw).abs() <= tuning.settle_deg.max(0.0).to_radians() {
            self.turning = false;
        }
        self.yaw = Some(yaw);
        yaw
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const TUNING: YawTuning = YawTuning::new();
    const DT: f32 = 1.0 / 90.0;

    fn look(head_deg: f32) -> YawInput {
        YawInput {
            head_yaw: head_deg.to_radians(),
            hands: None,
            moving: false,
            dt: DT,
        }
    }

    fn run(follow: &mut YawFollow, input: YawInput, seconds: f32) -> f32 {
        let mut yaw = 0.0;
        for _ in 0..(seconds / DT) as usize {
            yaw = follow.update(&input, &TUNING);
        }
        yaw
    }

    #[test]
    fn a_glance_inside_the_deadzone_turns_nothing() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        for deg in [10.0, -25.0, 29.0, 0.0] {
            assert_eq!(run(&mut follow, look(deg), 0.5), 0.0);
        }
        assert!(!follow.turning());
    }

    #[test]
    fn a_turn_past_the_deadzone_brings_the_body_round() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let yaw = run(&mut follow, look(90.0), 2.0);
        assert!((yaw.to_degrees() - 90.0).abs() <= TUNING.settle_deg + 0.1);
        assert!(!follow.turning());
    }

    #[test]
    fn the_turn_is_slew_limited() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let yaw = follow.update(&look(120.0), &TUNING);
        assert!((yaw.to_degrees() - TUNING.slew_deg_s * DT).abs() < 1e-3);
    }

    #[test]
    fn moving_narrows_the_deadzone() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let mut moving = look(20.0);
        moving.moving = true;
        assert!(run(&mut follow, moving, 1.0) > 0.2);
    }

    #[test]
    fn hands_held_out_pull_the_target() {
        let neck = Vec3::new(0.0, 1.6, 0.0);
        // Both hands out to the left (-X), at chest height.
        let hands = [
            Some(Vec3::new(-0.6, 1.3, -0.1)),
            Some(Vec3::new(-0.55, 1.3, 0.05)),
        ];
        let heading = hands_heading(neck, &hands, &TUNING).unwrap();
        assert!(heading.yaw > 1.3 && heading.weight > 0.9);
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let mut input = look(0.0);
        input.hands = Some(heading);
        follow.update(&input, &TUNING);
        assert!(follow.target() > 0.6);
        // Hands down at the sides say nothing.
        let down = [
            Some(Vec3::new(-0.2, 0.9, 0.0)),
            Some(Vec3::new(0.2, 0.9, 0.0)),
        ];
        assert_eq!(hands_heading(neck, &down, &TUNING), None);
    }

    #[test]
    fn rotating_the_frame_keeps_the_body_in_the_world() {
        let mut follow = YawFollow::new();
        follow.snap(0.3);
        follow.rotate(0.5);
        assert!((follow.yaw().unwrap() - 0.8).abs() < 1e-6);
    }

    proptest! {
        #[test]
        fn a_frame_never_turns_further_than_the_slew(
            heads in proptest::collection::vec(-180.0f32..180.0, 1..200),
        ) {
            let mut follow = YawFollow::new();
            follow.update(&look(0.0), &TUNING);
            let mut last = 0.0f32;
            for head in heads {
                let yaw = follow.update(&look(head), &TUNING);
                prop_assert!(wrap(yaw - last).abs() <= TUNING.slew_deg_s.to_radians() * DT + 1e-4);
                last = yaw;
            }
        }
    }
}
