//! Held guns as bodies: a gun in a hand whose body acts is a body of its own in the physics world,
//! joined to the hand bodies that hold it. The design is in `docs/mod/input/physical-guns.md`.
//!
//! - [`engine`]: the gun's own body, whose shape and mass the gun body takes;
//! - [`step`]: the gun bodies' lifecycle in the pre-simulation step: made, joined, kept on their
//!   seats, kicked by the shots, and destroyed;
//! - [`math`]: the pure parts: the mass, the seat and the deviation from it, the axes, and the shot.
//!
//! The guns in the hands, the shots, and what each step reports back to the hands, the rigid grip,
//! and the foregrip are the store the weapon handling publishes into
//! ([`jc3vr_body::controllers::held::guns`]); this module re-exports the readout's types.
//!
//! A gun's own body is in the world only while the weapon lies dropped or is mounted
//! ([`WeaponData::m_PfxInstance`]), and it belongs to the weapon, which the game may destroy at any
//! time. So the gun body is the mod's own: a body made with the gun's own shape (the new body takes
//! a reference of its own to it) and the gun's own mass, on the hand bodies' layer and in the
//! player's collision group, so it meets the world, characters, ragdolls, and vehicles, and not
//! the player, the hands, or the aim's ray. The joints join two bodies of the mod's own, so the
//! game destroying the weapon leaves nothing of the mod's naming a freed body; the gun body is
//! destroyed, its joints first, as soon as the gun leaves the hand, and with every hand body.
//!
//! Each tick runs in this order:
//!
//! 1. The weapon handling's input tick publishes each hand's gun ([`publish`]): the weapon, the
//!    gun, its own body, and which hand braces it on the foregrip. A gun counts only while the
//!    rigid grip pins it to the hand and no slide is moving it between the hand and the body.
//! 2. The hands those guns are in act ([`acting_hands`]), so their bodies are driven, with a
//!    holding hand's strength.
//! 3. The pre-simulation step makes each gun's body on its seat on the holding hand's body: the
//!    gun's last placement in the frame of the controller's palm the hand is driven to, so a hand
//!    body that lags its palm does not seat the gun off the hand. It waits for the hand body to
//!    settle on its palm, joins the gun body to it, and joins the bracing hand's body where that
//!    hand holds it. Each step it moves the gun body with the hand's as one rigid body, and the
//!    bracing hand's body with the gun's, so only the holding hand's drive steers the gun; puts it
//!    back on its seat past a few centimetres or degrees of stray, measures the stray
//!    ([`deviation`]), which the rigid grip's pin carries the drawn gun by within a couple of
//!    degrees, and gives the gun body each shot's kick ([`queue_shot`]). A brace whose tracked
//!    palm has left the gun breaks, and the foregrip lets go ([`take_support_break`]).
//!
//! The gun body's mass is its own body's (the weapon's pickup mass, shared between a dual-wield
//! pair's two guns) held to its weight class's, so that a rifle does not outweigh the hand that
//! swings it, and the hand holding it is driven with its full strength against the hand's and the
//! gun's mass together. The support and class-weight target is shared by the physical drive and
//! the drawn reference, so the gun stays near-rigid on a hand that droops under one-handed weight
//! and steadies with support.
//!
//! [`WeaponData::m_PfxInstance`]: jc3gi::equipment::weapon::WeaponData::m_PfxInstance
//! [`publish`]: jc3vr_body::controllers::held::guns::publish
//! [`deviation`]: jc3vr_body::controllers::held::guns::deviation
//! [`queue_shot`]: jc3vr_body::controllers::held::guns::queue_shot
//! [`take_support_break`]: jc3vr_body::controllers::held::guns::take_support_break

mod engine;
pub mod math;
pub(super) mod step;
pub mod targets;

use jc3vr_body::controllers::{
    held::{guns as held_guns, guns::GunHolds},
    mode::{AirborneVariant, Detection, Mode, OnFootVariant},
};
use jc3vr_core::published::Step;

pub(crate) fn mode_allows_bodies(detection: Detection) -> bool {
    matches!(
        detection.mode,
        Mode::OnFoot(OnFootVariant::Ground) | Mode::Airborne(AirborneVariant::Falling)
    )
}

/// The guns in the hands for step `step`: the last publication while it is current, and none
/// otherwise.
pub(super) fn holds_for_step(step: Step) -> GunHolds {
    held_guns::holds_at(step).unwrap_or_default()
}

/// The hands that act because they hold or brace a gun, from the last publication: their bodies
/// are driven and joined to the gun's.
pub(crate) fn acting_hands() -> [bool; 2] {
    held_guns::holds_at(Step::now()).map_or([false; 2], |h| h.hands())
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::mode::{Traversal, VehicleClass, VehicleSeat};

    use super::*;
    use crate::controllers::physical_hands::mode_acts;

    #[test]
    fn grapple_traversal_allows_gun_bodies_but_not_ordinary_palms() {
        for mode in [
            Mode::OnFoot(OnFootVariant::Ground),
            Mode::Airborne(AirborneVariant::Falling),
        ] {
            let mut detection = Detection {
                mode,
                ..Detection::default()
            };
            assert!(mode_acts(detection));
            assert!(mode_allows_bodies(detection));
            for traversal in [
                Traversal::PreReel,
                Traversal::ReelingIn,
                Traversal::PreHang,
                Traversal::ReeledIn,
            ] {
                detection.traversal = Some(traversal);
                assert!(!mode_acts(detection));
                assert!(mode_allows_bodies(detection));
            }
        }
    }

    #[test]
    fn gun_bodies_do_not_enable_other_modes() {
        for mode in [
            Mode::OnFoot(OnFootVariant::Swimming),
            Mode::Airborne(AirborneVariant::Wingsuit),
            Mode::Airborne(AirborneVariant::Parachute),
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver: true,
            }),
            Mode::Ui,
        ] {
            for traversal in [None, Some(Traversal::ReelingIn)] {
                let detection = Detection {
                    mode,
                    traversal,
                    ..Detection::default()
                };
                assert!(!mode_acts(detection));
                assert!(!mode_allows_bodies(detection));
            }
        }
    }
}
