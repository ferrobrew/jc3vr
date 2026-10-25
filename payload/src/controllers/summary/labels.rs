//! What the summary calls each action, input gesture, and mode. Pure.
//!
//! A game action is named by the game's own `buttonhint_*` localisation key, the one its controls
//! screen and HUD hints use for that action in that context, resolved at runtime
//! ([`crate::controllers::summary::localise`]). The mod carries the key names and nothing else for
//! them. The few things the game has no string for (the mod's own gestures, and the falling and
//! parachute headings) are in [`ModText`], the one table of mod strings, in English.

use jc3gi::input::input_action_map::Action::{self, *};

use crate::controllers::{
    bindings::tables::describe::{Gesture, ModAction},
    mode::{AirborneVariant, Mode, VehicleClass},
};

/// A string the summary shows.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Text {
    /// The game's localised string for this key.
    Game(&'static str),
    /// One of the mod's own strings.
    Mod(ModText),
}

/// How an action is labelled in a mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActionLabel {
    /// Shown, with what a press does and what a hold does, either of which may be absent.
    Shown {
        tap: Option<Text>,
        hold: Option<Text>,
    },
    /// Deliberately left out in this mode: the gamepad-button mirrors, which only feed the game's
    /// raw button reads, and the actions another label on the same input already covers or that do
    /// nothing in this mode.
    Hidden,
}

/// Separately permitted parts of the reload input in the current summary.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CurrentAction {
    NativeReload,
    HeldHolster,
}

impl CurrentAction {
    pub(crate) const fn text(self) -> Text {
        match self {
            Self::NativeReload => Text::Game("buttonhint_reload"),
            Self::HeldHolster => Text::Game("buttonhint_holster_weapon"),
        }
    }

    pub(crate) const fn gesture(self) -> Gesture {
        match self {
            Self::NativeReload => Gesture::Press,
            Self::HeldHolster => Gesture::Hold,
        }
    }
}

/// The game's key for the "hold" prefix.
pub const HOLD_KEY: &str = "buttonhint_hold";

/// How `action` is labelled in `mode`, or `None` for an action the summary has no label for. Every
/// action the tables inject has one ([`crate::controllers::bindings::tables::coverage`]).
pub fn action_label(action: Action, mode: Mode) -> Option<ActionLabel> {
    let class = match mode {
        Mode::Vehicle(seat) => Some(seat.class),
        _ => None,
    };
    let airborne = match mode {
        Mode::Airborne(variant) => Some(variant),
        _ => None,
    };
    let wingsuit = airborne == Some(AirborneVariant::Wingsuit);
    let personal_weapon = matches!(class, Some(VehicleClass::Motorcycle | VehicleClass::Sea));
    let tap = |key| ActionLabel::Shown {
        tap: Some(Text::Game(key)),
        hold: None,
    };
    let mod_tap = |text| ActionLabel::Shown {
        tap: Some(Text::Mod(text)),
        hold: None,
    };
    let shown_if = |shown: bool, key| if shown { tap(key) } else { ActionLabel::Hidden };
    Some(match action {
        BTN_A | BTN_B | BTN_X | BTN_Y | L1 | L2 | L3 | R1 | R2 | MOVE_ALL => ActionLabel::Hidden,
        BYPASS_FALL_PREVENTION | SUBMERSIBLE_SURFACE | SUBMERSIBLE_DIVE => ActionLabel::Hidden,

        MOVE_FORWARD => tap("buttonhint_move_forward"),
        MOVE_BACKWARD => tap("buttonhint_move_backward"),
        MOVE_LEFT => tap("buttonhint_move_left"),
        MOVE_RIGHT => tap("buttonhint_move_right"),
        FIRE_RIGHT => tap("buttonhint_fire_weapon"),
        RETRACT_GRAPPLE => tap("buttonhint_retract_tethers"),
        FIRE_GRAPPLE => tap("buttonhint_fire_grapple"),
        RELEASE_GRAPPLE | VEHICLE_RELEASE_GRAPPLE => tap("buttonhint_release_tethers"),
        PUSH_GRAPPLE => tap("buttonhint_hammer"),
        JUMP => shown_if(matches!(mode, Mode::OnFoot(_)), "buttonhint_jump"),
        OPEN_PARACHUTE => shown_if(
            matches!(
                airborne,
                Some(AirborneVariant::Falling | AirborneVariant::Wingsuit)
            ),
            "buttonhint_open_parachute",
        ),
        OPEN_WINGSUIT => shown_if(
            matches!(
                airborne,
                Some(AirborneVariant::Falling | AirborneVariant::Parachute)
            ),
            "buttonhint_wingsuit",
        ),
        STUNT_JUMP => match class {
            Some(VehicleClass::Helicopter) => tap("buttonhint_stunt_jump_helicopter"),
            Some(VehicleClass::Plane) => tap("buttonhint_stunt_jump_plane"),
            Some(VehicleClass::Sea) => tap("buttonhint_stunt_jump_boat"),
            Some(_) => tap("buttonhint_stunt_jump_car"),
            None => ActionLabel::Hidden,
        },
        RELOAD => ActionLabel::Shown {
            tap: Some(Text::Game("buttonhint_reload")),
            hold: Some(Text::Game("buttonhint_holster_weapon")),
        },
        MC_RELOAD if personal_weapon => ActionLabel::Shown {
            tap: Some(Text::Game("buttonhint_reload")),
            hold: Some(Text::Game("buttonhint_holster_weapon")),
        },
        MC_RELOAD => ActionLabel::Hidden,
        USE_ITEM => tap("buttonhint_use_item"),
        // On foot the vehicle is entered by the same press that interacts.
        ENTER_VEHICLE if class.is_some() => ActionLabel::Shown {
            tap: None,
            hold: Some(Text::Game("buttonhint_enter_gunner_seat")),
        },
        ENTER_VEHICLE => ActionLabel::Hidden,
        CANCEL if wingsuit => tap("buttonhint_air_brake"),
        // On foot and falling the use button carries it only while a ragdoll, to stay one.
        CANCEL if matches!(mode, Mode::OnFoot(_) | Mode::Airborne(_)) => ActionLabel::Hidden,
        CANCEL => tap("buttonhint_cancel"),
        WINGSUIT_AIRBRAKE => shown_if(wingsuit, "buttonhint_wingsuit_air_brake"),
        THROW_GRENADE => tap("buttonhint_grenade"),
        PLANT_EXPLOSIVE => tap("buttonhint_plant_explosive"),
        // The gamepad rules detonate once the action has been held past the hold time.
        DETONATE_EXPLOSIVE => ActionLabel::Shown {
            tap: None,
            hold: Some(Text::Game("buttonhint_detonate_explosive")),
        },
        NEXT_WEAPON => tap("buttonhint_next_weapon"),
        // The game names the slots only by their icons on the selection cross.
        SELECT_DUEL_WIELD => mod_tap(ModText::SelectDualWield),
        SELECT_TWO_HANDED => mod_tap(ModText::SelectTwoHanded),
        SELECT_TWO_HANDED_SPECIAL => mod_tap(ModText::SelectSpecial),
        SELECT_EXPLOSIVES => mod_tap(ModText::SelectExplosives),
        WINGSUIT_BOOST => tap("buttonhint_wingsuit_nitrous"),
        WINGSUIT_EVADE => tap("buttonhint_wingsuit_evade"),
        FIRE_WINGSUIT_WEAPON_MAIN => tap("buttonhint_wingsuit_weapon_primary"),
        FIRE_WINGSUIT_WEAPON_SECONDARY => tap("buttonhint_wingsuit_weapon_secondary"),

        EXIT_VEHICLE => tap("buttonhint_exit_vehicle"),
        FIRE_VEHICLE_WEAPON_PRIMARY => shown_if(
            class != Some(VehicleClass::Motorcycle),
            "buttonhint_fire_vehicle_weapon_primary",
        ),
        FIRE_VEHICLE_WEAPON_SECONDARY => tap("buttonhint_fire_vehicle_weapon_secondary"),
        MC_FIRE => shown_if(personal_weapon, "buttonhint_fire_weapon"),
        SOUND_HORN_SIREN => match class {
            Some(VehicleClass::Sea) => tap("buttonhint_sound_horn_siren_sea_vehicle"),
            Some(VehicleClass::Helicopter | VehicleClass::Plane) | None => ActionLabel::Hidden,
            Some(_) => tap("buttonhint_sound_horn_siren_land_vehicle"),
        },
        TOGGLE_MAGNET_1 => tap("buttonhint_toggle_magnet"),
        MECH_PUNCH => tap("buttonhint_mech_punch"),
        MECH_JUMP => tap("buttonhint_mech_jump"),
        MECH_FIRE_GRAVITY_WEAPON_PRIMARY => ActionLabel::Shown {
            tap: Some(Text::Game("buttonhint_mech_gravity_gun_release")),
            hold: Some(Text::Game("buttonhint_mech_gravity_gun_attract")),
        },
        MECH_FIRE_GRAVITY_WEAPON_SECONDARY => tap("buttonhint_mech_gravity_gun_drop"),
        MECH_FIRE_RIGHT_HAND_WEAPON => tap("buttonhint_fire_weapon"),
        USE_VEHICLE_MOD if class == Some(VehicleClass::Helicopter) => tap("buttonhint_nitrous"),
        USE_VEHICLE_MOD => ActionLabel::Shown {
            tap: Some(Text::Game("buttonhint_turbo_jump")),
            hold: Some(Text::Game("buttonhint_nitrous")),
        },
        HANDBRAKE => tap("buttonhint_handbrake"),
        ACCELERATE if class == Some(VehicleClass::Sea) => tap("buttonhint_forward_sea_vehicle"),
        ACCELERATE => tap("buttonhint_accelerate_land_vehicle"),
        REVERSE if class == Some(VehicleClass::Sea) => tap("buttonhint_backward_sea_vehicle"),
        REVERSE => tap("buttonhint_reverse_land_vehicle"),
        TURN_LEFT => shown_if(
            class != Some(VehicleClass::Motorcycle),
            "buttonhint_heli_turn_left",
        ),
        TURN_RIGHT => shown_if(
            class != Some(VehicleClass::Motorcycle),
            "buttonhint_heli_turn_right",
        ),
        BIKE_LEAN_LEFT | BIKE_LEAN_RIGHT | BIKE_TILT_FORWARD | BIKE_TILT_BACKWARD
            if class != Some(VehicleClass::Motorcycle) =>
        {
            ActionLabel::Hidden
        }
        BIKE_LEAN_LEFT => tap("buttonhint_lean_left_motorcycle"),
        BIKE_LEAN_RIGHT => tap("buttonhint_lean_right_motorcycle"),
        BIKE_TILT_FORWARD => tap("buttonhint_tilt_forward_motorcycle"),
        BIKE_TILT_BACKWARD => tap("buttonhint_tilt_backward_motorcycle"),
        HELI_INC_ALTITUDE => tap("buttonhint_heli_inc_altitude"),
        HELI_DEC_ALTITUDE => tap("buttonhint_heli_dec_altitude"),
        HELI_FORWARD => tap("buttonhint_heli_forward"),
        HELI_BACKWARD => tap("buttonhint_heli_backward"),
        HELI_ROLL_LEFT => tap("buttonhint_heli_roll_left"),
        HELI_ROLL_RIGHT => tap("buttonhint_heli_roll_right"),
        HELI_TURN_LEFT => tap("buttonhint_heli_turn_left"),
        HELI_TURN_RIGHT => tap("buttonhint_heli_turn_right"),
        // A car off the ground takes the plane's air control; only a plane names it.
        PLANE_INC_TRUST | PLANE_DEC_TRUST | PLANE_PITCH_UP | PLANE_PITCH_DOWN | PLANE_ROLL_LEFT
        | PLANE_ROLL_RIGHT | PLANE_TURN_LEFT | PLANE_TURN_RIGHT
            if class != Some(VehicleClass::Plane) =>
        {
            ActionLabel::Hidden
        }
        PLANE_INC_TRUST => tap("buttonhint_plane_inc_thrust"),
        PLANE_DEC_TRUST => tap("buttonhint_plane_dec_thrust"),
        PLANE_PITCH_UP => tap("buttonhint_plane_pitch_up"),
        PLANE_PITCH_DOWN => tap("buttonhint_plane_pitch_down"),
        PLANE_ROLL_LEFT => tap("buttonhint_plane_roll_left"),
        PLANE_ROLL_RIGHT => tap("buttonhint_plane_roll_right"),
        PLANE_TURN_LEFT => tap("buttonhint_plane_turn_left"),
        PLANE_TURN_RIGHT => tap("buttonhint_plane_turn_right"),

        GUI_LSTICK_UP | GUI_LSTICK_DOWN | GUI_LSTICK_LEFT | GUI_LSTICK_RIGHT | GUI_RSTICK_UP
        | GUI_RSTICK_DOWN | GUI_RSTICK_LEFT | GUI_RSTICK_RIGHT | GUI_SPECIAL_BUTTON
        | GUI_USE_BUTTON | GUI_FOCUS => ActionLabel::Hidden,
        GUI_OK => tap("buttonhint_ui_ok"),
        GUI_CANCEL => tap("buttonhint_ui_cancel"),
        GUI_FILTER => tap("buttonhint_ui_filter"),
        GUI_TAB_PREV => tap("buttonhint_ui_tab_prev"),
        GUI_TAB_NEXT => tap("buttonhint_ui_tab_next"),
        GUI_PAGE_PREV => tap("buttonhint_ui_page_prev"),
        GUI_PAGE_NEXT => tap("buttonhint_ui_page_next"),
        GUI_RECENTER => tap("buttonhint_ui_recenter"),
        SKIP_CUTSCENE => ActionLabel::Shown {
            tap: None,
            hold: Some(Text::Game("buttonhint_skip_cutscene")),
        },
        GUI_PAUSE => tap("buttonhint_pause"),
        GUI_PDA => tap("buttonhint_commlink"),
        _ => return None,
    })
}

/// The heading for `mode`: the game's controls-screen tab name where it has one.
pub fn heading(mode: Mode) -> Text {
    let tab = |key| Text::Game(key);
    match mode {
        Mode::OnFoot(_) => tab("gui_settings_controller_mapping_on_foot"),
        Mode::Vehicle(seat) => match seat.class {
            VehicleClass::Helicopter => tab("gui_settings_controller_mapping_helicopter"),
            VehicleClass::Plane => tab("gui_settings_controller_mapping_plane"),
            VehicleClass::Motorcycle => tab("gui_settings_controller_mapping_motorcycle"),
            VehicleClass::Sea => tab("gui_settings_controller_mapping_boat"),
            VehicleClass::Car | VehicleClass::Unknown => tab("gui_settings_controller_mapping_car"),
            VehicleClass::Mech => Text::Mod(ModText::Mech),
        },
        Mode::Airborne(AirborneVariant::Wingsuit) => {
            tab("gui_settings_controller_mapping_wingsuit")
        }
        Mode::Airborne(AirborneVariant::Falling) => Text::Mod(ModText::Falling),
        Mode::Airborne(AirborneVariant::Parachute) => Text::Mod(ModText::Parachute),
        Mode::Ui => tab("gui_settings_controller_mapping_ui"),
    }
}

/// The prefix a row's gesture puts before its label, if any.
pub fn gesture_prefix(gesture: Gesture) -> Option<Text> {
    match gesture {
        Gesture::Press => None,
        Gesture::Hold => Some(Text::Game(HOLD_KEY)),
        Gesture::HoldUp => Some(Text::Mod(ModText::HoldUp)),
        Gesture::Flick => Some(Text::Mod(ModText::Flick)),
        Gesture::Touch => Some(Text::Mod(ModText::Touch)),
        Gesture::PressLeftHalf => Some(Text::Mod(ModText::PressLeftHalf)),
        Gesture::PressRightHalf => Some(Text::Mod(ModText::PressRightHalf)),
        Gesture::PressUp => Some(Text::Mod(ModText::PressUp)),
        Gesture::PressDown => Some(Text::Mod(ModText::PressDown)),
        Gesture::PressLeft => Some(Text::Mod(ModText::PressLeft)),
        Gesture::PressRight => Some(Text::Mod(ModText::PressRight)),
        Gesture::Yank => Some(Text::Mod(ModText::Yank)),
        Gesture::DoubleTap => Some(Text::Mod(ModText::DoubleTap)),
        Gesture::WithBothGrips => Some(Text::Mod(ModText::WithBothGrips)),
        Gesture::WithGrip => Some(Text::Mod(ModText::WithGrip)),
    }
}

/// The label of a mod action.
pub fn mod_action(action: ModAction) -> Text {
    Text::Mod(match action {
        ModAction::Turn => ModText::Turn,
        ModAction::SeatedCrouch => ModText::SeatedCrouch,
        ModAction::SightCapture => ModText::SightCapture,
        ModAction::Holster => ModText::Holster,
        ModAction::ReleaseTethers => ModText::ReleaseTethers,
        ModAction::TightenTethers => ModText::TightenTethers,
        ModAction::Foregrip => ModText::Foregrip,
        ModAction::Magazine => ModText::Magazine,
        ModAction::BeltReload => ModText::BeltReload,
        ModAction::FlickEject => ModText::FlickEject,
        ModAction::WorkAction => ModText::WorkAction,
        ModAction::Grenade => ModText::Grenade,
        ModAction::C4 => ModText::C4,
        ModAction::OpenVehicleDoor => ModText::OpenVehicleDoor,
        ModAction::WristDetonator => ModText::WristDetonator,
        ModAction::ParachuteHandles => ModText::ParachuteHandles,
        ModAction::PullParachuteHandle => ModText::PullParachuteHandle,
        ModAction::ArmSteering => ModText::ArmSteering,
        ModAction::GestureDeploy => ModText::GestureDeploy,
        ModAction::GestureClose => ModText::GestureClose,
        ModAction::GestureBrake => ModText::GestureBrake,
        ModAction::Wheel => ModText::Wheel,
        ModAction::Handlebars => ModText::Handlebars,
        ModAction::HelicopterControls => ModText::HelicopterControls,
        ModAction::PlaneControls => ModText::PlaneControls,
        ModAction::WeaponWheel => ModText::WeaponWheel,
        ModAction::MapZoom => ModText::MapZoom,
        ModAction::ShowSummary => ModText::ShowSummary,
        ModAction::Recenter => ModText::Recenter,
        ModAction::DebugPanel => ModText::DebugPanel,
    })
}

/// The mod's own strings: what the game has no string for. English only; a translation would add
/// a column here.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ModText {
    OnScreen,
    MapZoom,
    Falling,
    Mech,
    Parachute,
    Flick,
    Touch,
    PressLeftHalf,
    PressRightHalf,
    Yank,
    DoubleTap,
    WithBothGrips,
    WithGrip,
    Turn,
    ReleaseTethers,
    TightenTethers,
    SeatedCrouch,
    SightCapture,
    Holster,
    Foregrip,
    Magazine,
    BeltReload,
    FlickEject,
    WorkAction,
    Grenade,
    C4,
    OpenVehicleDoor,
    WristDetonator,
    ParachuteHandles,
    PullParachuteHandle,
    ArmSteering,
    GestureDeploy,
    GestureClose,
    GestureBrake,
    Wheel,
    Handlebars,
    HelicopterControls,
    PlaneControls,
    SelectDualWield,
    SelectTwoHanded,
    SelectSpecial,
    SelectExplosives,
    PressUp,
    PressDown,
    PressLeft,
    PressRight,
    HoldUp,
    WeaponWheel,
    ShowSummary,
    Recenter,
    DebugPanel,
    /// The control of an action bound to no input.
    Unbound,
}

impl ModText {
    pub const fn english(self) -> &'static str {
        match self {
            ModText::Falling => "FALLING",
            ModText::Mech => "MECH",
            ModText::Parachute => "PARACHUTE",
            ModText::Flick => "FLICK",
            ModText::Touch => "TOUCH",
            ModText::PressLeftHalf => "PRESS LEFT",
            ModText::PressRightHalf => "PRESS RIGHT",
            ModText::Yank => "YANK",
            ModText::DoubleTap => "DOUBLE TAP",
            ModText::ReleaseTethers => "Tethered: Release Tethers",
            ModText::TightenTethers => "Tethered, Nothing Aimed At: Tighten Tethers",
            ModText::WithBothGrips => "BOTH GRIPS +",
            ModText::WithGrip => "GRIP +",
            ModText::Turn => "Turn",
            ModText::SeatedCrouch => "Crouch (seated)",
            ModText::SightCapture => "Capture Sight Offset",
            ModText::Holster => "At a Holster: Draw or Holster",
            ModText::Foregrip => "At the Foregrip: Steady the Weapon",
            ModText::Magazine => "At the Belt Pouch: Take a Round, Push It into the Gun",
            ModText::BeltReload => "Push the Gun onto the Belt Pouch: Reload",
            ModText::FlickEject => "Eject Magazine",
            ModText::WorkAction => "At the Slide or Bolt: Pull Back to Chamber",
            ModText::Grenade => "At the Pouch: Grab, Let Go to Throw",
            ModText::C4 => "At the Chest Pouch: Grab C4, Let Go to Throw",
            ModText::OpenVehicleDoor => "Open vehicle door",
            ModText::WristDetonator => "Fingertip into the Wrist Button: Detonate C4",
            ModText::ParachuteHandles => "At a Handle: Hold, Pull Down to Steer, Both to Brake",
            ModText::PullParachuteHandle => "Pull down to steer; pull both handles to brake",
            ModText::ArmSteering => "Spread Arms: Lower One to Bank, Both to Dive, Raise to Climb",
            ModText::GestureDeploy => "Lower Both Arms, Then Hold Spread: Request Wingsuit Deploy",
            ModText::GestureClose => {
                "Lower, Then Spread Both Arms to Arm; Hold Both Withdrawn: Request Wingsuit Close"
            }
            ModText::GestureBrake => {
                "Lower, Then Spread Both Arms to Arm; Turn Both Palms Forward While Spread: Brake"
            }
            ModText::Wheel => "Hold at the Wheel: Steer",
            ModText::Handlebars => "Hold at the Handlebars: Steer; Twist the Right Grip: Throttle",
            ModText::HelicopterControls => "Hold at the Cyclic or Collective: Fly",
            ModText::PlaneControls => "Hold at the Stick or Throttle: Fly",
            ModText::SelectDualWield => "Select Pistols",
            ModText::SelectTwoHanded => "Select Two-Handed Weapon",
            ModText::SelectSpecial => "Select Special Weapon",
            ModText::SelectExplosives => "Select Explosives",
            ModText::PressUp => "PRESS UP",
            ModText::PressDown => "PRESS DOWN",
            ModText::PressLeft => "PRESS LEFT",
            ModText::PressRight => "PRESS RIGHT",
            ModText::HoldUp => "HOLD UP",
            ModText::WeaponWheel => "Weapon Wheel: Point to Choose, Let Go to Equip",
            ModText::MapZoom => "Map: Zoom In and Out",
            ModText::OnScreen => "ON SCREEN",
            ModText::ShowSummary => "Show Controls",
            ModText::Recenter => "Recenter View",
            ModText::DebugPanel => "Mod Debug Panel",
            ModText::Unbound => "UNBOUND",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::bindings::tables::{
        coverage::{all_contexts, all_modes, injected},
        describe::{DescribeCtx, Effect, Features, PhysicalFeatures, describe},
    };

    #[test]
    fn every_injected_action_has_a_label() {
        for mode in all_modes() {
            for (dominant, table) in all_contexts() {
                for action in injected(mode, dominant, table) {
                    assert!(
                        action_label(action, mode).is_some(),
                        "{mode:?}: {action:?} is injected but has no label"
                    );
                }
            }
        }
    }

    #[test]
    fn every_described_action_has_a_label() {
        let features = Features {
            yank: true,
            holsters: true,
            foregrip: true,
            magazine: true,
            belt_reload: true,
            flick_eject: true,
            work_action: true,
            grenade_throw: true,
            c4_throw: true,
            wrist_detonator: true,
            sight_chord: true,
            trackpad_shows_summary: true,
            wheel_trackpad: true,
            wheel_stick: true,
            parachute_handles: true,
            arm_steering: true,
            physical: PhysicalFeatures {
                wheel: true,
                handlebars: true,
                helicopter: true,
                plane: true,
            },
        };
        for mode in all_modes() {
            for (dominant, table) in all_contexts() {
                let rows = describe(&DescribeCtx {
                    mode,
                    dominant,
                    table,
                    features,
                });
                for row in rows {
                    if let Effect::Actions(actions) = row.effect {
                        for action in actions {
                            assert!(action_label(action, mode).is_some(), "{mode:?}: {action:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_mirrors_are_hidden_and_the_overloads_carry_their_hold() {
        let on_foot = Mode::OnFoot(crate::controllers::mode::OnFootVariant::Ground);
        assert_eq!(action_label(BTN_A, on_foot), Some(ActionLabel::Hidden));
        assert_eq!(
            action_label(RELOAD, on_foot),
            Some(ActionLabel::Shown {
                tap: Some(Text::Game("buttonhint_reload")),
                hold: Some(Text::Game("buttonhint_holster_weapon")),
            })
        );
    }

    #[test]
    fn the_wingsuit_cancel_is_the_air_brake() {
        let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
        assert_eq!(
            action_label(CANCEL, wingsuit),
            Some(ActionLabel::Shown {
                tap: Some(Text::Game("buttonhint_air_brake")),
                hold: None
            })
        );
    }
}
