//! The per-mode binding tables: from the logical controls to the game actions wanted this tick.
//! Pure, with the little state the tables need across ticks (the system button's long press, what
//! each grip press was latched to, and the wingsuit's evade press).
//!
//! The tables follow the plan of record ("Per-mode bindings"), laid out for the Valve Index
//! controller, which has A and B on both hands: the plan's left X and Y are the off-hand's A and B.
//! The "right" and "left" of the plan are the dominant and the off hand, so a left-handed layout
//! mirrors every gameplay binding. The menu bindings stay on the physical hands, so that the
//! previous and next tabs and pages keep their sides.
//!
//! Where a controller input stands for a gamepad button, it injects every gameplay action the
//! game's default gamepad keymap puts on that button, and the game's own context picks the one that
//! applies (a virtual gamepad button). This is what keeps the game's tap and hold overloads
//! intact: the reload button is also the holster (hold), `USE_VEHICLE_MOD` is nitrous (hold) and
//! the turbo jump (tap), `ENTER_VEHICLE` held enters the gunner seat, and the grapple tethers on a
//! hold, all decided by the game from the hold time the tracker ([`crate::controllers::bindings::hold`])
//! writes. The system button is the one overload the mod resolves itself: a short press pauses and
//! a long press opens the commlink.
//!
//! One binding drives the mod rather than the game: on foot, flicking the dominant stick down
//! toggles the seated crouch ([`crate::controllers::roomscale::toggle_seated_crouch`]). The stick's
//! vertical axis is otherwise unused on foot, where only its horizontal axis turns the body, and the
//! flick works the same on the Index and Touch controllers.
//!
//! One chord drives the mod as well, and only while the sight tuning tool's marker is placed
//! ([`Controls::sight_chord`]): on foot, holding both grips and pressing the dominant A captures the
//! held weapon's sight offset ([`crate::controllers::aim::sights::tuning`]). While it is armed, the
//! dominant grip withholds the off-hand's grapple (squeeze the dominant grip first) and both grips
//! withhold the dominant A's jump, so the chord neither grapples nor jumps.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::{
        controls::{ButtonControl, Controls, rotate_stick},
        grab::GrabVolume,
        hold::{Desired, MapKind, Target, Want},
    },
    config::BindingsConfig,
    frame::Hand,
    mode::{AirborneVariant, Mode, VehicleClass, VehicleSeat},
};

/// One tick's output of the tables.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Step {
    /// The wanted game actions.
    pub desired: Vec<Desired>,
    /// The body-turn input for the headpose's VR turn scheme, `-1..=1`, positive turning right.
    /// Zero in the modes whose tables use the dominant stick themselves.
    pub body_turn: f32,
    /// The seated crouch toggle was flicked this tick.
    pub crouch_toggle: bool,
    /// The grab each hand's grip press started this tick, indexed by [`Hand::index`]: the volume the
    /// hand was in at the press edge. The grabbing subsystems act on it; the tables inject nothing
    /// for it.
    pub grabs: [Option<GrabVolume>; 2],
    /// The sight tuning capture chord was pressed this tick.
    pub sight_capture: bool,
}

/// The tables and their cross-tick state.
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
}

impl Bindings {
    pub const fn new() -> Self {
        Self {
            system: SystemButton { held: None },
            grips: [None; 2],
            evade_press: None,
            move_pushed_s: None,
            crouch_flicked: false,
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
            config.evade_flick,
            dt,
        );
        let off_grapple = self.grips[controls.dominant.other().index()] == Some(GripUse::Default);
        let dom_grip = self.grips[controls.dominant.index()] == Some(GripUse::Default);

        let mut out = Out::new(MapKind::Player);
        let mut sight_capture = false;
        let body_turn = match mode {
            Mode::OnFoot(_) => {
                sight_capture = chord.capture;
                on_foot(&mut out, controls, off_grapple, chord);
                controls.dom().stick_raw.x
            }
            Mode::Vehicle(seat) => {
                vehicle(&mut out, controls, seat, off_grapple, dom_grip);
                match seat.class {
                    VehicleClass::Helicopter | VehicleClass::Plane => 0.0,
                    _ => controls.dom().stick_raw.x,
                }
            }
            Mode::Airborne(variant) => {
                let evade = latch_evade(
                    &mut self.evade_press,
                    variant,
                    controls,
                    self.move_pushed_s,
                    config,
                );
                airborne(&mut out, controls, variant, off_grapple, dom_grip, evade);
                controls.dom().stick_raw.x
            }
            Mode::Ui => {
                out.map = MapKind::Ui;
                ui(&mut out, controls);
                0.0
            }
        };
        if !matches!(mode, Mode::Airborne(AirborneVariant::Wingsuit)) {
            self.evade_press = None;
        }
        self.system
            .step(&mut out, controls.system(), dt, config.long_press_s);
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
            sight_capture,
        }
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
    config: &BindingsConfig,
) -> bool {
    let a = controls.dom().a;
    if variant != AirborneVariant::Wingsuit || !a.held() {
        *latch = None;
        return false;
    }
    *latch.get_or_insert_with(|| {
        controls.off().stick.length() >= config.evade_flick
            && move_pushed_s.is_some_and(|s| s <= config.evade_window_s)
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
    fn step(&mut self, out: &mut Out, button: ButtonControl, dt: f32, long_press_s: f32) {
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

fn toggle(out: &mut Out, action: Action) {
    for map in [MapKind::PlayerThroughBlockers, MapKind::Ui] {
        out.desired.push(Desired {
            target: Target { map, action },
            want: Want::Click,
        });
    }
}

/// The virtual gamepad buttons: the gameplay actions the default gamepad keymap puts on each.
/// `PUSH_GRAPPLE` is left off B: under the hammer ability it is melee, which the controllers
/// trigger with a strike, and on B it would melee on every tether release.
const PAD_A: &[Action] = &[JUMP, OPEN_PARACHUTE, STUNT_JUMP, BTN_A];
const PAD_B: &[Action] = &[RELEASE_GRAPPLE, CANCEL, BYPASS_FALL_PREVENTION, BTN_B];
const PAD_X_ON_FOOT: &[Action] = &[RELOAD, BTN_X];
const PAD_Y_ON_FOOT: &[Action] = &[USE_ITEM, ENTER_VEHICLE, OPEN_WINGSUIT, BTN_Y];
const PAD_Y_VEHICLE: &[Action] = &[EXIT_VEHICLE, ENTER_VEHICLE, BTN_Y];
const PAD_RB_ON_FOOT: &[Action] = &[THROW_GRENADE, PLANT_EXPLOSIVE, DETONATE_EXPLOSIVE, R1];
const PAD_LB_VEHICLE: &[Action] = &[FIRE_VEHICLE_WEAPON_SECONDARY, FIRE_GRAPPLE, MC_RELOAD, L1];
const PAD_L3_VEHICLE: &[Action] = &[SOUND_HORN_SIREN, TOGGLE_MAGNET_1, L3];

/// On foot and swimming.
fn on_foot(out: &mut Out, c: &Controls, off_grapple: bool, chord: SightChord) {
    let (dom, off) = (c.dom(), c.off());
    let stick = rotate_stick(off.stick, c.move_rotation);
    out.axis(stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    out.axis(stick.x, MOVE_LEFT, MOVE_RIGHT);
    out.analog(stick.length().min(1.0), MOVE_ALL);

    out.hold(fire_trigger(c), &[FIRE_RIGHT, R2]);
    if c.dual_wield {
        // The off-hand's trigger fires its pistol, so the tethers retract from the stick click.
        out.hold(off.stick_click.held(), &[RETRACT_GRAPPLE, L2]);
    } else {
        out.hold(off.trigger_press.held(), &[RETRACT_GRAPPLE, L2]);
    }
    // A yank is a one-tick press, which the game reads as a tap.
    out.hold(
        (off_grapple && !chord.dominant_grip) || c.yank,
        &[FIRE_GRAPPLE, L1],
    );
    out.hold(dom.a.held() && !chord.armed, PAD_A);
    out.hold(dom.b.held(), PAD_X_ON_FOOT);
    out.hold(off.a.held(), PAD_Y_ON_FOOT);
    out.hold(off.b.held(), PAD_B);
    out.hold(dom.stick_click.held(), PAD_RB_ON_FOOT);
    // Weapons are drawn at the holsters; the stick click is the fallback. `NEXT_WEAPON` is a mouse
    // wheel action: one click per step.
    out.click(
        c.weapon_cycle && !c.dual_wield && off.stick_click.pressed,
        NEXT_WEAPON,
    );
}

/// Whether the hand-weapon fire action is held: the dominant trigger, or with a pistol in each hand
/// either trigger. The one fire action reaches the weapon, and the dual-wield split
/// ([`crate::controllers::weapons::dual_wield`]) hands it to the pistol of the pulling hand.
fn fire_trigger(c: &Controls) -> bool {
    c.dom().trigger_press.held() || (c.dual_wield && c.off().trigger_press.held())
}

/// Seated in a vehicle.
fn vehicle(out: &mut Out, c: &Controls, seat: VehicleSeat, off_grip: bool, dom_grip: bool) {
    let (dom, off) = (c.dom(), c.off());
    let stick = off.stick;
    out.hold(dom.a.held(), PAD_A);
    out.hold(off.a.held(), PAD_Y_VEHICLE);
    out.hold(off_grip, PAD_LB_VEHICLE);
    out.hold(off.stick_click.held(), PAD_L3_VEHICLE);
    match seat.class {
        VehicleClass::Helicopter => {
            out.analog(dom.trigger, HELI_INC_ALTITUDE);
            out.analog(off.trigger, HELI_DEC_ALTITUDE);
            out.axis(stick.y, HELI_BACKWARD, HELI_FORWARD);
            out.axis(stick.x, HELI_ROLL_LEFT, HELI_ROLL_RIGHT);
            out.axis(dom.stick.x, HELI_TURN_LEFT, HELI_TURN_RIGHT);
            out.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
            out.hold(dom.b.held(), &[USE_VEHICLE_MOD, BTN_B]);
        }
        VehicleClass::Plane => {
            out.analog(dom.trigger, PLANE_INC_TRUST);
            out.analog(off.trigger, PLANE_DEC_TRUST);
            // Stick forward pitches the nose down, as on the gamepad.
            out.axis(stick.y, PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            out.axis(stick.x, PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            out.axis(dom.stick.x, PLANE_TURN_LEFT, PLANE_TURN_RIGHT);
            out.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
        }
        VehicleClass::Car
        | VehicleClass::Motorcycle
        | VehicleClass::Sea
        | VehicleClass::Unknown => {
            out.analog(dom.trigger, ACCELERATE);
            out.analog(off.trigger, REVERSE);
            // The land and sea readers take the car steering, the motorcycle and jetski lean and
            // tilt, the air control of a car off the ground, and a submersible's depth, all from
            // the one stick, as on the gamepad.
            out.axis(stick.x, TURN_LEFT, TURN_RIGHT);
            out.axis(stick.x, BIKE_LEAN_LEFT, BIKE_LEAN_RIGHT);
            out.axis(stick.x, PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            out.axis(stick.y, BIKE_TILT_BACKWARD, BIKE_TILT_FORWARD);
            out.axis(stick.y, PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            if seat.class == VehicleClass::Sea {
                out.axis(stick.y, SUBMERSIBLE_SURFACE, SUBMERSIBLE_DIVE);
            }
            out.hold(off.b.held(), &[HANDBRAKE, BTN_X]);
            out.hold(dom.b.held(), &[USE_VEHICLE_MOD, BTN_B]);
            // The boat's weapon and the motorcycle and jetski personal weapon differ only by
            // action; each class reads its own and ignores the other.
            out.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, MC_FIRE, R1]);
        }
    }
    // Out of the driver's seat the triggers drive nothing, so the dominant one fires the weapon in
    // the hands or on the mount, aimed by the hands, and with a pistol in each hand either trigger
    // fires.
    if !seat.driver {
        out.hold(fire_trigger(c), &[FIRE_RIGHT, R2]);
    }
}

/// Falling, the wingsuit, and the parachute. Falling and under the parachute the hand weapons stay
/// in use, so the dominant hand keeps the on-foot trigger, reload, and grenade; the wingsuit's
/// dominant hand fires the weaponized suit's guns and boosts it.
fn airborne(
    out: &mut Out,
    c: &Controls,
    variant: AirborneVariant,
    off_grapple: bool,
    dom_grip: bool,
    evade: bool,
) {
    let (dom, off) = (c.dom(), c.off());
    // The air movement reads the move half-axes as the stick's own frame, not the camera's.
    let stick = off.stick;
    out.axis(stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    out.axis(stick.x, MOVE_LEFT, MOVE_RIGHT);
    out.analog(stick.length().min(1.0), MOVE_ALL);

    if variant == AirborneVariant::Wingsuit {
        // The air brake: `CANCEL` held (the upgrade's hold-B) and the weaponized suit's own brake.
        // `RETRACT_GRAPPLE` is left off, since it also keeps a stalling wingsuit from closing.
        out.hold(off.trigger_press.held(), &[CANCEL, WINGSUIT_AIRBRAKE]);
    } else {
        // As on foot, a pistol in each hand moves the tethers to the off-hand's stick click.
        let retract = if c.dual_wield {
            off.stick_click.held()
        } else {
            off.trigger_press.held()
        };
        out.hold(retract, &[RETRACT_GRAPPLE, WINGSUIT_AIRBRAKE, L2]);
    }
    if variant == AirborneVariant::Wingsuit {
        out.hold(dom.b.held(), &[WINGSUIT_BOOST]);
        out.hold(dom.trigger_press.held(), &[FIRE_WINGSUIT_WEAPON_MAIN, R2]);
        out.hold(dom_grip, &[FIRE_WINGSUIT_WEAPON_SECONDARY]);
    } else {
        out.hold(fire_trigger(c), &[FIRE_RIGHT, R2]);
        out.hold(dom.b.held(), PAD_X_ON_FOOT);
        out.hold(dom.stick_click.held(), PAD_RB_ON_FOOT);
    }
    if evade {
        out.hold(dom.a.held(), &[WINGSUIT_EVADE]);
    } else {
        out.hold(dom.a.held(), PAD_A);
    }
    out.hold(off_grapple || c.yank, &[FIRE_GRAPPLE, L1]);
    out.hold(off.a.held(), PAD_Y_ON_FOOT);
    out.hold(off.b.held(), PAD_B);
}

/// A game menu: the gamepad's menu layout on the physical hands.
fn ui(out: &mut Out, c: &Controls) {
    let left = c.hand(Hand::Left);
    let right = c.hand(Hand::Right);
    out.axis(left.stick.y, GUI_LSTICK_DOWN, GUI_LSTICK_UP);
    out.axis(left.stick.x, GUI_LSTICK_LEFT, GUI_LSTICK_RIGHT);
    out.axis(right.stick.y, GUI_RSTICK_DOWN, GUI_RSTICK_UP);
    out.axis(right.stick.x, GUI_RSTICK_LEFT, GUI_RSTICK_RIGHT);
    out.hold(right.a.held(), &[GUI_OK]);
    out.hold(right.b.held(), &[GUI_CANCEL]);
    out.hold(left.a.held(), &[GUI_SPECIAL_BUTTON, GUI_FILTER]);
    out.hold(left.b.held(), &[GUI_USE_BUTTON]);
    out.hold(left.grip.held(), &[GUI_TAB_PREV]);
    out.hold(right.grip.held(), &[GUI_TAB_NEXT]);
    out.hold(
        left.trigger_press.held() && c.pointer_hand != Some(Hand::Left),
        &[GUI_PAGE_PREV],
    );
    out.hold(
        right.trigger_press.held() && c.pointer_hand != Some(Hand::Right),
        &[GUI_PAGE_NEXT],
    );
    out.hold(left.stick_click.held(), &[GUI_FOCUS]);
    out.hold(right.stick_click.held(), &[GUI_RECENTER]);
}

/// The wanted inputs under construction, all on one map.
struct Out {
    map: MapKind,
    desired: Vec<Desired>,
}

impl Out {
    fn new(map: MapKind) -> Self {
        Self {
            map,
            desired: Vec::with_capacity(32),
        }
    }

    fn push(&mut self, action: Action, want: Want) {
        self.desired.push(Desired {
            target: Target {
                map: self.map,
                action,
            },
            want,
        });
    }

    /// Hold every action in `actions` while `held`.
    fn hold(&mut self, held: bool, actions: &[Action]) {
        if held {
            for &action in actions {
                self.push(action, Want::Hold(1.0));
            }
        }
    }

    /// An analog action, `0..=1`.
    fn analog(&mut self, value: f32, action: Action) {
        if value > 0.0 {
            self.push(action, Want::Analog(value));
        }
    }

    /// A signed axis split into its two non-negative half-axis actions.
    fn axis(&mut self, value: f32, negative: Action, positive: Action) {
        self.analog(-value, negative);
        self.analog(value, positive);
    }

    fn click(&mut self, clicked: bool, action: Action) {
        if clicked {
            self.push(action, Want::Click);
        }
    }
}

#[cfg(test)]
mod tests;
