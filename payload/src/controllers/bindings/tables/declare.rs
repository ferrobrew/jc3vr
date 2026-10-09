//! The per-mode binding tables, declared once over a [`Sink`] and read both by the injection
//! ([`EvalSink`](crate::controllers::bindings::tables::sink::EvalSink)) and by the controls summary
//! ([`DescribeSink`](crate::controllers::bindings::tables::describe::DescribeSink)).
//!
//! Each table names the semantic actions of its mode's action set
//! ([`crate::vr::input::semantic`]); which physical input drives each is the player's binding,
//! suggested by [`crate::vr::input::semantic::defaults`]. The module documentation of
//! [`crate::controllers::bindings::tables`] explains the virtual gamepad buttons.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::{
    controllers::{
        bindings::{
            controls::Controls,
            tables::sink::{Axis, Direction, Gate, Input, PadZone, Press, Sink},
        },
        frame::Hand,
        mode::{AirborneVariant, Mode, VehicleClass, VehicleSeat},
    },
    vr::input::semantic::vocabulary::{
        ActionId, HelicopterAction as H, LandVehicleAction as L, MenuAction as M,
        OnFootAction as F, PlaneAction as P, WingsuitAction as W,
    },
};

/// What the tables branch on besides the mode: the state that changes which actions are read,
/// rather than whether a read action is held.
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
        Mode::Airborne(AirborneVariant::Wingsuit) => wingsuit(sink),
        Mode::Airborne(_) => falling(sink, ctx),
        Mode::Ui => ui(sink, ctx),
    }
}

/// The virtual gamepad buttons: the gameplay actions the default gamepad keymap puts on each.
/// `PUSH_GRAPPLE` is left off B: under the hammer ability it is melee, and on B it would melee on
/// every tether release.
pub const PAD_A: &[Action] = &[JUMP, OPEN_PARACHUTE, STUNT_JUMP, BTN_A];
pub const PAD_B: &[Action] = &[RELEASE_GRAPPLE, CANCEL, BYPASS_FALL_PREVENTION, BTN_B];
pub const PAD_X_ON_FOOT: &[Action] = &[RELOAD, BTN_X];
pub const PAD_Y_ON_FOOT: &[Action] = &[USE_ITEM, ENTER_VEHICLE, OPEN_WINGSUIT, BTN_Y];
/// What the gamepad's B does held, beyond the tethers' release, which the grapple button's double
/// tap took: stepping off a ledge the character would stop at. It rides the use button held, since
/// using something never walks the character off a ledge.
pub const BYPASS: &[Action] = &[BYPASS_FALL_PREVENTION];
/// The gamepad's B held while a ragdoll, which keeps the character one. Only then: `CANCEL` cancels
/// other things elsewhere, so it is not on the use button otherwise.
pub const KEEP_RAGDOLLING: &[Action] = &[CANCEL];
pub const PAD_Y_VEHICLE: &[Action] = &[EXIT_VEHICLE, ENTER_VEHICLE, BTN_Y];
/// The detonation of the placed charges alone, which the game starts once the action has been
/// held past its hold time. The gamepad's RB also throws a grenade and plants C4; the controllers
/// take a grenade from its pouch and a charge from its own pouch instead, so neither rides along.
pub const DETONATE: &[Action] = &[DETONATE_EXPLOSIVE];
/// The gamepad's LB in a vehicle, without the grapple it also fires there: the grapple has a button
/// of its own ([`GRAPPLE_VEHICLE`]).
pub const PAD_LB_VEHICLE: &[Action] = &[FIRE_VEHICLE_WEAPON_SECONDARY, MC_RELOAD, L1];
/// The grapple in a vehicle.
pub const GRAPPLE_VEHICLE: &[Action] = &[FIRE_GRAPPLE];
/// The grapple on foot, falling, and in the wingsuit: the gamepad's LB.
pub const GRAPPLE: &[Action] = &[FIRE_GRAPPLE, L1];
pub const PAD_L3_VEHICLE: &[Action] = &[SOUND_HORN_SIREN, TOGGLE_MAGNET_1, L3];
/// The weapon slots on the gamepad's d-pad, by direction: the selection cross the game shows
/// while one is held lays them out the same way, and the release commits.
pub const WEAPON_SELECT: [(PadZone, Action); 4] = [
    (PadZone::Up, SELECT_DUEL_WIELD),
    (PadZone::Right, SELECT_TWO_HANDED),
    (PadZone::Left, SELECT_TWO_HANDED_SPECIAL),
    (PadZone::Down, SELECT_EXPLOSIVES),
];

/// A press of `action`.
fn p(action: impl Into<ActionId>) -> Press {
    Press::new(Input::Action(action.into()))
}

/// A press of the pad press `action` latched to `zone`.
fn zone(action: impl Into<ActionId>, zone: PadZone) -> Press {
    Press::new(Input::Zone(action.into(), zone))
}

/// On foot and swimming.
fn on_foot<S: Sink>(s: &mut S, ctx: &TableCtx) {
    let mv = F::Move.id();
    s.axis(Axis::MoveY(mv), MOVE_BACKWARD, MOVE_FORWARD);
    s.axis(Axis::MoveX(mv), MOVE_LEFT, MOVE_RIGHT);
    s.analog(Axis::MoveLength(mv), MOVE_ALL);

    s.hold(&fire(ctx), &[FIRE_RIGHT, R2]);
    s.hold(&[retract(ctx)], &[RETRACT_GRAPPLE, L2]);
    s.hold(&grapple(), GRAPPLE);
    s.hold(&[p(F::Jump).gated(Gate::UnlessChordArmed)], PAD_A);
    s.hold(&[p(F::Reload)], PAD_X_ON_FOOT);
    s.hold(&[p(F::Use)], PAD_Y_ON_FOOT);
    s.hold(&[p(F::Use)], BYPASS);
    s.hold(&[p(F::Use).gated(Gate::Ragdolling)], KEEP_RAGDOLLING);
    s.hold(&[p(F::ReleaseTethers)], PAD_B);
    s.hold(&[p(F::Detonate)], DETONATE);
    hand_weapon_fallbacks(s);
    // Weapons are drawn at the holsters; the weapon cycle is the fallback. `NEXT_WEAPON` is a
    // mouse wheel action: one click per step.
    if ctx.weapon_cycle && !ctx.dual_wield {
        s.click(p(F::NextWeapon), NEXT_WEAPON);
    }
}

/// The grapple, on its button or an analog input a player binds it to, unless the sight chord's
/// dominant grip withholds it. The yank gesture presses the analog action too
/// ([`crate::controllers::bindings::intents::Gestures`]).
fn grapple() -> [Press; 2] {
    [
        p(F::Grapple).gated(Gate::UnlessChordGrip),
        p(F::FireGrapple).gated(Gate::UnlessChordGrip),
    ]
}

/// The hand-weapon fire actions: the dominant trigger's, and with a pistol in the off-hand its
/// own. The one fire action reaches the weapon, and the dual-wield split
/// ([`crate::controllers::weapons::dual_wield`]) hands it to the pistol of the pulling hand.
fn fire(ctx: &TableCtx) -> Vec<Press> {
    if ctx.dual_wield {
        vec![p(F::Fire), p(F::FireOffHandPistol)]
    } else {
        vec![p(F::Fire)]
    }
}

/// The tether retract: its own action, or while the off-hand's trigger fires its pistol the
/// alternate one.
fn retract(ctx: &TableCtx) -> Press {
    if ctx.dual_wield {
        p(F::RetractTethersAlt)
    } else {
        p(F::RetractTethers)
    }
}

/// The hand-weapon actions with no default binding, kept for a player who binds them: the grenade
/// throw, the fallback for the throw gesture, and the weapon slot select on a pad.
fn hand_weapon_fallbacks<S: Sink>(s: &mut S) {
    s.hold(&[p(F::ThrowGrenade)], &[THROW_GRENADE]);
    for (z, action) in WEAPON_SELECT {
        s.hold(&[zone(F::WeaponSelectPress, z)], &[action]);
    }
}

/// The actions every vehicle set shares, in one class's set.
struct VehicleActions {
    stunt: ActionId,
    exit: ActionId,
    fire_primary: ActionId,
    fire_secondary: ActionId,
    horn: ActionId,
    fire: ActionId,
    fire_off_hand_pistol: ActionId,
    grapple: ActionId,
}

impl VehicleActions {
    fn of(class: VehicleClass) -> Self {
        match class {
            VehicleClass::Helicopter => Self {
                stunt: H::Stunt.id(),
                exit: H::ExitVehicle.id(),
                fire_primary: H::FirePrimary.id(),
                fire_secondary: H::FireSecondary.id(),
                horn: H::Horn.id(),
                fire: H::Fire.id(),
                fire_off_hand_pistol: H::FireOffHandPistol.id(),
                grapple: H::Grapple.id(),
            },
            VehicleClass::Plane => Self {
                stunt: P::Stunt.id(),
                exit: P::ExitVehicle.id(),
                fire_primary: P::FirePrimary.id(),
                fire_secondary: P::FireSecondary.id(),
                horn: P::Horn.id(),
                fire: P::Fire.id(),
                fire_off_hand_pistol: P::FireOffHandPistol.id(),
                grapple: P::Grapple.id(),
            },
            VehicleClass::Car
            | VehicleClass::Motorcycle
            | VehicleClass::Sea
            | VehicleClass::Mech
            | VehicleClass::Unknown => Self {
                stunt: L::Stunt.id(),
                exit: L::ExitVehicle.id(),
                fire_primary: L::FirePrimary.id(),
                fire_secondary: L::FireSecondary.id(),
                horn: L::Horn.id(),
                fire: L::Fire.id(),
                fire_off_hand_pistol: L::FireOffHandPistol.id(),
                grapple: L::Grapple.id(),
            },
        }
    }
}

/// Seated in a vehicle. The throttle actions are read in the driver's seat only: out of it they
/// drive nothing, and the triggers fire the weapon in the hands or on the mount instead.
fn vehicle<S: Sink>(s: &mut S, ctx: &TableCtx, seat: VehicleSeat) {
    let v = VehicleActions::of(seat.class);
    s.hold(&[p(v.stunt)], PAD_A);
    s.hold(&[p(v.exit)], PAD_Y_VEHICLE);
    s.hold(&[p(v.fire_secondary)], PAD_LB_VEHICLE);
    s.hold(&[p(v.grapple)], GRAPPLE_VEHICLE);
    s.hold(&[p(v.horn)], PAD_L3_VEHICLE);
    match seat.class {
        VehicleClass::Helicopter => {
            if seat.driver {
                s.analog(Axis::Value(H::CollectiveUp.id()), HELI_INC_ALTITUDE);
                s.analog(Axis::Value(H::CollectiveDown.id()), HELI_DEC_ALTITUDE);
            }
            let cyclic = H::Cyclic.id();
            s.axis(Axis::Y(cyclic), HELI_BACKWARD, HELI_FORWARD);
            s.axis(Axis::X(cyclic), HELI_ROLL_LEFT, HELI_ROLL_RIGHT);
            s.axis(Axis::Value(H::Yaw.id()), HELI_TURN_LEFT, HELI_TURN_RIGHT);
            s.hold(&[p(v.fire_primary)], &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
            s.hold(&[p(H::VehicleMod)], &[USE_VEHICLE_MOD, BTN_B]);
        }
        VehicleClass::Plane => {
            if seat.driver {
                s.analog(Axis::Value(P::ThrustUp.id()), PLANE_INC_TRUST);
                s.analog(Axis::Value(P::ThrustDown.id()), PLANE_DEC_TRUST);
            }
            // Stick forward pitches the nose down, as on the gamepad.
            let pitch_roll = P::PitchRoll.id();
            s.axis(Axis::Y(pitch_roll), PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            s.axis(Axis::X(pitch_roll), PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            s.axis(
                Axis::Value(P::Rudder.id()),
                PLANE_TURN_LEFT,
                PLANE_TURN_RIGHT,
            );
            s.hold(&[p(v.fire_primary)], &[FIRE_VEHICLE_WEAPON_PRIMARY, R1]);
        }
        VehicleClass::Mech => mech(s, seat),
        VehicleClass::Car
        | VehicleClass::Motorcycle
        | VehicleClass::Sea
        | VehicleClass::Unknown => {
            if seat.driver {
                s.analog(Axis::Value(L::Accelerate.id()), ACCELERATE);
                s.analog(Axis::Value(L::Reverse.id()), REVERSE);
            }
            // The land and sea readers take the car steering, the motorcycle and jetski lean and
            // tilt, the air control of a car off the ground, and a submersible's depth, all from
            // the one stick, as on the gamepad.
            let steer = L::Steer.id();
            s.axis(Axis::X(steer), TURN_LEFT, TURN_RIGHT);
            s.axis(Axis::X(steer), BIKE_LEAN_LEFT, BIKE_LEAN_RIGHT);
            s.axis(Axis::X(steer), PLANE_ROLL_LEFT, PLANE_ROLL_RIGHT);
            s.axis(Axis::Y(steer), BIKE_TILT_BACKWARD, BIKE_TILT_FORWARD);
            s.axis(Axis::Y(steer), PLANE_PITCH_UP, PLANE_PITCH_DOWN);
            if seat.class == VehicleClass::Sea {
                s.axis(Axis::Y(steer), SUBMERSIBLE_SURFACE, SUBMERSIBLE_DIVE);
            }
            s.hold(&[p(L::Handbrake)], &[HANDBRAKE, BTN_X]);
            s.hold(&[p(L::VehicleMod)], &[USE_VEHICLE_MOD, BTN_B]);
            // The boat's weapon and the motorcycle and jetski personal weapon differ only by
            // action; each class reads its own and ignores the other.
            s.hold(
                &[p(v.fire_primary)],
                &[FIRE_VEHICLE_WEAPON_PRIMARY, MC_FIRE, R1],
            );
        }
    }
    // Out of the driver's seat the fire actions fire the weapon in the hands or on the mount,
    // aimed by the hands, and with a pistol in the off-hand its own fire action fires too.
    if !seat.driver {
        let mut presses = vec![p(v.fire)];
        if ctx.dual_wield {
            presses.push(p(v.fire_off_hand_pistol));
        }
        s.hold(&presses, &[FIRE_RIGHT, R2]);
    }
}

/// The mech, on the land vehicle set's actions at the gamepad's places: the stick walks it (the
/// mech's move reads the plane's pitch and the car's turn), the triggers fire its right-hand weapon
/// and work its gravity gun (held to attract, let go to throw), the dominant grip drops what the
/// gravity gun holds, the off-hand grip punches, and the dominant A jumps. The vehicle's shared
/// actions (the exit, the grapple, the horn, and the grips' vehicle weapons) are declared by the
/// caller.
fn mech<S: Sink>(s: &mut S, seat: VehicleSeat) {
    let steer = L::Steer.id();
    s.axis(Axis::X(steer), TURN_LEFT, TURN_RIGHT);
    s.axis(Axis::Y(steer), PLANE_PITCH_UP, PLANE_PITCH_DOWN);
    if seat.driver {
        s.hold(&[p(L::Accelerate)], &[MECH_FIRE_RIGHT_HAND_WEAPON]);
        s.hold(&[p(L::Reverse)], &[MECH_FIRE_GRAVITY_WEAPON_PRIMARY]);
        s.hold(&[p(L::FirePrimary)], &[MECH_FIRE_GRAVITY_WEAPON_SECONDARY]);
        s.hold(&[p(L::FireSecondary)], &[MECH_PUNCH]);
        s.hold(&[p(L::Stunt)], &[MECH_JUMP]);
    }
}

/// Falling and under the parachute: the hand weapons stay in use, so the on-foot set's fire,
/// reload, and detonation stay bound, and the move action steers in its own frame.
fn falling<S: Sink>(s: &mut S, ctx: &TableCtx) {
    // The air movement reads the move half-axes as the stick's own frame, not the camera's.
    let mv = F::Move.id();
    s.axis(Axis::Y(mv), MOVE_BACKWARD, MOVE_FORWARD);
    s.axis(Axis::X(mv), MOVE_LEFT, MOVE_RIGHT);
    s.analog(Axis::Length(mv), MOVE_ALL);

    s.hold(&[retract(ctx)], &[RETRACT_GRAPPLE, WINGSUIT_AIRBRAKE, L2]);
    s.hold(&fire(ctx), &[FIRE_RIGHT, R2]);
    s.hold(&[p(F::Reload)], PAD_X_ON_FOOT);
    s.hold(&[p(F::Detonate)], DETONATE);
    s.hold(&[p(F::Jump).gated(Gate::UnlessChordArmed)], PAD_A);
    s.hold(&grapple(), GRAPPLE);
    s.hold(&[p(F::Use)], PAD_Y_ON_FOOT);
    s.hold(&[p(F::Use).gated(Gate::Ragdolling)], KEEP_RAGDOLLING);
    s.hold(&[p(F::ReleaseTethers)], PAD_B);
    hand_weapon_fallbacks(s);
}

/// The wingsuit: the weaponized suit's guns, the boost, and the air brake.
fn wingsuit<S: Sink>(s: &mut S) {
    let steer = W::Steer.id();
    s.axis(Axis::Y(steer), MOVE_BACKWARD, MOVE_FORWARD);
    s.axis(Axis::X(steer), MOVE_LEFT, MOVE_RIGHT);
    s.analog(Axis::Length(steer), MOVE_ALL);

    // The air brake: `CANCEL` held (the upgrade's hold-B) and the weaponized suit's own brake.
    // `RETRACT_GRAPPLE` is left off, since it also keeps a stalling wingsuit from closing.
    s.hold(&[p(W::AirBrake)], &[CANCEL, WINGSUIT_AIRBRAKE]);
    s.hold(&[p(W::Boost)], &[WINGSUIT_BOOST]);
    s.hold(&[p(W::FireGuns)], &[FIRE_WINGSUIT_WEAPON_MAIN, R2]);
    s.hold(&[p(W::FireSecondary)], &[FIRE_WINGSUIT_WEAPON_SECONDARY]);
    s.hold(
        &[p(W::OpenParachute).gated(Gate::EvadeFlick)],
        &[WINGSUIT_EVADE],
    );
    s.hold(&[p(W::OpenParachute).gated(Gate::NoEvadeFlick)], PAD_A);
    s.hold(&[p(W::Grapple), p(W::FireGrapple)], GRAPPLE);
    s.hold(&[p(W::Use)], PAD_Y_ON_FOOT);
    s.hold(&[p(W::ReleaseTethers)], PAD_B);
}

/// A game menu. Each hand's confirm, back, and stick act alike, whichever is dominant. A hand's
/// menu modifier (its grip), latched at each press, turns its confirm and back into the gamepad's
/// X and Y, its stick's left and right into the tab steps, and its trigger's page step into the
/// other page. The game repeats a held step itself, so the stick is read as one latched direction
/// at a time ([`Input::MenuStick`]). The native UI pointer's trigger is masked from the page
/// actions on its hand while the pointer shows.
fn ui<S: Sink>(s: &mut S, ctx: &TableCtx) {
    let confirm = M::MenuConfirm.id();
    let back = M::MenuBack.id();
    s.hold(&[p(confirm).gated(Gate::Ungripped)], &[GUI_OK]);
    s.hold(&[p(back).gated(Gate::Ungripped)], &[GUI_CANCEL]);
    s.hold(
        &[p(confirm).gated(Gate::Gripped)],
        &[GUI_SPECIAL_BUTTON, GUI_FILTER],
    );
    s.hold(&[p(back).gated(Gate::Gripped)], &[GUI_USE_BUTTON]);
    // The game's base menu input reads only the left stick's actions, so both sticks drive them.
    let stick = |d| Press::new(Input::MenuStick(d));
    s.hold(&[stick(Direction::Up)], &[GUI_LSTICK_UP]);
    s.hold(&[stick(Direction::Down)], &[GUI_LSTICK_DOWN]);
    s.hold(
        &[stick(Direction::Left).gated(Gate::Ungripped)],
        &[GUI_LSTICK_LEFT],
    );
    s.hold(
        &[stick(Direction::Right).gated(Gate::Ungripped)],
        &[GUI_LSTICK_RIGHT],
    );
    s.hold(
        &[stick(Direction::Left).gated(Gate::Gripped)],
        &[GUI_TAB_PREV],
    );
    s.hold(
        &[stick(Direction::Right).gated(Gate::Gripped)],
        &[GUI_TAB_NEXT],
    );
    // Where a controller has a trackpad, a press on either half of either trackpad also steps the
    // tabs. A touch alone does not, so the touch stays free to show the controls summary.
    if ctx.trackpad_paging {
        s.hold(&[zone(M::MenuTabPress, PadZone::Left)], &[GUI_TAB_PREV]);
        s.hold(&[zone(M::MenuTabPress, PadZone::Right)], &[GUI_TAB_NEXT]);
    }
    // Each trigger pages its own way, and with its grip held the other way, so both pages stay in
    // reach while the native UI pointer owns one hand's trigger.
    s.hold(
        &[
            p(M::MenuPagePrev).gated(Gate::Ungripped),
            p(M::MenuPageNext).gated(Gate::Gripped),
        ],
        &[GUI_PAGE_PREV],
    );
    s.hold(
        &[
            p(M::MenuPageNext).gated(Gate::Ungripped),
            p(M::MenuPagePrev).gated(Gate::Gripped),
        ],
        &[GUI_PAGE_NEXT],
    );
    s.hold(&[p(M::MenuFocus)], &[GUI_FOCUS]);
    s.hold(&[p(M::MenuRecenter)], &[GUI_RECENTER]);
}
