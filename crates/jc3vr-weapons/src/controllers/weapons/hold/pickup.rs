//! A weapon taken off the ground straight into the grabbing hand.
//!
//! The game picks a weapon up through its use-item action: a full-body pickup animation whose
//! track message completes the pickup ([`WeaponBase::PickupItem`]), whose equip puts the wielded
//! weapon away. A direct pickup takes the exact weapon the hand touched through the loadout table
//! instead ([`take_by_table`]): the weapon becomes the player's ([`WeaponBase::OnPickup`], which
//! sets its holder and takes it out of the physics world), its stored rounds go to the reserve,
//! the carry set keeps it, and the table takes it into the grabbing hand, whatever the other hand
//! holds. The reconcile then wields it when the hand is the primary and arms it there otherwise
//! ([`crate::controllers::weapons::inventory::reconcile`]), so the game's equip never runs and the
//! other hand keeps its weapon. A weapon whose slot is taken goes into the table beside the slot's
//! weapon, which stays where it is. Where the table cannot hold the weapon in a hand of its own
//! ([`crate::controllers::loadout::take`]: the table not running, the game's layout, a heavy
//! weapon, the embedded power core, or one weapon at a time), the game's pickup takes it
//! ([`take_by_game`]), with the slot's weapon kept on the body or dropped first.
//!
//! A weapon `PickupItem` does not put into the inventory (a hand grenade, which the game collects
//! by proximity, and a weapon with [`WeaponBase::m_InfinitePickup`], which gives a copy) and a
//! native pickup the animation forbids
//! ([`Inventory::GetAnimationAllowsPickup`](jc3gi::character::inventory::Inventory::GetAnimationAllowsPickup))
//! are refused, and the caller falls back to the use-item click. A table take does not queue that
//! animation and does not require its permission.
//!
//! A weapon from any other source (a rack, a locker, a character) is taken the same way once its
//! source gave it up ([`deliver`], from [`crate::controllers::weapons::hold::source`]): a rack's
//! weapon after the call that starts the game's own pickup of it ([`WeaponBase::PickupStart`]),
//! which takes it out of the rack's state and tells the rack it was picked up; the pickup animation
//! that would complete it is never queued. Every source checks the take first ([`preflight`]).
//!
//! While pistols are single ([`crate::controllers::weapons::dual_wield::single`]), a pistol taken
//! from the world is one pistol; a two-handed weapon is never one. A two-handed weapon taken into
//! the primary hand also displaces the weapon the game still wields from a socket
//! ([`displaced_by_take`]): the take's slot is selected once that weapon can go back to its
//! socket, and until then it stays docked.

use std::sync::LazyLock;

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    hash::hashlittle,
};
use jc3vr_body::controllers::{
    aim::gun,
    held::pickup::Refusal,
    physics::{characters::character_object, local_player::LocalPlayer},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{
        Delivery, ObjectAddress, TakeSource, WeaponType, live, slot_label, take,
        take::{TakeOffer, TakeRoute},
    },
    weapons::{
        dual_wield::single,
        engine::{credit_stored, gun_pose, slot_of},
        hold,
        hold::{act, equip, use_item},
        inventory,
        inventory::{carry, engine as inventory_engine},
    },
};

/// How a direct pickup treats the weapons around it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TakeOptions {
    /// How long the gun slides from where it was into the hand, seconds; zero puts it there at
    /// once.
    pub slide_s: f32,
    /// Where the weapon was taken from, for the table and the holding hand's readout: a take off
    /// a rack of a weapon out of the rack state is a locker's.
    pub source: TakeSource,
    /// How it reached the hand. Whatever took it, and wherever the hand closed on it, the weapon
    /// comes into the hand by its grip, aligned to shoot.
    pub delivery: Delivery,
}

/// Pick `weapon`, lying in the world, up into `hand` directly. Returns the slot the weapon belongs
/// to.
///
/// # Safety
///
/// `weapon` must be a live weapon (its body checked to belong to it this tick, or its holder held alive).
pub(crate) unsafe fn take(
    game: &GameTick<'_>,
    hand: Hand,
    weapon: &mut WeaponBase,
    options: TakeOptions,
) -> Result<WeaponSlot, Refusal> {
    let mut local_player = LocalPlayer::get(game).ok_or(Refusal::NoCharacter)?;
    let owner = local_player.character_mut();
    if weapon.m_State != WeaponState::E_WS_DROPPED {
        return Err(Refusal::NotDropped);
    }
    let admitted = unsafe { preflight(owner, weapon, hand, false) }?;
    unsafe { deliver(owner, hand, weapon, admitted, options) }
}

/// Carry out a take of `weapon` that [`preflight`] admitted as `admitted`, once its source gave it
/// up: lying in the world, or with its rack's pickup started. Returns the slot the weapon belongs
/// to.
///
/// # Safety
///
/// As [`take`].
pub unsafe fn deliver(
    owner: &mut Character,
    hand: Hand,
    weapon: &mut WeaponBase,
    admitted: (WeaponSlot, TakeRoute),
    options: TakeOptions,
) -> Result<WeaponSlot, Refusal> {
    let (slot, route) = admitted;
    match route {
        TakeRoute::Table { hand } => {
            unsafe { take_by_table(owner, hand, weapon, options) }?;
            Ok(slot)
        }
        TakeRoute::Game(why) => {
            tracing::debug!(target: "controllers", why = why.label(), "hold: the game's pickup takes the weapon");
            unsafe { take_by_game(owner, hand, weapon, slot, options) }
        }
    }
}

/// The one admission check of every take into a hand, made before its source gives the weapon up:
/// the weapon is one a hand takes ([`takeable`]), the table can take it into `hand` or the game's
/// pickup can ([`route`]), and on the game's route the animation allows a pickup. `copy` is a
/// weapon a rack with unlimited stock made, whose infinite-pickup setting does not refuse it.
///
/// # Safety
///
/// The owner and weapon must be live on the input thread outside the world update.
pub unsafe fn preflight(
    owner: &Character,
    weapon: &WeaponBase,
    hand: Hand,
    copy: bool,
) -> Result<(WeaponSlot, TakeRoute), Refusal> {
    complete_preflight(
        unsafe { takeable(weapon, copy) },
        || unsafe { route(weapon, Some(hand)) },
        || unsafe { owner.m_Inventory.GetAnimationAllowsPickup() },
    )
}

/// How a take of `weapon` into `hand`, or for the game's use-item pickup into any free hand
/// (`None`), is carried out, with the table as it is now.
///
/// # Safety
///
/// `weapon` must be live, on the game thread before the world update.
pub(crate) unsafe fn route(weapon: &WeaponBase, hand: Option<Hand>) -> Result<TakeRoute, Refusal> {
    let weapon_type =
        unsafe { inventory_engine::weapon_type(weapon) }.ok_or(Refusal::UnknownSlot)?;
    let snapshot = live::snapshot_now();
    take::route(
        snapshot.as_ref(),
        TakeOffer {
            hand,
            weapon: weapon_type,
            swaps: weapon.m_WeaponNameHash == *EMBEDDED_POWER_CORE,
        },
    )
    .map_err(Refusal::Hands)
}

/// Take `weapon`, which its source gave up, into `hand` through the loadout table,
/// from `options`' source: the weapon becomes the player's without the game's equip, the carry set keeps it,
/// the table takes it into the hand, and the reconcile wields it there when the hand is the primary
/// and arms it otherwise. The weapon in the other hand stays there, and one in the slot it would
/// take stays where the table has it.
///
/// # Safety
///
/// As [`take`], with the route checked to be the table's.
pub(crate) unsafe fn take_by_table(
    owner: &mut Character,
    hand: Hand,
    weapon: &mut WeaponBase,
    options: TakeOptions,
) -> Result<(), Refusal> {
    let TakeOptions {
        source, delivery, ..
    } = options;
    let address = weapon as *mut WeaponBase as usize;
    let weapon_type =
        unsafe { inventory_engine::weapon_type(weapon) }.ok_or(Refusal::UnknownSlot)?;
    let strong = unsafe { inventory_engine::strong_to(weapon) }.ok_or(Refusal::NotDropped)?;
    let from = unsafe { gun_pose(weapon) };
    // While pistols are single, a pistol from the world is one pistol. Only the dual-wield slot's
    // weapons are pistols, so a two-handed weapon is never made one.
    let pistol = unsafe { single::designatable(weapon) };
    if pistol && unsafe { single::designate_if_active(weapon, "a pistol taken from the world") } {
        unsafe { single::taken_from_world(owner, weapon) };
    } else {
        unsafe { credit_stored(owner, weapon) };
    }
    let socket = live::free_socket(weapon_type, None)
        .or_else(|| live::body_preference(weapon_type).first().copied())
        .ok_or(Refusal::NotTaken)?;
    let object = unsafe { carry::carry_loose(owner, strong, socket) }.map_err(|error| {
        tracing::info!(target: "controllers", %error, "weapons: the table could not carry a weapon taken from the world");
        Refusal::NotTaken
    })?;
    let other = live::held_kind(hand.other());
    let taken = unsafe { inventory::held_of(object) }
        .is_some_and(|held| live::take(hand, held, source, delivery));
    if !taken {
        // Back into the world where it is, rather than kept on the body unseen by the table.
        if let Some(strong) = carry::release(object) {
            unsafe { (*inventory_engine::weapon_of(&strong)).OnDrop() };
        }
        return Err(Refusal::NotTaken);
    }
    unsafe { inventory::reconcile::apply(owner) };
    act::arm(hand);
    // The game wields the selected slot's weapon. A two-handed weapon taken into the primary hand
    // is the weapon the player holds, so the game's selection follows it once the weapon the game
    // still wields from a socket can go back there; the displaced weapon stays docked until it
    // does, rather than coming off its socket into the free hand.
    if let Some(displaced) = unsafe { displaced_by_take(owner, hand, address, weapon_type) } {
        live::displaced(displaced);
    }
    if let Some(from) = from {
        act::slide_in(address, hand, from, options.slide_s);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(weapon_type.slot()),
        source = source.label(),
        delivery = delivery.label(),
        other = %other.map_or_else(|| "nothing".to_owned(), |kind| kind.to_string()),
        "hold: took the weapon into the {} hand through the table; the {} hand keeps what it holds",
        hand.label(),
        hand.other().label()
    );
    Ok(())
}

/// Take `weapon` into `hand` through the game's own pickup, which equips it and puts the wielded
/// weapon away: the route for what the table does not hold in a hand of its own.
///
/// # Safety
///
/// As [`take`].
unsafe fn take_by_game(
    owner: &mut Character,
    hand: Hand,
    weapon: &mut WeaponBase,
    slot: WeaponSlot,
    options: TakeOptions,
) -> Result<WeaponSlot, Refusal> {
    let address = weapon as *mut WeaponBase as usize;
    let occupant = owner.m_Inventory.m_Weapons[slot as usize].px as usize;
    // The weapon the pickup replaces stays where the loadout table has it on the body, out of the
    // slot the pickup takes.
    let carried = occupant != 0
        && occupant != address
        && unsafe { inventory::reconcile::vacate(owner, slot) };
    // On the game's pickup, a weapon already in the slot that the table does not keep is dropped to
    // the ground first, rather than dropped or despawned by the game's inventory add.
    if !carried
        && occupant != 0
        && occupant != address
        && unsafe { equip::drop_occupant(owner, slot) }
    {
        tracing::info!(
            target: "controllers",
            slot = slot_label(slot),
            "hold: dropped the weapon the pickup replaces"
        );
    }
    let from = unsafe { gun_pose(weapon) };
    hold::begin(
        hand,
        live::Expect::Take(options.source, options.delivery),
        Some(address),
    );
    unsafe { use_item::own_pickup(weapon, character_object(owner)) };
    let wielded = unsafe { gun::held_weapon(owner) }
        .is_some_and(|held| held as *const WeaponBase as usize == address);
    if !wielded {
        return Err(Refusal::NotWielded);
    }
    unsafe { hold::adopt(address) };
    // The pickup unequipped the weapon the other hand may hold, which stays there, armed.
    unsafe { inventory::reconcile::apply(owner) };
    if let Some(from) = from {
        act::slide_in(address, hand, from, options.slide_s);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(slot),
        source = options.source.label(),
        delivery = options.delivery.label(),
        "hold: picked the weapon up into the {} hand through the game's pickup",
        hand.label()
    );
    Ok(slot)
}

/// Find the weapon a take of the two-handed weapon at `address` into `hand` displaced from the
/// game's hands: a weapon the game still wields that the table has on a socket. The take makes the
/// two-hander the primary hand's weapon, so when the animation allows a holster the displaced
/// weapon is unequipped back onto its socket and the take's slot is selected; otherwise the game's
/// selection is left to the reconcile, which unequips and equips when it can. Returns the displaced
/// weapon, which the table keeps docked until the game stops wielding it there. A take of another
/// slot, one into the hand that is not the primary, one whose slot does not hold the weapon, and
/// one whose game-wielded weapon is in a hand displace nothing.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
unsafe fn displaced_by_take(
    owner: &mut Character,
    hand: Hand,
    address: usize,
    weapon_type: WeaponType,
) -> Option<ObjectAddress> {
    let slot = taken_slot(weapon_type)?;
    let stale = unsafe { gun::held_weapon(owner) }
        .map(|weapon| weapon as *const WeaponBase as usize)
        .filter(|&weapon| weapon != address)
        .filter(|&weapon| live::hand_of(weapon).is_none())?;
    let primary = live::snapshot_now().is_some_and(|snapshot| snapshot.primary == Some(hand));
    let in_slot = owner
        .m_Inventory
        .m_Weapons
        .get(slot as usize)
        .is_some_and(|weapon| weapon.px as usize == address);
    if !primary || !in_slot {
        return None;
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(slot),
        "hold: the game still wields a weapon on its socket after the take; it stays docked"
    );
    // Selecting the take's slot while the displaced weapon still wields would leave that weapon
    // wielded in no selected slot, which the reconcile never unequips: let go of it first.
    if unsafe { owner.m_Inventory.GetAnimationAllowsHolstering() } {
        let weapon = inventory_engine::slot_handle(&owner.m_Inventory, stale)
            .or_else(|| unsafe { carry::weapon_handle(stale) });
        if let Some(mut weapon) = weapon {
            unsafe { weapon.as_mut().OnUnequip() };
        }
        unsafe { owner.SetSelectedWeaponSlot(slot) };
        tracing::info!(
            target: "controllers",
            slot = slot_label(slot),
            "hold: selected the taken two-handed weapon's slot"
        );
    }
    ObjectAddress::from_address(stale)
}

/// The slot a take of `weapon_type` selects: the two-handed and special slots, which the
/// projection gives a long gun. A pistol, a heavy weapon, and a consumable select none.
fn taken_slot(weapon_type: WeaponType) -> Option<WeaponSlot> {
    match weapon_type.slot() {
        slot @ (WeaponSlot::E_SLOT_TWO_HANDED | WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL) => {
            Some(slot)
        }
        _ => None,
    }
}

/// The name hash of the embedded power core, whose pickup spawns and equips another weapon in its
/// place.
static EMBEDDED_POWER_CORE: LazyLock<u32> =
    LazyLock::new(|| hashlittle(b"w210_powercore_embedded") as u32);

/// Whether `weapon` can enter a direct pickup, whatever its state: its slot is one the inventory
/// holds, and it is not a hand grenade or, unless it is a rack's `copy`, a weapon that gives a
/// copy. The native route checks animation permission separately. Returns its slot.
///
/// # Safety
///
/// `weapon` must be live, on the game thread before the world update.
unsafe fn takeable(weapon: &WeaponBase, copy: bool) -> Result<WeaponSlot, Refusal> {
    let slot = unsafe { slot_of(weapon) }.ok_or(Refusal::UnknownSlot)?;
    if slot == WeaponSlot::E_SLOT_HAND_GRENADE {
        return Err(Refusal::Grenade);
    }
    if weapon.m_InfinitePickup && !copy {
        return Err(Refusal::InfinitePickup);
    }
    Ok(slot)
}

fn complete_preflight(
    takeable: Result<WeaponSlot, Refusal>,
    route: impl FnOnce() -> Result<TakeRoute, Refusal>,
    animation: impl FnOnce() -> bool,
) -> Result<(WeaponSlot, TakeRoute), Refusal> {
    let slot = takeable?;
    Ok((slot, animation_eligibility(route()?, animation)?))
}

fn animation_eligibility(
    route: TakeRoute,
    allows_animation: impl FnOnce() -> bool,
) -> Result<TakeRoute, Refusal> {
    if matches!(route, TakeRoute::Game(_)) && !allows_animation() {
        return Err(Refusal::AnimationForbids);
    }
    Ok(route)
}

#[cfg(test)]
mod tests {
    use jc3gi::equipment::weapon::WeaponClass;

    use super::*;
    use crate::controllers::loadout::take::GameTake;

    #[test]
    fn a_take_selects_a_two_handed_weapon_s_slot_and_no_other() {
        let weapon = |tag, slot, class| WeaponType::new(tag, slot, class as i32).unwrap();
        assert_eq!(
            taken_slot(weapon(
                0x300,
                WeaponSlot::E_SLOT_TWO_HANDED,
                WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
            )),
            Some(WeaponSlot::E_SLOT_TWO_HANDED)
        );
        assert_eq!(
            taken_slot(weapon(
                0x400,
                WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL,
                WeaponClass::E_WEAPON_CLASS_SNIPER,
            )),
            Some(WeaponSlot::E_SLOT_TWO_HANDED_SPECIAL)
        );
        for (tag, slot, class) in [
            (
                0x100,
                WeaponSlot::E_SLOT_DUAL_WIELD,
                WeaponClass::E_WEAPON_CLASS_PISTOL,
            ),
            (
                0x200,
                WeaponSlot::E_SLOT_DUAL_WIELD,
                WeaponClass::E_WEAPON_CLASS_SMG,
            ),
            (
                0x500,
                WeaponSlot::E_SLOT_HEAVY,
                WeaponClass::E_WEAPON_CLASS_RPG,
            ),
        ] {
            assert_eq!(taken_slot(weapon(tag, slot, class)), None);
        }
    }

    #[test]
    fn full_preflight_refuses_before_any_source_effect_and_preserves_table_bypass() {
        let slot = WeaponSlot::E_SLOT_TWO_HANDED;
        let mut effects = Vec::new();
        for route in [
            TakeRoute::Game(GameTake::TableStopped),
            TakeRoute::Table { hand: Hand::Left },
        ] {
            let preflight = complete_preflight(Ok(slot), || Ok(route), || false);
            if preflight.is_ok() {
                effects.extend(["OnDrop", "DropWeapon", "HitReaction"]);
            }
            if matches!(route, TakeRoute::Game(_)) {
                assert_eq!(preflight, Err(Refusal::AnimationForbids));
                assert!(effects.is_empty());
            } else {
                assert_eq!(effects, ["OnDrop", "DropWeapon", "HitReaction"]);
            }
        }
        assert_eq!(
            complete_preflight(
                Err(Refusal::Grenade),
                || panic!("route after refusal"),
                || panic!("animation after refusal")
            ),
            Err(Refusal::Grenade)
        );
    }

    #[test]
    fn a_rack_s_copy_is_admitted_where_the_weapon_that_gives_it_is_not() {
        let mut storage = Box::new(std::mem::MaybeUninit::<WeaponBase>::zeroed());
        // SAFETY: the mapped enums receive valid values before the weapon is borrowed.
        let weapon = unsafe {
            (&raw mut (*storage.as_mut_ptr()).m_Class)
                .write(jc3gi::equipment::weapon::WeaponClass::E_WEAPON_CLASS_NONE);
            (&raw mut (*storage.as_mut_ptr()).m_EquipSlot).write(WeaponSlot::E_SLOT_TWO_HANDED);
            storage.assume_init_mut()
        };
        weapon.m_InfinitePickup = true;
        assert_eq!(
            unsafe { takeable(weapon, false) },
            Err(Refusal::InfinitePickup)
        );
        assert_eq!(
            unsafe { takeable(weapon, true) },
            Ok(WeaponSlot::E_SLOT_TWO_HANDED)
        );
        // SAFETY: as above.
        unsafe {
            (&raw mut weapon.m_EquipSlot).write(WeaponSlot::E_SLOT_HAND_GRENADE);
        }
        assert_eq!(unsafe { takeable(weapon, true) }, Err(Refusal::Grenade));
    }

    #[test]
    fn a_table_take_does_not_query_the_native_animation_gate() {
        let route = TakeRoute::Table { hand: Hand::Left };
        assert_eq!(
            animation_eligibility(route, || panic!("table pickup has no native animation")),
            Ok(route)
        );
    }

    #[test]
    fn a_native_take_checks_animation_permission_before_mutating_inventory() {
        for reason in [GameTake::TableStopped, GameTake::Swaps] {
            let route = TakeRoute::Game(reason);
            assert_eq!(
                animation_eligibility(route, || false),
                Err(Refusal::AnimationForbids)
            );
            assert_eq!(animation_eligibility(route, || true), Ok(route));
        }
    }
}
