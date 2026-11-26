//! Collision capsules on Rico's drawn torso, for what the hands hold to meet.
//!
//! Rico has no physics bodies for his limbs while animated: his ragdoll exists only while a preset
//! is wanted, and its keyframed bodies would follow the animated pose before the mod's IK and sit
//! on a layer that props do not collide with. A held prop passed through the torso (a barrel swung
//! through Rico at 1438.8 s on the 14 November take), and a held ragdoll filled the camera.
//!
//! Two capsules follow the procedural torso's keep-out, the chest and the abdomen, slimmer than the
//! drawn body so the holsters and the pouches stay in reach and no head capsule meets the camera.
//! They are made as the hand bodies are, on the hand bodies' layer in the player's system group, so
//! they never touch the player's capsule or the hand bodies, then switched to keyframed, so that a
//! collision never pushes them and they never push the player; each pre-simulation step sets their
//! velocities to land on the drawn capsules at the step's end. The carry collision leaves them out
//! of the player's bodies it parts a held object from ([`is_proxy`]), so a held prop meets them.
//! They exist while the procedural torso poses the body under the controllers.
//!
//! A held object stopped against them leaves the holding hand lagging, which is the player's own
//! body in the way rather than the world: [`rests_against`] tells the hold so, and the hold's tear
//! does not count that lag as the object being caught. Their contacts never reach the held
//! objects' contact feedback (the strike haptics, the impact sounds, and the lag's push on the
//! player), which leaves out every body of the player's system group, and are noted here instead
//! ([`note_contact`]).

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::Character;
use jc3vr_body::controllers::{
    body::procedural::{solver::arms::Capsule as KeepOutCapsule, torso_keep_out},
    physics::bodies,
    scheme,
};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::physical_hands::{
    config::PHYSICAL_HANDS_CONFIG,
    physics::{
        body::{BodyShape, BodySpec, Capsule, HandBody, player_system_group},
        contact::Source,
        step::step_seconds,
    },
};

/// Whether `body` is one of the drawn torso's capsules. Lock-free, for the carry collision's
/// scan of the world's bodies.
pub(crate) fn is_proxy(body: u32) -> bool {
    body != u32::MAX && IDS.iter().any(|id| id.load(Ordering::Relaxed) == body)
}

/// Note that the held body `body` met one of the capsules: called from the held objects' contact
/// observation, inside the physics step.
pub(crate) fn note_contact(body: u32) {
    let mut met = MET.lock();
    if !met.contains(&body) {
        if met.len() >= MAX_TRACKED {
            met.remove(0);
        }
        met.push(body);
    }
}

/// Whether the held body `body` rests against the torso's capsules: it met one since it last came
/// clear of them ([`note_contact`]), and its box still reaches one within [`REACH_MARGIN_M`]. A
/// contact alone is not enough, since a resting contact is reported only as it starts; the box
/// alone is not either, since a long prop's box reaches the torso well before the prop does.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(crate) unsafe fn rests_against(body: u32) -> bool {
    let met = {
        let mut met = MET.lock();
        let index = met.iter().position(|b| *b == body);
        index.map(|i| met.remove(i)).is_some()
    };
    let capsules = *CAPSULES.lock();
    let reaches = unsafe { bodies::aabb(body) }.is_some_and(|(min, max)| {
        capsules
            .iter()
            .flatten()
            .any(|c| box_reaches_capsule(min, max, *c, REACH_MARGIN_M))
    });
    let at = Instant::now();
    let mut resting = RESTING.lock();
    // A body last asked about long ago was let go since, and starts afresh.
    resting.retain(|(_, asked)| at.saturating_duration_since(*asked) <= FORGET_AFTER);
    let was = resting.iter().any(|(b, _)| *b == body);
    let now = episode(was, met, reaches);
    resting.retain(|(b, _)| *b != body);
    if now {
        if resting.len() >= MAX_TRACKED {
            resting.remove(0);
        }
        resting.push((body, at));
    }
    if now != was {
        if now {
            tracing::info!(
                target: "controllers",
                body,
                "drawn body: a held object rests against Rico's torso capsules"
            );
        } else {
            tracing::info!(
                target: "controllers",
                body,
                "drawn body: a held object came clear of Rico's torso capsules"
            );
        }
    }
    now
}

/// Make, drive, or remove the drawn torso's capsules for this step.
///
/// # Safety
///
/// The thread that steps the physics world, before the step.
pub(super) unsafe fn step() {
    let enabled = scheme::enabled()
        && PHYSICAL_HANDS_CONFIG.query(|c| c.enabled && c.body_proxies)
        && scheme::is_controllers()
        && jc3vr_core::hooks::in_gameplay()
        && !jc3vr_core::is_shutting_down();
    let targets = enabled
        .then(|| unsafe { Character::GetLocalPlayerCharacter().as_ref() })
        .flatten()
        .and_then(|player| torso_keep_out(Mat4::from(player.m_WorldMatrixT1)))
        .map(|keep_out| keep_out.capsules.map(target));
    let mut bodies = BODIES.lock();
    let Some(targets) = targets.filter(|targets| targets.iter().all(Option::is_some)) else {
        unsafe { remove(&mut bodies) };
        return;
    };
    let targets = targets.map(|target| target.expect("checked above"));
    *CAPSULES.lock() = targets.map(|target| Some(target.segment()));
    for (i, target) in targets.iter().enumerate() {
        if bodies[i].is_none() {
            bodies[i] = unsafe { make(*target) };
            if let Some(body) = &bodies[i] {
                IDS[i].store(body.body_id(), Ordering::Relaxed);
            }
        }
        let Some(body) = bodies[i].as_mut() else {
            continue;
        };
        if unsafe { drive(body, *target) }.is_err() {
            if let Some(body) = bodies[i].take() {
                unsafe { body.destroy() };
            }
            IDS[i].store(u32::MAX, Ordering::Relaxed);
        }
    }
}

/// Remove the capsules, for the shutdown.
///
/// # Safety
///
/// As [`step`].
pub(super) unsafe fn clear() {
    unsafe { remove(&mut BODIES.lock()) };
}

/// Where a capsule is to land: its centre, its orientation with the capsule's axis along local Z,
/// and its size.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Target {
    centre: Vec3,
    orientation: Quat,
    capsule: Capsule,
}

impl Target {
    /// The capsule as its axis's two ends and its radius, game space.
    fn segment(&self) -> WorldCapsule {
        let half = self.orientation * Vec3::Z * self.capsule.half_length_m;
        WorldCapsule {
            a: self.centre - half,
            b: self.centre + half,
            radius_m: self.capsule.radius_m,
        }
    }
}

/// A capsule in game space: the ends of its axis and its radius.
#[derive(Debug, Copy, Clone, PartialEq)]
struct WorldCapsule {
    a: Vec3,
    b: Vec3,
    radius_m: f32,
}

/// Whether a held body rests against the capsules after this tick: it did, or met one since, and
/// its box still reaches one.
fn episode(was: bool, met: bool, reaches: bool) -> bool {
    reaches && (was || met)
}

/// Whether the box from `min` to `max` comes within `margin_m` of `capsule`, judged at points
/// along the capsule's axis.
fn box_reaches_capsule(min: Vec3, max: Vec3, capsule: WorldCapsule, margin_m: f32) -> bool {
    (0..=AXIS_SAMPLES).any(|i| {
        let point = capsule.a.lerp(capsule.b, i as f32 / AXIS_SAMPLES as f32);
        point.clamp(min, max).distance(point) <= capsule.radius_m + margin_m
    })
}

/// The target of a keep-out capsule, slimmed so the holsters and pouches stay in reach.
fn target(capsule: KeepOutCapsule) -> Option<Target> {
    let axis = capsule.b - capsule.a;
    let length = axis.length();
    if !(capsule.a.is_finite() && capsule.b.is_finite() && length > 1e-3) {
        return None;
    }
    Some(Target {
        centre: (capsule.a + capsule.b) * 0.5,
        orientation: Quat::from_rotation_arc(Vec3::Z, axis / length),
        capsule: Capsule {
            half_length_m: length * 0.5,
            radius_m: (capsule.radius * RADIUS_SHARE).max(0.02),
        },
    })
}

/// Make a capsule at `target`, keyframed and in the simulation.
///
/// # Safety
///
/// As [`step`].
unsafe fn make(target: Target) -> Option<HandBody> {
    let group = unsafe { player_system_group() }?;
    let spec = BodySpec {
        shape: BodyShape::Capsule(target.capsule),
        mass_kg: MASS_KG,
        system_group: group,
        contact_events: false,
        gravity: false,
    };
    let mut body = unsafe {
        HandBody::create(
            Source::Hand(Hand::Left),
            spec,
            target.centre,
            target.orientation,
        )
    }
    .ok()?;
    if unsafe { body.add() }.is_err() || unsafe { body.make_keyframed() }.is_err() {
        unsafe { body.destroy() };
        return None;
    }
    tracing::info!(
        target: "controllers",
        body = body.body_id(),
        "drawn body: made a torso capsule for held objects to meet"
    );
    Some(body)
}

/// Set `body`'s velocities so the step lands it on `target`.
///
/// # Safety
///
/// As [`step`].
unsafe fn drive(body: &mut HandBody, target: Target) -> Result<(), ()> {
    let motion = unsafe { body.motion() }.map_err(|_| ())?;
    let dt = step_seconds();
    let offset = target.centre - motion.position;
    // Far off (a teleport, a respawn), it is put there rather than flown.
    if offset.length() > SNAP_M {
        return unsafe { body.place(target.centre, target.orientation) }.map_err(|_| ());
    }
    let turn = (target.orientation * motion.orientation.inverse()).normalize();
    let (axis, angle) = turn.to_axis_angle();
    let angle = if angle > std::f32::consts::PI {
        angle - std::f32::consts::TAU
    } else {
        angle
    };
    let angular = if axis.is_finite() {
        axis * (angle / dt)
    } else {
        Vec3::ZERO
    };
    unsafe { body.set_velocities(offset / dt, angular) }.map_err(|_| ())
}

/// Remove every capsule.
///
/// # Safety
///
/// As [`step`].
unsafe fn remove(bodies: &mut [Option<HandBody>; 2]) {
    for (i, slot) in bodies.iter_mut().enumerate() {
        if let Some(body) = slot.take() {
            unsafe { body.destroy() };
        }
        IDS[i].store(u32::MAX, Ordering::Relaxed);
    }
    *CAPSULES.lock() = [None; 2];
}

static BODIES: Mutex<[Option<HandBody>; 2]> = Mutex::new([None, None]);

static IDS: [AtomicU32; 2] = [AtomicU32::new(u32::MAX), AtomicU32::new(u32::MAX)];

/// The capsules where the last step put them, game space.
static CAPSULES: Mutex<[Option<WorldCapsule>; 2]> = Mutex::new([None; 2]);

/// The held bodies that met a capsule since their last [`rests_against`].
static MET: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// The held bodies resting against the capsules, with when each was last asked about.
static RESTING: Mutex<Vec<(u32, Instant)>> = Mutex::new(Vec::new());

/// How long after it was last asked about a resting body is forgotten: its hold ended.
const FORGET_AFTER: Duration = Duration::from_millis(500);

/// The most held bodies [`MET`] and [`RESTING`] keep: the two hands hold two at most.
const MAX_TRACKED: usize = 8;

/// How near a held body's box must come to a capsule to count as still resting on it, metres: the
/// broad phase's box already reaches a little past the body.
const REACH_MARGIN_M: f32 = 0.03;

/// How many intervals a capsule's axis is judged at against a box.
const AXIS_SAMPLES: usize = 8;

/// How much of the keep-out's radius a capsule takes: slimmer than the drawn body, so a hand
/// reaching for a holster or a pouch meets nothing first.
const RADIUS_SHARE: f32 = 0.8;

/// The capsules' mass at creation, kilograms; keyframed, they move as driven whatever it is.
const MASS_KG: f32 = 40.0;

/// How far off its target a capsule is put there rather than flown, metres.
const SNAP_M: f32 = 1.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_body_rests_against_the_torso_from_a_contact_until_its_box_clears() {
        // A box that reaches the torso does not rest on it before a contact says it met it.
        assert!(!episode(false, false, true));
        assert!(episode(false, true, true));
        // It keeps resting without further contacts while its box still reaches the torso.
        assert!(episode(true, false, true));
        // Once clear, it rests no longer, and a contact without the box reaching is stale.
        assert!(!episode(true, false, false));
        assert!(!episode(false, true, false));
    }

    #[test]
    fn a_box_reaches_a_capsule_within_its_radius_and_the_margin() {
        let capsule = WorldCapsule {
            a: Vec3::new(0.0, 1.0, 0.0),
            b: Vec3::new(0.0, 1.4, 0.0),
            radius_m: 0.12,
        };
        let at = |x: f32| (Vec3::new(x, 1.1, -0.1), Vec3::new(x + 0.3, 1.3, 0.1));
        let (min, max) = at(0.14);
        assert!(box_reaches_capsule(min, max, capsule, 0.03));
        let (min, max) = at(0.2);
        assert!(!box_reaches_capsule(min, max, capsule, 0.03));
        // Above the axis's far end by more than the radius, it is clear.
        let above = (Vec3::new(-0.1, 1.6, -0.1), Vec3::new(0.1, 1.8, 0.1));
        assert!(!box_reaches_capsule(above.0, above.1, capsule, 0.03));
        // A box around the capsule reaches it.
        let around = (Vec3::splat(-1.0), Vec3::splat(3.0));
        assert!(box_reaches_capsule(around.0, around.1, capsule, 0.0));
    }
}
