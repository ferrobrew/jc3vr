//! Test support: every mode and table context, and the actions the evaluating tables inject in
//! each, for the guards that keep the description and its labels in step with the injection.

use std::collections::BTreeSet;

use glam::Vec2;
use jc3gi::input::input_action_map::Action;

use crate::{
    controllers::{
        bindings::{
            controls::{ButtonControl, Controls, HandControls},
            intents::simulate::simulate,
            tables::{Bindings, declare::TableCtx},
        },
        config::BindingsConfig,
        frame::Hand,
        mode::{AirborneVariant, Mode, OnFootVariant, VehicleClass, VehicleSeat},
    },
    vr::input::semantic::{SemanticSet, bound::Layout, defaults::default_binding},
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
                    for trackpad_paging in [false, true] {
                        contexts.push((
                            dominant,
                            TableCtx {
                                dual_wield,
                                weapon_cycle,
                                pointer_hand,
                                trackpad_paging,
                            },
                        ));
                    }
                }
            }
        }
    }
    contexts
}

/// Every action the tables inject in `mode` under `ctx` with the shipped default bindings, over a
/// set of ticks that between them press every input with the grips held and without, push every
/// stick each way, press each trackpad quadrant, hold each stick click alone past the chord
/// window, flick into the wingsuit's evade and not, and press the system button both briefly and
/// long.
pub fn injected(mode: Mode, dominant: Hand, ctx: TableCtx) -> BTreeSet<Action> {
    injected_with(mode, dominant, ctx, default_binding)
}

/// [`injected`], under `layout`.
pub fn injected_with(
    mode: Mode,
    dominant: Hand,
    ctx: TableCtx,
    layout: Layout,
) -> BTreeSet<Action> {
    let config = BindingsConfig::new();
    let base = Controls {
        dominant,
        dual_wield: ctx.dual_wield,
        weapon_cycle: ctx.weapon_cycle,
        pointer_hand: ctx.pointer_hand,
        trackpad_paging: ctx.trackpad_paging,
        ..Default::default()
    };
    let everything = |stick: Vec2, grip: bool, pad: Vec2| {
        let mut c = base;
        c.yank = true;
        for hand in &mut c.hands {
            *hand = HandControls {
                trigger: 1.0,
                trigger_press: DOWN,
                grip: if grip { DOWN } else { ButtonControl::default() },
                a: DOWN,
                b: DOWN,
                stick_click: ButtonControl::default(),
                system: ButtonControl::default(),
                trackpad_left: DOWN,
                trackpad_right: DOWN,
                stick,
                stick_raw: stick,
                trackpad: pad,
                grab_volume: None,
                grab_slow: false,
            };
        }
        c
    };
    let mut actions = BTreeSet::new();
    let set = SemanticSet::of(mode.action_set());
    let mut run = |ticks: &[(Controls, f32)]| {
        let mut bindings = Bindings::new();
        for (controls, dt) in ticks {
            let input = simulate(controls, set, layout);
            let step = bindings.step(mode, controls, &input, *dt, &config);
            actions.extend(step.desired.iter().map(|d| d.target.action));
        }
    };
    for grip in [false, true] {
        for (i, stick) in [
            Vec2::splat(0.8),
            Vec2::splat(-0.8),
            Vec2::new(0.8, 0.0),
            Vec2::new(-0.8, 0.0),
            Vec2::new(0.0, 0.8),
            Vec2::new(0.0, -0.8),
            Vec2::ZERO,
        ]
        .into_iter()
        .enumerate()
        {
            // The trackpads take each quadrant in turn, the centre too.
            let pad = [
                Vec2::new(0.0, 0.8),
                Vec2::new(0.8, 0.0),
                Vec2::new(-0.8, 0.0),
                Vec2::new(0.0, -0.8),
            ]
            .get(i)
            .copied()
            .unwrap_or(Vec2::ZERO);
            run(&[(everything(stick, grip, pad), DT)]);
        }
    }
    for hand in Hand::BOTH {
        let mut click = base;
        click.hands[hand.index()].stick_click = DOWN;
        run(&[
            (click, DT),
            (click, config.stick_click_windows()[hand.index()] * 2.0),
        ]);
    }
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
