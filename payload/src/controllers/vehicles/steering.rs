//! How the held steering wheel turns, and how its angle maps to the vehicle's steering. Pure.
//!
//! Each holding hand reports how far it turned the wheel since the last tick
//! ([`WheelGrip::turn`]), by one of two measures ([`WheelTracking`]):
//!
//! - the hand's twist about the wheel's axis, as a hand clamped to a rim turns with it. It does not
//!   depend on where the hub is, so a wheel fitted a few centimetres off, or a hand that drifts
//!   toward the hub, still turns it truly;
//! - the hand's travel around the hub in the wheel's plane, weighted down as the hand nears the hub,
//!   where its angle is unstable.
//!
//! Only the change since the last tick counts, from where the hand took hold, so grabbing never
//! jumps the wheel, and the turns accumulate ([`advance`]): the wheel goes past half a turn either
//! way, a hand let go and taken again further round carries on from the wheel's angle (a
//! hand-over), and the wheel stops at its lock, as a car's does. Two hands turn it by the mean of
//! their turns.
//!
//! The angle over the lock, past a small deadzone and shaped by the response exponent, is the
//! steering ([`steer_for_angle`]); [`angle_for_steer`] is its inverse, for drawing the wheel at the
//! vehicle's own steering. Let go, the wheel returns to centre at the return rate, easing in over
//! the last stretch ([`return_step`]).

use std::f32::consts::{PI, TAU};

use glam::{Quat, Vec3};

use crate::controllers::vehicles::{
    config::{WheelSpec, WheelTracking},
    wheel::WheelFrame,
};

/// A hand on the wheel: where it was on the last tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WheelGrip {
    /// The hand's angle around the hub, clockwise from the top, `None` at the hub.
    angle: Option<f32>,
    /// The hand's orientation in the cockpit frame, `None` without one.
    orientation: Option<Quat>,
}

impl WheelGrip {
    /// A hand taking the wheel at `position` with `orientation`.
    pub fn new(frame: &WheelFrame, position: Vec3, orientation: Option<Quat>) -> Self {
        Self {
            angle: frame.hand_angle(position),
            orientation: orientation.filter(|q| q.is_finite()),
        }
    }

    /// How far the hand, now at `position` with `orientation`, turned the wheel since the last
    /// tick, radians clockwise, with the weight its measure carries, `0..=1`. Updates the hand's
    /// last place.
    pub fn turn(
        &mut self,
        frame: &WheelFrame,
        tracking: WheelTracking,
        position: Vec3,
        orientation: Option<Quat>,
    ) -> Option<Turn> {
        let now = Self::new(frame, position, orientation);
        let last = std::mem::replace(self, now);
        let by_twist = match (tracking, last.orientation, now.orientation) {
            (WheelTracking::Twist, Some(last), Some(now)) => {
                Some(clockwise_twist(now * last.inverse(), frame.normal()))
            }
            _ => None,
        };
        if let Some(turn) = by_twist {
            return Some(Turn {
                radians: turn,
                weight: 1.0,
            });
        }
        let (Some(last), Some(now)) = (last.angle, now.angle) else {
            return None;
        };
        Some(Turn {
            radians: wrap_angle(now - last),
            weight: position_weight(frame, position),
        })
    }
}

/// One hand's turn of the wheel over a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Turn {
    /// Radians clockwise.
    pub radians: f32,
    /// How much the turn counts against the other hand's, `0..=1`.
    pub weight: f32,
}

/// The holding hands' turns together: their weighted mean, `None` when no hand's turn counts.
pub fn combine(turns: impl IntoIterator<Item = Turn>) -> Option<f32> {
    let (sum, weights) = turns
        .into_iter()
        .filter(|t| t.radians.is_finite() && t.weight.is_finite() && t.weight > 0.0)
        .fold((0.0, 0.0), |(sum, weights), t| {
            (sum + t.radians * t.weight, weights + t.weight)
        });
    (weights > 1e-4).then(|| sum / weights)
}

/// The wheel at `angle` turned by `turn`, stopped at the lock either way.
pub fn advance(angle: f32, turn: f32, spec: &WheelSpec) -> f32 {
    let lock = lock(spec);
    let turned = angle + if turn.is_finite() { turn } else { 0.0 };
    turned.clamp(-lock, lock)
}

/// The released wheel at `angle` after `dt` seconds of returning to centre: at the return rate,
/// slowing over the last stretch so that it settles rather than stops.
pub fn return_step(angle: f32, spec: &WheelSpec, dt: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    let ease = (angle.abs() / RETURN_EASE_DEG.to_radians()).clamp(RETURN_EASE_FLOOR, 1.0);
    let step = spec.return_deg_per_s.to_radians().max(0.0) * ease * dt.max(0.0);
    let next = angle - angle.clamp(-step, step);
    if next.abs() < SETTLED_DEG.to_radians() {
        0.0
    } else {
        next
    }
}

/// The steering, `-1..=1` with positive right, the wheel at `angle` (clockwise) gives: past the
/// deadzone, the rest of the way to the lock shaped by the response exponent.
pub fn steer_for_angle(angle: f32, spec: &WheelSpec) -> f32 {
    let deadzone = deadzone(spec);
    let magnitude = angle.abs();
    if !magnitude.is_finite() || magnitude <= deadzone {
        return 0.0;
    }
    let share = ((magnitude - deadzone) / (lock(spec) - deadzone).max(1e-3)).min(1.0);
    angle.signum() * share.powf(response(spec))
}

/// The wheel angle that gives the steering `steer`: the inverse of [`steer_for_angle`].
pub fn angle_for_steer(steer: f32, spec: &WheelSpec) -> f32 {
    if !steer.is_finite() || steer == 0.0 {
        return 0.0;
    }
    let deadzone = deadzone(spec);
    let share = steer.abs().min(1.0).powf(1.0 / response(spec));
    steer.signum() * (deadzone + share * (lock(spec) - deadzone).max(0.0))
}

/// Over this last stretch of its return, the released wheel slows, degrees.
const RETURN_EASE_DEG: f32 = 30.0;

/// The slowest the easing lets the return go, as a share of the return rate.
const RETURN_EASE_FLOOR: f32 = 0.2;

/// A returning wheel this close to centre is at centre, degrees.
const SETTLED_DEG: f32 = 0.25;

/// A hand closer to the hub than this share of the radius turns nothing by its travel; one past
/// twice it counts fully.
const HUB_SHARE: f32 = 0.25;

fn lock(spec: &WheelSpec) -> f32 {
    spec.lock_deg.to_radians().abs().max(1e-3)
}

fn deadzone(spec: &WheelSpec) -> f32 {
    spec.deadzone_deg.to_radians().clamp(0.0, lock(spec) * 0.5)
}

fn response(spec: &WheelSpec) -> f32 {
    if spec.response.is_finite() {
        spec.response.clamp(0.2, 5.0)
    } else {
        1.0
    }
}

/// The rotation `delta`'s twist about `axis` (the wheel's face normal, toward the driver), radians
/// clockwise as the driver sees it.
fn clockwise_twist(delta: Quat, axis: Vec3) -> f32 {
    let delta = if delta.w < 0.0 { -delta } else { delta };
    let along = Vec3::new(delta.x, delta.y, delta.z).dot(axis);
    let twist = 2.0 * along.atan2(delta.w);
    // Seen from the driver, along -axis, a positive rotation about the axis runs anticlockwise.
    if twist.is_finite() {
        -wrap_angle(twist)
    } else {
        0.0
    }
}

/// How much a hand at `position` counts by its travel around the hub: nothing near the hub, fully
/// from twice [`HUB_SHARE`] of the radius out.
fn position_weight(frame: &WheelFrame, position: Vec3) -> f32 {
    let d = position - frame.centre;
    let r = Vec3::new(d.dot(frame.right), d.dot(frame.up), 0.0).length();
    let share = r / frame.radius.max(0.05);
    ((share - HUB_SHARE) / HUB_SHARE).clamp(0.0, 1.0)
}

/// Wrap an angle difference into `-π..=π`.
fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;
    use crate::controllers::vehicles::config::PhysicalControlsConfig;

    fn spec() -> WheelSpec {
        PhysicalControlsConfig::new().wheel
    }

    fn frame() -> WheelFrame {
        WheelFrame::from_spec(&spec())
    }

    /// A hand's orientation turned clockwise (as the driver sees it) by `angle` about the wheel's
    /// axis from `base`.
    fn turned(frame: &WheelFrame, base: Quat, angle: f32) -> Quat {
        Quat::from_axis_angle(frame.normal(), -angle) * base
    }

    #[test]
    fn a_twist_about_the_axis_turns_the_wheel_by_as_much() {
        let frame = frame();
        let base = Quat::from_rotation_x(-0.4);
        let at = frame.rim_point(-FRAC_PI_2);
        let mut grip = WheelGrip::new(&frame, at, Some(base));
        let turn = grip
            .turn(
                &frame,
                WheelTracking::Twist,
                at,
                Some(turned(&frame, base, 0.3)),
            )
            .unwrap();
        assert!((turn.radians - 0.3).abs() < 1e-4, "{turn:?}");
        assert_eq!(turn.weight, 1.0);
    }

    #[test]
    fn a_twist_ignores_the_hand_s_place_and_a_swing_off_the_axis() {
        let frame = frame();
        let base = Quat::IDENTITY;
        let mut grip = WheelGrip::new(&frame, frame.centre, Some(base));
        // The hand sits on the hub and swings about the wheel's right axis: no turn.
        let swung = Quat::from_axis_angle(frame.right, 0.5) * base;
        let turn = grip
            .turn(
                &frame,
                WheelTracking::Twist,
                frame.centre + frame.up * 0.01,
                Some(swung),
            )
            .unwrap();
        assert!(turn.radians.abs() < 1e-4, "{turn:?}");
    }

    #[test]
    fn travel_around_the_hub_turns_the_wheel_and_counts_less_near_it() {
        let frame = frame();
        let mut grip = WheelGrip::new(&frame, frame.rim_point(0.0), None);
        let turn = grip
            .turn(&frame, WheelTracking::Position, frame.rim_point(0.2), None)
            .unwrap();
        assert!((turn.radians - 0.2).abs() < 1e-4 && turn.weight == 1.0);
        let near = frame.centre + (frame.rim_point(0.4) - frame.centre) * 0.3;
        let turn = grip
            .turn(&frame, WheelTracking::Position, near, None)
            .unwrap();
        assert!(turn.weight < 0.5, "{turn:?}");
        // Without an orientation, the twist falls back to the travel.
        let mut grip = WheelGrip::new(&frame, frame.rim_point(0.0), None);
        let turn = grip
            .turn(&frame, WheelTracking::Twist, frame.rim_point(0.1), None)
            .unwrap();
        assert!((turn.radians - 0.1).abs() < 1e-4);
    }

    #[test]
    fn turns_accumulate_past_half_a_turn_and_stop_at_the_lock() {
        let mut spec = spec();
        spec.lock_deg = 270.0;
        let frame = frame();
        let mut grip = WheelGrip::new(&frame, frame.rim_point(0.0), Some(Quat::IDENTITY));
        let mut angle = 0.0;
        let mut orientation = Quat::IDENTITY;
        for _ in 0..40 {
            orientation = turned(&frame, orientation, 0.1);
            let turn = grip
                .turn(
                    &frame,
                    WheelTracking::Twist,
                    frame.rim_point(0.0),
                    Some(orientation),
                )
                .unwrap();
            angle = advance(angle, turn.radians, &spec);
        }
        assert!((angle - 4.0).abs() < 1e-3, "{}", angle.to_degrees());
        for _ in 0..20 {
            angle = advance(angle, 0.1, &spec);
        }
        assert!((angle - 270f32.to_radians()).abs() < 1e-5);
        // Turning back unwinds straight away from the stop.
        assert!(advance(angle, -0.1, &spec) < angle);
    }

    #[test]
    fn a_hand_over_carries_on_from_the_wheel_s_angle() {
        let spec = spec();
        let frame = frame();
        let mut angle = 0.0;
        // The right hand turns the wheel a quarter turn.
        let mut grip = WheelGrip::new(&frame, frame.rim_point(FRAC_PI_2), Some(Quat::IDENTITY));
        let turn = grip
            .turn(
                &frame,
                WheelTracking::Twist,
                frame.rim_point(FRAC_PI_2),
                Some(turned(&frame, Quat::IDENTITY, FRAC_PI_2)),
            )
            .unwrap();
        angle = advance(angle, turn.radians, &spec);
        // It lets go and takes the wheel again with its hand turned back: no jump.
        let mut grip = WheelGrip::new(&frame, frame.rim_point(0.0), Some(Quat::IDENTITY));
        let still = grip
            .turn(
                &frame,
                WheelTracking::Twist,
                frame.rim_point(0.0),
                Some(Quat::IDENTITY),
            )
            .unwrap();
        angle = advance(angle, still.radians, &spec);
        assert!((angle - FRAC_PI_2).abs() < 1e-4);
        let more = grip
            .turn(
                &frame,
                WheelTracking::Twist,
                frame.rim_point(0.0),
                Some(turned(&frame, Quat::IDENTITY, 0.5)),
            )
            .unwrap();
        angle = advance(angle, more.radians, &spec);
        assert!((angle - (FRAC_PI_2 + 0.5)).abs() < 1e-4);
    }

    #[test]
    fn two_hands_turn_the_wheel_by_their_weighted_mean() {
        let one = Turn {
            radians: 0.2,
            weight: 1.0,
        };
        let other = Turn {
            radians: 0.0,
            weight: 1.0,
        };
        assert!((combine([one, other]).unwrap() - 0.1).abs() < 1e-6);
        let faint = Turn {
            radians: 1.0,
            weight: 0.0,
        };
        assert_eq!(combine([one, faint]), Some(0.2));
        assert_eq!(combine([faint]), None);
    }

    #[test]
    fn the_lock_steers_fully_past_a_deadzone_and_the_inverse_round_trips() {
        let mut spec = spec();
        let lock = spec.lock_deg.to_radians();
        assert_eq!(steer_for_angle(lock, &spec), 1.0);
        assert_eq!(steer_for_angle(-2.0 * lock, &spec), -1.0);
        assert_eq!(
            steer_for_angle(spec.deadzone_deg.to_radians() * 0.5, &spec),
            0.0
        );
        for response in [1.0, 1.6] {
            spec.response = response;
            for steer in [-0.8, -0.3, 0.05, 0.5, 1.0] {
                let back = steer_for_angle(angle_for_steer(steer, &spec), &spec);
                assert!((back - steer).abs() < 1e-4, "{response} {steer} {back}");
            }
        }
        // A response above one steers less than linearly near centre.
        let half = angle_for_steer(0.5, &PhysicalControlsConfig::new().wheel);
        assert!(steer_for_angle(half, &spec) < 0.5);
    }

    #[test]
    fn the_released_wheel_returns_to_centre_and_settles() {
        let spec = spec();
        let mut angle = 120f32.to_radians();
        let mut last = angle;
        let mut ticks = 0;
        while angle != 0.0 {
            angle = return_step(angle, &spec, 1.0 / 30.0);
            assert!(angle >= 0.0 && angle < last);
            last = angle;
            ticks += 1;
            assert!(ticks < 300);
        }
        assert_eq!(return_step(-0.001, &spec, 1.0 / 30.0), 0.0);
        assert_eq!(return_step(f32::NAN, &spec, 1.0 / 30.0), 0.0);
    }
}
