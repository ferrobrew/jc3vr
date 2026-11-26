//! The raw-input tables' declarations, frozen.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::{
        controls::Controls,
        tables::tests::legacy::sink::{
            Axis, Direction, Gate, Input,
            Input::{A, B, GripDefault, Stick, StickClick, TrackpadLeft, TrackpadRight, Trigger},
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
    /// See [`Controls::trackpad_paging`].
    pub trackpad_paging: bool,
}

impl TableCtx {
    pub fn of(controls: &Controls) -> Self {
        Self {
            dual_wield: controls.dual_wield,
            weapon_cycle: controls.weapon_cycle,
            pointer_hand: controls.pointer_hand,
            trackpad_paging: controls.trackpad_paging,
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
/// The dominant stick click's actions on foot and in the air: the detonation of the placed charges
/// alone, which the game starts once the action has been held past its hold time. The gamepad's
/// RB also throws a grenade and plants C4; the controllers take a grenade from its pouch and a
/// charge from its own pouch instead, so neither is on a button.
pub const STICK_CLICK_DETONATE: &[Action] = &[DETONATE_EXPLOSIVE];
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
    s.hold(&[press(Dom, StickClick)], STICK_CLICK_DETONATE);
    // Weapons are drawn at the holsters; the stick click is the fallback. `NEXT_WEAPON` is a mouse
    // wheel action: one click per step.
    if ctx.weapon_cycle && !ctx.dual_wield {
        s.click(press(Off, StickClick), NEXT_WEAPON);
    }
}

/// The hand-weapon fire inputs: the dominant trigger, and with a pistol in the off-hand its
/// trigger too. The one fire action reaches the weapon, and the dual-wield split
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
        | VehicleClass::Mech
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
    // the hands or on the mount, aimed by the hands, and with a pistol in the off-hand its trigger
    // fires too.
    if !seat.driver {
        s.hold(fire_trigger(ctx), &[FIRE_RIGHT, R2]);
    }
}

/// Falling, the wingsuit, and the parachute. Falling and under the parachute the hand weapons stay
/// in use, so the dominant hand keeps the on-foot trigger, reload, and detonation; the wingsuit's
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
        // As on foot, a pistol in the off-hand moves the tethers to the off-hand's stick click.
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
        s.hold(&[press(Dom, StickClick)], STICK_CLICK_DETONATE);
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

/// A game menu. The two hands navigate alike, whichever is dominant: either A confirms, either B
/// goes back, and either stick steps through the menu. A hand's grip is its modifier, latched at
/// each press: with it held, the stick's left and right step the tabs instead of adjusting the
/// selected row, and A and B are the gamepad's X and Y. The game repeats a held step itself, so the
/// stick is read as one latched direction at a time ([`Input::Stick`]).
fn ui<S: Sink>(s: &mut S, ctx: &TableCtx) {
    let either = |input: Input, gate: Gate| {
        [
            press(Left, input).gated(gate),
            press(Right, input).gated(gate),
        ]
    };
    s.hold(&either(A, Gate::Ungripped), &[GUI_OK]);
    s.hold(&either(B, Gate::Ungripped), &[GUI_CANCEL]);
    s.hold(&either(A, Gate::Gripped), &[GUI_SPECIAL_BUTTON, GUI_FILTER]);
    s.hold(&either(B, Gate::Gripped), &[GUI_USE_BUTTON]);
    // The game's base menu input reads only the left stick's actions, so both sticks drive them.
    s.hold(
        &either(Stick(Direction::Up), Gate::Always),
        &[GUI_LSTICK_UP],
    );
    s.hold(
        &either(Stick(Direction::Down), Gate::Always),
        &[GUI_LSTICK_DOWN],
    );
    s.hold(
        &either(Stick(Direction::Left), Gate::Ungripped),
        &[GUI_LSTICK_LEFT],
    );
    s.hold(
        &either(Stick(Direction::Right), Gate::Ungripped),
        &[GUI_LSTICK_RIGHT],
    );
    s.hold(
        &either(Stick(Direction::Left), Gate::Gripped),
        &[GUI_TAB_PREV],
    );
    s.hold(
        &either(Stick(Direction::Right), Gate::Gripped),
        &[GUI_TAB_NEXT],
    );
    // Where a controller has a trackpad, a press on either trackpad's left or right half also steps
    // the tabs. A touch alone does not, so the off-hand's touch stays free to show the controls
    // summary.
    if ctx.trackpad_paging {
        s.hold(&either(TrackpadLeft, Gate::Always), &[GUI_TAB_PREV]);
        s.hold(&either(TrackpadRight, Gate::Always), &[GUI_TAB_NEXT]);
    }
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
