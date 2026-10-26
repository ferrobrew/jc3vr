//! Route socket, release, and handover interactions for actor-free grenades.

use crate::controllers::{
    frame::Hand,
    loadout::{Place, Socket, live},
    weapons::{docks, grenade},
};

/// Handle a safe grenade's socket grab without entering the native weapon path.
///
/// `None` means the socket interaction is unrelated to a safe grenade. `Some(false)` means the
/// safe move was relevant but refused, so the caller must not fall through to native weapon code.
pub(crate) fn grab_socket(hand: Hand, socket: Socket) -> Option<bool> {
    if grenade::safe::in_hand(hand).is_some() {
        if !matches!(socket, Socket::Hip(_) | Socket::Chest) {
            return Some(false);
        }
        let Some(world) = docks::socket_world_pose(socket) else {
            return Some(false);
        };
        return Some(grenade::safe::stow(hand, socket, world));
    }
    let has_safe_item = live::safe_items()
        .into_iter()
        .any(|item| item.place == Place::Socket(socket));
    has_safe_item.then(|| grenade::safe::draw_stowed(socket, hand))
}

/// Stow a held safe grenade before the caller opens its grenade input.
///
/// Stow at the socket selected as available and reachable by the release coordinator.
pub(crate) fn stow_before_open(hand: Hand, socket: Option<Socket>) -> Option<bool> {
    grenade::safe::in_hand(hand)?;
    let Some(socket) = socket.filter(|socket| matches!(socket, Socket::Hip(_) | Socket::Chest))
    else {
        return Some(false);
    };
    let Some(world) = docks::socket_world_pose(socket) else {
        return Some(false);
    };
    Some(grenade::safe::stow(hand, socket, world))
}

/// Transfer a safe grenade between hands without constructing a native object.
pub(crate) fn transfer(from: Hand, to: Hand) -> Option<bool> {
    grenade::safe::in_hand(from)?;
    Some(grenade::safe::transfer(from, to))
}
