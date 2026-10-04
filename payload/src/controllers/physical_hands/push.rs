//! The push off surfaces, pure: the body's velocity away from what the palms press into.
//!
//! A palm pressed past the deadband into a surface whose normal is mostly horizontal pushes the
//! body along that normal, faster the deeper the press, up to the fastest push. As the body moves
//! away the press shallows, so the push settles where the palm rests on the surface: a spring with
//! no overshoot while the stiffness times the tick is below one.

use glam::{Vec2, Vec3};

use crate::controllers::physical_hands::config::PhysicalHandsConfig;

/// The horizontal push one palm asks for, world `(x, z)` metres per second, from a press `depth`
/// metres into a surface with the world `normal`.
pub fn hand_push(normal: Vec3, depth: f32, config: &PhysicalHandsConfig) -> Vec2 {
    let Some(normal) = normal.try_normalize() else {
        return Vec2::ZERO;
    };
    let horizontal = Vec2::new(normal.x, normal.z);
    if horizontal.length() < config.min_push_horizontal.max(1e-3) {
        return Vec2::ZERO;
    }
    let press = depth - config.push_deadband_m.max(0.0);
    if press.is_nan() || press <= 0.0 {
        return Vec2::ZERO;
    }
    let speed =
        (press * config.push_stiffness_per_s.max(0.0)).min(config.max_push_speed_mps.max(0.0));
    horizontal.normalize() * speed
}

/// Both palms' pushes together, no faster than the fastest push.
pub fn combine(pushes: [Vec2; 2], config: &PhysicalHandsConfig) -> Vec2 {
    let sum = pushes[0] + pushes[1];
    let max = config.max_push_speed_mps.max(0.0);
    if !sum.is_finite() {
        return Vec2::ZERO;
    }
    sum.clamp_length_max(max)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: PhysicalHandsConfig = PhysicalHandsConfig::new();

    #[test]
    fn a_press_into_a_wall_pushes_the_body_away() {
        // A wall ahead (-Z), its normal toward the player.
        let push = hand_push(Vec3::Z, 0.1, &CONFIG);
        let expected = (0.1 - CONFIG.push_deadband_m) * CONFIG.push_stiffness_per_s;
        assert!((push - Vec2::new(0.0, expected)).length() < 1e-5);
    }

    #[test]
    fn a_light_touch_does_not_push() {
        assert_eq!(
            hand_push(Vec3::Z, CONFIG.push_deadband_m * 0.5, &CONFIG),
            Vec2::ZERO
        );
        assert_eq!(hand_push(Vec3::Z, f32::NAN, &CONFIG), Vec2::ZERO);
    }

    #[test]
    fn pressing_down_on_the_floor_does_not_push() {
        assert_eq!(hand_push(Vec3::Y, 0.3, &CONFIG), Vec2::ZERO);
        // A slope steep enough pushes along its horizontal share.
        let push = hand_push(Vec3::new(1.0, 1.0, 0.0), 0.3, &CONFIG);
        assert!(push.x > 0.0 && push.y.abs() < 1e-6);
    }

    #[test]
    fn the_push_never_exceeds_the_fastest() {
        let deep = hand_push(Vec3::Z, 10.0, &CONFIG);
        assert!((deep.length() - CONFIG.max_push_speed_mps).abs() < 1e-5);
        let both = combine([deep, deep], &CONFIG);
        assert!((both.length() - CONFIG.max_push_speed_mps).abs() < 1e-5);
        // Opposed palms cancel.
        assert_eq!(combine([deep, -deep], &CONFIG), Vec2::ZERO);
    }

    #[test]
    fn the_spring_settles_without_overshoot() {
        let dt = 1.0 / 30.0;
        let mut depth = 0.3;
        let mut last = depth;
        for _ in 0..120 {
            let push = hand_push(Vec3::Z, depth, &CONFIG);
            depth -= push.length() * dt;
            assert!(depth >= CONFIG.push_deadband_m - 1e-6);
            assert!(depth <= last);
            last = depth;
        }
        assert!(depth - CONFIG.push_deadband_m < 0.01);
    }
}
