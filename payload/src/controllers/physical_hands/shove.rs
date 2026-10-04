//! A palm's soft contact with a loose body, pure: the impulse that shoves it each tick the palm
//! presses into it, and how much it gives.
//!
//! The contact is a velocity-level spring: each tick the palm presses into a dynamic body, the
//! body's material point under the palm is asked to move into the body at least as fast as the
//! hand does, plus a speed per metre of press past a deadband, so that a slow push slides a crate,
//! rolls a barrel, and tips what stands on a narrow base. The impulse that asks for it is the
//! point's mass along the normal ([`RigidBody::mass_along`]) times the speed it lacks, capped by
//! the hand's strength over the tick, so a heavy body takes a strong push slowly. The palm drags
//! the surface along with it by friction, up to the friction coefficient times the normal impulse.
//!
//! How much a body gives ([`Give`]) is measured, not assumed: the speed the point reached by the
//! next tick against the speed the last tick asked of it. A crate against a wall, or a car on its
//! brakes, gives nothing, however light the shove that could move it in the open; the share it
//! does not give is the resistance, which pushes the body away as a wall does.

use glam::Vec3;

use crate::controllers::physical_hands::{config::ShoveConfig, rigid::RigidBody};

/// The contact's tuning.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShoveTuning {
    /// The speed into the body per metre of press past the deadband, per second.
    pub stiffness_per_s: f32,
    pub deadband_m: f32,
    /// The fastest the contact asks the point to move into the body, metres per second.
    pub max_speed_mps: f32,
    /// The strongest push a hand makes, newtons.
    pub max_force_n: f32,
    /// The friction coefficient between the palm and the surface.
    pub friction: f32,
}

impl ShoveTuning {
    pub fn of(config: &ShoveConfig) -> Self {
        Self {
            stiffness_per_s: config.stiffness_per_s,
            deadband_m: config.deadband_m,
            max_speed_mps: config.max_speed_mps,
            max_force_n: config.max_force_n,
            friction: config.friction,
        }
    }
}

/// One tick's shove of a body.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Shove {
    /// The impulse to apply at the contact point, world space, newton seconds.
    pub impulse: Vec3,
    /// The share of the hand's strongest push the normal impulse used, `0..=1`.
    pub load: f32,
    /// The share of the wanted normal impulse the strength cap held back, `0..=1`.
    pub saturation: f32,
    /// The point's speed into the body asked for, metres per second.
    pub asked_mps: f32,
    /// The point's speed into the body before the shove, metres per second.
    pub speed_mps: f32,
}

/// The shove of `body` by a palm pressed `depth` metres into it at `at`, where its surface normal
/// (toward the hand) is `normal`, with the palm moving at `hand_velocity`, over a tick of `dt`
/// seconds.
pub fn shove(
    body: &RigidBody,
    at: Vec3,
    normal: Vec3,
    depth: f32,
    hand_velocity: Vec3,
    dt: f32,
    tuning: &ShoveTuning,
) -> Shove {
    let (Some(into), true) = (
        (-normal).try_normalize(),
        dt.is_finite() && dt > 0.0 && at.is_finite() && depth.is_finite(),
    ) else {
        return Shove::default();
    };
    let hand_velocity = if hand_velocity.is_finite() {
        hand_velocity
    } else {
        Vec3::ZERO
    };
    let point_velocity = body.point_velocity(at);
    let speed = point_velocity.dot(into);
    let press = (depth - tuning.deadband_m.max(0.0)).max(0.0);
    let asked = (hand_velocity.dot(into).max(0.0) + press * tuning.stiffness_per_s.max(0.0))
        .min(tuning.max_speed_mps.max(0.0));
    let lacking = asked - speed;
    let cap = tuning.max_force_n.max(0.0) * dt;
    let normal_mass = body.mass_along(at, into);
    if lacking <= 0.0 || normal_mass <= 0.0 || cap <= 0.0 {
        return Shove {
            asked_mps: asked,
            speed_mps: speed,
            saturation: if normal_mass <= 0.0 && lacking > 0.0 {
                1.0
            } else {
                0.0
            },
            ..Shove::default()
        };
    }
    let wanted = normal_mass * lacking;
    let normal_impulse = wanted.min(cap);

    // Friction: the surface is dragged toward the palm's own sliding motion, never past it.
    let slip = hand_velocity - point_velocity;
    let slip = slip - into * slip.dot(into);
    let friction = slip.try_normalize().map_or(Vec3::ZERO, |direction| {
        let mass = body.mass_along(at, direction);
        let wanted = mass * slip.length();
        direction * wanted.min(tuning.friction.max(0.0) * normal_impulse)
    });
    let impulse = into * normal_impulse + friction;
    Shove {
        impulse: if impulse.is_finite() {
            impulse
        } else {
            Vec3::ZERO
        },
        load: (normal_impulse / cap).clamp(0.0, 1.0),
        saturation: (1.0 - normal_impulse / wanted).clamp(0.0, 1.0),
        asked_mps: asked,
        speed_mps: speed,
    }
}

/// How much one palm's pressed body gives, across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Give {
    /// The body pressed on the last tick, and the speed into it asked of its point then.
    last: Option<(u32, f32)>,
    resistance: f32,
}

impl Give {
    pub const fn new() -> Self {
        Self {
            last: None,
            resistance: 0.0,
        }
    }

    /// Forget the body: the palm is in the open.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Fold a tick's shove of the body `body`, over `dt` seconds, smoothed over `smoothing_s`,
    /// and return the resistance, `0..=1`: one for a body that gives nothing.
    pub fn update(&mut self, body: u32, shove: &Shove, dt: f32, smoothing_s: f32) -> f32 {
        let measured = match self.last {
            Some((last, asked)) if last == body && asked > MIN_ASKED_MPS => {
                Some((1.0 - shove.speed_mps / asked).clamp(0.0, 1.0))
            }
            _ => None,
        };
        let raw = measured.map_or(shove.saturation, |m| m.max(shove.saturation));
        let share = if smoothing_s > 0.0 && dt.is_finite() && dt > 0.0 {
            1.0 - (-dt / smoothing_s).exp()
        } else {
            1.0
        };
        self.resistance = if self.last.is_some_and(|(last, _)| last == body) {
            self.resistance + (raw - self.resistance) * share
        } else {
            raw
        };
        if !self.resistance.is_finite() {
            self.resistance = 1.0;
        }
        self.last = Some((body, shove.asked_mps));
        self.resistance
    }
}

/// The slowest asked speed whose outcome measures the give, metres per second: below it, a body
/// at rest says nothing about how freely it moves.
const MIN_ASKED_MPS: f32 = 0.05;

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::physical_hands::rigid::tests::crate_body;

    const DT: f32 = 1.0 / 60.0;
    const TUNING: ShoveTuning = ShoveTuning {
        stiffness_per_s: 10.0,
        deadband_m: 0.005,
        max_speed_mps: 4.0,
        max_force_n: 600.0,
        friction: 0.5,
    };

    #[test]
    fn a_slow_press_moves_a_light_crate_at_the_hands_speed() {
        let body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        // The palm on the crate's -X face, pushing +X at a walking pace.
        let at = Vec3::new(-0.25, 0.0, 0.0);
        let s = shove(
            &body,
            at,
            Vec3::NEG_X,
            0.005,
            Vec3::new(0.3, 0.0, 0.0),
            DT,
            &TUNING,
        );
        let mut moved = body;
        moved.apply_impulse(s.impulse, at);
        assert!((moved.point_velocity(at).x - 0.3).abs() < 1e-3);
        assert!(s.saturation < 1e-6 && s.load < 1.0);
    }

    #[test]
    fn a_deeper_press_asks_more() {
        let body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        let at = Vec3::new(-0.25, 0.0, 0.0);
        let shallow = shove(&body, at, Vec3::NEG_X, 0.01, Vec3::ZERO, DT, &TUNING);
        let deep = shove(&body, at, Vec3::NEG_X, 0.05, Vec3::ZERO, DT, &TUNING);
        assert!(deep.asked_mps > shallow.asked_mps && shallow.asked_mps > 0.0);
        // Inside the deadband and still, nothing.
        let resting = shove(&body, at, Vec3::NEG_X, 0.004, Vec3::ZERO, DT, &TUNING);
        assert_eq!(resting.impulse, Vec3::ZERO);
    }

    #[test]
    fn a_heavy_body_takes_the_capped_push_slowly() {
        let car = crate_body(1500.0, Vec3::new(2.0, 1.5, 4.5), Vec3::ZERO, Quat::IDENTITY);
        let at = Vec3::new(0.0, 0.0, 2.25);
        let s = shove(
            &car,
            at,
            Vec3::Z,
            0.05,
            Vec3::new(0.0, 0.0, -1.0),
            DT,
            &TUNING,
        );
        assert!((s.impulse.length() - TUNING.max_force_n * DT).abs() < 1e-3);
        assert_eq!(s.load, 1.0);
        assert!(s.saturation > 0.9);
        let mut moved = car;
        moved.apply_impulse(s.impulse, at);
        assert!(moved.velocities.linear.length() < 0.01);
    }

    #[test]
    fn an_off_centre_push_tips_the_body() {
        let tall = crate_body(4.0, Vec3::new(0.3, 1.5, 0.3), Vec3::ZERO, Quat::IDENTITY);
        let at = Vec3::new(-0.15, 0.6, 0.0);
        let s = shove(
            &tall,
            at,
            Vec3::NEG_X,
            0.02,
            Vec3::new(0.5, 0.0, 0.0),
            DT,
            &TUNING,
        );
        let mut moved = tall;
        moved.apply_impulse(s.impulse, at);
        // Pushed +X above the centre, it turns about -Z.
        assert!(moved.velocities.angular.z < -0.1);
    }

    #[test]
    fn a_body_already_moving_away_is_not_pushed() {
        let mut body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        body.velocities.linear = Vec3::new(2.0, 0.0, 0.0);
        let s = shove(
            &body,
            Vec3::new(-0.25, 0.0, 0.0),
            Vec3::NEG_X,
            0.02,
            Vec3::new(0.5, 0.0, 0.0),
            DT,
            &TUNING,
        );
        assert_eq!(s.impulse, Vec3::ZERO);
        assert_eq!(s.saturation, 0.0);
    }

    #[test]
    fn friction_drags_the_surface_but_is_bounded_by_the_press() {
        let body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        let at = Vec3::new(0.0, 0.25, 0.0);
        // Pressing down on the lid while sliding along +X.
        let s = shove(
            &body,
            at,
            Vec3::Y,
            0.02,
            Vec3::new(1.0, -0.1, 0.0),
            DT,
            &TUNING,
        );
        let normal = -s.impulse.y;
        assert!(s.impulse.x > 0.0);
        assert!(s.impulse.x <= TUNING.friction * normal + 1e-5);
    }

    #[test]
    fn a_body_that_does_not_move_resists() {
        let body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        let at = Vec3::new(-0.25, 0.0, 0.0);
        let mut give = Give::new();
        let press = |speed: f32| {
            let mut b = body;
            b.velocities.linear.x = speed;
            shove(
                &b,
                at,
                Vec3::NEG_X,
                0.03,
                Vec3::new(0.3, 0.0, 0.0),
                DT,
                &TUNING,
            )
        };
        // In the open the crate keeps up with what was asked: little resistance.
        let mut free = Give::new();
        let asked = press(0.0).asked_mps;
        free.update(1, &press(0.0), DT, 0.0);
        assert!(free.update(1, &press(asked), DT, 0.0) < 0.05);
        // Against a wall it never moves: full resistance.
        give.update(1, &press(0.0), DT, 0.0);
        assert!(give.update(1, &press(0.0), DT, 0.0) > 0.99);
        // Another body starts afresh.
        assert!(give.update(2, &press(0.0), DT, 0.0) < 0.01);
    }

    #[test]
    fn the_resistance_is_smoothed() {
        let body = crate_body(5.0, Vec3::splat(0.5), Vec3::ZERO, Quat::IDENTITY);
        let s = shove(
            &body,
            Vec3::new(-0.25, 0.0, 0.0),
            Vec3::NEG_X,
            0.03,
            Vec3::new(0.3, 0.0, 0.0),
            DT,
            &TUNING,
        );
        let mut give = Give::new();
        give.update(1, &s, DT, 0.1);
        let once = give.update(1, &s, DT, 0.1);
        assert!(once > 0.0 && once < 0.5);
        give.reset();
        assert_eq!(give, Give::new());
    }
}
