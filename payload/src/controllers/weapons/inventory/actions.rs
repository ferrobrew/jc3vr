//! What the hands do at the sockets on the input tick: draw a weapon off its socket, as the
//! primary hand's or as the second weapon beside the wielded one, and put a held weapon onto a
//! free socket. Each is an event for the loadout table, which the reconcile then carries out in the
//! game's slots and hands ([`crate::controllers::weapons::inventory::reconcile`]). And where the
//! sockets' grab volumes go.
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
        loadout::{ObjectAddress, Place, Socket, live},
        weapons::{
            dual_wield, engine as weapons_engine,
            hold::{
                act, pickup,
                second::{self, SecondRefusal},
            },
            inventory::{
                self, carry,
                config::InventoryConfig,
                decide::{self, GrabDecision, HandsView},
                engine as inventory_engine,
                engine::ObjectKey,
                reconcile,
                sockets::{self, Fit, fit_of},
            },
        },
    },
};

/// A socket's grab volume, in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct SocketPoint {
    pub socket: Socket,
    pub point: Vec3,
    /// The hand the volume is for: a free socket's is the hand holding a weapon that fits it, to
    /// put it there; a carried weapon's is `None`, for any hand without a gun.
    pub for_hand: Option<Hand>,
}

/// Why a hand's grab of a socket did nothing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SocketRefusal {
    NoCharacter,
    /// The other hand holds a weapon, and a second weapon is not allowed.
    OtherHandFull,
    /// The socket is free and the hand holds nothing to put there.
    Empty,
    /// The held weapon does not fit the socket, or is not one the inventory carries.
    DoesNotFit,
    Second(SecondRefusal),
}

impl fmt::Display for SocketRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketRefusal::NoCharacter => write!(f, "socket: there is no local player"),
            SocketRefusal::OtherHandFull => write!(f, "socket: the other hand holds a weapon"),
            SocketRefusal::Empty => write!(f, "socket: nothing is carried there"),
            SocketRefusal::DoesNotFit => write!(f, "socket: the weapon does not fit there"),
            SocketRefusal::Second(refusal) => write!(f, "socket: {refusal}"),
        }
    }
}

impl std::error::Error for SocketRefusal {}

/// What a grab of a socket did.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum SocketAction {
    /// The carried weapon came into the hand as the wielded weapon.
    Drew,
    /// The carried weapon came into the hand as the second weapon, beside the wielded one.
    DrewSecond,
    /// The held weapon went onto the socket.
    Stowed,
}

/// What the weapon handling hands a socket grab.
pub(crate) struct SocketGrab {
    pub hand: Hand,
    pub socket: Socket,
    /// The wielded weapon and the hands holding its guns.
    pub wielded: Option<(ObjectKey, [bool; 2])>,
    /// The second weapon and its hand.
    pub second: Option<(ObjectKey, Hand)>,
    /// Whether a carried weapon may come into the free hand as the second weapon.
    pub second_allowed: bool,
    /// How long a drawn weapon slides into the hand, seconds.
    pub snap_s: f32,
    pub independent_pistols: bool,
}

/// The sockets' grab volumes in the world: each carried weapon's grip, and for each hand holding a
/// weapon in `held` (by hand), each free socket that fits it (a carried second weapon's own socket
/// among them), for putting it there.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn socket_points(
    owner: &Character,
    settings: &InventoryConfig,
    dominant: Hand,
    held: [Option<ObjectKey>; 2],
) -> Vec<SocketPoint> {
    if !(settings.enabled && settings.carry) || carry::stashed() {
        return Vec::new();
    }
    let mut out: Vec<SocketPoint> = unsafe { carry::grips() }
        .into_iter()
        .map(|(socket, point)| SocketPoint {
            socket,
            point,
            for_hand: None,
        })
        .collect();
    let layout = settings.layout(dominant);
    let taken = carry::taken();
    let to_world = Mat4::from(owner.m_WorldMatrixT1);
    for hand in Hand::BOTH {
        let Some(object) = held[hand.index()] else {
            continue;
        };
        let Some(fit) = (unsafe { fit_of_weapon(object as *const WeaponBase) }) else {
            continue;
        };
        let home = carry::home(object);
        for socket in layout.choices(fit) {
            if taken.contains(&socket) && home != Some(socket) {
                continue;
            }
            if let Some(point) = unsafe { socket_point(owner, to_world, settings, socket) } {
                out.push(SocketPoint {
                    socket,
                    point,
                    for_hand: Some(hand),
                });
            }
        }
    }
    out
}

/// Carry out `grab`: a hand without a gun takes the carried weapon off the socket (into the slot,
/// or as the second weapon while the other hand holds the wielded one), and a hand holding a
/// weapon puts it onto a free socket.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn grab(grab: &SocketGrab) -> Result<SocketAction, SocketRefusal> {
    let owner = unsafe { weapons_engine::local_player() }.ok_or(SocketRefusal::NoCharacter)?;
    let SocketGrab { hand, socket, .. } = *grab;
    let carried = unsafe { carry::on_socket(socket) };
    let view = HandsView {
        holds_wielded: grab.wielded.is_some_and(|(_, hands)| hands[hand.index()]),
        holds_second: grab.second.is_some_and(|(_, h)| h == hand),
        other_wields: grab
            .wielded
            .is_some_and(|(_, hands)| hands[hand.other().index()]),
        other_holds_second: grab.second.is_some_and(|(_, h)| h == hand.other()),
        second_allowed: grab.second_allowed,
    };
    match (decide::grab(carried.is_some(), view), carried) {
        (GrabDecision::Draw, Some(object)) => {
            unsafe { draw(owner, object, grab) }.map(|()| SocketAction::Drew)
        }
        (GrabDecision::DrawSecond, Some(object)) => {
            unsafe { second::take_carried(hand, object) }.map_err(SocketRefusal::Second)?;
            inventory::count_draw();
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                socket = socket.label(),
                "inventory: drew a carried weapon off the {} into the {} hand beside the wielded one",
                socket.label(),
                hand.label()
            );
            Ok(SocketAction::DrewSecond)
        }
        (GrabDecision::Stow, _) => {
            let both = grab.wielded.is_some_and(|(_, hands)| hands == [true; 2]);
            unsafe { stow(owner, hand, socket, both) }.map(|()| SocketAction::Stowed)
        }
        (GrabDecision::StowSecond, _) => {
            unsafe { stow_second(owner, hand, socket) }.map(|()| SocketAction::Stowed)
        }
        (GrabDecision::OtherHandFull, _) => Err(SocketRefusal::OtherHandFull),
        (GrabDecision::Occupied, _) => Err(SocketRefusal::DoesNotFit),
        (GrabDecision::Nothing | GrabDecision::Draw | GrabDecision::DrawSecond, _) => {
            Err(SocketRefusal::Empty)
        }
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
    socket: Socket,
    in_both_hands: bool,
) -> Result<(), SocketRefusal> {
    let (settings, dominant) = settings();
    if !(settings.enabled && settings.carry) || carry::taken().contains(&socket) {
        return Err(SocketRefusal::DoesNotFit);
    }
    let weapon = unsafe { gun::held_weapon(owner) }.ok_or(SocketRefusal::Empty)?;
    let fit = unsafe { fit_of_weapon(weapon) }.ok_or(SocketRefusal::DoesNotFit)?;
    let slot = unsafe { inventory_engine::slot_of(weapon) }.ok_or(SocketRefusal::DoesNotFit)?;
    // The dual-wield pair drawn in both hands stays together.
    if slot == WeaponSlot::E_SLOT_DUAL_WIELD && in_both_hands {
        return Err(SocketRefusal::DoesNotFit);
    }
    if !settings.layout(dominant).accepts(socket, fit) || carry::taken().contains(&socket) {
        return Err(SocketRefusal::DoesNotFit);
    }
    let object = ObjectAddress::from_address(weapon as *const WeaponBase as usize)
        .ok_or(SocketRefusal::Empty)?;
    live::depart(hand, object, live::Whereabouts::Carried(socket));
    if live::place_of(object.get()) != Some(Place::Socket(socket)) {
        return Err(SocketRefusal::DoesNotFit);
    }
    unsafe { reconcile::apply(owner) };
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

/// Put the second weapon, held in `hand`, onto the free `socket` that fits it.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn stow_second(
    owner: &mut Character,
    hand: Hand,
    socket: Socket,
) -> Result<(), SocketRefusal> {
    let (settings, dominant) = settings();
    let object = unsafe { second::weapon(owner) }
        .map(|w| w as *const WeaponBase as ObjectKey)
        .ok_or(SocketRefusal::Empty)?;
    let fit =
        unsafe { fit_of_weapon(object as *const WeaponBase) }.ok_or(SocketRefusal::DoesNotFit)?;
    let home = carry::home(object);
    let free = !carry::taken().contains(&socket) || home == Some(socket);
    if !(settings.enabled && settings.carry)
        || !free
        || !settings.layout(dominant).accepts(socket, fit)
    {
        return Err(SocketRefusal::DoesNotFit);
    }
    if second::source().is_none() {
        return Err(SocketRefusal::Empty);
    }
    unsafe { second::put_back(Some(socket)) };
    inventory::count_stow();
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        socket = socket.label(),
        "inventory: put the second weapon from the {} hand onto the {}",
        hand.label(),
        socket.label()
    );
    Ok(())
}

/// The settings the actions read and the dominant hand.
fn settings() -> (InventoryConfig, Hand) {
    Config::lock_query(|c| (c.controllers.weapons.inventory, c.controllers.acting_hand()))
}

/// Where a weapon on `socket` sits, in the world: the socket's bone moved by its offset.
unsafe fn socket_point(
    owner: &Character,
    to_world: Mat4,
    settings: &InventoryConfig,
    socket: Socket,
) -> Option<Vec3> {
    let bone = unsafe { grip::bone_model_pose(owner, sockets::bone(socket)) }?;
    let point = to_world.transform_point3(bone.position)
        + to_world.transform_vector3(settings.offsets.of(socket));
    point.is_finite().then_some(point)
}

/// Draw the weapon `object` off its socket into `grab`'s hand, sliding from where it sat: the table
/// takes the draw, and the reconcile puts it into its slot and wields it when the hand is the
/// primary, or arms it there beside the other hand's weapon.
unsafe fn draw(
    owner: &mut Character,
    object: ObjectKey,
    grab: &SocketGrab,
) -> Result<(), SocketRefusal> {
    let hand = grab.hand;
    // SAFETY: the carry set or the slot keeps the weapon alive.
    let weapon = unsafe { &*(object as *const WeaponBase) };
    let from = unsafe { pickup::gun_pose(weapon) };
    // The game's pair has a pistol per hand; a single pistol is a weapon of one gun.
    let slot = unsafe { inventory_engine::slot_of(weapon) };
    let pistols = slot == Some(WeaponSlot::E_SLOT_DUAL_WIELD) && !dual_wield::single::is(object);
    if pistols && grab.independent_pistols {
        dual_wield::attach::begin_draw(hand);
    }
    let held = unsafe { inventory::held_of(object) }.ok_or(SocketRefusal::DoesNotFit)?;
    if !live::arrive(hand, held) {
        return Err(SocketRefusal::OtherHandFull);
    }
    unsafe { reconcile::apply(owner) };
    act::arm(hand);
    if let Some(from) = from {
        act::slide_in(object, hand, from, grab.snap_s);
    }
    inventory::count_draw();
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        socket = grab.socket.label(),
        "inventory: drew a weapon off the {} into the {} hand",
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
