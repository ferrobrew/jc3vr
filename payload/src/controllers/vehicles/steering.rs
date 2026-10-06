//! How the held steering wheel turns, and how its angle maps to the vehicle's steering. Pure.
//!
//! Each holding hand reports how far it turned the wheel since the last tick
//! ([`WheelGrip::turn`]), by one of two measures ([`WheelTracking`]):
//!
//! - the hand's travel around the hub: the angle of its grip, projected into the wheel's plane,
//!   about the hub. This is the default, because it is what moves when a player turns a wheel with
//!   their arms. Near the hub that angle is unstable, so the hand's twist about the wheel's axis
//!   stands in for it there, blended in as the hand nears the hub;
//! - the hand's twist about the wheel's axis alone, as a hand clamped to a rim turns with it. It does
//!   not depend on where the hub is, so a wheel fitted a few centimetres off still turns truly, but
//!   it reads only the wrist's roll: arms that carry the hands round the rim with the wrists held
//!   still turn it barely at all.
//!
//! Only the change since the last tick counts, from where the hand took hold, so grabbing never
//! jumps the wheel, and the turns accumulate ([`advance`]): each tick's change is unwrapped, so the
//! wheel goes past half a turn either way, a hand let go and taken again further round carries on
//! from the wheel's angle (a hand-over), and the wheel stops at its lock, as a car's does.
//!
//! Two hands turn it as one rigid wheel would ([`combine`]): by the turn of the hand that turned it
//! furthest, held back by the other only when that one turns the other way. A relaxed hand that
//! lags behind therefore neither halves the turn nor stops it, and two hands pulling against each
//! other cancel.
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
        let twist = match (last.orientation, now.orientation) {
            (Some(last), Some(now)) => Some(clockwise_twist(now * last.inverse(), frame.normal())),
            _ => None,
        };
        let travel = match (last.angle, now.angle) {
            (Some(last), Some(now)) => {
                Some((wrap_angle(now - last), position_weight(frame, position)))
            }
            _ => None,
        };
        match (tracking, travel, twist) {
            (WheelTracking::Twist, _, Some(twist)) => Some(Turn {
                radians: twist,
                weight: 1.0,
            }),
            (WheelTracking::Position, Some((travel, weight)), Some(twist)) => Some(Turn {
                radians: travel * weight + twist * (1.0 - weight),
                weight: 1.0,
            }),
            (_, Some((travel, weight)), _) => Some(Turn {
                radians: travel,
                weight,
            }),
            (WheelTracking::Position, None, Some(twist)) => Some(Turn {
                radians: twist,
                weight: 1.0,
            }),
            (_, None, None) => None,
        }
    }
}

/// One hand's turn of the wheel over a tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Turn {
    /// Radians clockwise.
    pub radians: f32,
    /// How much of the turn counts, `0..=1`: less for a hand near the hub whose travel alone
    /// measures it.
    pub weight: f32,
}

/// The holding hands' turns together, as a rigid wheel held by both would turn: each turn counts by
/// its weight, the furthest-turning hand leads, and the other adds to it only when it turns the
/// other way, so that it holds the wheel back by as much. `None` when no hand's turn counts.
///
/// A mean of the two would halve the turn whenever one hand rests on the rim while the other turns
/// it, which is how a player often drives; taking the leader keeps the wheel with the hand that
/// moves it. A hand's jitter while both rest has no preferred sign, so the wheel does not drift.
pub fn combine(turns: [Option<Turn>; 2]) -> Option<f32> {
    let [one, other] = turns.map(|turn| {
        turn.filter(|t| t.radians.is_finite() && t.weight.is_finite() && t.weight > 0.0)
            .map(|t| t.radians * t.weight.min(1.0))
    });
    match (one, other) {
        (Some(one), Some(other)) => {
            let (lead, follow) = if one.abs() >= other.abs() {
                (one, other)
            } else {
                (other, one)
            };
            Some(if lead * follow < 0.0 {
                lead + follow
            } else {
                lead
            })
        }
        (one, other) => one.or(other),
    }
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

    fn turn(radians: f32) -> Option<Turn> {
        Some(Turn {
            radians,
            weight: 1.0,
        })
    }

    #[test]
    fn two_hands_turn_the_wheel_by_the_leading_hand() {
        // One hand turns, the other rests on the rim: the wheel goes with the one that turns.
        assert_eq!(combine([turn(0.2), turn(0.0)]), Some(0.2));
        assert_eq!(combine([turn(0.0), turn(-0.2)]), Some(-0.2));
        // Both turn the same way, one lagging: still the leader's.
        assert_eq!(combine([turn(0.1), turn(0.25)]), Some(0.25));
        // Pulling against each other holds the wheel back by the opposing hand's turn.
        assert_eq!(combine([turn(0.2), turn(-0.2)]), Some(0.0));
        assert!((combine([turn(0.2), turn(-0.05)]).unwrap() - 0.15).abs() < 1e-6);
        // A turn counts by its weight, and one with none, or not a number, not at all.
        let faint = Some(Turn {
            radians: 1.0,
            weight: 0.0,
        });
        assert_eq!(combine([turn(0.2), faint]), Some(0.2));
        assert_eq!(combine([faint, None]), None);
        assert_eq!(combine([turn(f32::NAN), turn(0.1)]), Some(0.1));
        let half = Some(Turn {
            radians: 0.4,
            weight: 0.5,
        });
        assert_eq!(combine([half, None]), Some(0.2));
    }

    #[test]
    fn by_default_the_hands_travel_round_the_rim_turns_the_wheel_with_the_wrists_still() {
        // The arms carry both hands round the rim while the wrists keep their orientation: the
        // twist alone reads nothing, the travel the whole turn.
        let spec = spec();
        assert_eq!(spec.turned_by, WheelTracking::Position);
        let frame = frame();
        let wrist = Some(Quat::from_rotation_x(-0.4));
        let start = [-FRAC_PI_2, FRAC_PI_2];
        for (tracking, expected) in [(WheelTracking::Position, 0.6), (WheelTracking::Twist, 0.0)] {
            let mut grips = start.map(|a| WheelGrip::new(&frame, frame.rim_point(a), wrist));
            let mut angle = 0.0;
            for step in 1..=6 {
                let turns = [0, 1].map(|i| {
                    let at = frame.rim_point(start[i] + 0.1 * step as f32);
                    grips[i].turn(&frame, tracking, at, wrist)
                });
                angle = advance(angle, combine(turns).unwrap(), &spec);
            }
            assert!((angle - expected).abs() < 1e-4, "{tracking:?} {angle}");
        }
    }

    #[test]
    fn the_travel_unwraps_past_half_a_turn() {
        let mut spec = spec();
        spec.lock_deg = 540.0;
        let frame = frame();
        let mut grip = WheelGrip::new(&frame, frame.rim_point(0.0), None);
        let mut angle = 0.0;
        // A turn and a quarter clockwise, crossing the bottom of the rim where the hand's angle
        // wraps from a half turn to minus a half turn.
        for step in 1..=50 {
            let at = frame.rim_point(step as f32 * TAU * 1.25 / 50.0);
            let turn = grip.turn(&frame, WheelTracking::Position, at, None);
            angle = advance(angle, combine([turn, None]).unwrap(), &spec);
        }
        assert!((angle - TAU * 1.25).abs() < 1e-3, "{}", angle.to_degrees());
    }

    #[test]
    fn near_the_hub_the_twist_stands_in_for_the_travel() {
        let frame = frame();
        let base = Quat::IDENTITY;
        let at = frame.centre + frame.up * 0.01;
        let mut grip = WheelGrip::new(&frame, at, Some(base));
        // The hand swings across the hub, where its angle jumps, while its wrist turns a little.
        let across = frame.centre - frame.up * 0.01;
        let turn = grip
            .turn(
                &frame,
                WheelTracking::Position,
                across,
                Some(turned(&frame, base, 0.1)),
            )
            .unwrap();
        assert!((turn.radians - 0.1).abs() < 1e-4, "{turn:?}");
        assert_eq!(turn.weight, 1.0);
    }

    #[test]
    fn the_lock_steers_fully_past_a_deadzone_and_the_inverse_round_trips() {
        let mut spec = spec();
        // The default is linear: halfway to the lock past the deadzone steers half.
        let deadzone = spec.deadzone_deg.to_radians();
        let halfway = deadzone + (spec.lock_deg.to_radians() - deadzone) * 0.5;
        assert!((steer_for_angle(halfway, &spec) - 0.5).abs() < 1e-5);
        assert!((steer_for_angle(-halfway, &spec) + 0.5).abs() < 1e-5);
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
