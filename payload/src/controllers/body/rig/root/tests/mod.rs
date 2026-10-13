//! The root's behaviour in the kinematic world ([`crate::controllers::body::rig::harness`]): the
//! chase following a walk in the room, the shift keeping the camera on the real head, the stick and
//! the room adding, several ticks to a frame, the lean, and the owners that stand the chase down.
//! The walls are in [`walls`].

mod walls;

use glam::{Quat, Vec2, Vec3};

use crate::controllers::body::{
    rig::{
        harness::{FRAME_HZ, Head, TICK_HZ, World},
        root::owner::RootOwner,
    },
    torso::EYES_TO_NECK_PIVOT,
};

/// The share of a minimum-jerk reach completed at `u` (`0..=1`), the usual model of a person
/// moving from one place to another.
pub(super) fn min_jerk(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    u * u * u * (10.0 - 15.0 * u + 6.0 * u * u)
}

/// How long a minimum-jerk reach of `distance` takes to peak at `speed`.
pub(super) fn min_jerk_duration(distance: f32, speed: f32) -> f32 {
    1.875 * distance / speed
}

/// A walking head's vertical bob, metres.
fn bob(t: f32) -> f32 {
    -0.01 * (0.5 - 0.5 * (std::f32::consts::TAU * 1.8 * t).cos())
}

/// A deterministic pseudo-random sequence in `0..1`.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[test]
fn walking_a_metre_keeps_the_body_under_the_head() {
    let mut world = World::new();
    world.run(0.5, |_| Head::standing(0.0, 0.0), |_| {});
    let start = world.time();
    let duration = min_jerk_duration(1.0, 1.4);
    let walk = move |t: f32| {
        let u = (t - start) / duration;
        let moving = (0.0..1.0).contains(&u);
        Head {
            eyes: Vec3::new(0.0, if moving { bob(t - start) } else { 0.0 }, -min_jerk(u)),
            orientation: Quat::IDENTITY,
        }
    };
    let mut worst: f32 = 0.0;
    let mut settled: f32 = 0.0;
    let stop = start + duration;
    world.run(duration + 1.0, walk, |s| {
        worst = worst.max(s.gap_m);
        if s.t >= stop + 0.3 {
            settled = settled.max(s.gap_m);
        }
    });
    assert!(worst < 0.03, "the gap reached {worst} m while walking");
    assert!(settled < 0.005, "the gap was {settled} m after stopping");
    assert!((world.root() - Vec2::new(0.0, -1.0)).length() < 0.005);
}

#[test]
fn a_long_random_walk_keeps_the_camera_on_the_real_head() {
    let view = Quat::from_rotation_y(0.7);
    let mut world = World::new().with_view(view, 1.25);
    let mut noise = Noise(0x2545_f491_4f6c_dd1d);
    // Waypoints in a four-metre room, walked one after another at up to 1.4 m/s.
    let mut legs = Vec::new();
    let mut from = Vec2::ZERO;
    let mut t = 0.0;
    let mut walked = 0.0;
    while walked < 100.0 {
        let to = Vec2::new(noise.next() * 4.0 - 2.0, noise.next() * 4.0 - 2.0);
        let distance = from.distance(to).max(0.05);
        let speed = 0.6 + noise.next() * 0.8;
        let duration = min_jerk_duration(distance, speed);
        legs.push((t, duration, from, to));
        t += duration;
        walked += distance;
        from = to;
    }
    let end = t;
    let head = move |t: f32| {
        let leg = legs
            .iter()
            .rev()
            .find(|(start, ..)| *start <= t)
            .copied()
            .unwrap_or((0.0, 1.0, Vec2::ZERO, Vec2::ZERO));
        let (start, duration, from, to) = leg;
        let at = from.lerp(to, min_jerk((t - start) / duration));
        Head {
            eyes: Vec3::new(at.x, bob(t), at.y),
            orientation: Quat::from_rotation_y((t * 0.3).sin()),
        }
    };
    let mut worst: f32 = 0.0;
    let mut worst_gap: f32 = 0.0;
    world.run(end + 1.0, head, |s| {
        worst = worst.max(s.camera_error_m);
        worst_gap = worst_gap.max(s.residual_m);
    });
    assert!(worst < 0.001, "the camera strayed {worst} m from the head");
    assert!(worst_gap < 0.06, "the residual reached {worst_gap} m");
    assert_eq!(world.ledger().counts().projected, 0);
}

#[test]
fn the_stick_and_a_walk_in_the_room_add() {
    let mut world = World::new();
    world.stick = Vec2::new(0.0, -3.0);
    let walk = |t: f32| Head::standing(min_jerk(t / min_jerk_duration(1.0, 1.0)), 0.0);
    let mut worst: f32 = 0.0;
    world.run(3.0, walk, |s| {
        worst = worst.max(s.camera_error_m);
    });
    // Three seconds of the stick forward, and the metre the player walked to the right.
    let expected = Vec2::new(1.0, -9.0);
    assert!(
        (world.root() - expected).length() < 0.005,
        "{}",
        world.root()
    );
    assert!(worst < 1e-3, "the camera strayed {worst} m");
}

#[test]
fn several_ticks_to_a_frame_never_overshoot() {
    for frame_hz in [TICK_HZ / 2.0, TICK_HZ / 3.0, FRAME_HZ] {
        let mut world = World::new().with_frame_rate(frame_hz);
        let mut furthest: f32 = 0.0;
        world.run(
            2.0,
            |_| Head::standing(0.3, 0.0),
            |s| {
                furthest = furthest.max(s.root.x);
            },
        );
        assert!(furthest <= 0.3 + 1e-5, "{frame_hz} Hz went to {furthest} m");
        assert!((world.root().x - 0.3).abs() < 1e-3, "{frame_hz} Hz");
    }
}

#[test]
fn a_walk_at_a_low_frame_rate_settles_without_overshooting() {
    let mut world = World::new().with_frame_rate(TICK_HZ / 2.0);
    let duration = min_jerk_duration(1.0, 1.4);
    let mut furthest: f32 = 0.0;
    world.run(
        duration + 1.5,
        |t| Head::standing(min_jerk(t / duration), 0.0),
        |s| furthest = furthest.max(s.root.x),
    );
    assert!(furthest < 1.02, "the root went to {furthest} m");
    assert!((world.root().x - 1.0).abs() < 0.005, "{}", world.root());
}

/// A lean over a table, with the neck pivot `reach` forward and `drop` down at its fullest: the
/// knees and hips bend first, then the head reaches out over it, looking down at it.
fn lean(t: f32, reach: f32, drop: f32) -> Head {
    leaning(min_jerk(t / 0.6), min_jerk((t - 0.3) / 0.7), reach, drop)
}

/// The head with the pivot `down` of the way to `drop` and `out` of the way to `reach`.
fn leaning(down: f32, out: f32, reach: f32, drop: f32) -> Head {
    let orientation = Quat::from_rotation_x(-0.5 * down);
    let pivot = EYES_TO_NECK_PIVOT + Vec3::new(0.0, -drop * down, -reach * out);
    Head {
        eyes: pivot - orientation * EYES_TO_NECK_PIVOT,
        orientation,
    }
}

#[test]
fn leaning_over_a_table_leaves_the_body_and_standing_up_closes_the_gap() {
    let mut world = World::new();
    let mut furthest: f32 = 0.0;
    world.run(
        3.0,
        |t| lean(t, 0.3, 0.12),
        |s| {
            furthest = furthest.max(s.root.length());
        },
    );
    assert!(furthest < 0.02, "the root moved {furthest} m");
    let held = world.root();
    // Standing back up, drawing the head back in before rising, the lean and the gap leave
    // together.
    let up = world.time();
    let mut last = None;
    let mut wandered: f32 = 0.0;
    world.run(
        2.0,
        |t| {
            let t = t - up;
            leaning(
                1.0 - min_jerk((t - 0.3) / 0.6),
                1.0 - min_jerk(t / 0.7),
                0.3,
                0.12,
            )
        },
        |s| {
            last = Some(*s);
            wandered = wandered.max((s.root - held).length());
        },
    );
    let last = last.unwrap();
    assert!(last.gap_m < 0.005, "{last:?}");
    assert!(wandered < 0.02, "the root moved {wandered} m standing up");
}

#[test]
fn a_climb_owns_the_root_and_clears_the_ledger() {
    let mut world = World::new();
    world.run(0.5, |_| Head::standing(0.0, 0.0), |_| {});
    world.owner = RootOwner::Climb;
    let credited = world.ledger().completed().current;
    world.run(
        1.0,
        |_| Head::standing(0.4, 0.0),
        |s| {
            assert_eq!(s.root, Vec2::ZERO);
        },
    );
    assert!(!world.ledger().pending());
    assert_eq!(world.ledger().completed().current, credited);
    // Back on foot, the body walks to the head.
    world.owner = RootOwner::Player;
    world.run(1.0, |_| Head::standing(0.4, 0.0), |_| {});
    assert!((world.root().x - 0.4).abs() < 1e-3, "{}", world.root());
}

#[test]
fn a_walk_far_from_the_world_origin_keeps_the_camera_on_the_head() {
    let mut world = World::new().at(Vec2::new(-3_000.0, 3_000.0));
    let duration = min_jerk_duration(2.0, 1.4);
    let mut worst: f32 = 0.0;
    world.run(
        duration + 1.0,
        |t| Head::standing(0.0, -2.0 * min_jerk(t / duration)),
        |s| worst = worst.max(s.camera_error_m),
    );
    assert!(worst < 0.001, "the camera strayed {worst} m");
    assert_eq!(world.ledger().counts().projected, 0);
}
