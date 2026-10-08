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
//! the gun is stopped.
//!
//! The probe is cast along the gun's pose before the push ([`apply`] notes it), so the push never
//! feeds back into where the wall is looked for. The pose is applied last on the cockpit-frame
//! hands ([`crate::controllers::hands::active_hands`]), after the sights and the recoil, so the
//! drawn gun, its sight line, and the IK targets all see the gun where the wall stops it.

pub mod config;
pub mod solve;

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    controllers::{
        aim::{
            ray::Ray,
            raycast::{self, CastOptions},
        },
        frame::{Hand, Pose},
        hands::CockpitHand,
        weapons::{
            collide::{
                config::CollideConfig,
                solve::{Barrel, Wall},
            },
            foregrip,
        },
    },
    headpose,
};

/// The long gun the hands hold, as the input tick measured it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CollideWeapon {
    pub holder: Hand,
    /// The muzzle in the holding wrist's frame, cockpit metres.
    pub muzzle: Vec3,
    /// The barrel's direction in the holding wrist's frame, a unit vector.
    pub barrel: Vec3,
}

/// The collision's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct CollideStatus {
    /// Whether a long gun is measured.
    pub weapon: bool,
    /// Whether the gun is against a wall on the last pose.
    pub touching: bool,
    pub turn_deg: f32,
    pub retract_m: f32,
    /// Times the gun came against a wall.
    pub contacts: u64,
    pub casts: u64,
}

pub fn status() -> CollideStatus {
    STATE.lock().status
}

/// Note the held long gun for the poses that follow, or `None` without one.
pub(crate) fn observe(weapon: Option<CollideWeapon>) {
    let mut state = STATE.lock();
    state.weapon = weapon;
    state.status.weapon = weapon.is_some();
    if weapon.is_none() {
        state.probe = None;
        state.wall = None;
        state.status.touching = false;
    }
}

/// Push the gun the hands hold out of the wall its probe last met. `head` is the head's cockpit
/// pose and `world_scale` the VR world scale the cockpit composes with.
pub fn apply(
    hands: &mut [Option<CockpitHand>; 2],
    head: Pose,
    world_scale: f32,
    config: &CollideConfig,
) {
    if !config.enabled {
        return;
    }
    let (weapon, wall) = {
        let state = STATE.lock();
        (state.weapon, state.wall)
    };
    let Some(weapon) = weapon else {
        return;
    };
    let Some(holder) = hands[weapon.holder.index()] else {
        return;
    };
    let barrel = barrel_of(holder, &weapon, config);
    let frame = WorldFrame::now(head, world_scale);
    let mut state = STATE.lock();
    state.probe = Some(frame.barrel_to_world(barrel));
    let wall = wall
        .filter(|w| w.at.elapsed() <= Duration::from_secs_f32(config.keep_s.max(0.0)))
        .map(|w| frame.wall_to_cockpit(w.wall));
    let push =
        wall.and_then(|w| solve::solve(barrel, w, config.max_turn_deg, config.max_retract_m));
    let touching = push.is_some();
    if touching && !state.status.touching {
        state.status.contacts += 1;
        let now = Instant::now();
        if state
            .last_log
            .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL)
        {
            state.last_log = Some(now);
            tracing::info!(target: "controllers", hand = ?weapon.holder, "collide: the held gun met a wall");
        }
    }
    state.status.touching = touching;
    state.status.turn_deg = push.map_or(0.0, |p| p.turn_deg);
    state.status.retract_m = push.map_or(0.0, |p| p.retract_m);
    drop(state);
    let Some(push) = push else {
        return;
    };
    hands[weapon.holder.index()] = Some(push.moved.hand(holder));
    let support = foregrip::status()
        .hold
        .filter(|h| h.engaged && h.holder == weapon.holder)
        .map(|h| h.holder.other());
    if let Some(h) = support
        && let Some(hand) = hands[h.index()]
    {
        hands[h.index()] = Some(push.moved.hand(hand));
    }
}

/// Cast the gun's probe from its pose before the push, and keep the wall it meets.
///
/// # Safety
///
/// The simulation thread outside the physics step, with `owner` the live local player character.
pub(crate) unsafe fn probe(owner: &Character) {
    let Some(probe) = STATE.lock().probe else {
        return;
    };
    let Some(ray) = Ray::new(probe.grip, probe.direction) else {
        return;
    };
    let options = CastOptions {
        ignore: [owner as *const Character as usize, 0],
        see_through: true,
        material: false,
    };
    // SAFETY: forwarded from the caller.
    let hit = unsafe { raycast::cast(&ray, probe.length.max(1e-3), &options) };
    let mut state = STATE.lock();
    state.status.casts += 1;
    // A miss keeps the last wall a moment past its sighting (see `keep_s`), so a probe that just
    // missed does not drop the push and let the gun bounce back in.
    if let Ok(Some(hit)) = hit {
        // A wall faces the grip: a normal facing away is taken from the probe.
        let normal = hit
            .normal
            .try_normalize()
            .filter(|n| n.dot(ray.direction) < 0.0)
            .unwrap_or(-ray.direction);
        state.wall = Some(SeenWall {
            wall: Wall {
                point: hit.position,
                normal,
            },
            at: Instant::now(),
        });
    }
}

/// The gun's segment in the cockpit frame: from the holding grip to past the muzzle.
fn barrel_of(holder: CockpitHand, weapon: &CollideWeapon, config: &CollideConfig) -> Barrel {
    let wrist = holder.wrist;
    let grip = holder.grip.position;
    let direction = (wrist.orientation * weapon.barrel).normalize_or_zero();
    let muzzle = wrist.position + wrist.orientation * weapon.muzzle;
    Barrel {
        grip,
        direction,
        length: ((muzzle - grip).dot(direction) + config.tip_margin_m).max(0.0),
    }
}

/// The shortest time between two contact log lines.
const LOG_INTERVAL: Duration = Duration::from_secs(1);

/// The transforms between the cockpit frame and the world at one moment.
#[derive(Debug, Copy, Clone, PartialEq)]
struct WorldFrame {
    head_world: Vec3,
    head_cockpit: Vec3,
    body: Quat,
    world_scale: f32,
}

impl WorldFrame {
    fn now(head: Pose, world_scale: f32) -> WorldFrame {
        WorldFrame {
            head_world: headpose::query().position,
            head_cockpit: head.position,
            body: headpose::xr::body_rotation(),
            world_scale,
        }
    }

    fn barrel_to_world(&self, b: Barrel) -> Barrel {
        Barrel {
            grip: self.head_world + self.body * ((b.grip - self.head_cockpit) * self.world_scale),
            direction: self.body * b.direction,
            length: b.length * self.world_scale,
        }
    }

    fn wall_to_cockpit(&self, w: Wall) -> Wall {
        let scale = if self.world_scale > 0.0 {
            self.world_scale
        } else {
            1.0
        };
        let inverse = self.body.inverse();
        Wall {
            point: self.head_cockpit + inverse * (w.point - self.head_world) / scale,
            normal: inverse * w.normal,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct SeenWall {
    /// World space.
    wall: Wall,
    at: Instant,
}

struct State {
    weapon: Option<CollideWeapon>,
    /// The gun's segment before the push, world space, for the next probe.
    probe: Option<Barrel>,
    wall: Option<SeenWall>,
    last_log: Option<Instant>,
    status: CollideStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    weapon: None,
    probe: None,
    wall: None,
    last_log: None,
    status: CollideStatus {
        weapon: false,
        touching: false,
        turn_deg: 0.0,
        retract_m: 0.0,
        contacts: 0,
        casts: 0,
    },
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_frame_carries_a_wall_back_where_the_barrel_went() {
        let frame = WorldFrame {
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
        let world = frame.barrel_to_world(barrel);
        let wall = frame.wall_to_cockpit(Wall {
            point: world.grip,
            normal: world.direction,
        });
        assert!(wall.point.distance(barrel.grip) < 1e-4);
        assert!(wall.normal.distance(barrel.direction) < 1e-5);
        assert!((world.length - 0.96).abs() < 1e-5);
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
        let config = CollideConfig::new();
        let b = barrel_of(hand, &weapon, &config);
        assert!((b.length - (0.7 + config.tip_margin_m)).abs() < 1e-5);
        assert_eq!(b.direction, Vec3::NEG_Z);
    }
}
