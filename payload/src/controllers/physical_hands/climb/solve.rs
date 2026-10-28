//! The climbing's motion, pure: where the gripping hands would have the body, the velocity that
//! takes it there within a physics step, the fling a release keeps, and the mantle over a ledge.
//! Everything is in game space, metres and seconds.
//!
//! A grip remembers where the body was when it took hold, and where the palm was in the rig: the
//! palm's place in the tracking space, turned by the body's facing and scaled by the VR world scale
//! into world metres, but not carried by the body's position. The hands would have the body wherever
//! keeps each palm on its grip: where the body was at the grip, moved back by how far the palm has
//! since moved in the rig, averaged over the gripping hands. Nothing in that target depends on
//! where the body or the view is now, so the climb is a plain closed loop on the body's position:
//! a hand held still holds the body still once it gets there.

use glam::{Affine3A, Quat, Vec2, Vec3};

/// One gripping hand: where the body and the palm were when it took hold, and where the palm is
/// in the rig now.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Follow {
    /// The player's body when the hand took hold, game space.
    pub body: Vec3,
    /// The palm in the rig when the hand took hold, world metres.
    pub grip: Vec3,
    /// The palm in the rig now, world metres.
    pub palm: Vec3,
}

impl Follow {
    /// Where this hand would have the body.
    pub fn target(&self) -> Vec3 {
        self.body + (self.grip - self.palm)
    }
}

/// Where the hands would have the body: the mean of the gripping hands' targets. `None` without a
/// gripping hand.
pub fn body_target(follows: impl IntoIterator<Item = Follow>) -> Option<Vec3> {
    let (sum, count) = follows
        .into_iter()
        .fold((Vec3::ZERO, 0u32), |(sum, count), f| {
            (sum + f.target(), count + 1)
        });
    if count == 0 {
        return None;
    }
    let target = sum / count as f32;
    target.is_finite().then_some(target)
}

/// How the body is moved toward its target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ClimbTuning {
    /// The share of the gap one step closes, `0..=1`.
    pub gain: f32,
    pub max_speed_mps: f32,
    pub step_s: f32,
}

/// The velocity that closes `tuning.gain` of the gap from `body` to `target` over one step, capped
/// at the fastest climb. Zero for a gap or step that is not finite.
pub fn climb_velocity(target: Vec3, body: Vec3, tuning: &ClimbTuning) -> Vec3 {
    if !(tuning.step_s.is_finite() && tuning.step_s > 0.0) {
        return Vec3::ZERO;
    }
    let velocity = (target - body) * tuning.gain.clamp(0.0, 1.0) / tuning.step_s;
    if !velocity.is_finite() {
        return Vec3::ZERO;
    }
    velocity.clamp_length_max(tuning.max_speed_mps.max(0.0))
}

/// Fold `velocity` into the smoothed velocity `smoothed` over `dt` seconds with the time constant
/// `smoothing_s`.
pub fn smooth(smoothed: Vec3, velocity: Vec3, dt: f32, smoothing_s: f32) -> Vec3 {
    if !(smoothed.is_finite() && velocity.is_finite()) {
        return Vec3::ZERO;
    }
    let blend = if smoothing_s > 0.0 && dt.is_finite() && dt > 0.0 {
        1.0 - (-dt / smoothing_s).exp()
    } else {
        1.0
    };
    smoothed.lerp(velocity, blend)
}

/// Where the point `point` (world space), fixed to a body that has moved from `from` to `to`, is
/// now.
pub fn carried(from: Affine3A, to: Affine3A, point: Vec3) -> Vec3 {
    let moved = (to * from.inverse()).transform_point3(point);
    if moved.is_finite() { moved } else { point }
}

/// The turn of a body that has moved from `from` to `to`, world space.
pub fn carried_turn(from: Affine3A, to: Affine3A) -> Quat {
    let (_, from_turn, _) = from.to_scale_rotation_translation();
    let (_, to_turn, _) = to.to_scale_rotation_translation();
    let turn = (to_turn * from_turn.inverse()).normalize();
    if turn.is_finite() {
        turn
    } else {
        Quat::IDENTITY
    }
}

/// The velocity the gripped bodies carry the climber with: the mean of the velocities of the
/// points the hands grip, capped at `max_speed_mps`. Zero without a grip.
pub fn carry_velocity(points: impl IntoIterator<Item = Vec3>, max_speed_mps: f32) -> Vec3 {
    let (sum, count) = points
        .into_iter()
        .filter(|v| v.is_finite())
        .fold((Vec3::ZERO, 0u32), |(sum, count), v| (sum + v, count + 1));
    if count == 0 {
        return Vec3::ZERO;
    }
    (sum / count as f32).clamp_length_max(max_speed_mps.max(0.0))
}

/// The velocity a release keeps: `scale` of the smoothed climbing velocity, capped at `max_speed`.
pub fn fling(smoothed: Vec3, scale: f32, max_speed: f32) -> Vec3 {
    if !smoothed.is_finite() || !scale.is_finite() {
        return Vec3::ZERO;
    }
    (smoothed * scale.max(0.0)).clamp_length_max(max_speed.max(0.0))
}

/// Filter total and carry together, resetting both when the contributing grip set changes.
pub fn smooth_motion(
    previous: (Vec3, Vec3),
    current: (Vec3, Vec3),
    changed: bool,
    step_s: f32,
    seconds: f32,
) -> (Vec3, Vec3) {
    if changed {
        current
    } else {
        (
            smooth(previous.0, current.0, step_s, seconds),
            smooth(previous.1, current.1, step_s, seconds),
        )
    }
}

/// Keep inherited point velocity separate from the capped, scaled hand-generated release.
pub fn release_velocity(total: Vec3, carry: Vec3, scale: f32, max_speed: f32) -> Vec3 {
    if !(total.is_finite() && carry.is_finite()) {
        return Vec3::ZERO;
    }
    carry + fling(total - carry, scale, max_speed)
}

/// The velocity a fling of `kept` carries `elapsed_s` seconds after the release: thrown, under
/// `gravity_mps2`, so that the game's own air handling, whenever it takes over, finds the body
/// moving as a throw would.
pub fn fling_carry(kept: Vec3, elapsed_s: f32, gravity_mps2: f32) -> Vec3 {
    if !(kept.is_finite() && elapsed_s.is_finite() && gravity_mps2.is_finite()) {
        return Vec3::ZERO;
    }
    kept - Vec3::Y * gravity_mps2.max(0.0) * elapsed_s.max(0.0)
}

/// The share of a climb's tension pulse a climb at `speed_mps` pulses with, `0..=1`: full at
/// `full_mps`.
pub fn tension(speed_mps: f32, full_mps: f32) -> f32 {
    if !(speed_mps.is_finite() && full_mps.is_finite() && full_mps > 0.0) {
        return 0.0;
    }
    (speed_mps / full_mps).clamp(0.0, 1.0)
}

/// How the mantle carries the body over a ledge.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MantleTuning {
    /// How far below the top the feet may be for the mantle to start, metres.
    pub start_m: f32,
    /// How far above the top the feet are lifted before the body goes over, metres.
    pub clearance_m: f32,
    /// How far past the grip the body is carried, metres.
    pub forward_m: f32,
    pub speed_mps: f32,
    /// The least smoothed upward speed of the climb for the mantle to start, metres per second.
    pub min_speed_mps: f32,
    /// How far the feet must have climbed since the climb started for a mantle to start while the
    /// proxy still stands on something, metres.
    pub min_climb_m: f32,
    /// How far past the top the feet must be carried before the mantle lets them down onto it,
    /// metres: about the proxy capsule's radius, so the whole capsule is over the ledge.
    pub land_past_m: f32,
    pub step_s: f32,
}

/// Whether a gripped surface is a ledge's top a mantle may carry the body over: its normal at
/// least `min_normal_y` upward, and, for a player who stood on something when the hand took hold,
/// more than the mantle's start height above the feet then. A floor or a low table gripped while
/// standing is never a ledge.
pub fn is_ledge(
    normal_y: f32,
    top_y: f32,
    feet_y: f32,
    standing: bool,
    min_normal_y: f32,
    tuning: &MantleTuning,
) -> bool {
    normal_y.is_finite()
        && normal_y >= min_normal_y
        && !(standing && top_y - feet_y <= tuning.start_m)
}

/// What decides whether the mantle starts this step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct MantleCheck {
    /// The feet's height, metres.
    pub feet_y: f32,
    /// The gripped ledge's top's height, metres.
    pub top_y: f32,
    /// The climb's smoothed upward speed, metres per second.
    pub rise_mps: f32,
    /// How far the feet have climbed since the climb started, metres.
    pub climbed_m: f32,
    /// Whether the proxy's own update found it standing on something this step.
    pub supported: bool,
}

/// Whether the mantle starts: the feet have come within `start_m` below the gripped top while the
/// hands pull the body up steadily, and the feet are off what they stood on, or have climbed far
/// enough that it is not the ground they started on.
pub fn mantle_starts(check: &MantleCheck, tuning: &MantleTuning) -> bool {
    let MantleCheck {
        feet_y,
        top_y,
        rise_mps,
        climbed_m,
        supported,
    } = *check;
    feet_y.is_finite()
        && top_y.is_finite()
        && feet_y >= top_y - tuning.start_m
        && rise_mps >= tuning.min_speed_mps
        && (!supported || climbed_m >= tuning.min_climb_m)
}

/// What the mantle does this step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum MantleMotion {
    /// Move the feet at this velocity.
    Move(Vec3),
    /// The feet are over the ledge where the mantle carries them: let go.
    Arrived,
}

/// A mantle under way: the gripped top, and the horizontal way over it, fixed when it starts.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Mantle {
    pub top: Vec3,
    /// The unit `(x, z)` direction from the body over the ledge.
    pub over: Vec2,
}

impl Mantle {
    /// The mantle over the top `top` for feet at `feet`: over the ledge is the horizontal way from
    /// the feet to the top. `None` when the feet are right under the top, with no way over.
    pub fn start(top: Vec3, feet: Vec3) -> Option<Self> {
        let over = Vec2::new(top.x - feet.x, top.z - feet.z).normalize_or_zero();
        (over != Vec2::ZERO && top.is_finite()).then_some(Self { top, over })
    }

    /// Where the feet end up: past the top along the way over, at the top's height plus the
    /// clearance.
    pub fn destination(&self, tuning: &MantleTuning) -> Vec3 {
        Vec3::new(
            self.top.x + self.over.x * tuning.forward_m,
            self.top.y + tuning.clearance_m,
            self.top.z + self.over.y * tuning.forward_m,
        )
    }

    /// What carries feet at `feet` over the ledge: straight up until they clear the top, then
    /// over it at that height, and once they are past the top by [`MantleTuning::land_past_m`],
    /// down onto it as they go on, so that the proxy lands rather than hovers at the clearance.
    /// Each part goes no faster than the mantle's speed, nor further in a step than it has left;
    /// feet past the top with no way left to go have arrived.
    pub fn motion(&self, feet: Vec3, tuning: &MantleTuning) -> MantleMotion {
        if !(feet.is_finite() && tuning.step_s.is_finite() && tuning.step_s > 0.0) {
            return MantleMotion::Move(Vec3::ZERO);
        }
        let destination = self.destination(tuning);
        let speed = tuning.speed_mps.max(0.0);
        let left = Vec2::new(destination.x - feet.x, destination.z - feet.z);
        let across = (left / tuning.step_s).clamp_length_max(speed);
        let past = Vec2::new(feet.x - self.top.x, feet.z - self.top.z).dot(self.over);
        if past >= tuning.land_past_m {
            if left.length() <= ARRIVED_M {
                return MantleMotion::Arrived;
            }
            return MantleMotion::Move(Vec3::new(across.x, -LAND_SPEED_MPS.min(speed), across.y));
        }
        let rise = ((destination.y - feet.y) / tuning.step_s).clamp(-speed, speed);
        if feet.y < destination.y - tuning.clearance_m * 0.5 {
            return MantleMotion::Move(Vec3::new(0.0, rise, 0.0));
        }
        MantleMotion::Move(Vec3::new(across.x, rise, across.y))
    }
}

/// How close to where the mantle carries them the feet count as there, metres.
const ARRIVED_M: f32 = 0.05;

/// How fast the mantle lets the feet down onto the ledge once they are over it, metres per second.
const LAND_SPEED_MPS: f32 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f32 = 1.0 / 30.0;

    fn tuning() -> ClimbTuning {
        ClimbTuning {
            gain: 1.0,
            max_speed_mps: 6.0,
            step_s: STEP,
        }
    }

    fn mantle_tuning() -> MantleTuning {
        MantleTuning {
            start_m: 0.9,
            clearance_m: 0.08,
            forward_m: 0.4,
            speed_mps: 2.0,
            min_speed_mps: 0.3,
            min_climb_m: 0.25,
            land_past_m: 0.3,
            step_s: STEP,
        }
    }

    fn check(feet_y: f32, rise_mps: f32, climbed_m: f32, supported: bool) -> MantleCheck {
        MantleCheck {
            feet_y,
            top_y: 10.0,
            rise_mps,
            climbed_m,
            supported,
        }
    }

    fn moving(motion: MantleMotion) -> Vec3 {
        match motion {
            MantleMotion::Move(v) => v,
            MantleMotion::Arrived => panic!("arrived"),
        }
    }

    #[test]
    fn a_point_on_a_moving_body_is_carried_with_it() {
        let from = Affine3A::from_translation(Vec3::new(10.0, 0.0, 0.0));
        let to = Affine3A::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(12.0, 1.0, 0.0),
        );
        // A point a metre along the body's +X ends a metre along its turned +X, which is -Z.
        let point = Vec3::new(11.0, 0.0, 0.0);
        assert!(carried(from, to, point).abs_diff_eq(Vec3::new(12.0, 1.0, -1.0), 1e-5));
        let turn = carried_turn(from, to);
        assert!((turn * Vec3::X).abs_diff_eq(Vec3::NEG_Z, 1e-5));
        // A body that has not moved carries nothing.
        assert!(carried(from, from, point).abs_diff_eq(point, 1e-6));
    }

    #[test]
    fn the_gripped_bodies_carry_the_climber_at_their_mean_speed() {
        assert_eq!(carry_velocity([], 50.0), Vec3::ZERO);
        let v = carry_velocity([Vec3::X * 10.0, Vec3::X * 14.0], 50.0);
        assert!(v.abs_diff_eq(Vec3::X * 12.0, 1e-5));
        assert!((carry_velocity([Vec3::Z * 90.0], 50.0).length() - 50.0).abs() < 1e-3);
        assert!(carry_velocity([Vec3::NAN, Vec3::Y], 50.0).abs_diff_eq(Vec3::Y, 1e-6));
    }

    #[test]
    fn a_fresh_grip_holds_the_body_where_it_is() {
        let body = Vec3::new(3.0, 10.0, -2.0);
        let palm = Vec3::new(0.4, 1.5, -0.3);
        let target = body_target([Follow {
            body,
            grip: palm,
            palm,
        }])
        .unwrap();
        assert!(target.abs_diff_eq(body, 1e-6));
        assert_eq!(climb_velocity(target, body, &tuning()), Vec3::ZERO);
    }

    #[test]
    fn pulling_the_hand_down_lifts_the_body() {
        let body = Vec3::ZERO;
        let grip = Vec3::new(0.3, 1.8, 0.4);
        let palm = grip - Vec3::Y * 0.2;
        let target = body_target([Follow { body, grip, palm }]).unwrap();
        assert!(target.abs_diff_eq(Vec3::Y * 0.2, 1e-6));
        let velocity = climb_velocity(target, body, &tuning());
        assert!(velocity.y > 0.0 && velocity.x.abs() < 1e-6 && velocity.z.abs() < 1e-6);
        // Pushing the hand away from the wall pulls the body toward it.
        let away = body_target([Follow {
            body,
            grip,
            palm: grip - Vec3::Z * 0.1,
        }])
        .unwrap();
        assert!(away.z > 0.0);
    }

    #[test]
    fn release_keeps_vehicle_speed_and_caps_only_relative_motion() {
        let carry = Vec3::X * 60.0;
        assert_eq!(
            release_velocity(carry + Vec3::Y * 20.0, carry, 2.0, 8.0),
            carry + Vec3::Y * 8.0
        );
        assert_eq!(release_velocity(carry, carry, 3.0, 8.0), carry);
        assert_eq!(release_velocity(carry + Vec3::Y, carry, 0.0, 8.0), carry);
    }

    #[test]
    fn moving_static_and_two_different_bodies_share_carry_without_last_hand_bias() {
        assert_eq!(
            carry_velocity([Vec3::X * 20.0, Vec3::ZERO], 100.0),
            Vec3::X * 10.0
        );
        assert_eq!(
            carry_velocity([Vec3::X * 20.0, Vec3::Z * 10.0], 100.0),
            Vec3::new(10.0, 0.0, 5.0)
        );
        assert_eq!(carry_velocity([Vec3::ZERO], 100.0), Vec3::ZERO);
        assert_eq!(
            release_velocity(Vec3::Y * 2.0, Vec3::ZERO, 1.0, 8.0),
            Vec3::Y * 2.0
        );
    }

    #[test]
    fn changing_the_grip_set_clears_old_carry_before_the_final_release() {
        let previous = (Vec3::X * 30.0 + Vec3::Y, Vec3::X * 30.0);
        let static_motion = (Vec3::Y * 2.0, Vec3::ZERO);
        let now = smooth_motion(previous, static_motion, true, 0.03, 0.1);
        assert_eq!(now, static_motion);
        assert_eq!(release_velocity(now.0, now.1, 1.0, 8.0), Vec3::Y * 2.0);
        let other_body = (Vec3::Z * 10.0 + Vec3::Y, Vec3::Z * 10.0);
        assert_eq!(
            smooth_motion(previous, other_body, true, 0.03, 0.1),
            other_body
        );
    }

    #[test]
    fn identical_filters_separate_carry_from_relative_motion() {
        let carry = Vec3::X * 30.0;
        let relative = Vec3::Y * 3.0;
        let total = smooth(Vec3::ZERO, carry + relative, 0.03, 0.1);
        let inherited = smooth(Vec3::ZERO, carry, 0.03, 0.1);
        let hand = smooth(Vec3::ZERO, relative, 0.03, 0.1);
        assert!((total - inherited).abs_diff_eq(hand, 1e-5));
    }

    #[test]
    fn two_hands_average_their_pulls() {
        let body = Vec3::ZERO;
        let left = Follow {
            body,
            grip: Vec3::new(-0.3, 2.0, 0.4),
            palm: Vec3::new(-0.3, 1.8, 0.4),
        };
        let right = Follow {
            body,
            grip: Vec3::new(0.3, 2.0, 0.4),
            palm: Vec3::new(0.3, 2.0, 0.4),
        };
        let target = body_target([left, right]).unwrap();
        assert!(target.abs_diff_eq(Vec3::Y * 0.1, 1e-6));
        assert_eq!(body_target([]), None);
    }

    #[test]
    fn a_hand_held_still_after_a_pull_stops_the_body() {
        // A pull of 30 cm down over ten steps, then the hand held still. The body follows the
        // pull and comes to rest the pull's height above where it gripped, whatever the gain.
        let start = Vec3::new(2.0, 5.0, 1.0);
        let grip = Vec3::new(0.2, 1.6, -0.4);
        for gain in [0.3, 0.8, 1.0] {
            let tuning = ClimbTuning { gain, ..tuning() };
            let mut body = start;
            let mut velocity = Vec3::ZERO;
            for step in 0..120 {
                let pulled = 0.3 * (step.min(10) as f32 / 10.0);
                let follow = Follow {
                    body: start,
                    grip,
                    palm: grip - Vec3::Y * pulled,
                };
                velocity = climb_velocity(body_target([follow]).unwrap(), body, &tuning);
                body += velocity * STEP;
            }
            assert!(velocity.length() < 1e-3, "gain {gain}: {velocity:?}");
            assert!(
                body.abs_diff_eq(start + Vec3::Y * 0.3, 1e-3),
                "gain {gain}: {body:?}"
            );
        }
    }

    #[test]
    fn the_climb_closes_its_gain_of_the_gap_within_the_speed_cap() {
        let half = ClimbTuning {
            gain: 0.5,
            ..tuning()
        };
        let velocity = climb_velocity(Vec3::Y * 0.1, Vec3::ZERO, &half);
        assert!((velocity.y * STEP - 0.05).abs() < 1e-5);
        let far = climb_velocity(Vec3::Y * 10.0, Vec3::ZERO, &tuning());
        assert!((far.length() - 6.0).abs() < 1e-4);
        let bad = ClimbTuning {
            step_s: 0.0,
            ..tuning()
        };
        assert_eq!(climb_velocity(Vec3::Y, Vec3::ZERO, &bad), Vec3::ZERO);
        assert_eq!(
            climb_velocity(Vec3::splat(f32::NAN), Vec3::ZERO, &tuning()),
            Vec3::ZERO
        );
    }

    #[test]
    fn the_fling_keeps_the_smoothed_speed_within_its_cap() {
        let mut smoothed = Vec3::ZERO;
        for _ in 0..60 {
            smoothed = smooth(smoothed, Vec3::Y * 3.0, STEP, 0.08);
        }
        assert!((smoothed.y - 3.0).abs() < 1e-3);
        assert!(fling(smoothed, 1.0, 8.0).abs_diff_eq(smoothed, 1e-6));
        assert!((fling(smoothed, 4.0, 8.0).length() - 8.0).abs() < 1e-4);
        assert_eq!(fling(Vec3::splat(f32::NAN), 1.0, 8.0), Vec3::ZERO);
        // One step of a sudden change moves the smoothed speed only part of the way.
        let once = smooth(Vec3::ZERO, Vec3::Y, STEP, 0.08);
        assert!(once.y > 0.0 && once.y < 1.0);
    }

    #[test]
    fn a_fling_is_carried_as_a_throw() {
        let kept = Vec3::new(1.0, 4.0, 0.0);
        assert_eq!(fling_carry(kept, 0.0, 9.81), kept);
        let later = fling_carry(kept, 0.2, 9.81);
        assert!((later.y - (4.0 - 1.962)).abs() < 1e-4 && later.x == 1.0);
        assert_eq!(fling_carry(Vec3::NAN, 0.1, 9.81), Vec3::ZERO);
        assert_eq!(tension(1.5, 3.0), 0.5);
        assert_eq!(tension(9.0, 3.0), 1.0);
        assert_eq!(tension(1.0, 0.0), 0.0);
    }

    #[test]
    fn the_mantle_starts_near_the_top_while_pulling_up_steadily() {
        let tuning = mantle_tuning();
        assert!(mantle_starts(&check(9.2, 1.0, 0.0, false), &tuning));
        // Too far below the top.
        assert!(!mantle_starts(&check(9.0, 1.0, 0.0, false), &tuning));
        // Not pulling up, or only twitching upward.
        assert!(!mantle_starts(&check(9.5, 0.0, 0.0, false), &tuning));
        assert!(!mantle_starts(&check(9.5, 0.1, 0.0, false), &tuning));
        // Still standing on what the climb started from, without having climbed.
        assert!(!mantle_starts(&check(9.5, 1.0, 0.1, true), &tuning));
        assert!(mantle_starts(&check(9.5, 1.0, 0.3, true), &tuning));
    }

    #[test]
    fn a_surface_near_a_standing_players_feet_is_no_ledge() {
        let tuning = mantle_tuning();
        // The floor, and a table top, gripped while standing.
        assert!(!is_ledge(1.0, 0.0, 0.0, true, 0.7, &tuning));
        assert!(!is_ledge(1.0, 0.8, 0.0, true, 0.7, &tuning));
        // A wall's top above the start height is a ledge, standing or hanging.
        assert!(is_ledge(1.0, 1.4, 0.0, true, 0.7, &tuning));
        assert!(is_ledge(1.0, 0.8, 0.0, false, 0.7, &tuning));
        // A wall's face is not a top.
        assert!(!is_ledge(0.1, 1.4, 0.0, false, 0.7, &tuning));
        assert!(!is_ledge(f32::NAN, 1.4, 0.0, false, 0.7, &tuning));
    }

    #[test]
    fn the_mantle_rises_then_goes_over() {
        let tuning = mantle_tuning();
        let top = Vec3::new(0.0, 10.0, 1.0);
        let feet = Vec3::new(0.0, 9.5, 0.6);
        let mantle = Mantle::start(top, feet).unwrap();
        assert!(mantle.over.abs_diff_eq(Vec2::Y, 1e-6));
        let rising = moving(mantle.motion(feet, &tuning));
        assert!(rising.y > 0.0 && rising.x == 0.0 && rising.z == 0.0);
        assert!(rising.y <= tuning.speed_mps + 1e-6);
        // Clear of the top, the body goes over it.
        let clear = Vec3::new(0.0, 10.08, 0.6);
        let over = moving(mantle.motion(clear, &tuning));
        assert!(over.z > 0.0 && over.y.abs() < 1e-4);
        let there = mantle.destination(&tuning);
        assert!(there.abs_diff_eq(Vec3::new(0.0, 10.08, 1.4), 1e-5));
        // Right under the top there is no way over.
        assert_eq!(Mantle::start(top, Vec3::new(0.0, 9.5, 1.0)), None);
    }

    #[test]
    fn the_mantle_lands_on_the_ledge_instead_of_hovering() {
        let tuning = mantle_tuning();
        let top = Vec3::new(0.0, 10.0, 1.0);
        let mantle = Mantle::start(top, Vec3::new(0.0, 9.5, 0.6)).unwrap();
        // Past the top by the capsule's radius, the feet are let down as they go on.
        let over = moving(mantle.motion(Vec3::new(0.0, 10.08, 1.32), &tuning));
        assert!(over.y < 0.0 && over.z > 0.0, "{over:?}");
        // At the destination they have arrived, whatever their height.
        assert_eq!(
            mantle.motion(Vec3::new(0.0, 10.08, 1.39), &tuning),
            MantleMotion::Arrived
        );
        // Simulated, a mantle that never finds support still arrives within its time.
        let mut feet = Vec3::new(0.0, 9.5, 0.6);
        let mut arrived = false;
        for _ in 0..90 {
            match mantle.motion(feet, &tuning) {
                MantleMotion::Move(v) => feet += v * STEP,
                MantleMotion::Arrived => {
                    arrived = true;
                    break;
                }
            }
        }
        assert!(arrived, "{feet:?}");
    }
}
