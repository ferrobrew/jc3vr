//! What the hands do with the carry set on the input tick: draw a carried weapon off its socket,
//! put the held weapon onto a free socket, and carry the weapon a pickup replaces rather than drop
//! it. And where the sockets' grab volumes go.
//!
//! Everything here runs on the input tick, on the main thread outside the world update.

use std::fmt;

use glam::{Mat4, Vec3};
use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::WeaponBase,
};

use crate::{
    config::Config,
    controllers::{
        aim::gun,
        frame::Hand,
        hands::grip,
        weapons::{
            dual_wield, engine as weapons_engine,
            hold::{self, act, holder::Cause, pickup},
            holster::slot_label,
            inventory::{
                self, carry,
                carry::CarryRefusal,
                config::InventoryConfig,
                engine as inventory_engine,
                registry::ObjectKey,
                sockets::{Fit, SocketId, fit_of},
            },
        },
    },
};

/// A socket's grab volume, in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SocketPoint {
    pub socket: SocketId,
    pub point: Vec3,
    /// Whether a carried weapon sits there; a free socket's volume is for the hand holding a
    /// weapon that fits it.
    pub occupied: bool,
}

/// Why a hand's grab of a socket did nothing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SocketRefusal {
    NoCharacter,
    /// The other hand holds a weapon, and a carried weapon is drawn only into a hand when the
    /// other is free.
    OtherHandFull,
    /// The socket is free and the hand holds nothing to put there.
    Empty,
    /// The held weapon does not fit the socket, or is not one the inventory carries.
    DoesNotFit,
    Carry(CarryRefusal),
}

impl fmt::Display for SocketRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketRefusal::NoCharacter => write!(f, "socket: there is no local player"),
            SocketRefusal::OtherHandFull => write!(f, "socket: the other hand holds a weapon"),
            SocketRefusal::Empty => write!(f, "socket: nothing is carried there"),
            SocketRefusal::DoesNotFit => write!(f, "socket: the weapon does not fit there"),
            SocketRefusal::Carry(refusal) => write!(f, "socket: {refusal}"),
        }
    }
}

impl std::error::Error for SocketRefusal {}

/// What a grab of a socket did.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SocketAction {
    /// The carried weapon came into the hand.
    Drew,
    /// The held weapon went onto the socket.
    Stowed,
}

/// What the weapon handling hands a socket grab.
pub(crate) struct SocketGrab {
    pub hand: Hand,
    pub socket: SocketId,
    /// The wielded weapon and the hands holding its guns.
    pub wielded: Option<(ObjectKey, [bool; 2])>,
    /// Whether the other hand holds the mod's second weapon.
    pub other_holds_second: bool,
    /// How long a drawn weapon slides into the hand, seconds.
    pub snap_s: f32,
    pub independent_pistols: bool,
}

/// The sockets' grab volumes in the world: each carried weapon's grip, and each free socket that
/// fits the wielded weapon `wielded`, for putting it there.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn socket_points(
    owner: &Character,
    settings: &InventoryConfig,
    dominant: Hand,
    wielded: Option<ObjectKey>,
) -> Vec<SocketPoint> {
    if !(settings.enabled && settings.carry) {
        return Vec::new();
    }
    let mut out: Vec<SocketPoint> = unsafe { carry::grips() }
        .into_iter()
        .map(|(socket, point)| SocketPoint {
            socket,
            point,
            occupied: true,
        })
        .collect();
    let Some(fit) = wielded.and_then(|w| unsafe { fit_of_weapon(w as *const WeaponBase) }) else {
        return out;
    };
    let layout = settings.layout(dominant);
    let taken = carry::taken();
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    for socket in layout.choices(fit) {
        if taken.contains(&socket) {
            continue;
        }
        let Some(bone) = (unsafe { grip::bone_model_pose(owner, socket.bone()) }) else {
            continue;
        };
        let point = to_world.transform_point3(bone.position)
            + to_world.transform_vector3(settings.offsets.of(socket));
        if point.is_finite() {
            out.push(SocketPoint {
                socket,
                point,
                occupied: false,
            });
        }
    }
    out
}

/// Carry out `grab`: a hand without a gun takes the carried weapon off the socket, and the hand
/// holding the wielded weapon puts it onto a free socket.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn grab(grab: &SocketGrab) -> Result<SocketAction, SocketRefusal> {
    let owner = unsafe { weapons_engine::local_player() }.ok_or(SocketRefusal::NoCharacter)?;
    let SocketGrab { hand, socket, .. } = *grab;
    let holds_wielded = grab.wielded.is_some_and(|(_, hands)| hands[hand.index()]);
    match carry::on_socket(socket) {
        Some(object) if !holds_wielded => {
            let other_full = grab.other_holds_second
                || grab
                    .wielded
                    .is_some_and(|(_, hands)| hands[hand.other().index()]);
            if other_full {
                return Err(SocketRefusal::OtherHandFull);
            }
            unsafe { draw(owner, object, grab) }.map(|()| SocketAction::Drew)
        }
        Some(_) => Err(SocketRefusal::DoesNotFit),
        None if holds_wielded => {
            let both = grab.wielded.is_some_and(|(_, hands)| hands == [true; 2]);
            unsafe { stow(owner, hand, socket, both) }.map(|()| SocketAction::Stowed)
        }
        None => Err(SocketRefusal::Empty),
    }
}

/// Put the wielded weapon, held in `hand`, onto the free socket `socket`. Called when the hand's
/// grip opens there or presses there.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn stow(
    owner: &mut Character,
    hand: Hand,
    socket: SocketId,
    in_both_hands: bool,
) -> Result<(), SocketRefusal> {
    let (settings, dominant) = settings();
    if !(settings.enabled && settings.carry) || carry::on_socket(socket).is_some() {
        return Err(SocketRefusal::DoesNotFit);
    }
    let weapon = unsafe { gun::held_weapon(owner) }.ok_or(SocketRefusal::Empty)?;
    let fit = unsafe { fit_of_weapon(weapon) }.ok_or(SocketRefusal::DoesNotFit)?;
    let slot = unsafe { inventory_engine::slot_of(weapon) }.ok_or(SocketRefusal::DoesNotFit)?;
    // The dual-wield pair drawn in both hands stays together.
    if slot == WeaponSlot::E_SLOT_DUAL_WIELD && in_both_hands {
        return Err(SocketRefusal::DoesNotFit);
    }
    if !settings.layout(dominant).accepts(socket, fit) {
        return Err(SocketRefusal::DoesNotFit);
    }
    unsafe { carry::carry_from_slot(owner, slot, socket, settings.offsets.of(socket)) }
        .map_err(SocketRefusal::Carry)?;
    inventory::count_stow();
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        socket = socket.label(),
        "inventory: put the weapon from the {} hand onto the {}",
        hand.label(),
        socket.label()
    );
    Ok(())
}

/// Before a pickup into `slot`, which holds another weapon, carry that weapon on a free socket so
/// that the pickup does not drop it. Returns whether it was carried.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn carry_replaced(owner: &mut Character, slot: WeaponSlot) -> bool {
    let (settings, dominant) = settings();
    if !(settings.enabled && settings.carry) {
        return false;
    }
    let Some(occupant) = (unsafe { owner.m_Inventory.m_Weapons[slot as usize].px.as_ref() }) else {
        return false;
    };
    let Some(fit) = (unsafe { fit_of_weapon(occupant) }) else {
        return false;
    };
    let Some(socket) = settings.layout(dominant).choose(fit, &carry::taken()) else {
        tracing::info!(
            target: "controllers",
            slot = slot_label(slot),
            "inventory: no socket is free for the weapon the pickup replaces"
        );
        return false;
    };
    match unsafe { carry::carry_from_slot(owner, slot, socket, settings.offsets.of(socket)) } {
        Ok(_) => {
            inventory::count_carried_replaced();
            true
        }
        Err(refusal) => {
            tracing::info!(target: "controllers", %refusal, "inventory: the weapon the pickup replaces was not carried");
            false
        }
    }
}

/// The settings the actions read and the dominant hand.
fn settings() -> (InventoryConfig, Hand) {
    Config::lock_query(|c| (c.controllers.weapons.inventory, c.controllers.acting_hand()))
}

/// Draw the carried weapon `object` into `grab`'s hand: into its slot and equipped, sliding from
/// where it sat on the socket.
unsafe fn draw(
    owner: &mut Character,
    object: ObjectKey,
    grab: &SocketGrab,
) -> Result<(), SocketRefusal> {
    let hand = grab.hand;
    // SAFETY: the carry set keeps the weapon alive until the promotion hands it to the slot.
    let weapon = unsafe { &*(object as *const WeaponBase) };
    let from = unsafe { pickup::gun_pose(weapon) };
    let slot = unsafe { inventory_engine::slot_of(weapon) };
    hold::begin(hand, Cause::Draw, Some(object));
    let pistols = slot == Some(WeaponSlot::E_SLOT_DUAL_WIELD);
    if pistols && grab.independent_pistols {
        dual_wield::attach::begin_draw(hand);
    }
    let occupant = unsafe { carry::promote(owner, object) }.map_err(SocketRefusal::Carry)?;
    if !pistols {
        hold::adopt(object, hand);
    }
    if let Some(from) = from {
        act::slide_in(object, hand, from, grab.snap_s);
    }
    inventory::count_draw();
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        socket = grab.socket.label(),
        occupant_carried = occupant.is_some(),
        "inventory: drew a carried weapon off the {} into the {} hand",
        grab.socket.label(),
        hand.label()
    );
    Ok(())
}

/// What fits the weapon at `weapon`, live.
unsafe fn fit_of_weapon(weapon: *const WeaponBase) -> Option<Fit> {
    let weapon = unsafe { weapon.as_ref() }?;
    let slot = unsafe { inventory_engine::slot_of(weapon) }?;
    // Read as the raw value: an unset or unknown class is out of the enum's range.
    let class = unsafe { (&raw const weapon.m_Class).cast::<i32>().read() };
    fit_of(slot, class)
}
