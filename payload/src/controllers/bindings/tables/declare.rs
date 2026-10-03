//! The per-mode binding tables, declared once over a [`Sink`] and read both by the injection
//! ([`EvalSink`](crate::controllers::bindings::tables::sink::EvalSink)) and by the controls summary
//! ([`DescribeSink`](crate::controllers::bindings::tables::describe::DescribeSink)).
//!
//! The tables follow the plan of record ("Per-mode bindings"); the module documentation of
//! [`crate::controllers::bindings::tables`] explains the layout and its virtual gamepad buttons.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::{
        controls::Controls,
        tables::sink::{
            Axis, Gate, Input,
            Input::{A, B, Grip, GripDefault, StickClick, Trigger},
            Press,
            Role::{self, Dom, Left, Off, Right},
            Sink,
        },
    },
    frame::Hand,
    mode::{AirborneVariant, Mode, VehicleClass, VehicleSeat},
};

/// What the tables branch on besides the mode: the state that changes which inputs are bound,
/// rather than whether a bound input is held.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct TableCtx {
    /// See [`Controls::dual_wield`].
    pub dual_wield: bool,
    /// See [`Controls::weapon_cycle`].
    pub weapon_cycle: bool,
    /// See [`Controls::pointer_hand`].
    pub pointer_hand: Option<Hand>,
}

impl TableCtx {
    pub fn of(controls: &Controls) -> Self {
        Self {
            dual_wield: controls.dual_wield,
            weapon_cycle: controls.weapon_cycle,
            pointer_hand: controls.pointer_hand,
        }
    }
}

/// Declare the table for `mode` to `sink`.
pub fn declare<S: Sink>(sink: &mut S, mode: Mode, ctx: &TableCtx) {
    match mode {
        Mode::OnFoot(_) => on_foot(sink, ctx),
        Mode::Vehicle(seat) => vehicle(sink, ctx, seat),
        Mode::Airborne(variant) => airborne(sink, ctx, variant),
        Mode::Ui => ui(sink, ctx),
    }
}

/// The virtual gamepad buttons: the gameplay actions the default gamepad keymap puts on each.
/// `PUSH_GRAPPLE` is left off B: under the hammer ability it is melee, which the controllers
/// trigger with a strike, and on B it would melee on every tether release.
pub const PAD_A: &[Action] = &[JUMP, OPEN_PARACHUTE, STUNT_JUMP, BTN_A];
pub const PAD_B: &[Action] = &[RELEASE_GRAPPLE, CANCEL, BYPASS_FALL_PREVENTION, BTN_B];
pub const PAD_X_ON_FOOT: &[Action] = &[RELOAD, BTN_X];
pub const PAD_Y_ON_FOOT: &[Action] = &[USE_ITEM, ENTER_VEHICLE, OPEN_WINGSUIT, BTN_Y];
pub const PAD_Y_VEHICLE: &[Action] = &[EXIT_VEHICLE, ENTER_VEHICLE, BTN_Y];
pub const PAD_RB_ON_FOOT: &[Action] = &[THROW_GRENADE, PLANT_EXPLOSIVE, DETONATE_EXPLOSIVE, R1];
pub const PAD_LB_VEHICLE: &[Action] = &[FIRE_VEHICLE_WEAPON_SECONDARY, FIRE_GRAPPLE, MC_RELOAD, L1];
pub const PAD_L3_VEHICLE: &[Action] = &[SOUND_HORN_SIREN, TOGGLE_MAGNET_1, L3];

const fn press(role: Role, input: Input) -> Press {
    Press::new(role, input)
}

/// The grapple: the off-hand's grip, unless the sight chord's dominant grip withholds it, or the
/// yank.
const GRAPPLE: &[Press] = &[
    press(Off, GripDefault).gated(Gate::UnlessChordGrip),
    Press::YANK,
];

/// On foot and swimming.
fn on_foot<S: Sink>(s: &mut S, ctx: &TableCtx) {
    s.axis(Axis::MoveY, MOVE_BACKWARD, MOVE_FORWARD);
    s.axis(Axis::MoveX, MOVE_LEFT, MOVE_RIGHT);
    s.analog(Axis::MoveLength, MOVE_ALL);

    s.hold(fire_trigger(ctx), &[FIRE_RIGHT, R2]);
    s.hold(&[retract(ctx)], &[RETRACT_GRAPPLE, L2]);
    s.hold(GRAPPLE, &[FIRE_GRAPPLE, L1]);
    s.hold(&[press(Dom, A).gated(Gate::UnlessChordArmed)], PAD_A);
    s.hold(&[press(Dom, B)], PAD_X_ON_FOOT);
    s.hold(&[press(Off, A)], PAD_Y_ON_FOOT);
    s.hold(&[press(Off, B)], PAD_B);
    s.hold(&[press(Dom, StickClick)], PAD_RB_ON_FOOT);
    // Weapons are drawn at the holsters; the stick click is the fallback. `NEXT_WEAPON` is a mouse
    // wheel action: one click per step.
    if ctx.weapon_cycle && !ctx.dual_wield {
        s.click(press(Off, StickClick), NEXT_WEAPON);
    }
}

/// The hand-weapon fire inputs: the dominant trigger, or with a pistol in each hand either trigger.
/// The one fire action reaches the weapon, and the dual-wield split
/// ([`crate::controllers::weapons::dual_wield`]) hands it to the pistol of the pulling hand.
fn fire_trigger(ctx: &TableCtx) -> &'static [Press] {
    const ONE: &[Press] = &[press(Dom, Trigger)];
    const BOTH: &[Press] = &[press(Dom, Trigger), press(Off, Trigger)];
    if ctx.dual_wield { BOTH } else { ONE }
}

/// The tether retract: the off-hand's trigger, or its stick click while that trigger fires its
/// pistol.
fn retract(ctx: &TableCtx) -> Press {
    if ctx.dual_wield {
        press(Off, StickClick)
    } else {
        press(Off, Trigger)
    }
}

/// Seated in a vehicle.
fn vehicle<S: Sink>(s: &mut S, ctx: &TableCtx, seat: VehicleSeat) {
    s.hold(&[press(Dom, A)], PAD_A);
    s.hold(&[press(Off, A)], PAD_Y_VEHICLE);
    s.hold(&[press(Off, GripDefault)], PAD_LB_VEHICLE);
    s.hold(&[press(Off, StickClick)], PAD_L3_VEHICLE);
    let dom_grip = &[press(Dom, GripDefault)];
    match seat.class {
        VehicleClass::Helicopter => {
            s.analog(Axis::Trigger(Dom), HELI_INC_ALTITUDE);
            s.analog(Axis::Trigger(Off), HELI_DEC_ALTITUDE);
            s.axis(Axis::StickY(Off), HELI_BACKWARD, HELI_FORWARD);
            s.axis(Axis::StickX(Off), HELI_ROLL_LEFT, HELI_ROLL_RIGHT);
            s.axis(Axis::StickX(Dom), HELI_TURN_LEFT, HELI_TURN_RIGHT);
            s.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
            s.hold(&[press(Dom, B)], &[USE_VEHICLE_MOD, BTN_B]);
        }
        VehicleClass::Plane => {
            s.analog(Axis::Trigger(Dom), PLANE_INC_TRUST);
            s.analog(Axis::Trigger(Off), PLANE_DEC_TRUST);
            // Stick forward pitches the nose down, as on the gamepad.
            s.axis(Axis::StickY(Off), PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            s.axis(Axis::StickX(Off), PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            s.axis(Axis::StickX(Dom), PLANE_TURN_LEFT, PLANE_TURN_RIGHT);
            s.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
        }
        VehicleClass::Car
        | VehicleClass::Motorcycle
        | VehicleClass::Sea
        | VehicleClass::Unknown => {
            s.analog(Axis::Trigger(Dom), ACCELERATE);
            s.analog(Axis::Trigger(Off), REVERSE);
            // The land and sea readers take the car steering, the motorcycle and jetski lean and
            // tilt, the air control of a car off the ground, and a submersible's depth, all from
            // the one stick, as on the gamepad.
            s.axis(Axis::StickX(Off), TURN_LEFT, TURN_RIGHT);
            s.axis(Axis::StickX(Off), BIKE_LEAN_LEFT, BIKE_LEAN_RIGHT);
            s.axis(Axis::StickX(Off), PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            s.axis(Axis::StickY(Off), BIKE_TILT_BACKWARD, BIKE_TILT_FORWARD);
            s.axis(Axis::StickY(Off), PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            if seat.class == VehicleClass::Sea {
                s.axis(Axis::StickY(Off), SUBMERSIBLE_SURFACE, SUBMERSIBLE_DIVE);
            }
            s.hold(&[press(Off, B)], &[HANDBRAKE, BTN_X]);
            s.hold(&[press(Dom, B)], &[USE_VEHICLE_MOD, BTN_B]);
            // The boat's weapon and the motorcycle and jetski personal weapon differ only by
            // action; each class reads its own and ignores the other.
            s.hold(dom_grip, &[FIRE_VEHICLE_WEAPON_PRIMARY, MC_FIRE, R1]);
        }
    }
    // Out of the driver's seat the triggers drive nothing, so the dominant one fires the weapon in
    // the hands or on the mount, aimed by the hands, and with a pistol in each hand either trigger
    // fires.
    if !seat.driver {
        s.hold(fire_trigger(ctx), &[FIRE_RIGHT, R2]);
    }
}

/// Falling, the wingsuit, and the parachute. Falling and under the parachute the hand weapons stay
/// in use, so the dominant hand keeps the on-foot trigger, reload, and grenade; the wingsuit's
/// dominant hand fires the weaponized suit's guns and boosts it.
fn airborne<S: Sink>(s: &mut S, ctx: &TableCtx, variant: AirborneVariant) {
    // The air movement reads the move half-axes as the stick's own frame, not the camera's.
    s.axis(Axis::StickY(Off), MOVE_BACKWARD, MOVE_FORWARD);
    s.axis(Axis::StickX(Off), MOVE_LEFT, MOVE_RIGHT);
    s.analog(Axis::StickLength(Off), MOVE_ALL);

    let wingsuit = variant == AirborneVariant::Wingsuit;
    if wingsuit {
        // The air brake: `CANCEL` held (the upgrade's hold-B) and the weaponized suit's own brake.
        // `RETRACT_GRAPPLE` is left off, since it also keeps a stalling wingsuit from closing.
        s.hold(&[press(Off, Trigger)], &[CANCEL, WINGSUIT_AIRBRAKE]);
    } else {
        // As on foot, a pistol in each hand moves the tethers to the off-hand's stick click.
        s.hold(&[retract(ctx)], &[RETRACT_GRAPPLE, WINGSUIT_AIRBRAKE, L2]);
    }
    if wingsuit {
        s.hold(&[press(Dom, B)], &[WINGSUIT_BOOST]);
        s.hold(&[press(Dom, Trigger)], &[FIRE_WINGSUIT_WEAPON_MAIN, R2]);
        s.hold(
            &[press(Dom, GripDefault)],
            &[FIRE_WINGSUIT_WEAPON_SECONDARY],
        );
    } else {
        s.hold(fire_trigger(ctx), &[FIRE_RIGHT, R2]);
        s.hold(&[press(Dom, B)], PAD_X_ON_FOOT);
        s.hold(&[press(Dom, StickClick)], PAD_RB_ON_FOOT);
    }
    if wingsuit {
        s.hold(&[press(Dom, A).gated(Gate::EvadeFlick)], &[WINGSUIT_EVADE]);
        s.hold(&[press(Dom, A).gated(Gate::NoEvadeFlick)], PAD_A);
    } else {
        s.hold(&[press(Dom, A)], PAD_A);
    }
    s.hold(&[press(Off, GripDefault), Press::YANK], &[FIRE_GRAPPLE, L1]);
    s.hold(&[press(Off, A)], PAD_Y_ON_FOOT);
    s.hold(&[press(Off, B)], PAD_B);
}

/// A game menu: the gamepad's menu layout on the physical hands.
fn ui<S: Sink>(s: &mut S, ctx: &TableCtx) {
    s.axis(Axis::StickY(Left), GUI_LSTICK_DOWN, GUI_LSTICK_UP);
    s.axis(Axis::StickX(Left), GUI_LSTICK_LEFT, GUI_LSTICK_RIGHT);
    s.axis(Axis::StickY(Right), GUI_RSTICK_DOWN, GUI_RSTICK_UP);
    s.axis(Axis::StickX(Right), GUI_RSTICK_LEFT, GUI_RSTICK_RIGHT);
    s.hold(&[press(Right, A)], &[GUI_OK]);
    s.hold(&[press(Right, B)], &[GUI_CANCEL]);
    s.hold(&[press(Left, A)], &[GUI_SPECIAL_BUTTON, GUI_FILTER]);
    s.hold(&[press(Left, B)], &[GUI_USE_BUTTON]);
    s.hold(&[press(Left, Grip)], &[GUI_TAB_PREV]);
    s.hold(&[press(Right, Grip)], &[GUI_TAB_NEXT]);
    // The pointer's hand keeps its trigger for the native UI pointer.
    if ctx.pointer_hand != Some(Hand::Left) {
        s.hold(&[press(Left, Trigger)], &[GUI_PAGE_PREV]);
    }
    if ctx.pointer_hand != Some(Hand::Right) {
        s.hold(&[press(Right, Trigger)], &[GUI_PAGE_NEXT]);
    }
    s.hold(&[press(Left, StickClick)], &[GUI_FOCUS]);
    s.hold(&[press(Right, StickClick)], &[GUI_RECENTER]);
}
