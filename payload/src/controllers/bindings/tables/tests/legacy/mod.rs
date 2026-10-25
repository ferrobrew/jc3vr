//! The binding tables as they were before they read the semantic actions: every game meaning read
//! from the raw inputs by role (the dominant hand and the off-hand) and position. Frozen, as the
//! oracle the semantic tables are checked against ([`super::equivalence`]); nothing outside the
//! tests reads it.

pub mod declare;
pub mod menu;
pub mod sink;

use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::{
        controls::{ButtonControl, Controls},
        grab::GrabVolume,
        hold::{Desired, MapKind, Target, Want},
        tables::{
            Step,
            tests::legacy::{
                declare::{TableCtx, declare},
                sink::{EvalSink, Latches, MenuLatch},
            },
        },
    },
    config::BindingsConfig,
    frame::Hand,
    mode::{AirborneVariant, Mode, VehicleClass},
};

/// The raw-input tables and their cross-tick state.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    system: SystemButton,
    /// What each hand's current grip press was latched to at its press edge, by [`Hand::index`].
    grips: [Option<GripUse>; 2],
    /// Whether the current wingsuit A press was latched as an evade.
    evade_press: Option<bool>,
    /// How long the move stick has been pushed out of its rest, seconds, or `None` while it rests.
    move_pushed_s: Option<f32>,
    /// Whether the crouch flick has fired and waits for the stick to come back.
    crouch_flicked: bool,
    /// Each hand's menu press latches, by [`Hand::index`].
    menu: [MenuLatch; 2],
}

impl Bindings {
    pub const fn new() -> Self {
        Self {
            system: SystemButton { held: None },
            grips: [None; 2],
            evade_press: None,
            move_pushed_s: None,
            crouch_flicked: false,
            menu: [MenuLatch {
                a: None,
                b: None,
                stick: None,
            }; 2],
        }
    }

    /// Evaluate the tables for `mode`.
    pub fn step(
        &mut self,
        mode: Mode,
        controls: &Controls,
        dt: f32,
        config: &BindingsConfig,
    ) -> Step {
        for (latch, hand) in self.menu.iter_mut().zip(&controls.hands) {
            menu::step(latch, hand, config.menu_stick_push);
        }
        let chord = match mode {
            Mode::OnFoot(_) => SightChord::of(controls),
            _ => SightChord::default(),
        };
        let off_hand = controls.dominant.other();
        let grabs = Hand::BOTH.map(|hand| {
            let volume = controls.hand(hand).grab_volume;
            // The chord's off-hand grip press is part of the chord, not a grab: it must not let go
            // of a foregrip the off-hand holds.
            let volume = volume.filter(|v| {
                !(chord.dominant_grip && hand == off_hand && *v == GrabVolume::Foregrip)
            });
            latch_grip(
                &mut self.grips[hand.index()],
                controls.hand(hand).grip,
                volume,
            )
        });
        self.move_pushed_s = step_pushed(
            self.move_pushed_s,
            controls.off().stick.length(),
            EVADE_FLICK,
            dt,
        );
        let evade = match mode {
            Mode::Airborne(variant) => {
                latch_evade(&mut self.evade_press, variant, controls, self.move_pushed_s)
            }
            _ => false,
        };
        let latches = Latches {
            grip_default: self.grips.map(|g| g == Some(GripUse::Default)),
            chord_grip: chord.dominant_grip,
            chord_armed: chord.armed,
            evade,
            menu: self.menu,
        };
        let map = match mode {
            Mode::Ui => MapKind::Ui,
            _ => MapKind::Player,
        };
        let mut out = EvalSink::new(controls, latches, map);
        declare(&mut out, mode, &TableCtx::of(controls));
        let body_turn = body_turn(mode, controls);
        if !matches!(mode, Mode::Airborne(AirborneVariant::Wingsuit)) {
            self.evade_press = None;
        }
        self.system
            .step(&mut out.desired, system(controls), dt, config.long_press_s);
        let crouch_toggle = matches!(mode, Mode::OnFoot(_))
            && flick_down(
                &mut self.crouch_flicked,
                controls.dom().stick_raw.y,
                config.crouch_flick,
            );
        Step {
            desired: out.desired,
            body_turn,
            crouch_toggle,
            grabs,
            sight_capture: chord.capture,
            wheel: Default::default(),
            grapple_held: false,
        }
    }
}

/// The body-turn input for the headpose's VR turn scheme: the dominant stick's horizontal axis,
/// except in the modes whose tables use that stick themselves.
pub fn body_turn(mode: Mode, controls: &Controls) -> f32 {
    if body_turn_bound(mode) {
        controls.dom().stick_raw.x
    } else {
        0.0
    }
}

/// Whether the dominant stick's horizontal axis turns the body in `mode`.
pub const fn body_turn_bound(mode: Mode) -> bool {
    match mode {
        Mode::OnFoot(_) | Mode::Airborne(_) => true,
        Mode::Vehicle(seat) => {
            !matches!(seat.class, VehicleClass::Helicopter | VehicleClass::Plane)
        }
        Mode::Ui => false,
    }
}

/// What a grip press does, decided at its press edge and kept for the whole press, so that a hand
/// leaving a volume mid-press does not turn a grab into a grapple.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum GripUse {
    /// The grip's default action (the off-hand's grapple, a vehicle weapon).
    Default,
    /// A grab of the volume the hand was in, reported once in [`Step::grabs`]; the tables inject
    /// nothing for it.
    Grab(GrabVolume),
}

/// Latch a grip press at its press edge. Returns the volume a press latched as a grab this tick.
fn latch_grip(
    latch: &mut Option<GripUse>,
    grip: ButtonControl,
    volume: Option<GrabVolume>,
) -> Option<GrabVolume> {
    if !grip.held() {
        *latch = None;
        return None;
    }
    if latch.is_some() && !(grip.pressed && grip.released && !grip.down) {
        return None;
    }
    let latched = volume.map_or(GripUse::Default, GripUse::Grab);
    *latch = Some(latched);
    match latched {
        GripUse::Grab(volume) => Some(volume),
        GripUse::Default => None,
    }
}

/// A downward flick of a stick axis `y` (+Y forward) past `threshold`: fires once, then re-arms
/// once the stick is back above half the threshold.
fn flick_down(fired: &mut bool, y: f32, threshold: f32) -> bool {
    let threshold = threshold.clamp(0.05, 1.0);
    if !y.is_finite() {
        return false;
    }
    if *fired {
        if y > -threshold * 0.5 {
            *fired = false;
        }
        false
    } else if y <= -threshold {
        *fired = true;
        true
    } else {
        false
    }
}

/// The sight tuning capture chord on foot: both grips held and the dominant A pressed, armed only
/// while [`Controls::sight_chord`] is set.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct SightChord {
    /// The dominant grip is held with the chord armed: the off-hand's grapple is withheld.
    dominant_grip: bool,
    /// Both grips are held with the chord armed: the dominant A's jump is withheld.
    armed: bool,
    /// The dominant A was pressed with both grips held.
    capture: bool,
}

impl SightChord {
    fn of(c: &Controls) -> Self {
        if !c.sight_chord {
            return Self::default();
        }
        let (dom, off) = (c.dom(), c.off());
        let dominant_grip = dom.grip.held();
        let armed = dominant_grip && off.grip.held();
        Self {
            dominant_grip,
            armed,
            capture: armed && dom.a.pressed,
        }
    }
}

/// How far the move stick had to be pushed when the wingsuit's A was pressed for the press to
/// evade, `0..1`. The stick rested below half of it.
const EVADE_FLICK: f32 = 0.8;

/// How recently the move stick had to have left its rest for the wingsuit's A to evade, seconds.
const EVADE_WINDOW_S: f32 = 0.25;

/// Advance how long the move stick has been out of its rest: it rests below half the evade flick,
/// and the time counts from when it last left the rest.
fn step_pushed(pushed_s: Option<f32>, magnitude: f32, evade_flick: f32, dt: f32) -> Option<f32> {
    let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
    if !magnitude.is_finite() || magnitude < evade_flick * 0.5 {
        return None;
    }
    Some(pushed_s.map_or(0.0, |s| s + dt))
}

/// Decide at the wingsuit A's press edge whether the press evades or acts as the virtual A. It
/// evades only on a flick: the move stick is past the evade flick and left its rest within the
/// evade window. A stick held over to steer or dive is not a flick, so A still opens the parachute.
fn latch_evade(
    latch: &mut Option<bool>,
    variant: AirborneVariant,
    controls: &Controls,
    move_pushed_s: Option<f32>,
) -> bool {
    let a = controls.dom().a;
    if variant != AirborneVariant::Wingsuit || !a.held() {
        *latch = None;
        return false;
    }
    *latch.get_or_insert_with(|| {
        controls.off().stick.length() >= EVADE_FLICK
            && move_pushed_s.is_some_and(|s| s <= EVADE_WINDOW_S)
    })
}

/// The system button: a press shorter than the long-press time clicks pause on release; holding it
/// that long clicks the commlink at once, and the release then does nothing. Both clicks go to the
/// player map through its blockers, which is where the game reads the toggles while a menu is up,
/// and to the UI map, which the open screen reads.
#[derive(Debug, Clone, Default)]
struct SystemButton {
    /// How long the current press has been held, and whether it already fired the long press.
    held: Option<(f32, bool)>,
}

impl SystemButton {
    fn step(&mut self, out: &mut Vec<Desired>, button: ButtonControl, dt: f32, long_press_s: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if button.held() {
            let (seconds, fired) = match self.held {
                Some((seconds, fired)) => (seconds + dt, fired),
                None => (0.0, false),
            };
            let long = !fired && seconds >= long_press_s;
            if long {
                toggle(out, Action::GUI_PDA);
            }
            self.held = Some((seconds, fired || long));
        } else if let Some((_, fired)) = self.held.take()
            && !fired
        {
            toggle(out, Action::GUI_PAUSE);
        }
    }
}

/// The system button of either hand.
fn system(controls: &Controls) -> ButtonControl {
    let [l, r] = controls.hands.map(|h| h.system);
    ButtonControl {
        down: l.down || r.down,
        pressed: l.pressed || r.pressed,
        released: (l.released || r.released) && !(l.down || r.down),
    }
}

fn toggle(out: &mut Vec<Desired>, action: Action) {
    for map in [MapKind::PlayerThroughBlockers, MapKind::Ui] {
        out.push(Desired {
            target: Target { map, action },
            want: Want::Click,
        });
    }
}
