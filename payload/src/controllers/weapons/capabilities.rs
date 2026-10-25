//! Held-gun interaction permission and physical-hand trigger ownership, independent of locomotion.

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};

use crate::controllers::{
    frame::{self, Hand},
    hands::tiers::{self, HandTier, TierState},
    loadout::live,
    mode::{AirborneVariant, Mode, OnFootVariant},
    weapons::{
        dual_wield::single, foregrip, hold::armed, inventory::engine::slot_of, regions, reload,
    },
};

/// The held-gun operations available in a control mode. Tracking and input ownership gate each hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct Capabilities {
    pub fire: bool,
    pub release: bool,
    pub transfer: bool,
    pub support: bool,
    pub dock: bool,
    pub reload: bool,
}

impl Capabilities {
    pub(crate) const fn of(mode: Mode) -> Self {
        match mode {
            Mode::OnFoot(OnFootVariant::Ground | OnFootVariant::Swimming)
            | Mode::Airborne(
                AirborneVariant::Falling | AirborneVariant::Parachute | AirborneVariant::Wingsuit,
            )
            | Mode::Vehicle(_) => Self {
                fire: true,
                release: true,
                transfer: true,
                support: true,
                dock: true,
                reload: true,
            },
            Mode::Ui => Self {
                fire: false,
                release: false,
                transfer: false,
                support: false,
                dock: false,
                reload: false,
            },
        }
    }
}

/// How a physical hand participates in holding a gun, regardless of the acting-hand setting.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum GunHand {
    #[default]
    Empty,
    Grip,
    Carry,
    Support,
    /// One gun of the native pair, whose shared fire input alternates the guns.
    NativePair(NativePair),
}

/// The orientation of the native pair's shared weapon identity across both physical hands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct NativePair {
    pub first_hand: Hand,
}

/// A hand's trigger is reserved while it holds any part of a gun. Only a firing grip can shoot.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct TriggerClaim {
    pub reserved: bool,
    pub fire: bool,
}

pub(crate) fn trigger_claims(
    capabilities: Capabilities,
    hands: [GunHand; 2],
    ready: [bool; 2],
) -> [TriggerClaim; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        TriggerClaim {
            reserved: capabilities.fire && hands[i] != GunHand::Empty,
            fire: capabilities.fire && ready[i] && hands[i] == GunHand::Grip,
        }
    })
}

/// A physical hand's gun identity and role, for the input owner's neutral ownership transfer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct GunOwner {
    pub weapon: usize,
    pub role: GunHand,
}

/// The current ownership after any draw, release, or handover in the input tick.
pub(crate) fn gun_owners_now() -> [Option<GunOwner>; 2] {
    let support = foregrip::status().hold.filter(|hold| hold.engaged);
    let held = Hand::BOTH.map(|hand| {
        let weapon = live::in_hand(hand).or_else(|| armed::in_hand(hand))?;
        // SAFETY: the live table or the armed set keeps the weapon alive on the input thread.
        let slot = unsafe {
            (weapon as *const WeaponBase)
                .as_ref()
                .and_then(|weapon| slot_of(weapon))
        };
        Some(DiscoveredGun {
            weapon,
            slot,
            single: single::is(weapon),
            carrying: regions::carrying(weapon).is_some(),
        })
    });
    discover_owners(held, support.map(|hold| (hold.weapon, hold.holder.other())))
}

/// Current engine and mechanic permission for a held gun's fire hint, independent of trigger value.
/// Runs after final ownership reconciliation on the game input thread.
pub(crate) fn fire_ready_now(ready: [bool; 2], block_actions: bool) -> [bool; 2] {
    // SAFETY: the input thread resolves the current local character without retaining the borrow.
    let animation = unsafe { Character::GetLocalPlayerCharacter().as_ref() }
        .is_some_and(|owner| unsafe { owner.m_Inventory.GetAnimationAllowsShooting() });
    let owners = gun_owners_now();
    let action = tiers::status();
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        ready[i]
            && animation
            && owners[i].is_some_and(|owner| {
                matches!(owner.role, GunHand::Grip | GunHand::NativePair(_))
                    && !reload::holds_fire(owner.weapon, hand)
                    && !(block_actions
                        && action.state == Some(TierState::WeaponAction)
                        && action.effective[i] == HandTier::ActionRuns)
            })
    })
}

#[derive(Debug, Copy, Clone)]
struct DiscoveredGun {
    weapon: usize,
    slot: Option<WeaponSlot>,
    single: bool,
    carrying: bool,
}

fn discover_owners(
    held: [Option<DiscoveredGun>; 2],
    support: Option<(usize, Hand)>,
) -> [Option<GunOwner>; 2] {
    if let Some((first_hand, pair)) = Hand::BOTH.into_iter().find_map(|hand| {
        held[hand.index()]
            .filter(|gun| gun.slot == Some(WeaponSlot::E_SLOT_DUAL_WIELD) && !gun.single)
            .map(|gun| (hand, gun))
    }) {
        return [Some(GunOwner {
            weapon: pair.weapon,
            role: GunHand::NativePair(NativePair { first_hand }),
        }); 2];
    }
    Hand::BOTH.map(|hand| {
        held[hand.index()]
            .map(|gun| GunOwner {
                weapon: gun.weapon,
                role: if gun.carrying {
                    GunHand::Carry
                } else {
                    GunHand::Grip
                },
            })
            .or_else(|| {
                support
                    .filter(|(_, supporting)| *supporting == hand)
                    .map(|(weapon, _)| GunOwner {
                        weapon,
                        role: GunHand::Support,
                    })
            })
    })
}

pub(crate) fn gun_hands_now() -> [GunHand; 2] {
    gun_owners_now().map(|owner| {
        owner
            .filter(|owner| owner.weapon != 0)
            .map_or(GunHand::Empty, |owner| owner.role)
    })
}

/// Tracking validity alone can retain an extrapolated pose; firing requires a tracked grip.
pub(crate) fn tracked_hands() -> [bool; 2] {
    frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let hand = frame.hand(hand);
            frame.focused && hand.active && hand.grip.is_tracked()
        })
    })
    .unwrap_or([false; 2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::mode::{VehicleClass, VehicleSeat};

    fn modes() -> Vec<Mode> {
        let mut modes = vec![
            Mode::OnFoot(OnFootVariant::Ground),
            Mode::OnFoot(OnFootVariant::Swimming),
            Mode::Airborne(AirborneVariant::Falling),
            Mode::Airborne(AirborneVariant::Parachute),
            Mode::Airborne(AirborneVariant::Wingsuit),
        ];
        for class in [
            VehicleClass::Car,
            VehicleClass::Motorcycle,
            VehicleClass::Sea,
            VehicleClass::Helicopter,
            VehicleClass::Plane,
            VehicleClass::Mech,
            VehicleClass::Unknown,
        ] {
            for driver in [false, true] {
                modes.push(Mode::Vehicle(VehicleSeat { class, driver }));
            }
        }
        modes
    }

    #[test]
    fn unsplit_native_pair_discovery_reserves_both_hands_without_substitute_fire() {
        let pair = DiscoveredGun {
            weapon: 1,
            slot: Some(WeaponSlot::E_SLOT_DUAL_WIELD),
            single: false,
            carrying: false,
        };
        for first_hand in Hand::BOTH {
            let held = Hand::BOTH.map(|hand| (hand == first_hand).then_some(pair));
            let owners = discover_owners(held, None);
            assert_eq!(
                owners,
                [Some(GunOwner {
                    weapon: pair.weapon,
                    role: GunHand::NativePair(NativePair { first_hand }),
                }); 2]
            );
            let roles = owners.map(|owner| owner.unwrap().role);
            let claims = trigger_claims(
                Capabilities::of(Mode::OnFoot(OnFootVariant::Ground)),
                roles,
                [true; 2],
            );
            assert_eq!(claims.map(|claim| claim.reserved), [true; 2]);
            assert_eq!(claims.map(|claim| claim.fire), [false; 2]);
            for dominant in Hand::BOTH {
                assert!(
                    claims[dominant.other().index()].reserved,
                    "the native off-hand fire route must replace trigger retract"
                );
            }
            let single = discover_owners(
                held.map(|gun| {
                    gun.map(|gun| DiscoveredGun {
                        single: true,
                        ..gun
                    })
                }),
                None,
            );
            assert_eq!(single[first_hand.index()].unwrap().role, GunHand::Grip);
            assert!(single[first_hand.other().index()].is_none());
        }
    }

    #[test]
    fn held_gun_operations_do_not_depend_on_locomotion_or_seat() {
        let on_foot = Capabilities::of(Mode::OnFoot(OnFootVariant::Ground));
        for mode in modes() {
            assert_eq!(Capabilities::of(mode), on_foot, "{mode:?}");
        }
        assert_eq!(Capabilities::of(Mode::Ui), Capabilities::default());
    }

    #[test]
    fn physical_hands_own_only_their_own_guns_in_every_gameplay_mode() {
        for mode in modes() {
            for gun_hand in Hand::BOTH {
                let hands = Hand::BOTH.map(|hand| {
                    if hand == gun_hand {
                        GunHand::Grip
                    } else {
                        GunHand::Empty
                    }
                });
                let claims = trigger_claims(Capabilities::of(mode), hands, [true; 2]);
                assert_eq!(
                    claims[gun_hand.index()],
                    TriggerClaim {
                        reserved: true,
                        fire: true
                    }
                );
                assert_eq!(claims[gun_hand.other().index()], TriggerClaim::default());
            }
            assert_eq!(
                trigger_claims(Capabilities::of(mode), [GunHand::Grip; 2], [true; 2]),
                [TriggerClaim {
                    reserved: true,
                    fire: true
                }; 2],
            );
        }
    }

    #[test]
    fn carrying_and_support_reserve_the_trigger_without_firing() {
        for mode in modes() {
            for hand in Hand::BOTH {
                for occupied in [GunHand::Carry, GunHand::Support] {
                    let hands =
                        Hand::BOTH.map(|h| if h == hand { occupied } else { GunHand::Grip });
                    let claims = trigger_claims(Capabilities::of(mode), hands, [true; 2]);
                    assert_eq!(
                        claims[hand.index()],
                        TriggerClaim {
                            reserved: true,
                            fire: false
                        }
                    );
                    assert!(claims[hand.other().index()].fire);
                }
            }
        }
    }

    #[test]
    fn input_owner_clears_cached_fire_while_ui_and_action_set_are_paused() {
        let source = include_str!("../bindings/mod.rs");
        let pending_branch = source
            .split_once("if set_pending {")
            .unwrap()
            .1
            .split_once("let paused =")
            .unwrap()
            .0;
        assert!(pending_branch.contains("clear_controller();"));
        assert!(pending_branch.contains("weapons::hold::tick::publish_fire([false; 2]);"));
        assert!(source.contains("} else {\n        weapons::hold::tick::publish_fire([false; 2]);\n        weapons::wheel::pause();"));
        let tick = include_str!("hold/tick.rs");
        assert!(tick.contains("if !wielded || pair {\n        return None;"));
        assert_eq!(tick.matches(".GetAnimationAllowsShooting()").count(), 2);
    }

    #[test]
    fn ui_or_unready_hands_cannot_fire() {
        assert_eq!(
            trigger_claims(Capabilities::of(Mode::Ui), [GunHand::Grip; 2], [true; 2]),
            [TriggerClaim::default(); 2],
        );
        for mode in modes() {
            for ready in [[false; 2], [true, false], [false, true]] {
                let claims = trigger_claims(Capabilities::of(mode), [GunHand::Grip; 2], ready);
                assert_eq!(claims.map(|claim| claim.fire), ready);
                assert_eq!(claims.map(|claim| claim.reserved), [true; 2]);
            }
        }
    }
}
