//! The support and weight targets shared by the physical hand drive and its drawn reference.
//!
//! The vehicle controls' targets come first, through the vehicle handling's registration
//! ([`register_vehicle_targets`]), as in the hand composition; the held guns' support and weight
//! targets follow, through the weapon handling's registration with the held guns' store
//! ([`held_guns::register_targets`]).

use std::sync::OnceLock;

use glam::Vec3;
use jc3vr_body::controllers::{
    hands::CockpitHand,
    held::{guns as held_guns, guns::GunHolds},
    physics::steps::steps,
};
use jc3vr_input::frame::Hand;

/// Register the vehicle controls' targets: called with the cockpit-frame hands, it draws a hand
/// holding a control or a handle there. One registration: a second is refused and logged.
pub(crate) fn register_vehicle_targets(apply: fn(&mut [Option<CockpitHand>; 2])) {
    if VEHICLE_TARGETS.set(apply).is_err() {
        tracing::error!("hand bodies: vehicle targets are already registered; keeping the first");
    }
}

/// Whether the vehicle controls' targets are registered.
pub(crate) fn vehicle_targets_registered() -> bool {
    VEHICLE_TARGETS.get().is_some()
}

/// Give the physical drive the support and weight targets of the currently published guns.
/// Reads no native objects and does not compose active hands recursively.
pub(crate) fn apply_physical(hands: &mut [Option<CockpitHand>; 2], head: Vec3) {
    let holds = held_guns::holds_at(steps()).unwrap_or_default();
    let raw = *hands;
    if let Some(vehicle_targets) = VEHICLE_TARGETS.get() {
        vehicle_targets(hands);
    }
    held_guns::apply_targets(hands, head);
    retain_gun_targets(hands, raw, holds);
}

static VEHICLE_TARGETS: OnceLock<fn(&mut [Option<CockpitHand>; 2])> = OnceLock::new();

fn retain_gun_targets(
    hands: &mut [Option<CockpitHand>; 2],
    raw: [Option<CockpitHand>; 2],
    holds: GunHolds,
) {
    for hand in Hand::BOTH {
        let i = hand.index();
        if !holds.hands()[i] {
            hands[i] = raw[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use glam::Quat;
    use jc3vr_body::controllers::{
        hands::pose as hand_pose,
        held::guns::{HeftClass, HeldGun},
    };
    use jc3vr_input::frame::Pose;

    use super::*;
    use crate::controllers::physical_hands::{
        palm::PalmBox,
        physics::guns::math::{intended_seat, seated},
        tuning::PALMS,
    };

    fn hand(position: Vec3) -> CockpitHand {
        let pose = Pose {
            position,
            orientation: Quat::IDENTITY,
        };
        CockpitHand {
            wrist: pose,
            grip: pose,
            aim: pose,
        }
    }

    #[test]
    fn a_drooping_turning_hand_seats_its_gun_body_on_the_drawn_gun() {
        // The weight turns the holding hand about its grip, pitching it down and trailing a turn;
        // wherever it leaves the wrist, the gun body seats on the gun drawn on it.
        for frame in 0..90 {
            let t = frame as f32 / 90.0;
            let mut shown = hand(Vec3::new(t * 0.1, -0.2, -0.6));
            shown.wrist.orientation =
                Quat::from_rotation_y(t * 0.3) * Quat::from_rotation_x(-t * 0.6);
            let palm = PalmBox::from_wrist(shown.wrist, 1.0, &PALMS);
            let palm = Pose {
                position: palm.centre,
                orientation: palm.orientation,
            };
            let gun_on_wrist = Pose {
                position: Vec3::new(0.0, -0.02, -0.04),
                orientation: Quat::IDENTITY,
            };
            let drawn_gun = hand_pose::mul(shown.wrist, gun_on_wrist);
            let seat = intended_seat(palm, drawn_gun);
            let body = seated(palm, seat);
            assert!(body.position.abs_diff_eq(drawn_gun.position, 1e-5));
            assert!(body.orientation.angle_between(drawn_gun.orientation) < 1e-4);
        }
    }

    #[test]
    fn only_holding_and_supporting_hands_take_the_physical_target() {
        for holder in Hand::BOTH {
            let raw = [Some(hand(Vec3::X)), Some(hand(Vec3::NEG_X))];
            let adjusted = [Some(hand(Vec3::Y)), Some(hand(Vec3::NEG_Y))];
            let mut holds = GunHolds::default();
            holds.guns[holder.index()] = Some(HeldGun {
                weapon: NonZeroUsize::new(1).unwrap(),
                gun: NonZeroUsize::new(2).unwrap(),
                wrapper: NonZeroUsize::new(3).unwrap(),
                body: 4,
                class: HeftClass::Heavy,
                guns: 1,
                carry: None,
            });
            let mut one = adjusted;
            retain_gun_targets(&mut one, raw, holds);
            assert_eq!(one[holder.index()], adjusted[holder.index()]);
            assert_eq!(one[holder.other().index()], raw[holder.other().index()]);
            holds.braces[holder.other().index()] = true;
            let mut two = adjusted;
            retain_gun_targets(&mut two, raw, holds);
            assert_eq!(two, adjusted);
            retain_gun_targets(&mut two, raw, GunHolds::default());
            assert_eq!(two, raw);
        }
    }
}
