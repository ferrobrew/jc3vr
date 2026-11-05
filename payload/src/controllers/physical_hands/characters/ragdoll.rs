//! A ragdolling character's bodies: the bone a hand or the grapple takes it by, and the weight of
//! the whole ragdoll that bone drags. The engine side is in `docs/engine/character/ragdoll.md`.
//!
//! The bodies are read from the character's own ragdoll ([`Character::m_Ragdoll`]) on every call,
//! so a ragdoll the game has freed or rebuilt since is never trusted, and each body is read through
//! [`physical_hands::line_body`], which checks that its engine instance still belongs to its owner
//! and that it is dynamic: a ragdoll whose bones the animation drives keyframed has none to take.
//!
//! Every function here runs on the game thread outside the physics step.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::{character::character::Character, physics::hknpRagdoll};

use crate::controllers::physical_hands;

/// One dynamic body of a character's ragdoll.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Bone {
    /// The object that owns the body, as a physics query reports it.
    pub owner: NonZeroUsize,
    pub body: u32,
    /// The body's centre of mass, world space.
    pub centre: Vec3,
    pub mass_kg: f32,
}

/// A ragdolling character's dynamic bodies: the bone nearest a point, and the whole ragdoll's mass.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Ragdoll {
    bones: Vec<Bone>,
}

impl Ragdoll {
    /// The dynamic bodies of `character`'s ragdoll while it ragdolls
    /// ([`Character::IsRagdolling`]), or `None` when it does not, or has no dynamic body.
    ///
    /// # Safety
    ///
    /// `character` must be a live character.
    pub(crate) unsafe fn of(character: &Character) -> Option<Self> {
        if !unsafe { character.IsRagdolling() } {
            return None;
        }
        let instance = unsafe { character.m_Ragdoll.px.as_ref() }?;
        let ragdoll = unsafe { instance.m_Ragdoll.as_ref() }?;
        let ids = &ragdoll.m_bodyIds;
        let count = usize::try_from(ids.size).ok()?.min(MAX_BONES);
        if ids.data.is_null() || count == 0 {
            return None;
        }
        // SAFETY: the ragdoll's own array of `size` body ids, read on the thread that steps the
        // world, outside the step.
        let ids = unsafe { std::slice::from_raw_parts(ids.data, count) };
        let bones: Vec<Bone> = ids
            .iter()
            .copied()
            .filter(|&body| body != hknpRagdoll::INVALID_BODY_ID)
            .filter_map(|body| {
                let (owner, state) = unsafe { physical_hands::line_body(body) }?;
                Some(Bone {
                    owner,
                    body,
                    centre: state.pose.centre,
                    mass_kg: state.mass_kg?,
                })
            })
            .collect();
        Self::from_bones(bones)
    }

    /// A ragdoll of `bones`, or `None` without any.
    pub(crate) fn from_bones(bones: Vec<Bone>) -> Option<Self> {
        let bones: Vec<Bone> = bones
            .into_iter()
            .filter(|b| b.centre.is_finite() && b.mass_kg.is_finite() && b.mass_kg > 0.0)
            .collect();
        (!bones.is_empty()).then_some(Self { bones })
    }

    /// The bone whose centre is nearest `point`.
    pub(crate) fn nearest(&self, point: Vec3) -> Option<Bone> {
        self.bones
            .iter()
            .copied()
            .min_by(|a, b| {
                a.centre
                    .distance_squared(point)
                    .total_cmp(&b.centre.distance_squared(point))
            })
            .filter(|_| point.is_finite())
    }

    /// The bone of the body `body`, when it is one of the ragdoll's.
    pub(crate) fn bone(&self, body: u32) -> Option<Bone> {
        self.bones.iter().copied().find(|b| b.body == body)
    }

    /// The whole ragdoll's mass, kilograms: what a hand or a line pulling one bone drags.
    pub(crate) fn mass_kg(&self) -> f32 {
        self.bones.iter().map(|b| b.mass_kg).sum()
    }
}

/// The most bodies read from one ragdoll: a human ragdoll has a few dozen at most, so this bounds
/// a corrupt count, not a real ragdoll.
const MAX_BONES: usize = 128;

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(body: u32, centre: Vec3, mass_kg: f32) -> Bone {
        Bone {
            owner: NonZeroUsize::new(64).unwrap(),
            body,
            centre,
            mass_kg,
        }
    }

    #[test]
    fn the_nearest_bone_to_the_palm_is_taken_and_the_whole_body_weighs() {
        let ragdoll = Ragdoll::from_bones(vec![
            bone(1, Vec3::new(0.0, 0.2, 0.0), 12.0),
            bone(2, Vec3::new(0.0, 0.9, 0.0), 30.0),
            bone(3, Vec3::new(0.3, 1.4, 0.0), 4.0),
        ])
        .unwrap();
        assert_eq!(ragdoll.nearest(Vec3::new(0.25, 1.3, 0.0)).unwrap().body, 3);
        assert_eq!(ragdoll.nearest(Vec3::new(0.0, 0.0, 0.0)).unwrap().body, 1);
        assert_eq!(ragdoll.nearest(Vec3::NAN), None);
        assert!((ragdoll.mass_kg() - 46.0).abs() < 1e-4);
        assert_eq!(ragdoll.bone(2).unwrap().mass_kg, 30.0);
        assert_eq!(ragdoll.bone(9), None);
    }

    #[test]
    fn a_ragdoll_without_a_sound_body_is_none() {
        assert_eq!(Ragdoll::from_bones(Vec::new()), None);
        assert_eq!(
            Ragdoll::from_bones(vec![
                bone(1, Vec3::NAN, 10.0),
                bone(2, Vec3::ZERO, 0.0),
                bone(3, Vec3::ZERO, f32::INFINITY),
            ]),
            None
        );
    }
}
