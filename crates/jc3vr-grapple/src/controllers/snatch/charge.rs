//! Physical retrieval of a placed charge without a replacement actor during flight.
//!
//! The original charge stays on its weapon's active list while its native body travels. A missed
//! catch or a cancelled line therefore leaves one live charge, with the same detonation ownership.
//! Only the catch requests the ordinary placed-charge swap. Pooled ammunition uses a weak handle:
//! a strong reference must not outlive the ammunition manager's forced removal.

use std::num::NonZeroUsize;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::{
        explosive::{Explosive, PlantedExplosive},
        weapon::PlantedExplosiveWeapon,
    },
    game::GameObject,
    physics::PhysicsGameObject,
};
use jc3vr_body::controllers::{
    aim::targets::weak_target, held::native::object_of, physics::refs::Weak,
};
use jc3vr_weapons::controllers::weapons::{
    c4::placed::ChargeIdentity as Identity, grenade::catch::body_of,
};

/// A placed charge's allocation identity and weak lifetime during retrieval.
pub(super) struct FlightCharge {
    weak: Weak,
    identity: Identity,
}

impl FlightCharge {
    /// The allocation retained by the recovery queue after the flight ends.
    pub(super) fn identity(&self) -> Identity {
        self.identity
    }

    /// Whether the original allocation is still on the local C4 weapon's active list.
    ///
    /// The caller runs on the game thread, outside the physics step.
    pub(super) fn live(&self) -> bool {
        // SAFETY: the snatch accesses the target only in the input tick's game stage.
        unsafe { self.placed() }.is_some()
    }

    /// Detach the original charge through the engine's drop path, retaining its active-list entry.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update and the physics step.
    pub(super) unsafe fn detach(&mut self) -> Option<(u32, NonZeroUsize)> {
        let placed = unsafe { self.placed() }?;
        let explosive = unsafe { &*placed.cast::<Explosive>() };
        let (body, owner) = unsafe { body_of(explosive) }?;
        if !owner_matches(self.identity.charge, owner.get()) {
            return None;
        }
        // DropExplosive restores the native body to the simulation. The charge remains live, so
        // its active count, detonation registration, and rendered model belong to that same actor.
        unsafe { (*placed).DropExplosive() };
        Some((body, owner))
    }

    unsafe fn placed(&self) -> Option<*mut PlantedExplosive> {
        let game_object = unsafe { weak_target(self.weak.raw()) }? as usize;
        if game_object != game_object_address(self.identity.charge)? {
            return None;
        }
        let placed = unsafe { active(self.identity.charge) }?;
        let explosive = unsafe { &*placed.cast::<Explosive>() };
        self.identity
            .matches(explosive.m_PoolIndex, explosive.m_InstanceUniqueId)
            .then_some(placed)
    }
}

/// Validate the actual active list before obtaining a weak reference to an offered charge.
///
/// # Safety
///
/// The game thread, before the world update; no serial weapon update runs concurrently.
pub(super) unsafe fn flight(identity: Identity) -> Option<FlightCharge> {
    let placed = unsafe { active(identity.charge) }?;
    let explosive = unsafe { &*placed.cast::<Explosive>() };
    flight_with(
        identity,
        explosive.m_PoolIndex,
        explosive.m_InstanceUniqueId,
        || {
            let object = game_object_address(identity.charge)? as *const GameObject;
            Some(unsafe { Weak::of(&(*object).m_WeakThis) })
        },
    )
}

fn flight_with(
    identity: Identity,
    pool: u32,
    instance: u32,
    weak: impl FnOnce() -> Option<Weak>,
) -> Option<FlightCharge> {
    if !identity.matches(pool, instance) {
        return None;
    }
    Some(FlightCharge {
        weak: weak()?,
        identity,
    })
}

fn game_object_address(charge: usize) -> Option<usize> {
    charge.checked_add(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
}

fn owner_matches(charge: usize, owner: usize) -> bool {
    game_object_address(charge)
        .zip(owner.checked_add(PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize))
        .is_some_and(|(charge_object, owner_object)| charge_object == owner_object)
}

unsafe fn active(charge: usize) -> Option<*mut PlantedExplosive> {
    let player = unsafe { Character::GetLocalPlayerCharacter().as_ref() }?;
    let slot = WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE as usize;
    let base = unsafe { player.m_Inventory.m_Weapons.get(slot)?.px.as_ref() }?;
    let object = object_of(base);
    if !unsafe { (*object).IsType(PlantedExplosiveWeapon::TYPE_ID()) } {
        return None;
    }
    let weapon = unsafe { &*std::ptr::from_ref(base).cast::<PlantedExplosiveWeapon>() };
    let list = &weapon.m_ActiveExplosives;
    if list.begin.is_null() || list.is_empty() {
        return None;
    }
    unsafe { std::slice::from_raw_parts(list.begin, list.len()) }
        .iter()
        .map(|entry| entry.m_Explosive)
        .find(|placed| !placed.is_null() && *placed as usize == charge)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use jc3vr_input::frame::Hand;

    use super::*;
    use crate::controllers::snatch::{
        catch::{CatchHand, CatchTuning, CatchWatch},
        rules::{FlightLimits, FlightSample, Verdict, verdict},
        target::OfferKind,
    };

    #[test]
    fn a_recycled_pool_slot_is_not_the_original_charge() {
        let identity = Identity {
            charge: 1,
            pool: 7,
            instance: 19,
        };
        assert!(identity.matches(7, 19));
        assert!(!identity.matches(7, 20));
        assert!(!identity.matches(8, 19));
    }

    #[test]
    fn a_published_charge_offer_rejects_a_recycled_address_before_obtaining_a_weak_handle() {
        let original = Identity {
            charge: 1,
            pool: 7,
            instance: 19,
        };
        let offer = OfferKind::PlacedCharge { identity: original };
        let OfferKind::PlacedCharge { identity } = offer else {
            unreachable!();
        };
        assert_eq!(identity, original);
        for (pool, instance) in [(7, 20), (8, 19)] {
            assert!(
                flight_with(identity, pool, instance, || {
                    panic!("a replacement must not obtain a weak handle or reach detach")
                })
                .is_none()
            );
        }
        let mut obtained = false;
        assert!(
            flight_with(identity, 7, 19, || {
                obtained = true;
                None
            })
            .is_none()
        );
        assert!(obtained);
    }

    #[test]
    fn the_hook_owner_and_the_explosive_reference_reach_the_same_game_object() {
        let charge = 0x1000;
        let object = game_object_address(charge).unwrap();
        let owner = object - PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize;
        assert!(owner_matches(charge, owner));
        assert!(!owner_matches(charge, charge));
        assert!(!owner_matches(charge, owner + 1));
    }

    #[test]
    fn a_charge_uses_the_flight_slack_and_swept_catch_before_delivery() {
        let mut catch = CatchWatch::default();
        let tuning = CatchTuning {
            zone_m: 0.2,
            grace_s: 0.1,
        };
        let limits = FlightLimits {
            slack_radius_m: 0.5,
            line_m: 20.0,
            max_slack_s: 0.5,
        };
        let hands = [
            Some(CatchHand {
                palm: Vec3::ZERO,
                gripping: true,
            }),
            None,
        ];
        let mut tick = |from: Vec3, to: Vec3, slack_s: Option<f32>| {
            verdict(
                &FlightSample {
                    to,
                    velocity: Vec3::Z * 6.0,
                    hand: Vec3::ZERO,
                    catchers: [Some(Vec3::ZERO), None],
                    catch: catch.step(1.0 / 30.0, from, to, hands, &tuning),
                    slack_s,
                },
                false,
                &limits,
            )
        };
        assert_eq!(
            tick(Vec3::NEG_Z * 5.0, Vec3::NEG_Z * 4.8, None),
            Verdict::Pulling
        );
        assert_eq!(
            tick(Vec3::NEG_Z * 0.8, Vec3::NEG_Z * 0.4, None),
            Verdict::Slack
        );
        assert_eq!(
            tick(Vec3::NEG_Z * 0.4, Vec3::Z * 0.4, Some(0.03)),
            Verdict::Caught(Hand::Left)
        );
    }

    #[test]
    fn a_charge_that_misses_the_hand_is_never_delivered() {
        let mut catch = CatchWatch::default();
        let from = Vec3::new(1.0, 0.0, -0.4);
        let to = Vec3::new(1.0, 0.0, 0.4);
        let caught = catch.step(
            0.03,
            from,
            to,
            [
                Some(CatchHand {
                    palm: Vec3::ZERO,
                    gripping: true,
                }),
                None,
            ],
            &CatchTuning {
                zone_m: 0.2,
                grace_s: 0.1,
            },
        );
        assert_eq!(caught.caught, None);
        let sample = FlightSample {
            to,
            velocity: Vec3::Z * 6.0,
            hand: Vec3::ZERO,
            catchers: [Some(Vec3::ZERO), None],
            catch: caught,
            slack_s: Some(0.6),
        };
        assert_eq!(
            verdict(
                &sample,
                false,
                &FlightLimits {
                    slack_radius_m: 0.5,
                    line_m: 20.0,
                    max_slack_s: 0.5,
                }
            ),
            Verdict::Missed
        );
    }

    #[test]
    fn invalid_object_address_arithmetic_is_rejected() {
        assert_eq!(game_object_address(usize::MAX), None);
        assert!(!owner_matches(usize::MAX, 1));
        assert!(!owner_matches(1, usize::MAX));
        assert!(!owner_matches(usize::MAX, usize::MAX));
    }
}
