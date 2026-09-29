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
}

/// The tables and their cross-tick state.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    system: SystemButton,
    /// What each hand's current grip press was latched to at its press edge, by [`Hand::index`].
    grips: [Option<GripUse>; 2],
    /// Whether the current wingsuit A press was latched as an evade.
    evade_press: Option<bool>,
    /// Whether the crouch flick has fired and waits for the stick to come back.
    crouch_flicked: bool,
}

impl Bindings {
    pub const fn new() -> Self {
        Self {
            system: SystemButton { held: None },
            grips: [None; 2],
            evade_press: None,
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
        for hand in Hand::BOTH {
            latch_grip(
                &mut self.grips[hand.index()],
                controls.hand(hand).grip,
                controls.hand(hand).grab_volume,
            );
        }
        let off_grapple = self.grips[controls.dominant.other().index()] == Some(GripUse::Default);
        let dom_grip = self.grips[controls.dominant.index()] == Some(GripUse::Default);

        let mut out = Out::new(MapKind::Player);
        let body_turn = match mode {
            Mode::OnFoot(_) => {
                on_foot(&mut out, controls, off_grapple);
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
                let evade = latch_evade(&mut self.evade_press, variant, controls, config);
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
        }
    }
}

/// What a grip press does, decided at its press edge and kept for the whole press, so that a hand
/// leaving a volume mid-press does not turn a grab into a grapple.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum GripUse {
    /// The grip's default action (the off-hand's grapple, a vehicle weapon).
    Default,
    /// A grab of the volume the hand was in. The grabbing subsystems consume it; the tables inject
    /// nothing for it.
    #[allow(
        dead_code,
        reason = "read by the grabbing subsystems once grab volumes exist"
    )]
    Grab(GrabVolume),
}

fn latch_grip(latch: &mut Option<GripUse>, grip: ButtonControl, volume: Option<GrabVolume>) {
    if !grip.held() {
        *latch = None;
    } else if latch.is_none() || (grip.pressed && grip.released && !grip.down) {
        *latch = Some(volume.map_or(GripUse::Default, GripUse::Grab));
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

/// Decide at the wingsuit A's press edge whether the press evades (the move stick is flicked) or
/// acts as the virtual A.
fn latch_evade(
    latch: &mut Option<bool>,
    variant: AirborneVariant,
    controls: &Controls,
    config: &BindingsConfig,
) -> bool {
    let a = controls.dom().a;
    if variant != AirborneVariant::Wingsuit || !a.held() {
        *latch = None;
        return false;
    }
    *latch.get_or_insert_with(|| controls.off().stick.length() >= config.evade_flick)
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
fn on_foot(out: &mut Out, c: &Controls, off_grapple: bool) {
    let (dom, off) = (c.dom(), c.off());
    let stick = rotate_stick(off.stick, c.move_rotation);
    out.axis(stick.y, MOVE_BACKWARD, MOVE_FORWARD);
    out.axis(stick.x, MOVE_LEFT, MOVE_RIGHT);
    out.analog(stick.length().min(1.0), MOVE_ALL);

    out.hold(dom.trigger_press.held(), &[FIRE_RIGHT, R2]);
    out.hold(off.trigger_press.held(), &[RETRACT_GRAPPLE, L2]);
    out.hold(off_grapple, &[FIRE_GRAPPLE, L1]);
    out.hold(dom.a.held(), PAD_A);
    out.hold(dom.b.held(), PAD_X_ON_FOOT);
    out.hold(off.a.held(), PAD_Y_ON_FOOT);
    out.hold(off.b.held(), PAD_B);
    out.hold(dom.stick_click.held(), PAD_RB_ON_FOOT);
    // Until holsters exist, the move stick's click cycles the weapons. `NEXT_WEAPON` is a mouse
    // wheel action: one click per step.
    out.click(off.stick_click.pressed, NEXT_WEAPON);
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
}

/// Falling, the wingsuit, and the parachute.
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
        out.hold(
            off.trigger_press.held(),
            &[RETRACT_GRAPPLE, WINGSUIT_AIRBRAKE, L2],
        );
    }
    out.hold(dom.b.held(), &[WINGSUIT_BOOST]);
    if evade {
        out.hold(dom.a.held(), &[WINGSUIT_EVADE]);
    } else {
        out.hold(dom.a.held(), PAD_A);
    }
    out.hold(dom.trigger_press.held(), &[FIRE_WINGSUIT_WEAPON_MAIN, R2]);
    out.hold(dom_grip, &[FIRE_WINGSUIT_WEAPON_SECONDARY]);
    out.hold(off_grapple, &[FIRE_GRAPPLE, L1]);
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
    out.hold(left.trigger_press.held(), &[GUI_PAGE_PREV]);
    out.hold(right.trigger_press.held(), &[GUI_PAGE_NEXT]);
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
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::controllers::{bindings::controls::HandControls, mode::OnFootVariant};

    const DT: f32 = 1.0 / 30.0;
    const CONFIG: BindingsConfig = BindingsConfig::new();
    const ON_FOOT: Mode = Mode::OnFoot(OnFootVariant::Ground);

    fn down() -> ButtonControl {
        ButtonControl {
            down: true,
            pressed: true,
            released: false,
        }
    }

    fn controls(
        dominant: Hand,
        set: impl FnOnce(&mut HandControls, &mut HandControls),
    ) -> Controls {
        let mut c = Controls {
            dominant,
            ..Default::default()
        };
        let [left, right] = &mut c.hands;
        let (dom, off) = match dominant {
            Hand::Right => (right, left),
            Hand::Left => (left, right),
        };
        set(dom, off);
        c
    }

    fn wants(step: &Step, map: MapKind, action: Action) -> Option<Want> {
        step.desired
            .iter()
            .find(|d| d.target == Target { map, action })
            .map(|d| d.want)
    }

    fn player(step: &Step, action: Action) -> Option<Want> {
        wants(step, MapKind::Player, action)
    }

    #[test]
    fn nothing_pressed_wants_nothing() {
        let mut b = Bindings::default();
        let step = b.step(ON_FOOT, &Controls::default(), DT, &CONFIG);
        assert!(step.desired.is_empty());
        assert_eq!(step.body_turn, 0.0);
    }

    #[test]
    fn on_foot_face_buttons_follow_the_plan() {
        let mut b = Bindings::default();
        let c = controls(Hand::Right, |dom, off| {
            dom.a = down();
            off.b = down();
        });
        let step = b.step(ON_FOOT, &c, DT, &CONFIG);
        for action in [JUMP, OPEN_PARACHUTE, STUNT_JUMP, RELEASE_GRAPPLE, CANCEL] {
            assert_eq!(player(&step, action), Some(Want::Hold(1.0)), "{action:?}");
        }
        assert_eq!(player(&step, PUSH_GRAPPLE), None);
        assert_eq!(player(&step, RELOAD), None);
    }

    #[test]
    fn the_dominant_hand_mirrors_the_layout() {
        let mut b = Bindings::default();
        let right_handed = controls(Hand::Right, |dom, off| {
            dom.trigger_press = down();
            off.grip = down();
        });
        let left_handed = controls(Hand::Left, |dom, off| {
            dom.trigger_press = down();
            off.grip = down();
        });
        // The same logical presses land on opposite physical hands, and fire the same actions.
        assert!(right_handed.hands[1].trigger_press.down);
        assert!(left_handed.hands[0].trigger_press.down);
        for c in [right_handed, left_handed] {
            let step = b.step(ON_FOOT, &c, DT, &CONFIG);
            assert_eq!(player(&step, FIRE_RIGHT), Some(Want::Hold(1.0)));
            assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
            b = Bindings::default();
        }
    }

    #[test]
    fn the_move_stick_splits_into_half_axes_and_rotates() {
        let mut b = Bindings::default();
        let mut c = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, 0.5));
        let step = b.step(ON_FOOT, &c, DT, &CONFIG);
        assert_eq!(player(&step, MOVE_FORWARD), Some(Want::Analog(0.5)));
        assert_eq!(player(&step, MOVE_BACKWARD), None);
        assert_eq!(player(&step, MOVE_ALL), Some(Want::Analog(0.5)));

        c.move_rotation = std::f32::consts::FRAC_PI_2;
        let step = b.step(ON_FOOT, &c, DT, &CONFIG);
        let Some(Want::Analog(left)) = player(&step, MOVE_LEFT) else {
            panic!("no left move");
        };
        assert!((left - 0.5).abs() < 1e-5);
        assert!(matches!(
            player(&step, MOVE_FORWARD),
            None | Some(Want::Analog(_))
        ));
    }

    #[test]
    fn the_right_stick_turns_the_body_on_foot_but_yaws_a_helicopter() {
        let mut b = Bindings::default();
        let c = controls(Hand::Right, |dom, _| {
            dom.stick_raw = Vec2::new(0.8, 0.0);
            dom.stick = Vec2::new(0.76, 0.0);
        });
        assert_eq!(b.step(ON_FOOT, &c, DT, &CONFIG).body_turn, 0.8);
        let heli = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Helicopter,
            driver: true,
        });
        let step = b.step(heli, &c, DT, &CONFIG);
        assert_eq!(step.body_turn, 0.0);
        assert_eq!(player(&step, HELI_TURN_RIGHT), Some(Want::Analog(0.76)));
    }

    #[test]
    fn a_grip_press_in_a_grab_volume_does_not_grapple() {
        let mut b = Bindings::default();
        let grab = controls(Hand::Right, |_, off| {
            off.grip = down();
            off.grab_volume = Some(GrabVolume::Holster);
        });
        let step = b.step(ON_FOOT, &grab, DT, &CONFIG);
        assert_eq!(player(&step, FIRE_GRAPPLE), None);
        // Leaving the volume mid-press keeps the press a grab.
        let left_volume = controls(Hand::Right, |_, off| {
            off.grip = ButtonControl {
                down: true,
                ..Default::default()
            };
        });
        let step = b.step(ON_FOOT, &left_volume, DT, &CONFIG);
        assert_eq!(player(&step, FIRE_GRAPPLE), None);
        // A fresh press outside the volume grapples.
        b.step(ON_FOOT, &Controls::default(), DT, &CONFIG);
        let step = b.step(ON_FOOT, &left_volume, DT, &CONFIG);
        assert_eq!(player(&step, FIRE_GRAPPLE), Some(Want::Hold(1.0)));
    }

    #[test]
    fn the_car_uses_triggers_as_throttle() {
        let mut b = Bindings::default();
        let car = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Car,
            driver: true,
        });
        let c = controls(Hand::Right, |dom, off| {
            dom.trigger = 0.6;
            off.stick = Vec2::new(-0.4, 0.0);
            off.b = down();
            dom.b = down();
        });
        let step = b.step(car, &c, DT, &CONFIG);
        assert_eq!(player(&step, ACCELERATE), Some(Want::Analog(0.6)));
        assert_eq!(player(&step, TURN_LEFT), Some(Want::Analog(0.4)));
        assert_eq!(player(&step, TURN_RIGHT), None);
        assert_eq!(player(&step, HANDBRAKE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, USE_VEHICLE_MOD), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, SUBMERSIBLE_DIVE), None);
    }

    #[test]
    fn the_plane_pitches_down_on_stick_forward() {
        let mut b = Bindings::default();
        let plane = Mode::Vehicle(VehicleSeat {
            class: VehicleClass::Plane,
            driver: true,
        });
        let c = controls(Hand::Right, |_, off| off.stick = Vec2::new(0.0, 1.0));
        let step = b.step(plane, &c, DT, &CONFIG);
        assert_eq!(player(&step, PLANE_PITCH_DOWN), Some(Want::Analog(1.0)));
        assert_eq!(player(&step, PLANE_PITCH_UP), None);
    }

    #[test]
    fn the_wingsuit_evades_on_a_flicked_a() {
        let mut b = Bindings::default();
        let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
        let flick = controls(Hand::Right, |dom, off| {
            dom.a = down();
            off.stick = Vec2::new(0.9, 0.0);
        });
        let step = b.step(wingsuit, &flick, DT, &CONFIG);
        assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
        assert_eq!(player(&step, OPEN_PARACHUTE), None);
        // The latch holds for the press even once the stick recentres.
        let held = controls(Hand::Right, |dom, _| dom.a = down());
        let step = b.step(wingsuit, &held, DT, &CONFIG);
        assert_eq!(player(&step, WINGSUIT_EVADE), Some(Want::Hold(1.0)));
        // A fresh press without the flick opens the parachute.
        b.step(wingsuit, &Controls::default(), DT, &CONFIG);
        let step = b.step(wingsuit, &held, DT, &CONFIG);
        assert_eq!(player(&step, OPEN_PARACHUTE), Some(Want::Hold(1.0)));
    }

    #[test]
    fn menus_use_the_ui_map_on_the_physical_hands() {
        let mut b = Bindings::default();
        let c = controls(Hand::Left, |dom, off| {
            // Left-handed: the dominant hand is the left one.
            dom.grip = down();
            off.a = down();
        });
        let step = b.step(Mode::Ui, &c, DT, &CONFIG);
        assert_eq!(
            wants(&step, MapKind::Ui, GUI_TAB_PREV),
            Some(Want::Hold(1.0))
        );
        assert_eq!(wants(&step, MapKind::Ui, GUI_OK), Some(Want::Hold(1.0)));
        assert!(step.desired.iter().all(|d| d.target.map == MapKind::Ui));
    }

    fn system(down: bool, pressed: bool, released: bool) -> Controls {
        let mut c = Controls::default();
        c.hands[0].system = ButtonControl {
            down,
            pressed,
            released,
        };
        c
    }

    fn clicked(step: &Step, action: Action) -> bool {
        wants(step, MapKind::PlayerThroughBlockers, action) == Some(Want::Click)
            && wants(step, MapKind::Ui, action) == Some(Want::Click)
    }

    #[test]
    fn a_short_system_press_pauses_on_release() {
        let mut b = Bindings::default();
        let step = b.step(ON_FOOT, &system(true, true, false), DT, &CONFIG);
        assert!(step.desired.is_empty());
        let step = b.step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
        assert!(clicked(&step, GUI_PAUSE));
        assert!(!clicked(&step, GUI_PDA));
    }

    #[test]
    fn a_tap_between_ticks_still_pauses() {
        let mut b = Bindings::default();
        b.step(ON_FOOT, &system(false, true, true), DT, &CONFIG);
        let step = b.step(ON_FOOT, &Controls::default(), DT, &CONFIG);
        assert!(clicked(&step, GUI_PAUSE));
    }

    #[test]
    fn a_long_system_press_opens_the_commlink_once() {
        let mut b = Bindings::default();
        let held = system(true, false, false);
        let ticks = (CONFIG.long_press_s / DT).ceil() as usize + 1;
        let mut commlinks = 0;
        for _ in 0..ticks * 2 {
            let step = b.step(ON_FOOT, &held, DT, &CONFIG);
            commlinks += usize::from(clicked(&step, GUI_PDA));
        }
        assert_eq!(commlinks, 1);
        let step = b.step(ON_FOOT, &system(false, false, true), DT, &CONFIG);
        assert!(!clicked(&step, GUI_PAUSE));
    }

    #[test]
    fn the_system_button_works_in_menus() {
        let mut b = Bindings::default();
        b.step(Mode::Ui, &system(true, true, false), DT, &CONFIG);
        let step = b.step(Mode::Ui, &Controls::default(), DT, &CONFIG);
        assert!(clicked(&step, GUI_PAUSE));
    }

    #[test]
    fn a_crouch_flick_fires_once_until_the_stick_returns() {
        let mut fired = false;
        assert!(!flick_down(&mut fired, -0.5, 0.8));
        assert!(flick_down(&mut fired, -0.9, 0.8));
        assert!(!flick_down(&mut fired, -1.0, 0.8));
        assert!(!flick_down(&mut fired, -0.3, 0.8));
        assert!(flick_down(&mut fired, -0.85, 0.8));
        assert!(!flick_down(&mut fired, f32::NAN, 0.8));
    }
}
