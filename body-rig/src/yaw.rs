//! The body's yaw, in two layers: the chest follows the player's head continuously within a clamp,
//! and the legs (Rico's facing) turn only for a large or sustained turn of the head, or while the
//! body moves.
//!
//! A glance to the side turns the chest toward it at once and twists the neck for the rest, with no
//! visible turn of the body. The chest is critically damped toward the head's heading, held within
//! the clamp of the legs' heading. The legs turn as a discrete act: when the head goes past the
//! turn angle, or stays past the sustain angle for the sustain time, or stays past the moving
//! deadzone for the moving sustain time while the body moves, a turn starts and runs at a slew rate
//! until the legs face the head again, within a small settle angle. A head held steady a little off
//! the legs, too little for any of those, settles them under it slowly once it has stayed still for
//! a while. The hands pull the chest their way, held out toward one side, but never the legs: aiming
//! or reaching to one side turns the torso, not the stance.
//!
//! Looking down at the body, the head's heading says little about where the body should face: the
//! player is looking at a holster or a hand, not turning. Past the look-down pitch, only the large
//! turn turns the legs, and the chest's follow fades out until, past the full look-down pitch, the
//! chest holds where it is.

use glam::{Vec2, Vec3};

use crate::{
    angle::{wrap, yaw_of_flat},
    smooth::critically_damped,
};

/// The yaw's tuning.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YawTuning {
    /// The furthest the chest turns from the legs, degrees.
    pub chest_clamp_deg: f32,
    /// How quickly the chest follows the head, seconds: the smoothing time of a critically damped
    /// spring.
    pub chest_follow_s: f32,
    /// The head this far from the legs turns them at once, degrees.
    pub turn_deg: f32,
    /// The head held this far from the legs for [`sustain_s`](Self::sustain_s) turns them, degrees.
    pub sustain_deg: f32,
    /// How long the head must stay past [`sustain_deg`](Self::sustain_deg), seconds.
    pub sustain_s: f32,
    /// The head held this far from the legs for [`moving_sustain_s`](Self::moving_sustain_s)
    /// turns them while the body moves, degrees: a walking body faces where it looks sooner.
    pub moving_deadzone_deg: f32,
    /// How long the head must stay past [`moving_deadzone_deg`](Self::moving_deadzone_deg) while
    /// the body moves, seconds: the head's sway in a walk, and a look to one side and back, pass
    /// the deadzone only briefly, and would otherwise turn the legs to and fro.
    pub moving_sustain_s: f32,
    /// A turn of the legs stops once they are this close to the head, degrees.
    pub settle_deg: f32,
    /// The fastest the legs turn, degrees per second.
    pub slew_deg_s: f32,
    /// The head counts as steady while it stays within this of where its steady spell began,
    /// degrees.
    pub steady_deg: f32,
    /// A head steady this long, seconds, settles the legs under it...
    pub steady_s: f32,
    /// ...when it is further than this from them, degrees.
    pub steady_settle_deg: f32,
    /// How fast the settle turns the legs, degrees per second: slowly enough to read as shifting
    /// one's weight rather than as a turn.
    pub steady_slew_deg_s: f32,
    /// The head pitched further down than this looks at the body, degrees: it starts only the large
    /// turn, and the chest's follow starts to fade.
    pub look_down_deg: f32,
    /// The head pitched further down than this holds the chest where it is, degrees.
    pub look_down_full_deg: f32,
    /// How far the hands, held out, pull the chest's target from the head toward their own heading,
    /// as a share of the angle between them.
    pub hand_bias: f32,
    /// How far out from the neck a hand must be held to count at all, metres; it counts fully a
    /// further [`HAND_FULL_M`] out.
    pub hand_min_m: f32,
}

impl YawTuning {
    pub const fn new() -> Self {
        Self {
            chest_clamp_deg: 45.0,
            chest_follow_s: 0.15,
            turn_deg: 60.0,
            sustain_deg: 40.0,
            sustain_s: 1.0,
            moving_deadzone_deg: 25.0,
            moving_sustain_s: 0.35,
            settle_deg: 5.0,
            slew_deg_s: 200.0,
            steady_deg: 6.0,
            steady_s: 2.0,
            steady_settle_deg: 8.0,
            steady_slew_deg_s: 40.0,
            look_down_deg: 55.0,
            look_down_full_deg: 70.0,
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

/// The hands' pull on the chest.
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

/// One frame's inputs to the yaw.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct YawInput {
    /// The head's heading.
    pub head_yaw: f32,
    /// The head's pitch from level, radians, positive looking up.
    pub head_pitch: f32,
    /// The hands' pull on the chest, when they have one.
    pub hands: Option<HandsHeading>,
    /// Whether the body moves (a step, or the stick).
    pub moving: bool,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// What started a turn of the legs.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TurnCause {
    /// The head went past the turn angle.
    Large,
    /// The head stayed past the sustain angle for the sustain time.
    Sustained,
    /// The head went past the moving deadzone while the body moved.
    Moving,
    /// The head stayed steady off the legs while the body stood.
    Steady,
}

impl TurnCause {
    pub fn label(self) -> &'static str {
        match self {
            TurnCause::Large => "large",
            TurnCause::Sustained => "sustained",
            TurnCause::Moving => "moving",
            TurnCause::Steady => "steady",
        }
    }
}

/// The yaw's state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct YawFollow {
    /// The legs' heading, once known.
    yaw: Option<f32>,
    /// The chest's heading, once known.
    chest: Option<f32>,
    /// The chest's turn rate, radians per second.
    chest_rate: f32,
    /// The turn of the legs under way, and what started it.
    turning: Option<TurnCause>,
    /// How long the head has stayed past the sustain angle, seconds.
    sustained: f32,
    /// How long the head has stayed past the moving deadzone on one side while the body moves,
    /// seconds.
    moving_sustained: f32,
    /// The side of the legs the head was on last frame, as the sign of its angle from them.
    moving_side: f32,
    /// The head's heading on the last frame.
    target: f32,
    /// Where the head's steady spell began, and how long it has lasted, seconds.
    steady_from: Option<f32>,
    steady: f32,
}

impl YawFollow {
    pub const fn new() -> Self {
        Self {
            yaw: None,
            chest: None,
            chest_rate: 0.0,
            turning: None,
            sustained: 0.0,
            moving_sustained: 0.0,
            moving_side: 0.0,
            target: 0.0,
            steady_from: None,
            steady: 0.0,
        }
    }

    /// The legs' heading, once known.
    pub fn yaw(&self) -> Option<f32> {
        self.yaw
    }

    /// The chest's heading, once known.
    pub fn chest(&self) -> Option<f32> {
        self.chest
    }

    /// The heading the legs turn toward: the head's.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether a turn of the legs is under way.
    pub fn turning(&self) -> bool {
        self.turning.is_some()
    }

    /// What started the turn of the legs under way.
    pub fn turn_cause(&self) -> Option<TurnCause> {
        self.turning
    }

    /// Face `yaw` at once, legs and chest, as a recenter does.
    pub fn snap(&mut self, yaw: f32) {
        *self = Self::new();
        if yaw.is_finite() {
            self.yaw = Some(wrap(yaw));
            self.chest = Some(wrap(yaw));
            self.target = wrap(yaw);
        }
    }

    /// Turn the frame the headings are measured in by `turn` radians about +Y (new frame from old),
    /// leaving the body where it faces in the world.
    pub fn rotate(&mut self, turn: f32) {
        if !turn.is_finite() {
            return;
        }
        self.yaw = self.yaw.map(|yaw| wrap(yaw + turn));
        self.chest = self.chest.map(|chest| wrap(chest + turn));
        self.target = wrap(self.target + turn);
        self.steady_from = self.steady_from.map(|from| wrap(from + turn));
    }

    /// Advance one frame and return the legs' heading.
    pub fn update(&mut self, input: &YawInput, tuning: &YawTuning) -> f32 {
        if !input.head_yaw.is_finite() {
            return self.yaw.unwrap_or(0.0);
        }
        let dt = if input.dt.is_finite() {
            input.dt.max(0.0)
        } else {
            0.0
        };
        let head = wrap(input.head_yaw);
        self.target = head;
        let Some(yaw) = self.yaw else {
            self.snap(head);
            return head;
        };

        let error = wrap(head - yaw);
        let past = |deg: f32| error.abs() > deg.max(0.0).to_radians();
        let looking_down = input.head_pitch.is_finite()
            && input.head_pitch < -tuning.look_down_deg.max(0.0).to_radians();
        // A look at the body does not count toward a sustained turn: the head's heading points at
        // the body, not where the player means to face.
        if past(tuning.sustain_deg) && !looking_down {
            self.sustained += dt;
        } else {
            self.sustained = 0.0;
        }
        // The head must stay past the deadzone on one side: a sway from one side to the other
        // starts the clock again.
        let side = error.signum();
        self.moving_sustained =
            if !(input.moving && past(tuning.moving_deadzone_deg)) || looking_down {
                0.0
            } else if side == self.moving_side {
                self.moving_sustained + dt
            } else {
                dt
            };
        self.moving_side = side;
        let steady = self.observe_steady(head, looking_down, dt, tuning);
        // A settle lasts only while the head stays steady over a standing body; any other turn
        // takes over from there.
        if self.turning == Some(TurnCause::Steady) && (!steady || input.moving || looking_down) {
            self.turning = None;
        }
        if self.turning.is_none() {
            self.turning = if past(tuning.turn_deg) {
                Some(TurnCause::Large)
            } else if looking_down {
                None
            } else if self.sustained >= tuning.sustain_s.max(0.0) && past(tuning.sustain_deg) {
                Some(TurnCause::Sustained)
            } else if input.moving
                && past(tuning.moving_deadzone_deg)
                && self.moving_sustained >= tuning.moving_sustain_s.max(0.0)
            {
                Some(TurnCause::Moving)
            } else if steady && !input.moving && past(tuning.steady_settle_deg) {
                Some(TurnCause::Steady)
            } else {
                None
            };
        }
        let yaw = if let Some(cause) = self.turning {
            let slew = match cause {
                TurnCause::Steady => tuning.steady_slew_deg_s,
                TurnCause::Large | TurnCause::Sustained | TurnCause::Moving => tuning.slew_deg_s,
            };
            let step = slew.max(0.0).to_radians() * dt;
            let yaw = wrap(yaw + error.clamp(-step, step));
            if wrap(head - yaw).abs() <= tuning.settle_deg.max(0.0).to_radians() {
                self.turning = None;
                self.sustained = 0.0;
                self.moving_sustained = 0.0;
            }
            yaw
        } else {
            yaw
        };
        self.yaw = Some(yaw);
        self.update_chest(yaw, head, input, dt, tuning);
        yaw
    }

    /// Fold the head's heading into its steady spell, and return whether the head has been steady
    /// for the steady time. A head that leaves the steady band starts a new spell where it is, and
    /// a look at the body counts toward none.
    fn observe_steady(
        &mut self,
        head: f32,
        looking_down: bool,
        dt: f32,
        tuning: &YawTuning,
    ) -> bool {
        let band = tuning.steady_deg.max(0.0).to_radians();
        match self.steady_from {
            Some(from) if !looking_down && wrap(head - from).abs() <= band => self.steady += dt,
            _ => {
                self.steady_from = Some(head);
                self.steady = 0.0;
            }
        }
        self.steady >= tuning.steady_s.max(0.0)
    }

    /// Move the chest toward the head, pulled by the hands, within the clamp of the legs. Looking
    /// down at the body, the follow fades toward holding the chest where it is.
    fn update_chest(&mut self, yaw: f32, head: f32, input: &YawInput, dt: f32, tuning: &YawTuning) {
        let wanted = match input.hands {
            Some(hands) if hands.yaw.is_finite() && hands.weight.is_finite() => wrap(
                head + wrap(hands.yaw - head)
                    * tuning.hand_bias.clamp(0.0, 1.0)
                    * hands.weight.clamp(0.0, 1.0),
            ),
            _ => head,
        };
        let clamp = tuning.chest_clamp_deg.clamp(0.0, 180.0).to_radians();
        let goal = wrap(yaw + wrap(wanted - yaw).clamp(-clamp, clamp));
        let chest = self.chest.unwrap_or(goal);
        let goal = wrap(chest + wrap(goal - chest) * look_down_follow(input.head_pitch, tuning));
        let (moved, rate) = smooth_damp(chest, goal, self.chest_rate, tuning.chest_follow_s, dt);
        // The clamp holds however fast the legs turn under the chest.
        let chest = wrap(yaw + wrap(moved - yaw).clamp(-clamp, clamp));
        self.chest = Some(chest);
        self.chest_rate = if rate.is_finite() { rate } else { 0.0 };
    }
}

/// How much of the chest's follow survives the head's pitch `pitch`: all of it above the look-down
/// pitch, none past the full look-down pitch, and a linear fade between.
fn look_down_follow(pitch: f32, tuning: &YawTuning) -> f32 {
    if !pitch.is_finite() {
        return 1.0;
    }
    let start = tuning.look_down_deg.max(0.0).to_radians();
    let full = tuning.look_down_full_deg.max(0.0).to_radians();
    let down = -pitch;
    if down <= start {
        1.0
    } else if down >= full {
        0.0
    } else {
        1.0 - (down - start) / (full - start)
    }
}

/// The [`critically_damped`] spring from the heading `from`, turning at `rate`, toward `to` the
/// short way round, over `dt` seconds with a smoothing time of `smooth_s`: the new heading and
/// rate. It never overshoots.
fn smooth_damp(from: f32, to: f32, rate: f32, smooth_s: f32, dt: f32) -> (f32, f32) {
    let change = wrap(from - to);
    let (mut offset, mut rate) = critically_damped(change, rate, 0.0, smooth_s, dt);
    // Arriving past the goal from either side lands on it.
    if (change > 0.0) != (offset > 0.0) && change != 0.0 {
        offset = 0.0;
        rate = 0.0;
    }
    (wrap(to + offset), rate)
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
            head_pitch: 0.0,
            hands: None,
            moving: false,
            dt: DT,
        }
    }

    fn run(follow: &mut YawFollow, input: YawInput, seconds: f32) -> f32 {
        let mut yaw = follow.yaw().unwrap_or(0.0);
        for _ in 0..(seconds / DT) as usize {
            yaw = follow.update(&input, &TUNING);
        }
        yaw
    }

    #[test]
    fn a_glance_turns_the_chest_and_not_the_legs() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        assert_eq!(run(&mut follow, look(30.0), 0.5), 0.0);
        assert!(!follow.turning());
        assert!((follow.chest().unwrap().to_degrees() - 30.0).abs() < 1.0);
    }

    #[test]
    fn the_chest_stays_within_its_clamp() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        // Inside the turn angle and for less than the sustain time: the legs stay.
        assert_eq!(run(&mut follow, look(55.0), 0.8), 0.0);
        assert!((follow.chest().unwrap().to_degrees() - TUNING.chest_clamp_deg).abs() < 0.1);
    }

    #[test]
    fn a_large_turn_brings_the_legs_round() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let yaw = run(&mut follow, look(90.0), 2.0);
        assert!((yaw.to_degrees() - 90.0).abs() <= TUNING.settle_deg + 0.1);
        assert!(!follow.turning());
    }

    #[test]
    fn a_sustained_turn_brings_the_legs_round() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        follow.update(&look(50.0), &TUNING);
        assert_eq!(follow.turn_cause(), None);
        run(&mut follow, look(50.0), TUNING.sustain_s * 0.9);
        assert!(!follow.turning());
        run(&mut follow, look(50.0), TUNING.sustain_s * 0.2);
        assert_eq!(follow.turn_cause(), Some(TurnCause::Sustained));
        let yaw = run(&mut follow, look(50.0), 1.0);
        assert!((yaw.to_degrees() - 50.0).abs() <= TUNING.settle_deg + 0.1);
    }

    #[test]
    fn looking_back_resets_the_sustain() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        for _ in 0..4 {
            run(&mut follow, look(50.0), TUNING.sustain_s * 0.6);
            run(&mut follow, look(0.0), 0.1);
        }
        assert!(!follow.turning());
        assert_eq!(follow.yaw(), Some(0.0));
    }

    #[test]
    fn the_turn_is_slew_limited() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let yaw = follow.update(&look(120.0), &TUNING);
        assert!((yaw.to_degrees() - TUNING.slew_deg_s * DT).abs() < 1e-3);
    }

    #[test]
    fn moving_narrows_the_turn() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let mut moving = look(TUNING.moving_deadzone_deg + 5.0);
        moving.moving = true;
        assert!(run(&mut follow, moving, 1.0) > 0.2);
        assert!(!follow.turning());
    }

    #[test]
    fn a_walk_that_sways_past_the_moving_deadzone_keeps_the_legs() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let sway = TUNING.moving_deadzone_deg + 5.0;
        let brief = TUNING.moving_sustain_s * 0.6;
        for side in [1.0, -1.0, 1.0, -1.0, 1.0, -1.0] {
            let mut moving = look(sway * side);
            moving.moving = true;
            run(&mut follow, moving, brief);
            assert!(!follow.turning(), "{side}");
        }
        assert_eq!(follow.yaw(), Some(0.0));
    }

    fn look_down(head_deg: f32, pitch_deg: f32) -> YawInput {
        YawInput {
            head_pitch: pitch_deg.to_radians(),
            ..look(head_deg)
        }
    }

    #[test]
    fn a_steady_head_settles_the_legs_slowly() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let off = 20.0;
        assert_eq!(run(&mut follow, look(off), TUNING.steady_s * 0.95), 0.0);
        assert!(!follow.turning());
        let mut last = follow.yaw().unwrap();
        let mut settled = false;
        for _ in 0..(2.0 / DT) as usize {
            let yaw = follow.update(&look(off), &TUNING);
            assert!(wrap(yaw - last).abs() <= TUNING.steady_slew_deg_s.to_radians() * DT + 1e-5);
            if follow.turning() {
                assert_eq!(follow.turn_cause(), Some(TurnCause::Steady));
            } else if yaw != 0.0 {
                settled = true;
            }
            last = yaw;
        }
        assert!(settled);
        assert!((last.to_degrees() - off).abs() <= TUNING.settle_deg + 0.1);
    }

    #[test]
    fn a_restless_head_does_not_settle_the_legs() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let swing = TUNING.steady_deg * 1.5;
        for i in 0..6 {
            let head = 20.0 + if i % 2 == 0 { swing } else { -swing } * 0.5;
            run(&mut follow, look(head), TUNING.steady_s * 0.8);
            assert!(!follow.turning(), "{i}");
        }
        assert_eq!(follow.yaw(), Some(0.0));
    }

    #[test]
    fn a_head_steady_close_to_the_legs_leaves_them() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let near = TUNING.steady_settle_deg * 0.8;
        assert_eq!(run(&mut follow, look(near), TUNING.steady_s * 3.0), 0.0);
    }

    #[test]
    fn a_moving_body_does_not_settle() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let mut moving = look(TUNING.moving_deadzone_deg * 0.8);
        moving.moving = true;
        assert_eq!(run(&mut follow, moving, TUNING.steady_s * 2.0), 0.0);
    }

    #[test]
    fn looking_down_starts_only_the_large_turn() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let down = -(TUNING.look_down_deg + 10.0);
        // Past the sustain angle, and steady, for long enough to start either.
        assert_eq!(run(&mut follow, look_down(50.0, down), 3.0), 0.0);
        assert!(!follow.turning());
        let yaw = run(&mut follow, look_down(90.0, down), 1.0);
        assert!((yaw.to_degrees() - 90.0).abs() <= TUNING.settle_deg + 0.1);
    }

    #[test]
    fn a_look_at_the_body_does_not_count_toward_a_sustained_turn() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let down = -(TUNING.look_down_deg + 10.0);
        run(&mut follow, look_down(50.0, down), 3.0);
        follow.update(&look(50.0), &TUNING);
        assert!(!follow.turning());
        run(&mut follow, look(50.0), TUNING.sustain_s * 1.1);
        assert_eq!(follow.turn_cause(), Some(TurnCause::Sustained));
    }

    #[test]
    fn looking_down_holds_the_chest() {
        let mut follow = YawFollow::new();
        follow.update(&look(0.0), &TUNING);
        let full = -(TUNING.look_down_full_deg + 5.0);
        run(&mut follow, look_down(30.0, full), 2.0);
        assert!(follow.chest().unwrap().abs() < 1e-4);
        // Halfway into the fade, the chest moves on toward the head, more slowly.
        let half = -(TUNING.look_down_deg + TUNING.look_down_full_deg) * 0.5;
        follow.update(&look_down(30.0, half), &TUNING);
        let slow = follow.chest().unwrap();
        let mut level = YawFollow::new();
        level.update(&look(0.0), &TUNING);
        level.update(&look(30.0), &TUNING);
        assert!(slow > 0.0 && slow < level.chest().unwrap());
        // Above the look-down pitch, it follows in full.
        run(&mut follow, look(30.0), 2.0);
        assert!((follow.chest().unwrap().to_degrees() - 30.0).abs() < 0.5);
    }

    #[test]
    fn hands_held_out_pull_the_chest_and_not_the_legs() {
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
        let yaw = run(&mut follow, input, 3.0);
        assert_eq!(yaw, 0.0);
        assert!(follow.chest().unwrap() > 0.6);
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
        assert!((follow.chest().unwrap() - 0.8).abs() < 1e-6);
    }

    #[test]
    fn the_spring_settles_without_overshooting() {
        let (mut heading, mut rate) = (0.0f32, 0.0f32);
        let mut furthest = 0.0f32;
        for _ in 0..200 {
            (heading, rate) = smooth_damp(heading, 1.0, rate, 0.15, DT);
            furthest = furthest.max(heading);
        }
        assert!(furthest <= 1.0 + 1e-5);
        assert!((heading - 1.0).abs() < 1e-3);
    }

    proptest! {
        #[test]
        fn a_frame_never_turns_the_legs_further_than_the_slew(
            heads in proptest::collection::vec(-180.0f32..180.0, 1..200),
        ) {
            let mut follow = YawFollow::new();
            follow.update(&look(0.0), &TUNING);
            let mut last = 0.0f32;
            for head in heads {
                let yaw = follow.update(&look(head), &TUNING);
                prop_assert!(wrap(yaw - last).abs() <= TUNING.slew_deg_s.to_radians() * DT + 1e-4);
                let chest = follow.chest().unwrap();
                prop_assert!(
                    wrap(chest - yaw).abs() <= TUNING.chest_clamp_deg.to_radians() + 1e-4
                );
                last = yaw;
            }
        }
    }
}
