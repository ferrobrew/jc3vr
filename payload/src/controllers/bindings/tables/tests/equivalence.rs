//! The semantic tables against the raw-input tables they replace ([`super::legacy`]).
//!
//! Given the semantic input that the layout the raw-input tables encoded ([`legacy_layout`]) makes
//! of a tick of raw controls ([`simulate`]), the semantic tables must want exactly what the
//! raw-input tables wanted for those controls, in the same order, tick by tick, for every mode and
//! table context of [`crate::controllers::bindings::tables::coverage`] and a battery of press
//! sequences. That separates the port of the tables from the change of the default layout: the
//! shipped defaults are free to move, and this still checks that each meaning is read as before.
//!
//! The semantic tables differ from the raw-input ones on purpose in three places, which the
//! comparison accounts for:
//!
//! - Out of the driver's seat, the throttle actions are not read; the raw-input tables injected
//!   the throttle from the triggers there too, where it drives nothing ([`normalise`]).
//! - In a vehicle the grapple has its own action, which the raw layout put on the secondary
//!   weapon's button, so its writes follow that button's rather than sit among them ([`in_order`]).
//! - The yank is a source of the grapple action, so the sight tuning chord's grip withholds a
//!   yank as it withholds the grip's grapple; no sequence yanks during the chord.

use glam::Vec2;

use super::*;
use crate::{
    controllers::bindings::{
        controls::radial_deadzone,
        grab::HolsterSite,
        tables::{
            coverage::{all_contexts, all_modes},
            tests::legacy,
        },
    },
    vr::input::semantic::{
        defaults::{Component, DefaultBinding, Side},
        vocabulary::{
            GlobalAction, HelicopterAction, LandVehicleAction, MenuAction, OnFootAction,
            PlaneAction, WingsuitAction,
        },
    },
};

#[test]
fn the_semantic_tables_want_what_the_raw_tables_wanted() {
    let mut checked = 0usize;
    for mode in all_modes() {
        for (dominant, ctx) in all_contexts() {
            for (name, config, ticks) in sequences(mode, dominant, &ctx) {
                let mut old = legacy::Bindings::new();
                let mut new = Bindings::new();
                for (i, (controls, dt)) in ticks.iter().enumerate() {
                    let input =
                        simulate(controls, SemanticSet::of(mode.action_set()), legacy_layout);
                    let want = in_order(
                        normalise(old.step(mode, controls, *dt, &config), mode),
                        mode,
                    );
                    let got = in_order(new.step(mode, controls, &input, *dt, &config), mode);
                    assert_eq!(
                        got, want,
                        "{mode:?}, {dominant:?}-handed, {ctx:?}: {name}, tick {i}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100_000, "{checked} ticks compared");
}

/// The wanted actions of a vehicle mode in one order: the grapple has its own action there, so
/// its writes come after the secondary weapon's button rather than among them.
fn in_order(mut step: Step, mode: Mode) -> Step {
    if matches!(mode, Mode::Vehicle(_)) {
        step.desired.sort_by_key(|d| d.target);
    }
    step
}

/// The throttle actions out of the driver's seat, which the semantic tables do not read.
fn normalise(mut step: Step, mode: Mode) -> Step {
    if let Mode::Vehicle(seat) = mode
        && !seat.driver
    {
        const THROTTLE: [Action; 6] = [
            ACCELERATE,
            REVERSE,
            HELI_INC_ALTITUDE,
            HELI_DEC_ALTITUDE,
            PLANE_INC_TRUST,
            PLANE_DEC_TRUST,
        ];
        step.desired
            .retain(|d| !THROTTLE.contains(&d.target.action));
    }
    step
}

type Sequence = (String, BindingsConfig, Vec<(Controls, f32)>);

/// The press sequences for one mode and context: every input of each hand pressed, held, and
/// released alone and with that hand's grip, taps between ticks, the sticks and trackpads each
/// way, grabs, the stick clicks under the chord window, the system button short and long, the
/// yank, the sight chord, the wingsuit's flick, and everything at once.
fn sequences(mode: Mode, dominant: Hand, ctx: &TableCtx) -> Vec<Sequence> {
    let base = Controls {
        dominant,
        dual_wield: ctx.dual_wield,
        weapon_cycle: ctx.weapon_cycle,
        pointer_hand: ctx.pointer_hand,
        trackpad_paging: ctx.trackpad_paging,
        ..Default::default()
    };
    let mut out: Vec<Sequence> = Vec::new();
    let mut add = |name: String, config: BindingsConfig, ticks: Vec<(Controls, f32)>| {
        out.push((name, config, ticks));
    };

    type Setter = fn(&mut HandControls, ButtonControl);
    let buttons: [(&str, Setter); 7] = [
        ("a", |h, b| h.a = b),
        ("b", |h, b| h.b = b),
        ("trigger", |h, b| {
            h.trigger_press = b;
            h.trigger = if b.down { 0.9 } else { 0.0 };
        }),
        ("grip", |h, b| h.grip = b),
        ("system", |h, b| h.system = b),
        ("trackpad left", |h, b| {
            h.trackpad_left = b;
            h.trackpad = if b.held() {
                Vec2::new(-0.7, 0.1)
            } else {
                Vec2::ZERO
            };
        }),
        ("trackpad right", |h, b| {
            h.trackpad_right = b;
            h.trackpad = if b.held() {
                Vec2::new(0.7, -0.1)
            } else {
                Vec2::ZERO
            };
        }),
    ];
    for hand in Hand::BOTH {
        for (name, set) in buttons {
            for with_grip in [false, true] {
                let ticks = [PRESS, HELD, HELD, RELEASE, UP, TAP, UP]
                    .into_iter()
                    .map(|b| {
                        let mut c = base;
                        let h = &mut c.hands[hand.index()];
                        if with_grip {
                            h.grip = HELD;
                        }
                        set(h, b);
                        (c, DT)
                    })
                    .collect();
                add(
                    format!("{name} on {hand:?}, grip {with_grip}"),
                    CONFIG,
                    ticks,
                );
            }
        }
        // The grip squeezed mid-press, and let go mid-press.
        let ticks = [
            (PRESS, false),
            (HELD, true),
            (HELD, false),
            (RELEASE, false),
        ]
        .into_iter()
        .map(|(a, grip)| {
            let mut c = base;
            let h = &mut c.hands[hand.index()];
            h.a = a;
            h.b = a;
            if grip {
                h.grip = PRESS;
            }
            (c, DT)
        })
        .collect();
        add(format!("grip mid-press on {hand:?}"), CONFIG, ticks);
        // An analog trigger below and past its press.
        let ticks = [0.03, 0.3, 0.6, 1.0, 0.4, 0.0]
            .into_iter()
            .map(|pull| {
                let mut c = base;
                let h = &mut c.hands[hand.index()];
                h.trigger = pull;
                h.trigger_press = if pull >= 0.6 { HELD } else { UP };
                (c, DT)
            })
            .collect();
        add(format!("trigger pull on {hand:?}"), CONFIG, ticks);
        // The stick each way, and swung round, with and without the grip.
        for with_grip in [false, true] {
            let ticks = [
                Vec2::new(0.0, 0.9),
                Vec2::new(0.4, 0.8),
                Vec2::new(0.9, 0.3),
                Vec2::new(0.9, -0.2),
                Vec2::new(0.0, -0.9),
                Vec2::new(-0.9, -0.4),
                Vec2::new(-0.2, 0.1),
                Vec2::new(0.6, 0.6),
                Vec2::ZERO,
            ]
            .into_iter()
            .map(|stick| {
                let mut c = base;
                let h = &mut c.hands[hand.index()];
                h.stick_raw = stick;
                h.stick = radial(stick);
                if with_grip {
                    h.grip = HELD;
                }
                (c, DT)
            })
            .collect();
            add(
                format!("stick on {hand:?}, grip {with_grip}"),
                CONFIG,
                ticks,
            );
        }
        // A grip press in a grab volume, held as the hand leaves it, and a fresh press outside.
        if mode != Mode::Ui {
            for volume in [
                GrabVolume::Foregrip,
                GrabVolume::Holster(HolsterSite::Chest),
            ] {
                let ticks = [
                    (PRESS, Some(volume)),
                    (HELD, None),
                    (RELEASE, None),
                    (PRESS, None),
                    (HELD, None),
                    (UP, None),
                    (TAP, Some(volume)),
                    (UP, None),
                ]
                .into_iter()
                .map(|(grip, volume)| {
                    let mut c = base;
                    let h = &mut c.hands[hand.index()];
                    h.grip = grip;
                    h.grab_volume = volume;
                    (c, DT)
                })
                .collect();
                add(format!("grab {volume:?} on {hand:?}"), CONFIG, ticks);
            }
        }
        // A lone stick click, under the chord window and without it.
        for (config, label) in [(CONFIG, "no window"), (CHORD_CONFIG, "window")] {
            let mut ticks: Vec<_> = std::iter::once(PRESS)
                .chain(std::iter::repeat_n(HELD, 15))
                .chain([RELEASE, UP, TAP, UP, UP])
                .map(|click| {
                    let mut c = base;
                    c.hands[hand.index()].stick_click = click;
                    (c, DT)
                })
                .collect();
            ticks.push((base, DT));
            add(format!("stick click on {hand:?}, {label}"), config, ticks);
        }
    }
    // Both stick clicks: the recenter chord.
    let ticks = std::iter::once([PRESS, UP])
        .chain([[HELD, PRESS]])
        .chain(std::iter::repeat_n([HELD, HELD], 40))
        .chain([[RELEASE, HELD], [UP, RELEASE], [UP, UP]])
        .map(|[l, r]| {
            let mut c = base;
            c.hands[0].stick_click = l;
            c.hands[1].stick_click = r;
            (c, DT)
        })
        .collect();
    add("stick click chord".into(), CHORD_CONFIG, ticks);
    // A long system press.
    let ticks = std::iter::once(PRESS)
        .chain(std::iter::repeat_n(HELD, 30))
        .chain([RELEASE, UP])
        .map(|system| {
            let mut c = base;
            c.hands[dominant.index()].system = system;
            (c, DT)
        })
        .collect();
    add("long system press".into(), CONFIG, ticks);
    // The yank, with and without the grapple grip.
    let ticks = [
        (true, UP),
        (false, UP),
        (true, HELD),
        (false, HELD),
        (false, UP),
    ]
    .into_iter()
    .map(|(yank, grip)| {
        let mut c = base;
        c.yank = yank;
        c.hands[dominant.other().index()].grip = grip;
        (c, DT)
    })
    .collect();
    add("yank".into(), CONFIG, ticks);
    // The sight chord: both grips, then the dominant A.
    let ticks = [
        (PRESS, UP, UP),
        (HELD, PRESS, UP),
        (HELD, HELD, PRESS),
        (HELD, HELD, HELD),
        (HELD, RELEASE, HELD),
        (UP, UP, RELEASE),
    ]
    .into_iter()
    .map(|(dom_grip, off_grip, a)| {
        let mut c = base;
        c.sight_chord = true;
        c.hands[dominant.index()].grip = dom_grip;
        c.hands[dominant.other().index()].grip = off_grip;
        c.hands[dominant.other().index()].grab_volume =
            (mode != Mode::Ui).then_some(GrabVolume::Foregrip);
        c.hands[dominant.index()].a = a;
        (c, DT)
    })
    .collect();
    add("sight chord".into(), CONFIG, ticks);
    // The wingsuit's flick into a press, and a stick held over before the press.
    for (name, sticks) in [
        (
            "flick",
            vec![Vec2::ZERO, Vec2::new(0.95, 0.0), Vec2::new(0.95, 0.0)],
        ),
        ("held over", vec![Vec2::new(0.95, 0.0); 12]),
    ] {
        let last = sticks.len() - 1;
        let mut ticks: Vec<_> = sticks
            .into_iter()
            .enumerate()
            .map(|(i, stick)| {
                let mut c = base;
                let off = &mut c.hands[dominant.other().index()];
                off.stick_raw = stick;
                off.stick = radial(stick);
                c.hands[dominant.index()].a = if i == last { PRESS } else { UP };
                (c, DT)
            })
            .collect();
        let mut held = base;
        held.hands[dominant.index()].a = HELD;
        ticks.push((held, DT));
        ticks.push((base, DT));
        add(format!("wingsuit {name}"), CONFIG, ticks);
    }
    // Everything at once, each trackpad on its own half, the sticks pushed.
    for grip in [UP, HELD] {
        let mut c = base;
        for (hand, pad) in Hand::BOTH.into_iter().zip([-0.7, 0.7]) {
            let h = &mut c.hands[hand.index()];
            *h = HandControls {
                trigger: 1.0,
                trigger_press: HELD,
                grip,
                a: HELD,
                b: HELD,
                stick_click: UP,
                system: UP,
                trackpad_left: if pad < 0.0 { HELD } else { UP },
                trackpad_right: if pad > 0.0 { HELD } else { UP },
                stick: radial(Vec2::new(0.5, 0.7)),
                stick_raw: Vec2::new(0.5, 0.7),
                trackpad: Vec2::new(pad, 0.0),
                grab_volume: None,
                grab_slow: false,
            };
        }
        let mut pressed = c;
        for h in &mut pressed.hands {
            for b in [&mut h.trigger_press, &mut h.a, &mut h.b] {
                *b = PRESS;
            }
            if h.grip.down {
                h.grip = PRESS;
            }
            for b in [&mut h.trackpad_left, &mut h.trackpad_right] {
                if b.down {
                    *b = PRESS;
                }
            }
        }
        add(
            format!("everything, grip {}", grip.down),
            CONFIG,
            vec![(pressed, DT), (c, DT), (c, DT), (base, DT)],
        );
    }
    out
}

fn radial(stick: Vec2) -> Vec2 {
    radial_deadzone(stick, CONFIG.move_deadzone)
}

const UP: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: false,
};
const PRESS: ButtonControl = ButtonControl {
    down: true,
    pressed: true,
    released: false,
};
const HELD: ButtonControl = ButtonControl {
    down: true,
    pressed: false,
    released: false,
};
const RELEASE: ButtonControl = ButtonControl {
    down: false,
    pressed: false,
    released: true,
};
const TAP: ButtonControl = ButtonControl {
    down: false,
    pressed: true,
    released: true,
};

/// The default layout the raw-input tables encoded, frozen: the dominant A jumps, the dominant B
/// reloads, the off-hand A uses, and in vehicles the off-hand A exits.
pub(crate) fn legacy_layout(set: SemanticSet, index: usize) -> Option<DefaultBinding> {
    use Component::*;
    use Side::*;
    let on = |side, component| Some(DefaultBinding { side, component });
    match set {
        SemanticSet::Global => {
            use GlobalAction::*;
            match GlobalAction::ALL[index] {
                Pause => on(Both, System),
                OpenCommlink | RecenterView | ToggleDebugPanel => None,
                ShowControlsSummary => on(Off, TrackpadTouch),
            }
        }
        SemanticSet::OnFoot => {
            use OnFootAction::*;
            match OnFootAction::ALL[index] {
                Move => on(Off, Thumbstick),
                Turn => on(Dom, ThumbstickX),
                CrouchFlick => on(Dom, ThumbstickY),
                Fire => on(Dom, TriggerValue),
                FireOffHandPistol | RetractTethers => on(Off, TriggerValue),
                RetractTethersAlt | NextWeapon => on(Off, ThumbstickClick),
                FireGrapple => on(Off, GripPress),
                Jump => on(Dom, LowerFace),
                Use => on(Off, LowerFace),
                Reload => on(Dom, UpperFace),
                ReleaseTethers => on(Off, UpperFace),
                Detonate => on(Dom, ThumbstickClick),
                ThrowGrenade | WeaponSelectPad | WeaponSelectPress | WeaponWheel | Grapple => None,
            }
        }
        SemanticSet::Wingsuit => {
            use WingsuitAction::*;
            match WingsuitAction::ALL[index] {
                Steer => on(Off, Thumbstick),
                Turn => on(Dom, ThumbstickX),
                AirBrake => on(Off, TriggerValue),
                FireGuns => on(Dom, TriggerValue),
                FireSecondary => on(Dom, GripPress),
                Boost => on(Dom, UpperFace),
                OpenParachute => on(Dom, LowerFace),
                Use => on(Off, LowerFace),
                ReleaseTethers => on(Off, UpperFace),
                FireGrapple => on(Off, GripPress),
                Grapple => None,
            }
        }
        SemanticSet::LandVehicle => {
            use LandVehicleAction::*;
            match LandVehicleAction::ALL[index] {
                Steer => on(Off, Thumbstick),
                Turn => on(Dom, ThumbstickX),
                Accelerate | Fire => on(Dom, TriggerValue),
                Reverse | FireOffHandPistol => on(Off, TriggerValue),
                FirePrimary => on(Dom, GripPress),
                // The grapple shared the secondary weapon's button.
                FireSecondary | Grapple => on(Off, GripPress),
                Stunt => on(Dom, LowerFace),
                ExitVehicle => on(Off, LowerFace),
                VehicleMod => on(Dom, UpperFace),
                Handbrake => on(Off, UpperFace),
                Horn => on(Off, ThumbstickClick),
            }
        }
        SemanticSet::Helicopter => {
            use HelicopterAction::*;
            match HelicopterAction::ALL[index] {
                Cyclic => on(Off, Thumbstick),
                Yaw => on(Dom, ThumbstickX),
                CollectiveUp | Fire => on(Dom, TriggerValue),
                CollectiveDown | FireOffHandPistol => on(Off, TriggerValue),
                FirePrimary => on(Dom, GripPress),
                FireSecondary | Grapple => on(Off, GripPress),
                Stunt => on(Dom, LowerFace),
                ExitVehicle => on(Off, LowerFace),
                VehicleMod => on(Dom, UpperFace),
                Horn => on(Off, ThumbstickClick),
            }
        }
        SemanticSet::Plane => {
            use PlaneAction::*;
            match PlaneAction::ALL[index] {
                PitchRoll => on(Off, Thumbstick),
                Rudder => on(Dom, ThumbstickX),
                ThrustUp | Fire => on(Dom, TriggerValue),
                ThrustDown | FireOffHandPistol => on(Off, TriggerValue),
                FirePrimary => on(Dom, GripPress),
                FireSecondary | Grapple => on(Off, GripPress),
                Stunt => on(Dom, LowerFace),
                ExitVehicle => on(Off, LowerFace),
                Horn => on(Off, ThumbstickClick),
            }
        }
        SemanticSet::Menu => {
            use MenuAction::*;
            match MenuAction::ALL[index] {
                MenuConfirm => on(Both, LowerFace),
                MenuBack => on(Both, UpperFace),
                MenuModifier => on(Both, GripPress),
                MenuNavigate => on(Both, Thumbstick),
                MenuTabPad => on(Both, Trackpad),
                MenuTabPress => on(Both, TrackpadForce),
                MenuPagePrev => on(Left, TriggerValue),
                MenuPageNext => on(Right, TriggerValue),
                PointerSelect => on(Both, TriggerValue),
                MenuFocus => on(Left, ThumbstickClick),
                MenuRecenter => on(Right, ThumbstickClick),
                MapZoom => on(Off, ThumbstickY),
            }
        }
    }
}
