//! Held long guns that collide with the world: the plan of record's colliding held weapons
//! (`docs/mod/input/physical-weapons.md`, "Guns against walls").
//!
//! A gun in the hands has no body in the physics world (the weapon's bodies are out of the world
//! while it is wielded), so nothing stops it at a wall: a rifle held up against one goes through.
//! The gun's collision proxy is a ray along it: from the holding grip to just past the muzzle,
//! cast in the simulation phase outside the physics step with the player aim's own filter, as the
//! gun aim's rays are ([`probe`]). The wall it meets, a plane, is kept for the hand poses that
//! follow, and each pose pushes the gun out of it ([`solve`]): turned about the grip, away from
//! the wall, so it slides along it, and pulled back into the hands when it meets the wall head-on.
//! The holding hand rides the gun, and the hand on its foregrip with it, so the hands follow where
//! the gun is stopped. Each hand's long gun, the wielded weapon or one armed beside it, has its own
//! probe and its own wall.
//!
//! The probe is cast along the gun's pose before the push ([`apply`] notes it), so the push never
//! feeds back into where the wall is looked for. The pose is applied last on the cockpit-frame
//! hands ([`crate::controllers::hands::active_hands`]), after the sights and the recoil, so the
//! drawn gun, its sight line, and the IK targets all see the gun where the wall stops it.

pub mod config;
pub mod solve;

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    aim::{
        ray::Ray,
        raycast::{self, CastOptions},
    },
    cockpit::CockpitTransform,
    frame::{Hand, Pose},
    hands::CockpitHand,
    weapons::{
        ads::solve::RigidMove,
        collide::{
            config::CollideConfig,
            solve::{Barrel, Wall},
        },
        foregrip,
    },
};

/// A long gun in a hand, as the input tick measured it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CollideWeapon {
    pub holder: Hand,
    /// The muzzle in the holding wrist's frame, cockpit metres.
    pub muzzle: Vec3,
    /// The barrel's direction in the holding wrist's frame, a unit vector.
    pub barrel: Vec3,
}

/// One hand's gun against the walls.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandCollideStatus {
    /// Whether the hand's long gun is measured.
    pub weapon: bool,
    /// Whether the gun is against a wall on the last pose.
    pub touching: bool,
    pub turn_deg: f32,
    pub retract_m: f32,
}

/// The collision's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CollideStatus {
    /// Each hand's long gun, by [`Hand::index`].
    pub hands: [HandCollideStatus; 2],
    /// Times a gun came against a wall.
    pub contacts: u64,
    pub casts: u64,
}

impl CollideStatus {
    /// Whether either hand's long gun is measured.
    pub fn weapon(&self) -> bool {
        self.hands.iter().any(|h| h.weapon)
    }

    /// Whether either hand's gun is against a wall.
    pub fn touching(&self) -> bool {
        self.hands.iter().any(|h| h.touching)
    }
}

pub fn status() -> CollideStatus {
    STATE.lock().status
}

/// Note the long gun in each hand for the poses that follow, by [`Hand::index`], or `None` for a
/// hand without one. A gun is taken only in its own hand's place.
pub(crate) fn observe(weapons: [Option<CollideWeapon>; 2]) {
    let mut guard = STATE.lock();
    let state = &mut *guard;
    for hand in Hand::BOTH {
        let weapon = weapons[hand.index()].filter(|w| w.holder == hand);
        let h = &mut state.hands[hand.index()];
        h.weapon = weapon;
        let status = &mut state.status.hands[hand.index()];
        status.weapon = weapon.is_some();
        if weapon.is_none() {
            h.probe = None;
            h.wall = None;
            *status = HandCollideStatus::default();
        }
    }
}

/// Push each hand's gun out of the wall its probe last met. `head` is the head's cockpit pose and
/// `world_scale` the VR world scale the cockpit composes with.
pub fn apply(
    hands: &mut [Option<CockpitHand>; 2],
    head: Pose,
    world_scale: f32,
    config: &CollideConfig,
) {
    if !config.enabled {
        return;
    }
    let frame = CockpitTransform::current(head.position, world_scale);
    let support = foregrip::status().hold.filter(|h| h.engaged);
    for hand in Hand::BOTH {
        let Some(push) = push_hand(hand, hands, &frame) else {
            continue;
        };
        if let Some(held) = hands[hand.index()] {
            hands[hand.index()] = Some(push.hand(held));
        }
        // The hand on this gun's foregrip rides with it.
        let off = hand.other();
        if support.is_some_and(|h| h.holder == hand)
            && let Some(bracing) = hands[off.index()]
        {
            hands[off.index()] = Some(push.hand(bracing));
        }
    }
}

/// Cast each hand's gun's probe from its pose before the push, and keep the wall it meets.
///
/// # Safety
///
/// The simulation thread outside the physics step, with a live local player character.
pub(crate) unsafe fn probe() {
    let probes = STATE.lock().hands.map(|h| h.probe);
    // SAFETY: forwarded from the caller.
    let options = unsafe { CastOptions::ignoring_player(0) }.seeing_through();
    for hand in Hand::BOTH {
        let Some(probe) = probes[hand.index()] else {
            continue;
        };
        let Some(ray) = Ray::new(probe.grip, probe.direction) else {
            continue;
        };
        // SAFETY: forwarded from the caller.
        let hit = unsafe { raycast::cast(&ray, probe.length.max(1e-3), &options) };
        let mut state = STATE.lock();
        state.status.casts += 1;
        // A miss keeps the last wall a moment past its sighting (see `keep_s`), so a probe that
        // just missed does not drop the push and let the gun bounce back in.
        if let Ok(Some(hit)) = hit {
            // A wall faces the grip: a normal facing away is taken from the probe.
            let normal = hit
                .normal
                .try_normalize()
                .filter(|n| n.dot(ray.direction) < 0.0)
                .unwrap_or(-ray.direction);
            state.hands[hand.index()].wall = Some(SeenWall {
                wall: Wall {
                    point: hit.position,
                    normal,
                },
                at: Instant::now(),
            });
        }
    }
}

/// How far past the muzzle the gun's probe reaches, metres, so the muzzle stops just short of a
/// wall.
const TIP_MARGIN_M: f32 = 0.03;

/// The furthest the gun turns away from a wall about its grip, degrees.
const MAX_TURN_DEG: f32 = 70.0;

/// The furthest the gun is pulled back into the hands, metres.
const MAX_RETRACT_M: f32 = 0.35;

/// How long a wall is kept after the probe last met it, seconds.
const KEEP_S: f32 = 0.15;

/// The push of `hand`'s gun out of its wall, noting the probe for the next cast and the readout.
fn push_hand(
    hand: Hand,
    hands: &[Option<CockpitHand>; 2],
    frame: &CockpitTransform,
) -> Option<RigidMove> {
    let mut guard = STATE.lock();
    let state = &mut *guard;
    let h = &mut state.hands[hand.index()];
    let weapon = h.weapon?;
    let holder = hands[hand.index()]?;
    let barrel = barrel_of(holder, &weapon);
    h.probe = Some(barrel_to_world(frame, barrel));
    let wall = h
        .wall
        .filter(|w| w.at.elapsed() <= Duration::from_secs_f32(KEEP_S))
        .map(|w| wall_to_cockpit(frame, w.wall));
    let push = wall.and_then(|w| solve::solve(barrel, w, MAX_TURN_DEG, MAX_RETRACT_M));
    let touching = push.is_some();
    let status = &mut state.status.hands[hand.index()];
    if touching && !status.touching {
        state.status.contacts += 1;
        let now = Instant::now();
        if state
            .last_log
            .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL)
        {
            state.last_log = Some(now);
            tracing::info!(target: "controllers", hand = ?hand, "collide: the held gun met a wall");
        }
    }
    let status = &mut state.status.hands[hand.index()];
    status.touching = touching;
    status.turn_deg = push.map_or(0.0, |p| p.turn_deg);
    status.retract_m = push.map_or(0.0, |p| p.retract_m);
    push.map(|p| p.moved)
}

/// The gun's segment in the cockpit frame: from the holding grip to past the muzzle.
fn barrel_of(holder: CockpitHand, weapon: &CollideWeapon) -> Barrel {
    let wrist = holder.wrist;
    let grip = holder.grip.position;
    let direction = (wrist.orientation * weapon.barrel).normalize_or_zero();
    let muzzle = wrist.position + wrist.orientation * weapon.muzzle;
    Barrel {
        grip,
        direction,
        length: ((muzzle - grip).dot(direction) + TIP_MARGIN_M).max(0.0),
    }
}

/// The shortest time between two contact log lines.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

/// `b`, a barrel in the cockpit frame, in the world.
fn barrel_to_world(frame: &CockpitTransform, b: Barrel) -> Barrel {
    Barrel {
        grip: frame.point_to_world(b.grip),
        direction: frame.direction_to_world(b.direction),
        length: b.length * frame.scale(),
    }
}

/// `w`, a wall in the world, in the cockpit frame.
fn wall_to_cockpit(frame: &CockpitTransform, w: Wall) -> Wall {
    Wall {
        point: frame.point_to_cockpit(w.point),
        normal: frame.direction_to_cockpit(w.normal),
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct SeenWall {
    /// World space.
    wall: Wall,
    at: Instant,
}

/// One hand's long gun and the wall its probe met.
#[derive(Debug, Copy, Clone, PartialEq)]
struct HandCollide {
    weapon: Option<CollideWeapon>,
    /// The gun's segment before the push, world space, for the next probe.
    probe: Option<Barrel>,
    wall: Option<SeenWall>,
}

struct State {
    hands: [HandCollide; 2],
    last_log: Option<Instant>,
    status: CollideStatus,
}

const IDLE_HAND: HandCollideStatus = HandCollideStatus {
    weapon: false,
    touching: false,
    turn_deg: 0.0,
    retract_m: 0.0,
};

static STATE: Mutex<State> = Mutex::new(State {
    hands: [HandCollide {
        weapon: None,
        probe: None,
        wall: None,
    }; 2],
    last_log: None,
    status: CollideStatus {
        hands: [IDLE_HAND; 2],
        contacts: 0,
        casts: 0,
    },
});

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    #[test]
    fn the_cockpit_transform_carries_a_wall_back_where_the_barrel_went() {
        let frame = CockpitTransform {
            head_world: Vec3::new(10.0, 2.0, -5.0),
            head_cockpit: Vec3::new(0.0, 1.7, 0.0),
            body: Quat::from_rotation_y(0.7),
            world_scale: 1.2,
        };
        let barrel = Barrel {
            grip: Vec3::new(0.2, 1.4, -0.3),
            direction: Vec3::NEG_Z,
            length: 0.8,
        };
        let world = barrel_to_world(&frame, barrel);
        let wall = wall_to_cockpit(
            &frame,
            Wall {
                point: world.grip,
                normal: world.direction,
            },
        );
        assert!(wall.point.distance(barrel.grip) < 1e-4);
        assert!(wall.normal.distance(barrel.direction) < 1e-5);
        assert!((world.length - 0.96).abs() < 1e-5);
    }

    #[test]
    fn each_gun_in_a_hand_meets_its_own_wall() {
        // The only test that drives the shared state.
        let pose = |position| Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        let at = |x: f32| CockpitHand {
            wrist: pose(Vec3::new(x, 0.0, 0.0)),
            grip: pose(Vec3::new(x, 0.0, 0.0)),
            aim: pose(Vec3::new(x, 0.0, 0.0)),
        };
        let rifle = |holder| CollideWeapon {
            holder,
            muzzle: Vec3::new(0.0, 0.0, -0.8),
            barrel: Vec3::NEG_Z,
        };
        // A gun is taken only in its own hand's place.
        observe([Some(rifle(Hand::Right)), None]);
        assert!(!status().weapon());
        observe([Some(rifle(Hand::Left)), Some(rifle(Hand::Right))]);
        assert!(status().hands.iter().all(|h| h.weapon));
        // A wall ahead of the right gun's muzzle alone.
        STATE.lock().hands[Hand::Right.index()].wall = Some(SeenWall {
            wall: Wall {
                point: Vec3::new(0.3, 0.0, -0.5),
                normal: Vec3::Z,
            },
            at: Instant::now(),
        });
        let frame = CockpitTransform {
            head_world: Vec3::ZERO,
            head_cockpit: Vec3::ZERO,
            body: Quat::IDENTITY,
            world_scale: 1.0,
        };
        let hands = [Some(at(-0.3)), Some(at(0.3))];
        assert!(push_hand(Hand::Right, &hands, &frame).is_some());
        assert!(push_hand(Hand::Left, &hands, &frame).is_none());
        let s = status();
        assert!(s.hands[Hand::Right.index()].touching);
        assert!(!s.hands[Hand::Left.index()].touching);
        // Both probes are kept for the next cast.
        assert!(STATE.lock().hands.iter().all(|h| h.probe.is_some()));
        observe([None; 2]);
        assert!(!status().weapon() && !status().touching());
    }

    #[test]
    fn the_segment_runs_from_the_grip_past_the_muzzle() {
        let pose = |position| Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        let hand = CockpitHand {
            wrist: pose(Vec3::ZERO),
            grip: pose(Vec3::new(0.0, -0.02, -0.05)),
            aim: pose(Vec3::ZERO),
        };
        let weapon = CollideWeapon {
            holder: Hand::Right,
            muzzle: Vec3::new(0.0, 0.05, -0.75),
            barrel: Vec3::NEG_Z,
        };
        let b = barrel_of(hand, &weapon);
        assert!((b.length - (0.7 + TIP_MARGIN_M)).abs() < 1e-5);
        assert_eq!(b.direction, Vec3::NEG_Z);
    }
}
