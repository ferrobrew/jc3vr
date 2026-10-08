//! Articulated world objects: a door, a gate, a hatch, or a lever found in the world, whose body
//! hangs from the world or from another body by an enabled Havok constraint. The hand takes it as
//! a loose object whatever its weight, up to [`PickupConfig::max_articulated_mass_kg`], since the
//! constraint carries it: the hold asks for the velocities that carry the grab point to the hand,
//! and the physics step removes what the constraint forbids, so the object swings about its hinge,
//! slides along its runner, or turns about its pivot. The design is in
//! `docs/mod/input/physical-hands.md`, "Articulated objects", the engine side in
//! `docs/engine/physics/havok-bodies.md`.
//!
//! A body is articulated when the world's constraint manager holds an enabled constraint between it
//! and another body that is not one of the hand bodies: the hands' own grip joints
//! ([`crate::controllers::physical_hands::physics`]) are left out, so a held crate is not taken for
//! a hinged one.
//!
//! [`PickupConfig::max_articulated_mass_kg`]: crate::controllers::physical_hands::config::PickupConfig::max_articulated_mass_kg

use jc3gi::physics::{hknpConstraint, hknpConstraintManager};

use crate::controllers::physical_hands::world::World;

/// A constraint slot, as far as an articulation cares.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ConstraintSlot {
    pub body_a: u32,
    pub body_b: u32,
    pub disabled: bool,
}

/// The body `body` is joined to by an enabled constraint among `slots`, other than a body
/// `excluded` says to leave out; `None` when there is none. A destroyed constraint's slot, whose
/// first body is invalid, joins nothing.
pub fn joined_to(
    slots: impl IntoIterator<Item = ConstraintSlot>,
    body: u32,
    excluded: impl Fn(u32) -> bool,
) -> Option<u32> {
    slots
        .into_iter()
        .filter(|s| s.body_a != hknpConstraint::INVALID_BODY_ID && !s.disabled)
        .find_map(|s| {
            let other = if s.body_a == body {
                s.body_b
            } else if s.body_b == body {
                s.body_a
            } else {
                return None;
            };
            (other != body && !excluded(other)).then_some(other)
        })
}

/// The body `body` is joined to by an enabled constraint in the live world, leaving out the hand
/// bodies `hand_bodies`; `None` when there is none or no world.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn joined(body: u32, hand_bodies: [Option<u32>; 2]) -> Option<u32> {
    let world = unsafe { World::get() }?;
    let manager = &world.world().m_constraintManager;
    if manager.m_constraintBuffer.is_null()
        || manager.m_capacity == 0
        || manager.m_capacity == hknpConstraintManager::INVALID_CONSTRAINT_ID
    {
        return None;
    }
    let capacity = manager.m_capacity.min(MAX_CONSTRAINTS) as usize;
    // SAFETY: the manager's buffer holds its capacity's slots, read while the world is idle.
    let slots = unsafe { std::slice::from_raw_parts(manager.m_constraintBuffer, capacity) };
    joined_to(
        slots.iter().map(|s| ConstraintSlot {
            body_a: s.m_bodyIdA,
            body_b: s.m_bodyIdB,
            disabled: s.m_flags & hknpConstraint::IS_DISABLED != 0,
        }),
        body,
        |other| hand_bodies.contains(&Some(other)),
    )
}

/// The most constraint slots read, against a corrupt manager.
const MAX_CONSTRAINTS: u32 = 1 << 16;

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(body_a: u32, body_b: u32, disabled: bool) -> ConstraintSlot {
        ConstraintSlot {
            body_a,
            body_b,
            disabled,
        }
    }

    #[test]
    fn a_hinged_body_is_joined_to_what_it_hangs_from() {
        let slots = [slot(4, 5, false), slot(9, 12, false)];
        assert_eq!(joined_to(slots, 12, |_| false), Some(9));
        assert_eq!(joined_to(slots, 4, |_| false), Some(5));
        assert_eq!(joined_to(slots, 7, |_| false), None);
    }

    #[test]
    fn disabled_destroyed_and_hand_joints_join_nothing() {
        let destroyed = slot(hknpConstraint::INVALID_BODY_ID, 12, false);
        assert_eq!(
            joined_to([slot(9, 12, true), destroyed], 12, |_| false),
            None
        );
        // The hand's own grip joint is left out, and the object's hinge still found.
        let slots = [slot(30, 12, false), slot(12, 2, false)];
        assert_eq!(joined_to(slots, 12, |b| b == 30), Some(2));
        assert_eq!(joined_to([slot(30, 12, false)], 12, |b| b == 30), None);
    }
}
