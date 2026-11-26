//! Registers the weapon handling's side of the held-item seam ([`jc3vr_body::controllers::held`]): the
//! query functions behind each hand's armed weapon, item kind, wielded gun, rack and slide state,
//! and the rigid grip's placement reads.

use jc3vr_body::controllers::{
    held,
    held::{HeldItem, HeldSource, PlacementSource},
};
use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{ItemKind, live},
    weapons,
    weapons::{
        c4,
        dual_wield::single,
        gun_bodies, hold,
        hold::{act, armed, rack::paired::stack_offset, take},
        regions, reload,
    },
};

/// Register the source once; the controllers' install calls it before any hook is enabled.
pub(crate) fn register() {
    held::register_held_source(HeldSource {
        armed_in: armed::in_hand,
        armed_hand_of: armed::hand_of,
        armed_weapon: armed::weapon_of,
        owned_holder: hold::owned_holder,
        item,
        wielded_gun_hands,
        detonator_hand: c4::act_detonator_hand,
        is_single_pistol: single::is,
        rack_stack_offset: stack_offset,
        adjust_part: reload::adjust_part,
        placement: PlacementSource {
            begin: take::begin_handle_placement,
            record: take::record_handle_placement,
            snap: act::snap_view,
            slide: regions::slide_for,
            body_deviation: gun_bodies::deviation,
            carried_for: armed::carried_for,
        },
    });
}

/// The kind of the loadout table's item in `hand`.
fn item(hand: Hand) -> Option<HeldItem> {
    live::held_kind(hand).map(|kind| match kind {
        ItemKind::Weapon(_) => HeldItem::Weapon,
        ItemKind::Prop => HeldItem::Prop,
        ItemKind::Grenade => HeldItem::Grenade,
        ItemKind::Charge => HeldItem::Charge,
    })
}

/// The hands holding a gun of the wielded weapon, from the weapon handling's last tick.
fn wielded_gun_hands() -> [bool; 2] {
    let status = weapons::status();
    status
        .wielded
        .filter(|_| status.active)
        .map_or([false; 2], |w| w.weapon.wielded.hands)
}
