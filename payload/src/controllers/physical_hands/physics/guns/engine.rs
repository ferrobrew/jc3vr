//! The held guns' engine reads: the guns in the hands, read on the weapon handling's input tick,
//! and, in the pre-simulation step, the gun's own body whose shape and mass the gun body takes.

use std::{fmt, num::NonZeroUsize, ptr::NonNull, time::Duration};

use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponData},
    havok::hknpShape,
    physics::{PfxRigidBody, hknpBody},
};

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    hands::grip,
    physical_hands::{
        physics::{
            body::{half_to_f32, instance_of},
            guns::{
                GunHolds, HeldGun,
                math::{GunAxes, gun_axes},
            },
        },
        world::World,
    },
    weapons::{foregrip, heft::HeftClass, hold::act, regions},
};

/// Read the guns in the hands: each hand whose gun the rigid grip pinned lately, of one of
/// `weapons` (the wielded weapon and the second weapon, each with its weight class), with no slide
/// moving it, and the hand that braces a holding hand's gun on its foregrip.
///
/// # Safety
///
/// The weapon handling's input tick, on the main thread outside the world update, with `weapons`
/// the weapons read live on it.
pub(crate) unsafe fn read_holds(weapons: [Option<(usize, HeftClass)>; 2]) -> GunHolds {
    let grips = grip::status().hands;
    let mut holds = GunHolds::default();
    for hand in Hand::BOTH {
        let Some(placed) = grips[hand.index()] else {
            continue;
        };
        let Some((weapon, class)) = weapons
            .into_iter()
            .flatten()
            .find(|(w, _)| *w == placed.weapon)
        else {
            continue;
        };
        if !placed.pinned
            || placed.at.elapsed() > PINNED_LATELY
            || act::snap_view(weapon, hand).is_some()
        {
            continue;
        }
        // SAFETY: forwarded from the caller; the weapon was read live this tick.
        holds.guns[hand.index()] =
            unsafe { held_gun(weapon, placed.gun, class) }.map(|gun| HeldGun {
                carry: regions::carry_mark(weapon, hand),
                ..gun
            });
    }
    if let Some(hold) = foregrip::status().hold.filter(|h| h.engaged) {
        let off = hold.holder.other();
        let holding =
            holds.guns[hold.holder.index()].is_some_and(|g| g.weapon.get() == hold.weapon);
        holds.braces[off.index()] = holding && holds.guns[off.index()].is_none();
    }
    holds
}

/// What a gun body is made from: the gun's own body's shape and inverse mass, and where the gun is.
pub(super) struct GunSource {
    pub shape: NonNull<hknpShape>,
    /// The gun's own body's inverse mass; zero for an infinite one.
    pub inverse_mass: f32,
    /// The gun's pose at its last placement, game space.
    pub pose: Pose,
    pub axes: Option<GunAxes>,
}

/// Why a gun body could not be made.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GunError {
    /// The physics system or its world does not exist.
    NoPhysicsWorld,
    /// The gun's own body is gone, or is no longer the gun's: the weapon was destroyed or changed.
    GunGone,
    /// The gun's own body has no shape.
    NoShape,
    /// The gun's placement is not finite.
    BadPose,
    /// The gun is too far from the hand that holds it: it is not in the hand yet.
    NotInHand,
    /// The holding hand's body is not on its palm yet, so a gun seated on it would sit off the
    /// hand.
    HandUnsettled,
    /// The gun body is not in the player's collision group, so it would collide with the player
    /// and the hands.
    NotInPlayerGroup,
    /// The gun's own body is in the simulation: the weapon was dropped or mounted.
    OwnBodyInWorld,
    /// The gun body could not be made, or joined.
    Body(&'static str),
}

impl fmt::Display for GunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            GunError::NoPhysicsWorld => "the physics world does not exist",
            GunError::GunGone => "the gun's own body is gone or no longer the gun's",
            GunError::NoShape => "the gun's own body has no shape",
            GunError::BadPose => "the gun's placement is not finite",
            GunError::NotInHand => "the gun is too far from the hand that holds it",
            GunError::HandUnsettled => "the holding hand's body is not on its palm yet",
            GunError::NotInPlayerGroup => "the gun body is not in the player's collision group",
            GunError::OwnBodyInWorld => "the gun's own body is in the world",
            GunError::Body(what) => what,
        };
        write!(f, "held guns: {what}")
    }
}

impl std::error::Error for GunError {}

/// Read what `held`'s gun body is made from, after checking that the gun's own body still belongs
/// to the gun the input tick read: its slot holds a body whose engine instance is the gun's
/// wrapper, which the wrapper's destruction, with the gun's, would have cleared.
///
/// # Safety
///
/// The thread that steps the physics world, outside the step.
pub(super) unsafe fn read_source(held: &HeldGun) -> Result<GunSource, GunError> {
    unsafe { check_own_body(held) }?;
    let world = unsafe { World::get() }.ok_or(GunError::NoPhysicsWorld)?;
    let world = world.world();
    let slot = unsafe { &*world.m_bodies.add(held.body as usize) };
    // SAFETY: the body's instance is the gun's wrapper, so the gun that owns it is alive.
    let data = unsafe { &*(held.gun.get() as *const WeaponData) };
    if data.m_PfxInstance as usize != held.wrapper.get()
        || data.m_Owner as usize != held.weapon.get()
    {
        return Err(GunError::GunGone);
    }
    let shape = NonNull::new(slot.m_shape).ok_or(GunError::NoShape)?;
    let motion = unsafe { &*world.m_motions.add(slot.m_motionId as usize) };
    let inverse_mass = half_to_f32(motion.m_inverseInertia[3]);
    let (_, orientation, position) =
        Mat4::from(data.m_WorldMatrixT1).to_scale_rotation_translation();
    if !(position.is_finite() && orientation.is_finite()) {
        return Err(GunError::BadPose);
    }
    let orientation = orientation.normalize();
    // SAFETY: the gun is alive, as above.
    let fire_point = unsafe { gun::fire_point(data) };
    Ok(GunSource {
        shape,
        inverse_mass,
        pose: Pose {
            position,
            orientation,
        },
        axes: gun_axes(fire_point, orientation.inverse() * Vec3::Y),
    })
}

/// Check that `held`'s gun is still alive and in the hand: its own body's slot holds a body whose
/// engine instance is the gun's wrapper (the wrapper's destruction, with the gun's, would have
/// cleared it), and that body is out of the simulation, as it is while the weapon is held. A gun
/// dropped or mounted since the input tick has its own body back in the world, where the gun body
/// would meet it.
///
/// # Safety
///
/// As [`read_source`].
pub(super) unsafe fn check_own_body(held: &HeldGun) -> Result<(), GunError> {
    let world = unsafe { World::get() }.ok_or(GunError::NoPhysicsWorld)?;
    let world = world.world();
    if held.body > world.m_peakBodyIndex || world.m_bodies.is_null() || world.m_motions.is_null() {
        return Err(GunError::GunGone);
    }
    let slot = unsafe { &*world.m_bodies.add(held.body as usize) };
    if slot.m_motionId == hknpBody::INVALID_MOTION_ID
        || unsafe { instance_of(world, held.body) } != Some(held.wrapper.get())
    {
        return Err(GunError::GunGone);
    }
    // SAFETY: the body's instance is the gun's live wrapper.
    let wrapper = unsafe { &*(held.wrapper.get() as *const PfxRigidBody) };
    if unsafe { wrapper.IsAddedToWorld() } {
        return Err(GunError::OwnBodyInWorld);
    }
    Ok(())
}

/// How lately the rigid grip must have pinned a gun for it to count as in the hand.
const PINNED_LATELY: Duration = Duration::from_millis(250);

/// The gun `index` of `weapon`, with its own body.
///
/// # Safety
///
/// As [`read_holds`].
unsafe fn held_gun(weapon: usize, index: usize, class: HeftClass) -> Option<HeldGun> {
    let base = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let guns = unsafe { gun::guns(base) }.count();
    let data = unsafe { gun::guns(base) }.nth(index).flatten()?;
    let wrapper = unsafe { data.m_PfxInstance.as_ref() }?;
    Some(HeldGun {
        weapon: NonZeroUsize::new(weapon)?,
        gun: NonZeroUsize::new(data as *const WeaponData as usize)?,
        wrapper: NonZeroUsize::new(data.m_PfxInstance as usize)?,
        body: wrapper.m_BodyId,
        class,
        guns,
        carry: None,
    })
}
