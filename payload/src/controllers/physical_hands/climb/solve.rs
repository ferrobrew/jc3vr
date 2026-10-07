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

use glam::{Vec2, Vec3};

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

/// The velocity a release keeps: `scale` of the smoothed climbing velocity, capped at `max_speed`.
pub fn fling(smoothed: Vec3, scale: f32, max_speed: f32) -> Vec3 {
    if !smoothed.is_finite() || !scale.is_finite() {
        return Vec3::ZERO;
    }
    (smoothed * scale.max(0.0)).clamp_length_max(max_speed.max(0.0))
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
    pub step_s: f32,
}

/// Whether the mantle starts: the feet at `feet_y` have come within `start_m` below the gripped
/// top at `top_y`, while the hands pull the body up.
pub fn mantle_starts(feet_y: f32, top_y: f32, pulling_up: bool, tuning: &MantleTuning) -> bool {
    pulling_up && feet_y.is_finite() && top_y.is_finite() && feet_y >= top_y - tuning.start_m
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

    /// The velocity that carries feet at `feet` over the ledge: straight up until they clear the
    /// top, then over it at that height. Each part goes no faster than the mantle's speed, nor
    /// further in a step than it has left.
    pub fn velocity(&self, feet: Vec3, tuning: &MantleTuning) -> Vec3 {
        if !(feet.is_finite() && tuning.step_s.is_finite() && tuning.step_s > 0.0) {
            return Vec3::ZERO;
        }
        let destination = self.destination(tuning);
        let speed = tuning.speed_mps.max(0.0);
        let rise = ((destination.y - feet.y) / tuning.step_s).clamp(-speed, speed);
        if feet.y < destination.y - tuning.clearance_m * 0.5 {
            return Vec3::new(0.0, rise, 0.0);
        }
        let left = Vec2::new(destination.x - feet.x, destination.z - feet.z);
        let across = (left / tuning.step_s).clamp_length_max(speed);
        Vec3::new(across.x, rise, across.y)
    }
}

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
            step_s: STEP,
        }
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
    fn the_mantle_starts_near_the_top_while_pulling_up() {
        let tuning = mantle_tuning();
        assert!(mantle_starts(9.2, 10.0, true, &tuning));
        assert!(!mantle_starts(9.0, 10.0, true, &tuning));
        assert!(!mantle_starts(9.5, 10.0, false, &tuning));
    }

    #[test]
    fn the_mantle_rises_then_goes_over() {
        let tuning = mantle_tuning();
        let top = Vec3::new(0.0, 10.0, 1.0);
        let feet = Vec3::new(0.0, 9.5, 0.6);
        let mantle = Mantle::start(top, feet).unwrap();
        assert!(mantle.over.abs_diff_eq(Vec2::Y, 1e-6));
        let rising = mantle.velocity(feet, &tuning);
        assert!(rising.y > 0.0 && rising.x == 0.0 && rising.z == 0.0);
        assert!(rising.y <= tuning.speed_mps + 1e-6);
        // Clear of the top, the body goes over it.
        let clear = Vec3::new(0.0, 10.08, 0.6);
        let over = mantle.velocity(clear, &tuning);
        assert!(over.z > 0.0 && over.y.abs() < 1e-4);
        let there = mantle.destination(&tuning);
        assert!(there.abs_diff_eq(Vec3::new(0.0, 10.08, 1.4), 1e-5));
        // Right under the top there is no way over.
        assert_eq!(Mantle::start(top, Vec3::new(0.0, 9.5, 1.0)), None);
    }
}
