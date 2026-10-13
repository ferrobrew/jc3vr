//! The push-back against walls in the kinematic world: walking into one, backing away, and sliding
//! along one.

use glam::{Vec2, Vec3};

use crate::controllers::body::rig::{
    config::PushbackConfig,
    harness::{Head, Wall, World},
    root::tests::{min_jerk, min_jerk_duration},
};

const CONFIG: PushbackConfig = PushbackConfig::new();

/// A wall across the room ahead: the root stays at or behind `z = -0.3`.
fn wall_ahead() -> Wall {
    Wall {
        normal: Vec2::new(0.0, -1.0),
        offset: 0.3,
    }
}

/// The head walking `distance` forward from `from_z` at up to `speed`, starting at `start`.
fn walk_z(start: f32, from_z: f32, distance: f32, speed: f32) -> impl Fn(f32) -> Head {
    let duration = min_jerk_duration(distance.abs(), speed);
    move |t| Head::standing(0.0, from_z - distance * min_jerk((t - start) / duration))
}

#[test]
fn walking_into_a_wall_holds_the_view_then_fades() {
    let mut world = World::new();
    world.walls.push(wall_ahead());
    let mut held: f32 = 0.0;
    let mut faded_early = false;
    world.run(3.0, walk_z(0.0, 0.0, 1.0, 1.0), |s| {
        let ahead = -s.measurement.target.y;
        if s.debt_m < CONFIG.debt_cap_m - 1e-4 {
            held = held.max(s.gap_m);
        }
        // Black only once the head is the threshold, the cap, and the fade's span past the root.
        if s.pushback.fade >= 1.0 {
            let past = CONFIG.obstructed_m + CONFIG.debt_cap_m + CONFIG.fade_span_m;
            faded_early |= ahead + s.debt_m < past - 0.01;
        }
    });
    assert!((world.root().y + 0.3).abs() < 1e-4, "{}", world.root());
    // The view is held at the threshold, give or take the two steps the ledger needs to see the
    // obstruction.
    assert!(
        held < CONFIG.obstructed_m + 0.08,
        "the view went {held} m past the root"
    );
    assert!((world.pushback().debt() - CONFIG.debt_cap_m).abs() < 1e-4);
    assert_eq!(world.pushback().episodes(), 1);
    assert!(!faded_early);
    let mut last = None;
    world.run(0.1, walk_z(0.0, 0.0, 1.0, 1.0), |s| last = Some(*s));
    assert_eq!(last.unwrap().pushback.fade, 1.0);
}

#[test]
fn backing_away_clears_the_fade_and_keeps_the_slide() {
    let mut world = World::new();
    world.walls.push(wall_ahead());
    world.run(3.0, walk_z(0.0, 0.0, 1.0, 1.0), |_| {});
    let back = world.time();
    let mut last = None;
    world.run(4.0, walk_z(back, -1.0, -1.0, 1.0), |s| last = Some(*s));
    let last = last.unwrap();
    assert_eq!(last.pushback.fade, 0.0);
    assert_eq!(world.pushback().debt(), 0.0);
    // The room slid against the world by the cap: the camera stays that far from the real head,
    // and the body has followed the camera back.
    let eyes = Vec3::ZERO;
    let slide = world.camera(eyes) - world.real_head(eyes);
    assert!(
        (slide - Vec2::new(0.0, CONFIG.debt_cap_m)).length() < 0.01,
        "{slide}"
    );
    assert!(
        (world.root() - world.camera(eyes)).length() < 0.01,
        "{}",
        world.root()
    );
}

#[test]
fn sliding_along_a_wall_does_not_pump() {
    let mut world = World::new();
    world.walls.push(wall_ahead());
    // Into the wall by half a metre past where the body stops, and hold.
    world.run(2.5, walk_z(0.0, 0.0, 0.6, 1.0), |_| {});
    let debt = world.pushback().debt();
    assert!(debt > 0.0);
    // Then along it for three metres, the head as far into the wall as before.
    let start = world.time();
    let duration = min_jerk_duration(3.0, 1.0);
    let mut pushed: f32 = 0.0;
    world.run(
        duration + 1.0,
        |t| Head::standing(3.0 * min_jerk((t - start) / duration), -0.6),
        |s| pushed += s.pushback.push.length(),
    );
    assert!(pushed < 0.01, "the slide pushed {pushed} m more");
    assert_eq!(world.pushback().episodes(), 1);
    assert!(world.pushback().debt() <= debt + 0.01);
    assert!((world.root().x - 3.0).abs() < 0.01, "{}", world.root());
}
