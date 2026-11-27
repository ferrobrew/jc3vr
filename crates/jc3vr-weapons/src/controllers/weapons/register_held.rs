//! Registers the weapon handling's side of the held-item seam ([`jc3vr_body::controllers::held`]): the
//! query functions behind each hand's contents, armed weapon, item kind, wielded gun, two-hand grip,
//! grenade, C4, and long gun, the steal offer, rack and slide state, the reload's zones, and the rigid
//! grip's placement reads, whose captures the loadout table's hand moves invalidate.

use glam::Vec3;
use jc3vr_body::controllers::{
    held,
    held::{
        HeldContents, HeldItem, HeldSource, PlacementSource,
        zones::{GunZones, KnockZone, ReloadViews, ZoneAxis},
    },
};
use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{ItemKind, live},
    weapons::{
        c4, collide, contents,
        dual_wield::single,
        foregrip, grenade, hold,
        hold::{act, armed, rack::paired::stack_offset, take},
        regions, reload,
        reload::{knock, zones},
        state::last_wielded,
    },
};

/// Register the source once; the controllers' install calls it before any hook is enabled.
pub fn register() {
    live::register_hands_moved(take::invalidate_handle_placements);
    held::register_held_source(HeldSource {
        armed_in: armed::in_hand,
        armed_hand_of: armed::hand_of,
        armed_weapon: armed::weapon_of,
        owned_holder: hold::owned_holder,
        contents: hand_contents,
        item,
        wielded_gun_hands,
        detonator_hand: c4::act_detonator_hand,
        last_wielded_hands,
        two_hand_holder,
        throwing_hand: c4::throwing_hand,
        detonator_holder: c4::detonator_hand,
        grenade_in: grenade::holds,
        long_gun_muzzle,
        steal_offer: take::steal_offer,
        reload_views,
        is_single_pistol: single::is,
        rack_stack_offset: stack_offset,
        adjust_part: reload::adjust_part,
        placement: PlacementSource {
            begin: take::begin_handle_placement,
            record: take::record_handle_placement,
            snap: act::snap_view,
            slide: regions::slide_for,
            body_deviation: jc3vr_body::controllers::held::guns::deviation,
            carried_for: armed::carried_for,
        },
    });
}

/// What each hand holds, sampled now ([`contents::now`]): the readers below the weapon handling
/// read between ticks, after the simulation's C4 update has moved the throw, and keep the gun hands
/// through a stand-down, which the record published at the end of the input tick does neither of.
fn hand_contents() -> [HeldContents; 2] {
    contents::now().map(|hand| HeldContents {
        item: hand.item.map(held_item),
        wielded: hand.wielded,
        armed: hand.armed,
        grenade: hand.grenade.is_some(),
        charge: hand.charge.is_some(),
        detonator: hand.detonator,
        braces: hand.braces,
    })
}

/// The kind of the loadout table's item in `hand`.
fn item(hand: Hand) -> Option<HeldItem> {
    live::held_kind(hand).map(held_item)
}

fn held_item(kind: ItemKind) -> HeldItem {
    match kind {
        ItemKind::Weapon(_) => HeldItem::Weapon,
        ItemKind::Prop => HeldItem::Prop,
        ItemKind::Grenade => HeldItem::Grenade,
        ItemKind::Charge => HeldItem::Charge,
    }
}

/// The hands holding a gun of the wielded weapon, from the weapon handling's last tick.
fn wielded_gun_hands() -> [bool; 2] {
    let (wielded, active) = last_wielded();
    wielded
        .filter(|_| active)
        .map_or([false; 2], |w| w.wielded.hands)
}

/// The hands holding a gun of the wielded weapon, from the weapon handling's last tick, whether or
/// not it is active.
fn last_wielded_hands() -> [bool; 2] {
    last_wielded().0.map_or([false; 2], |w| w.wielded.hands)
}

/// The holding hand of the engaged two-hand grip.
fn two_hand_holder() -> Option<Hand> {
    foregrip::two_hand_grip().map(|grip| grip.holder)
}

/// The muzzle of the long gun the input tick measured in `hand`.
fn long_gun_muzzle(hand: Hand) -> Option<Vec3> {
    collide::measured(hand).map(|gun| gun.muzzle)
}

/// The reload's zones and knock zone from the last input tick, the knock's flash judged at `now`.
fn reload_views(now: Stamp) -> ReloadViews {
    ReloadViews {
        zones: zones::views().map(|view| {
            view.map(|view| GunZones {
                holder: view.holder,
                magazine: view.magazine.map(|zone| {
                    let (top, bottom) = zone.ends();
                    ZoneAxis {
                        top,
                        bottom,
                        radius: zone.radius,
                    }
                }),
                handover: view.handover,
                at_magazine: view.at_magazine,
            })
        }),
        knock: knock::view().map(|view| {
            let (top, bottom) = view.zone.ends();
            KnockZone {
                holder: view.holder,
                zone: ZoneAxis {
                    top,
                    bottom,
                    radius: view.zone.radius,
                },
                tip: view.tip,
                fast: view.fast,
                flashing: view.flashing(now),
            }
        }),
    }
}
