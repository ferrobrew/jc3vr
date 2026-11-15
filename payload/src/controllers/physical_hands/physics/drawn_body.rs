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

use std::sync::atomic::{AtomicU32, Ordering};

use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        body::procedural::{solver::arms::Capsule as KeepOutCapsule, torso_keep_out},
        frame::Hand,
        physical_hands::physics::{
            body::{BodyShape, BodySpec, Capsule, HandBody, player_system_group},
            contact::Source,
            step::step_seconds,
        },
        scheme,
    },
    hooks,
};

/// Whether `body` is one of the drawn torso's capsules. Lock-free, for the carry collision's
/// scan of the world's bodies.
pub(crate) fn is_proxy(body: u32) -> bool {
    body != u32::MAX && IDS.iter().any(|id| id.load(Ordering::Relaxed) == body)
}

/// Make, drive, or remove the drawn torso's capsules for this step.
///
/// # Safety
///
/// The thread that steps the physics world, before the step.
pub(super) unsafe fn step() {
    let enabled = Config::lock_query(|c| {
        c.controllers.enabled
            && c.controllers.physical_hands.enabled
            && c.controllers.physical_hands.body_proxies
    }) && scheme::is_controllers()
        && hooks::in_gameplay()
        && !crate::is_shutting_down();
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
}

static BODIES: Mutex<[Option<HandBody>; 2]> = Mutex::new([None, None]);

static IDS: [AtomicU32; 2] = [AtomicU32::new(u32::MAX), AtomicU32::new(u32::MAX)];

/// How much of the keep-out's radius a capsule takes: slimmer than the drawn body, so a hand
/// reaching for a holster or a pouch meets nothing first.
const RADIUS_SHARE: f32 = 0.8;

/// The capsules' mass at creation, kilograms; keyframed, they move as driven whatever it is.
const MASS_KG: f32 = 40.0;

/// How far off its target a capsule is put there rather than flown, metres.
const SNAP_M: f32 = 1.0;
