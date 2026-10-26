//! The support and weight targets shared by the physical hand drive and its drawn reference.

use std::time::Instant;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    hands::CockpitHand,
    physical_hands::physics::guns::{self, GunHolds},
    vehicles,
    weapons::{
        config::{ForegripConfig, HeftConfig},
        foregrip, heft,
    },
};

/// Compose support and weight once, before either driving a gun hand or drawing its body offset.
/// The physical drive keeps only the hands of current gun holds; ordinary active-hand composition
/// uses this same operation for every hand. The body supplies collisions and recoil afterward.
pub(crate) fn apply(
    hands: &mut [Option<CockpitHand>; 2],
    head: Vec3,
    foregrip: &ForegripConfig,
    heft: &HeftConfig,
) {
    foregrip::apply(hands, foregrip);
    heft::apply(hands, head, heft);
}

/// Give the physical drive the support and weight targets of the currently published guns.
/// Reads no native objects and does not compose active hands recursively.
pub(crate) fn apply_physical(
    hands: &mut [Option<CockpitHand>; 2],
    head: Vec3,
    foregrip: &ForegripConfig,
    heft: &HeftConfig,
) {
    let holds = guns::SHARED
        .lock()
        .fresh_holds(Instant::now())
        .unwrap_or_default();
    let raw = *hands;
    vehicles::apply(hands);
    apply(hands, head, foregrip, heft);
    retain_gun_targets(hands, raw, holds);
}

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

    use super::*;
    use crate::controllers::{
        frame::Pose,
        hands::pose as hand_pose,
        physical_hands::{
            PhysicalHandsConfig,
            palm::PalmBox,
            physics::guns::{
                HeldGun,
                math::{intended_seat, seated},
            },
        },
        weapons::{
            config::BracedHeftConfig,
            heft::{HeftClass, TestWeight},
        },
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
    fn heavy_weight_drives_the_palm_and_gun_once_for_both_hands() {
        let physical = PhysicalHandsConfig::new();
        let config = HeftConfig::new();
        for holder in Hand::BOTH {
            let mut spring = TestWeight::default();
            let mut shown = hand(Vec3::new(0.0, -0.2, -0.6));
            for frame in 0..90 {
                let t = frame as f32 / 90.0;
                let yaw = Quat::from_rotation_y(t * 0.3);
                let mut raw = hand(Vec3::new(
                    (holder.index() as f32 * 2.0 - 1.0) * 0.2 + t * 0.1,
                    -0.2,
                    -0.6,
                ));
                raw.wrist.orientation = yaw;
                raw.grip.orientation = yaw;
                shown = spring.apply(raw, HeftClass::Heavy, 1.0, &config, t);
                assert_eq!(shown.grip.position, raw.grip.position);
                assert!((shown.wrist.orientation * Vec3::NEG_Z).y < -0.1);
                let palm = PalmBox::from_wrist(shown.wrist, 1.0, &physical);
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
            let one_hand_pitch = (shown.wrist.orientation * Vec3::NEG_Z).y;
            for frame in 0..90 {
                shown = spring.apply(
                    hand(Vec3::new(0.0, -0.2, -0.6)),
                    HeftClass::Heavy,
                    0.0,
                    &config,
                    1.0 + frame as f32 / 90.0,
                );
            }
            assert!((shown.wrist.orientation * Vec3::NEG_Z).y > one_hand_pitch + 0.1);
            let disabled = HeftConfig {
                enabled: false,
                braced: BracedHeftConfig {
                    enabled: false,
                    ..config.braced
                },
                ..config
            };
            let raw = hand(Vec3::new(0.0, -0.2, -0.6));
            let mut no_weight = TestWeight::default();
            assert_eq!(
                no_weight.apply(raw, HeftClass::Heavy, 1.0, &disabled, 0.0),
                raw
            );
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
