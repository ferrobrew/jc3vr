//! Articulated world objects: a door, a gate, a hatch, or a lever found in the world, whose body
//! hangs from the world or from another body by an enabled Havok constraint. The hand takes it as
//! a loose object whatever its weight, up to [`PickupTuning::max_articulated_mass_kg`], since the
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
//! While a hand holds one, the bodies its constraints join it to (its [`assembly`]) are damped
//! within themselves as a held ragdoll is ([`damp_assembly`]): the hand drives the body it holds
//! alone, and its neighbours, hung off it on their own constraints, kept their momentum and rang
//! against the hand (a gas cylinder in a rack on the 14 November take spasmed for five seconds).
//!
//! [`PickupTuning::max_articulated_mass_kg`]: crate::controllers::physical_hands::pickup::PickupTuning::max_articulated_mass_kg

use std::{collections::VecDeque, num::NonZeroUsize};

use glam::Vec3;
use jc3gi::physics::{hknpConstraint, hknpConstraintManager};

use crate::controllers::physical_hands::{self, physics::drawn_body, remote, rigid, world::World};

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

/// The bodies of the assembly `body` belongs to: `body` first, then every body joined to it, one
/// constraint or more away, by enabled constraints among `slots`, in the order they are found. A
/// body `excluded` says to leave out is neither taken nor passed through, and nor is one that is
/// not `dynamic`, since the world's fixed body or a keyframed one would join every body hung from
/// it. At most `max` bodies are taken.
pub fn assembly(
    slots: &[ConstraintSlot],
    body: u32,
    dynamic: impl Fn(u32) -> bool,
    excluded: impl Fn(u32) -> bool,
    max: usize,
) -> Vec<u32> {
    let mut found = vec![body];
    let mut queue = VecDeque::from([body]);
    while let Some(at) = queue.pop_front() {
        for slot in slots
            .iter()
            .filter(|s| s.body_a != hknpConstraint::INVALID_BODY_ID && !s.disabled)
        {
            let other = if slot.body_a == at {
                slot.body_b
            } else if slot.body_b == at {
                slot.body_a
            } else {
                continue;
            };
            if found.len() >= max {
                return found;
            }
            if found.contains(&other) || excluded(other) || !dynamic(other) {
                continue;
            }
            found.push(other);
            queue.push_back(other);
        }
    }
    found
}

/// The body `body` is joined to by an enabled constraint in the live world, leaving out the hand
/// bodies `hand_bodies`; `None` when there is none or no world.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn joined(body: u32, hand_bodies: [Option<u32>; 2]) -> Option<u32> {
    let slots = unsafe { live_slots() }?;
    joined_to(slots, body, |other| hand_bodies.contains(&Some(other)))
}

/// What [`damp_assembly`] found of a held body's assembly.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct AssemblyDamping {
    /// The dynamic bodies of the assembly, the held one among them.
    pub bodies: usize,
    /// Their whole mass, kilograms.
    pub mass_kg: f32,
}

/// Damp the assembly of the held body `body` within itself over a step of `dt` seconds at a rate of
/// `per_s` ([`rigid::internal_damping`]): every dynamic body joined to it ([`assembly`]), leaving
/// out the hand bodies `hand_bodies` and the drawn body's capsules, eases toward the assembly's
/// mass-weighted velocity by impulses at its centre that add up to nothing. `None` without a world
/// or when `body` cannot be read; a body the game freed since the read is passed over.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn damp_assembly(
    body: u32,
    hand_bodies: [Option<u32>; 2],
    per_s: f32,
    dt: f32,
) -> Option<AssemblyDamping> {
    let slots: Vec<ConstraintSlot> = unsafe { live_slots() }?
        .filter(|s| s.body_a != hknpConstraint::INVALID_BODY_ID && !s.disabled)
        .collect();
    let excluded = |other: u32| hand_bodies.contains(&Some(other)) || drawn_body::is_proxy(other);
    let dynamic = |other: u32| {
        unsafe { physical_hands::line_body(other) }
            .is_some_and(|(_, state)| state.mass_kg.is_some())
    };
    let ids = assembly(&slots, body, dynamic, excluded, MAX_ASSEMBLY_BODIES);
    let members: Vec<Member> = ids
        .into_iter()
        .filter_map(|body| {
            let (owner, state) = unsafe { physical_hands::line_body(body) }?;
            Some(Member {
                body,
                owner,
                mass_kg: state.mass_kg?,
                linear: state.velocities.linear,
                centre: state.pose.centre,
            })
        })
        .collect();
    if members.first().is_none_or(|m| m.body != body) {
        return None;
    }
    let masses: Vec<(f32, Vec3)> = members.iter().map(|m| (m.mass_kg, m.linear)).collect();
    let impulses = rigid::internal_damping(&masses, per_s, dt);
    for (member, impulse) in members.iter().zip(impulses) {
        // A body the game freed since the read is passed over.
        let _ = unsafe { remote::push(member.body, member.owner, impulse, member.centre) };
    }
    Some(AssemblyDamping {
        bodies: members.len(),
        mass_kg: masses.iter().map(|(mass, _)| mass).sum(),
    })
}

/// One dynamic body of a held body's assembly, as read for its damping.
struct Member {
    body: u32,
    owner: NonZeroUsize,
    mass_kg: f32,
    linear: Vec3,
    centre: Vec3,
}

/// The live world's constraint slots; `None` without a world or a usable constraint manager.
///
/// # Safety
///
/// The game thread, outside the physics step. The slots are read lazily, so the iterator must be
/// used up before the world steps.
unsafe fn live_slots() -> Option<impl Iterator<Item = ConstraintSlot>> {
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
    Some(slots.iter().map(|s| ConstraintSlot {
        body_a: s.m_bodyIdA,
        body_b: s.m_bodyIdB,
        disabled: s.m_flags & hknpConstraint::IS_DISABLED != 0,
    }))
}

/// The most bodies one assembly's damping takes: a rack or a hinged contraption has a handful, so
/// this bounds a walk into something far larger, not a real assembly.
const MAX_ASSEMBLY_BODIES: usize = 32;

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

    #[test]
    fn an_assembly_walks_its_constraints_but_not_through_fixed_or_hand_bodies() {
        // 12 hangs from 9, which hangs from the fixed world body 0 beside 40, hung there too; the
        // hand body 30 holds 12, and 13 hangs from 12 by a disabled constraint.
        let slots = [
            slot(9, 12, false),
            slot(0, 9, false),
            slot(0, 40, false),
            slot(30, 12, false),
            slot(12, 13, true),
            slot(hknpConstraint::INVALID_BODY_ID, 12, false),
        ];
        let dynamic = |b: u32| b != 0;
        let hands = |b: u32| b == 30;
        assert_eq!(assembly(&slots, 12, dynamic, hands, 32), vec![12, 9]);
        // Without the fixed body standing in the way, the walk reaches 40 through it.
        assert_eq!(
            assembly(&slots, 12, |_| true, hands, 32),
            vec![12, 9, 0, 40]
        );
        assert_eq!(
            assembly(&slots, 12, dynamic, |_| false, 32),
            vec![12, 9, 30]
        );
        assert_eq!(assembly(&slots, 7, dynamic, hands, 32), vec![7]);
    }

    #[test]
    fn an_assembly_stops_at_its_cap_and_never_repeats_a_body() {
        // A ring of four bodies.
        let slots = [
            slot(1, 2, false),
            slot(2, 3, false),
            slot(3, 4, false),
            slot(4, 1, false),
        ];
        assert_eq!(
            assembly(&slots, 1, |_| true, |_| false, 32),
            vec![1, 2, 4, 3]
        );
        assert_eq!(assembly(&slots, 1, |_| true, |_| false, 2), vec![1, 2]);
    }
}
