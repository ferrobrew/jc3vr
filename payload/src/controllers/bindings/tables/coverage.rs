//! Test support: every mode and table context, and the actions the evaluating tables inject in
//! each, for the guards that keep the description and its labels in step with the injection.

use std::collections::BTreeSet;

use glam::Vec2;
use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::{
        controls::{ButtonControl, Controls, HandControls},
        tables::{Bindings, declare::TableCtx},
    },
    config::BindingsConfig,
    frame::Hand,
    mode::{AirborneVariant, Mode, OnFootVariant, VehicleClass, VehicleSeat},
};

/// Every mode the detector reports.
pub fn all_modes() -> Vec<Mode> {
    let mut modes = vec![
        Mode::OnFoot(OnFootVariant::Ground),
        Mode::OnFoot(OnFootVariant::Swimming),
        Mode::Airborne(AirborneVariant::Falling),
        Mode::Airborne(AirborneVariant::Wingsuit),
        Mode::Airborne(AirborneVariant::Parachute),
        Mode::Ui,
    ];
    for class in [
        VehicleClass::Helicopter,
        VehicleClass::Plane,
        VehicleClass::Motorcycle,
        VehicleClass::Car,
        VehicleClass::Sea,
        VehicleClass::Unknown,
    ] {
        for driver in [true, false] {
            modes.push(Mode::Vehicle(VehicleSeat { class, driver }));
        }
    }
    modes
}

/// Every table context, under both handednesses.
pub fn all_contexts() -> Vec<(Hand, TableCtx)> {
    let mut contexts = Vec::new();
    for dominant in Hand::BOTH {
        for dual_wield in [false, true] {
            for weapon_cycle in [false, true] {
                for pointer_hand in [None, Some(Hand::Left), Some(Hand::Right)] {
                    contexts.push((
                        dominant,
                        TableCtx {
                            dual_wield,
                            weapon_cycle,
                            pointer_hand,
                        },
                    ));
                }
            }
        }
    }
    contexts
}

/// Every action the tables inject in `mode` under `ctx` over a set of ticks that between them
/// press every input, push every stick both ways, flick into the wingsuit's evade and not, and
/// press the system button both briefly and long.
pub fn injected(mode: Mode, dominant: Hand, ctx: TableCtx) -> BTreeSet<Action> {
    let config = BindingsConfig::new();
    let base = Controls {
        dominant,
        dual_wield: ctx.dual_wield,
        weapon_cycle: ctx.weapon_cycle,
        pointer_hand: ctx.pointer_hand,
        ..Default::default()
    };
    let everything = |stick: Vec2| {
        let mut c = base;
        c.yank = true;
        for hand in &mut c.hands {
            *hand = HandControls {
                trigger: 1.0,
                trigger_press: DOWN,
                grip: DOWN,
                a: DOWN,
                b: DOWN,
                stick_click: DOWN,
                system: ButtonControl::default(),
                stick,
                stick_raw: stick,
                grab_volume: None,
            };
        }
        c
    };
    let mut actions = BTreeSet::new();
    let mut run = |ticks: &[(Controls, f32)]| {
        let mut bindings = Bindings::new();
        for (controls, dt) in ticks {
            let step = bindings.step(mode, controls, *dt, &config);
            actions.extend(step.desired.iter().map(|d| d.target.action));
        }
    };
    run(&[(everything(Vec2::splat(0.8)), DT)]);
    run(&[(everything(Vec2::splat(-0.8)), DT)]);
    run(&[(everything(Vec2::ZERO), DT)]);
    let mut system = base;
    system.hands[0].system = DOWN;
    run(&[(system, DT), (base, DT)]);
    run(&[(system, DT), (system, config.long_press_s * 2.0)]);
    actions
}

const DT: f32 = 1.0 / 30.0;

const DOWN: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};
