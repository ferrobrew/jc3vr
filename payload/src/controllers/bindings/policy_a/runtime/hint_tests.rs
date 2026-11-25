//! Current descriptions retain the same aggregate permissions as table execution.

use jc3gi::input::input_action_map::Action;

use super::{
    snatch_tests::{GRIP, LEFT_A, MODE, TRIGGER, fixture},
    *,
};
use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            summary::permitted_routes,
            tables::{
                Bindings,
                declare::TableCtx,
                describe::{
                    Control as ShownControl, DescribeCtx, Effect, Features, ModAction,
                    describe_current,
                },
            },
            trigger_ownership::{self, TriggerMask},
        },
        grab::GrabVolume,
        loadout::Socket,
        mode::{Mode, VehicleClass, VehicleSeat},
    },
    vr::input::semantic::vocabulary::LandVehicleAction,
};

#[test]
fn wheel_held_grip_fire_is_absent_before_row_merge_and_returns_after_release() {
    let id = LandVehicleAction::FireSecondary.id();
    let f = super::snatch_tests::fixture_mode(&[(id, &[GRIP])], SemanticSet::LandVehicle);
    let mode = Mode::Vehicle(VehicleSeat {
        class: VehicleClass::Car,
        driver: true,
    });
    let mut bindings = Bindings::new();
    let mut controls = Controls {
        dominant: Hand::Right,
        ..Default::default()
    };
    controls.hands[0].grip = ButtonControl {
        down: true,
        pressed: true,
        released: false,
    };
    controls.hands[0].grab_volume = Some(GrabVolume::VehicleControl(
        crate::controllers::vehicles::physical::ControlKind::Wheel,
    ));
    for held in [true, false] {
        if !held {
            controls.hands[0].grip = ButtonControl {
                down: false,
                pressed: false,
                released: true,
            };
        }
        let step = bindings.step(mode, &controls, &f.input, 0.01, &BindingsConfig::new());
        let routes = permitted_routes(
            &bindings,
            mode,
            &controls,
            &f.input,
            &f.policy,
            [TriggerMask::default(); 2],
        );
        let ctx = DescribeCtx {
            mode,
            dominant: Hand::Right,
            table: TableCtx::of(&controls),
            features: Features::default(),
        };
        let rows = describe_current(&ctx, &f.table, Some(&routes));
        let hinted = rows.iter().any(|row| row.control == ShownControl::Grip(Hand::Left) && matches!(&row.effect, Effect::Actions(actions) if actions.contains(&Action::FIRE_VEHICLE_WEAPON_SECONDARY)));
        let executed = step
            .desired
            .iter()
            .any(|desired| desired.target.action == Action::FIRE_VEHICLE_WEAPON_SECONDARY);
        assert_eq!((hinted, executed), (!held, !held));
    }
}

#[test]
fn current_rows_match_execution_for_grip_trigger_aggregate_and_ready_other_hand() {
    for masked in [Physical::Grip, Physical::Trigger] {
        for aggregate in [false, true] {
            let id = OnFootAction::Jump.id();
            let physical = if masked == Physical::Grip {
                GRIP
            } else {
                TRIGGER
            };
            let mixed = if aggregate {
                vec![physical, A, LEFT_A]
            } else {
                vec![physical, LEFT_A]
            };
            let mut f = fixture(&[(id, &mixed)]);
            let mut controls = Controls {
                dominant: Hand::Right,
                ..Default::default()
            };
            let mut bindings = Bindings::new();
            if masked == Physical::Grip {
                controls.hands[0].grip = ButtonControl {
                    down: true,
                    pressed: true,
                    released: false,
                };
                controls.hands[0].grab_volume = Some(GrabVolume::Socket(Socket::Hip(Hand::Left)));
            }
            let triggers = if masked == Physical::Trigger {
                [
                    TriggerMask::default(),
                    TriggerMask {
                        reserved: true,
                        ..Default::default()
                    },
                ]
            } else {
                [TriggerMask::default(); 2]
            };
            trigger_ownership::mask_routes(&mut f.input, MODE, triggers);
            let step = bindings.step(MODE, &controls, &f.input, 0.01, &BindingsConfig::new());
            let routes =
                permitted_routes(&bindings, MODE, &controls, &f.input, &f.policy, triggers);
            let ctx = DescribeCtx {
                mode: MODE,
                dominant: Hand::Right,
                table: TableCtx::of(&controls),
                features: Features::default(),
            };
            let rows = describe_current(&ctx, &f.table, Some(&routes));
            let hinted = rows.iter().any(|row| {
                matches!(&row.effect, Effect::Actions(actions) if actions.contains(&Action::JUMP))
                    && row.control != ShownControl::Unbound
            });
            assert_eq!(
                hinted,
                step.desired
                    .iter()
                    .any(|desired| desired.target.action == Action::JUMP)
            );
            let blocked_hand = if masked == Physical::Grip {
                Hand::Left
            } else {
                Hand::Right
            };
            assert!(!routes.contains(&(id, blocked_hand)));
            assert_eq!(
                routes.contains(&(id, blocked_hand.other())),
                masked == Physical::Trigger || aggregate
            );
        }
    }
}

#[test]
fn outside_table_rows_obey_recovery_and_unresolved_routes() {
    let id = OnFootAction::WeaponWheel.id();
    for source in [A, "/user/hand/right/input/unknown/click"] {
        let f = fixture(&[(id, &[source])]);
        for recovering in [false, true] {
            let mut controls = Controls {
                dominant: Hand::Right,
                ..Default::default()
            };
            controls.masked_hands[1] = recovering;
            let routes = permitted_routes(
                &Bindings::new(),
                MODE,
                &controls,
                &f.input,
                &f.policy,
                [TriggerMask::default(); 2],
            );
            let ctx = DescribeCtx {
                mode: MODE,
                dominant: Hand::Right,
                table: TableCtx::of(&controls),
                features: Features {
                    wheel_stick: true,
                    ..Default::default()
                },
            };
            let rows = describe_current(&ctx, &f.table, Some(&routes));
            assert_eq!(
                rows.iter()
                    .any(|row| row.effect == Effect::Mod(ModAction::WeaponWheel)
                        && row.control == ShownControl::A(Hand::Right)),
                source == A && !recovering
            );
        }
    }
}
