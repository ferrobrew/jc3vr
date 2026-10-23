//! A weapon taken off the ground straight into the grabbing hand.
//!
//! The game picks a weapon up through its use-item action: a full-body pickup animation whose
//! track message completes the pickup ([`WeaponBase::PickupItem`]), with whichever weapon the
//! character's interaction proxy offers. A swimming character instead calls `PickupItem` at once,
//! with no animation. The direct pickup makes that same call on the exact weapon the hand
//! touched: the weapon goes into the inventory and is equipped there and then, its magazine as it
//! was, and the hold puts it in the grabbing hand ([`crate::controllers::weapons::hold`]). A
//! weapon already in its slot is dropped or despawned by the game's own inventory add, as with
//! any pickup.
//!
//! A weapon `PickupItem` does not put into the inventory (a hand grenade, which the game collects
//! by proximity, and a weapon with [`WeaponBase::m_InfinitePickup`], which gives a copy) and a
//! pickup the animation forbids
//! ([`Inventory::GetAnimationAllowsPickup`](jc3gi::character::inventory::Inventory::GetAnimationAllowsPickup))
//! are refused, and the caller falls back to the use-item click.
//!
//! A weapon on a rack is taken the same way, after the call that starts the game's own pickup of
//! it ([`WeaponBase::PickupStart`]), which takes it out of the rack's state and tells the rack it
//! was picked up; the pickup animation that would complete it is never queued.
//!
//! While pistols are single ([`crate::controllers::weapons::dual_wield::single`]), a pistol taken
//! from the world is one pistol. With the slot's pistol already there and the other hand holding
//! the wielded weapon, it is held beside it, kept by the carry set, rather than taking the slot.

use std::fmt;

use glam::Mat4;
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObject,
};

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        frame::{Hand, Pose},
        loadout::{live, slot_label},
        weapons::{
            dual_wield::single,
            engine::{self, slot_from_raw},
            hold::{self, Cause, act, armed, equip},
            inventory::{self, carry, engine as inventory_engine},
            regions::{self, CarryCause, WorldTouch},
        },
    },
};

/// Why a direct pickup did not take the weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player.
    NoCharacter,
    /// The weapon is no longer lying in the world.
    NotDropped,
    /// The weapon an interaction offers is neither on its rack nor lying in the world: its raw state.
    NotOffered { state: i32 },
    /// The weapon's slot is out of range.
    UnknownSlot,
    /// A hand grenade: the game collects it by proximity.
    Grenade,
    /// The weapon gives a copy when picked up.
    InfinitePickup,
    /// The character's animation forbids a pickup now.
    AnimationForbids,
    /// The pickup ran, but the weapon is not in the hands afterwards.
    NotWielded,
}

impl Refusal {
    /// Whether the game's use-item action should be tried instead: the direct pickup did not touch
    /// the weapon.
    pub fn falls_back(self) -> bool {
        !matches!(self, Refusal::NotWielded)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refusal::NoCharacter => "there is no local player",
            Refusal::NotDropped => "the weapon is not lying in the world",
            Refusal::NotOffered { state } => {
                return write!(
                    f,
                    "weapon pickup: the offered weapon is neither on its rack nor lying in the world (state {state})"
                );
            }
            Refusal::UnknownSlot => "the weapon's slot is out of range",
            Refusal::Grenade => "a hand grenade is collected by the game",
            Refusal::InfinitePickup => "the weapon gives a copy when picked up",
            Refusal::AnimationForbids => "the animation forbids a pickup",
            Refusal::NotWielded => "the weapon is not in the hands after the pickup",
        };
        write!(f, "weapon pickup: {why}")
    }
}

impl std::error::Error for Refusal {}

/// How a direct pickup treats the weapons around it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TakeOptions {
    /// A weapon already in the slot is dropped to the ground first, rather than dropped or
    /// despawned by the game's inventory add.
    pub drop_replaced: bool,
    /// How long the gun slides from where it was into the hand, seconds; zero puts it there at
    /// once.
    pub slide_s: f32,
    /// What took the weapon, for the holding hand's readout.
    pub cause: Cause,
    /// Where the hand closed on the weapon, when a long gun is taken where the hand touched it
    /// ([`crate::controllers::weapons::regions`]); without one, it comes into the hand at its grip.
    pub touch: Option<WorldTouch>,
}

/// Pick `weapon` up into `hand` directly. Returns the slot the weapon went into.
///
/// # Safety
///
/// `weapon` must be a live weapon (its body checked to belong to it this tick, or its holder held
/// alive), on the main thread outside the world update.
pub(crate) unsafe fn take(
    hand: Hand,
    weapon: &mut WeaponBase,
    options: TakeOptions,
) -> Result<WeaponSlot, Refusal> {
    let owner = unsafe { engine::local_player() }.ok_or(Refusal::NoCharacter)?;
    // An interaction offers a rack's weapon in the rack state, and a weapon its placement rule
    // set down at a spot (a locker's) lying in the world; only the rack state needs the game's own
    // pickup started first.
    let offered = options.cause == Cause::Rack;
    let from_rack = offered && weapon.m_State == WeaponState::E_WS_WEAPON_RACK;
    match (offered, weapon.m_State) {
        (_, WeaponState::E_WS_DROPPED) | (true, WeaponState::E_WS_WEAPON_RACK) => {}
        (false, _) => return Err(Refusal::NotDropped),
        (true, _) => {
            let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
            return Err(Refusal::NotOffered { state });
        }
    }
    let slot = unsafe { takeable(owner, weapon) }?;
    let address = weapon as *mut WeaponBase as usize;
    let occupant = owner.m_Inventory.m_Weapons[slot as usize].px as usize;
    // While pistols are single, a pistol from the world is one pistol, and the slot's pistol
    // stays where it is when this one can be held beside the wielded weapon.
    let single = slot == WeaponSlot::E_SLOT_DUAL_WIELD && single::active();
    if single && occupant != 0 && occupant != address && !from_rack {
        match unsafe { take_beside(owner, hand, weapon) } {
            Ok(()) => return Ok(slot),
            Err(why) => {
                tracing::debug!(target: "controllers", why, "hold: the pistol takes the slot instead");
            }
        }
    }
    // The weapon the pickup replaces stays where the loadout table has it on the body, out of the
    // slot the pickup takes.
    let carried = occupant != 0
        && occupant != address
        && unsafe { inventory::reconcile::vacate(owner, slot) };
    if options.drop_replaced
        && !carried
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
    let carry = from
        .zip(options.touch)
        .filter(|_| slot != WeaponSlot::E_SLOT_DUAL_WIELD)
        .and_then(|(gun, touch)| unsafe {
            regions::engine::world_carry(weapon, hand, gun, &touch)
        });
    hold::begin(hand, options.cause, Some(address));
    let activator = activator(owner);
    if from_rack {
        unsafe { weapon.PickupStart(activator) };
    }
    unsafe { weapon.PickupItem(activator) };
    let wielded = unsafe { gun::held_weapon(owner) }
        .is_some_and(|held| held as *const WeaponBase as usize == address);
    if !wielded {
        return Err(Refusal::NotWielded);
    }
    unsafe { hold::adopt(address) };
    // The pickup unequipped the weapon the other hand may hold, which stays there, armed.
    unsafe { inventory::reconcile::apply(owner) };
    if let Some(carry) = carry {
        regions::begin_carry(address, hand, carry, CarryCause::Pickup);
    }
    if let Some(from) = from {
        act::slide_in(address, hand, from, options.slide_s);
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        slot = slot_label(slot),
        cause = options.cause.label(),
        "hold: picked the weapon up into the {} hand",
        hand.label()
    );
    Ok(slot)
}

/// Take the single pistol `weapon`, lying in the world, into `hand` beside the weapon the other
/// hand wields: the carry set keeps it, and the table takes it into the hand at once. Refused,
/// touching nothing, when a weapon in each hand is off, the other hand does not hold the wielded
/// weapon, `hand` holds something, or no socket that would take it is free.
///
/// # Safety
///
/// As [`take`], with `weapon` lying in the world.
unsafe fn take_beside(
    owner: &mut Character,
    hand: Hand,
    weapon: &mut WeaponBase,
) -> Result<(), &'static str> {
    let hold = Config::lock_query(|c| c.controllers.weapons.hold);
    if !(hold.either_hand && hold.second_weapon) {
        return Err("a weapon in each hand is off");
    }
    if live::in_hand(hand).is_some() || armed::in_hand(hand).is_some() {
        return Err("the hand holds a weapon already");
    }
    let other_wields = unsafe { engine::held_weapon(owner) }
        .is_some_and(|w| w.wielded.hands[hand.other().index()] && !w.wielded.hands[hand.index()]);
    if !other_wields {
        return Err("the other hand does not hold the wielded weapon");
    }
    // The carry set keeps it for the socket it would go onto, should the hand let it go there.
    let weapon_type =
        unsafe { inventory_engine::weapon_type(weapon) }.ok_or("the pistol is not a weapon")?;
    let socket = live::free_socket(weapon_type, None).ok_or("no socket would take the pistol")?;
    let strong = unsafe { inventory_engine::strong_to(weapon) }.ok_or("the pistol is gone")?;
    unsafe { single::designate(weapon, "a pistol taken from the world") };
    unsafe { single::taken_from_world(owner, weapon) };
    let object = unsafe { carry::carry_loose(owner, strong, socket) }
        .map_err(|_| "the pistol was not carried")?;
    if !unsafe { hold::take_into(hand, object) } {
        tracing::info!(target: "controllers", "hold: the pistol stays on the body");
        return Ok(());
    }
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        "hold: picked the pistol up into the {} hand beside the wielded weapon",
        hand.label()
    );
    Ok(())
}

/// Whether `owner` can take `weapon` into a hand by a direct pickup, whatever its state: its slot
/// is one the inventory holds it in, it is not a hand grenade or a weapon that gives a copy, and
/// the animation allows a pickup. Returns its slot.
///
/// # Safety
///
/// `owner` must be the live local player character and `weapon` live, on the main thread outside
/// the world update.
pub(crate) unsafe fn takeable(
    owner: &Character,
    weapon: &WeaponBase,
) -> Result<WeaponSlot, Refusal> {
    // Read as the raw value: an unset slot is out of the enum's range.
    let slot = unsafe { (&raw const weapon.m_EquipSlot).cast::<i32>().read() };
    let slot = slot_from_raw(slot).ok_or(Refusal::UnknownSlot)?;
    if slot == WeaponSlot::E_SLOT_HAND_GRENADE {
        return Err(Refusal::Grenade);
    }
    if weapon.m_InfinitePickup {
        return Err(Refusal::InfinitePickup);
    }
    if !unsafe { owner.m_Inventory.GetAnimationAllowsPickup() } {
        return Err(Refusal::AnimationForbids);
    }
    Ok(slot)
}

/// `owner`'s game-object subobject, which the pickups take as their activator.
pub(crate) fn activator(owner: &mut Character) -> *mut GameObject {
    (owner as *mut Character as usize + Character::GAME_OBJECT_BASE_OFFSET as usize)
        as *mut GameObject
}

/// The world pose of `weapon`'s first gun.
pub(crate) unsafe fn gun_pose(weapon: &WeaponBase) -> Option<Pose> {
    let first = unsafe { gun::guns(weapon) }.next().flatten()?;
    let (_, orientation, position) =
        Mat4::from(first.m_WorldMatrixT1).to_scale_rotation_translation();
    (position.is_finite() && orientation.is_finite()).then(|| Pose {
        position,
        orientation: orientation.normalize(),
    })
}
