//! Physical weapon handling: the plan of record's "Holsters", "Two-handed weapons", "Reloads",
//! "Grenades", and "Grip semantics" (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`volumes`]: the body's sockets and pouches (the holsters, the grenade pouch, the reload
//!   pouch, and the C4 pouch), placed from the player's body dimensions;
//! - [`holster`]: what a grab at a holster does: draw its slot or holster the held weapon;
//! - [`docks`]: the holsters moved onto the guns docked in them;
//! - [`foregrip`]: the two-handed weapon's foregrip and the two-handed pose;
//! - [`recoil`]: the recoil offset between the controller and the gun, with its return spring;
//! - [`heft`]: the one-handed weight of a long gun: sag, lag, sway, a stronger kick, and a wider
//!   spread, gone while it is braced on the foregrip;
//! - [`fire`]: the per-shot seam that kicks the recoil and marks the shot's hand;
//! - [`grip_watch`]: a diagnostic watch on how each held gun sits in its hand;
//! - [`dual_wield`]: each dual-wielded pistol drawn by its own hand and fired by its own hand's
//!   trigger;
//! - [`hold`]: which hand holds the wielded weapon (the hand that drew it, took it off the
//!   ground, or was handed it), the attach bones that put it there, and the direct pickup;
//! - [`regions`]: a long gun's parts, derived from its model's box and its known points, and a
//!   gun taken, braced, or carried by whichever part the hand closes on;
//! - [`inventory`]: every weapon the player owns, holds, or let go of nearby, and the ones it
//!   keeps from the game's despawn;
//! - [`camera_kick`]: the game's camera recoil, suppressed while the guns aim by hand;
//! - [`grenade`]: the physical grenade throw;
//! - [`c4`]: the C4 throw from the chest pouch and the wrist detonator;
//! - [`reload`]: the true reload: eject, load from the belt pouch, and chamber by hand;
//! - [`empty`]: the game's automatic switch away from an empty weapon suppressed, and the dry fire
//!   pulsed;
//! - [`unarmed_fire`]: the fire input with nothing in the hands kept from drawing a weapon;
//! - [`scope`]: scoped weapons: the game's precision aim suppressed, and the lens on the scope;
//! - [`wheel`]: the game's weapon selection cross, opened with a held input and steered by the
//!   hand;
//! - [`config`]: the weapon-handling settings.
//!
//! The weapon handling runs on the input tick, in two halves around the bindings' tables
//! ([`crate::controllers::bindings::input_tick`]):
//!
//! 1. [`begin_tick`] reads the wielded weapon and provides the weapon handling's grip targets
//!    ([`crate::controllers::grab`]): the sockets and pouches on the body, reached by the player's
//!    own grip before the body-scale remap while they ride the player's body, each pouch with a
//!    reach margin a hand that misses it by a little still takes it within; the held gun's parts,
//!    reached by the hand on the gun; the weapons in the world without a body (in characters'
//!    hands, on racks, in lockers); and the grenade's pin, a live grenade, a magazine in the world,
//!    and the carried weapons' sockets. A braced foregrip is offered to its hand as held.
//! 2. [`finish_tick`] acts on the grabs the grip model hands it: a holster grab draws or holsters,
//!    a grab of the held gun's grip by the free hand hands the gun over, a foregrip grab braces the
//!    gun, a grenade-pouch grab primes a grenade for the throw, a C4-pouch grab primes a charge,
//!    and a reload-pouch grab takes a round for the reload. Opening the grip on a gun docks it in
//!    the nearest socket that fits it under the hand, or lets it fall
//!    ([`crate::controllers::grab::resolve::dock_target`]). It runs the throws, the wrist
//!    detonator, and the reload, and holds the fire inputs back while a reload, a draw, or a
//!    holster plays (the action-runs hand tier) and while the reload holds the gun from firing.
//!
//! A gun is held while its grip is: nothing here draws or holsters except a grab and its release.
//! The markers on the targets are the grip model's ([`crate::controllers::grab::markers`]).
//! Everything acts only while the bindings inject, which is only under the controller scheme with
//! a controller active.

pub mod ads;
mod begin;
pub mod c4;
mod camera_kick;
pub mod collide;
pub mod config;
pub mod docks;
pub mod dual_wield;
pub mod empty;
mod engine;
mod finish;
pub(crate) mod fire;
pub mod foregrip;
pub mod grenade;
pub mod grip_watch;
pub mod heft;
pub mod hold;
pub mod holster;
pub mod inventory;
pub mod recoil;
pub mod regions;
pub mod reload;
pub mod scope;
mod state;
mod types;
pub mod unarmed_fire;
pub mod volumes;
pub mod wheel;

pub(crate) use begin::begin_tick;
// Re-exported so the weapon-handling submodules that need them (the weapon wheel, the reload) can
// reach them at the weapon handling's own path, as they did before the input tick's two halves
// moved out of this file.
use begin::{gameplay, weighted_hands};
pub(crate) use finish::{FinishInputs, finish_tick};
use re_utilities::hook_library::HookLibrary;
pub(crate) use state::stand_down;
pub use state::status;
pub use types::{TickStart, WeaponsStatus};

/// The weapon handling's hooks, aggregated by [`crate::controllers::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(fire::hook_library())
        .with_hook_library(camera_kick::hook_library())
        .with_hook_library(grenade::hook_library())
        .with_hook_library(c4::hook_library())
        .with_hook_library(scope::hook_library())
        .with_hook_library(hold::hook_library())
        .with_hook_library(inventory::hook_library())
        .with_hook_library(empty::hook_library())
        .with_hook_library(heft::hook_library())
        .with_hook_library(reload::hook_library())
        .with_hook_library(wheel::hook_library())
}

#[cfg(test)]
mod tests {
    use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};

    use crate::controllers::{
        bindings::{
            controls::ButtonControl,
            hold::{Desired, MapKind, Target, Want},
        },
        config::ControllersConfig,
        frame::Hand,
        grab::{GrabVolume, HolsterSite},
        mode::{AirborneVariant, Mode, OnFootVariant},
        weapons::{
            begin::{entered_body_volumes, gameplay},
            engine::HeldWeapon,
            finish::{cues_grab, docks_without_act, hold_fire_back, primary_trigger_idle},
            foregrip::Transition,
            heft, holster,
            holster::HolsterAction,
            types::GrabOutcome,
        },
    };

    #[test]
    fn the_grips_are_free_only_on_foot_and_under_the_parachute() {
        assert!(gameplay(Mode::OnFoot(OnFootVariant::Ground)));
        assert!(gameplay(Mode::Airborne(AirborneVariant::Parachute)));
        assert!(!gameplay(Mode::Airborne(AirborneVariant::Wingsuit)));
        assert!(!gameplay(Mode::OnFoot(OnFootVariant::Swimming)));
        assert!(!gameplay(Mode::Ui));
    }

    #[test]
    fn a_hand_entering_a_holster_or_a_pouch_is_cued_once() {
        let hip = Some(GrabVolume::Holster(HolsterSite::Hip(Hand::Right)));
        let pouch = Some(GrabVolume::MagazinePouch);
        let foregrip = Some(GrabVolume::Foregrip);
        assert_eq!(
            entered_body_volumes([None, None], [pouch, hip]),
            [true, true]
        );
        // Staying in a volume is not entering it again.
        assert_eq!(
            entered_body_volumes([pouch, hip], [pouch, hip]),
            [false, false]
        );
        // Moving from one body volume straight into another is.
        assert_eq!(
            entered_body_volumes([hip, None], [pouch, None]),
            [true, false]
        );
        // The foregrip is not on the body; its grab is cued instead.
        assert_eq!(
            entered_body_volumes([None; 2], [foregrip, None]),
            [false, false]
        );
        assert!(cues_grab(GrabOutcome::Foregrip(Transition::Grabbed)));
        assert!(cues_grab(GrabOutcome::Holster(HolsterAction::Holster {
            slot: WeaponSlot::E_SLOT_TWO_HANDED
        })));
        assert!(!cues_grab(GrabOutcome::Holster(HolsterAction::Refused {
            slot: WeaponSlot::E_SLOT_TWO_HANDED
        })));
        assert!(!cues_grab(GrabOutcome::Magazine));
    }

    fn rifle_in(hand: Hand) -> HeldWeapon {
        HeldWeapon {
            address: 0x1000,
            wielded: holster::Wielded {
                slot: WeaponSlot::E_SLOT_TWO_HANDED,
                hands: Hand::BOTH.map(|h| h == hand),
            },
            holder: Some(hand),
            two_handed: true,
            class: heft::HeftClass::of(0, true),
        }
    }

    fn triggers(left: bool, right: bool) -> [ButtonControl; 2] {
        let pulled = |down| ButtonControl {
            down,
            ..ButtonControl::default()
        };
        [pulled(left), pulled(right)]
    }

    #[test]
    fn only_a_hand_holding_the_gun_fires_it() {
        let left_rifle = Some(rifle_in(Hand::Left));
        // The other trigger alone fires nothing while the gun is in this hand.
        assert!(primary_trigger_idle(left_rifle, &triggers(false, true)));
        // The holding hand's trigger fires it, with or without the other trigger.
        assert!(!primary_trigger_idle(left_rifle, &triggers(true, false)));
        assert!(!primary_trigger_idle(left_rifle, &triggers(true, true)));
        // No trigger pulled holds nothing back.
        assert!(!primary_trigger_idle(left_rifle, &triggers(false, false)));
        // Mirrored: a second weapon's trigger in the left hand does not fire the right-held gun.
        let right_rifle = Some(rifle_in(Hand::Right));
        assert!(primary_trigger_idle(right_rifle, &triggers(true, false)));
        assert!(!primary_trigger_idle(right_rifle, &triggers(false, true)));
        // Pistols in both hands fire from either trigger, and no weapon is the game's as before.
        let mut pair = rifle_in(Hand::Right);
        pair.wielded.hands = [true; 2];
        assert!(!primary_trigger_idle(Some(pair), &triggers(true, false)));
        assert!(!primary_trigger_idle(None, &triggers(true, true)));
    }

    #[test]
    fn pistols_dock_only_when_each_hand_draws_its_own() {
        let mut config = ControllersConfig::new();
        let pistols = WeaponSlot::E_SLOT_DUAL_WIELD;
        let rifle = WeaponSlot::E_SLOT_TWO_HANDED;
        assert!(docks_without_act(&config, pistols));
        assert!(docks_without_act(&config, rifle));
        config.weapons.dual_wield.independent_draw = false;
        assert!(!docks_without_act(&config, pistols));
        assert!(docks_without_act(&config, rifle));
        config.weapons.dual_wield.independent_draw = true;
        config.weapons.hold.pistol_dock = false;
        assert!(!docks_without_act(&config, pistols));
        config.weapons.hold.pistol_dock = true;
        config.weapons.hold.instant_dock = false;
        assert!(
            docks_without_act(&config, pistols),
            "the switches are separate"
        );
        assert!(!docks_without_act(&config, rifle));
        assert!(!docks_without_act(&config, WeaponSlot::E_SLOT_HEAVY));
    }

    #[test]
    fn a_weapon_action_holds_only_the_fire_inputs_back() {
        let want = |action| Desired {
            target: Target {
                map: MapKind::Player,
                action,
            },
            want: Want::Hold(1.0),
        };
        let mut desired = vec![
            want(Action::FIRE_RIGHT),
            want(Action::R2),
            want(Action::JUMP),
        ];
        hold_fire_back(&mut desired);
        assert_eq!(desired, vec![want(Action::JUMP)]);
    }
}
