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
    /// Shown, with the key of what a press does and the key of what a hold does, either of which
    /// may be absent.
    Shown {
        tap: Option<&'static str>,
        hold: Option<&'static str>,
    },
    /// Deliberately left out in this mode: the gamepad-button mirrors, which only feed the game's
    /// raw button reads, and the actions another label on the same input already covers or that do
    /// nothing in this mode.
    Hidden,
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
        tap: Some(key),
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
        RELEASE_GRAPPLE => tap("buttonhint_release_tethers"),
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
            tap: Some("buttonhint_reload"),
            hold: Some("buttonhint_holster_weapon"),
        },
        MC_RELOAD if personal_weapon => ActionLabel::Shown {
            tap: Some("buttonhint_reload"),
            hold: Some("buttonhint_holster_weapon"),
        },
        MC_RELOAD => ActionLabel::Hidden,
        USE_ITEM => tap("buttonhint_use_item"),
        // On foot the vehicle is entered by the same press that interacts.
        ENTER_VEHICLE if class.is_some() => ActionLabel::Shown {
            tap: None,
            hold: Some("buttonhint_enter_gunner_seat"),
        },
        ENTER_VEHICLE => ActionLabel::Hidden,
        CANCEL if wingsuit => tap("buttonhint_air_brake"),
        CANCEL => tap("buttonhint_cancel"),
        WINGSUIT_AIRBRAKE => shown_if(wingsuit, "buttonhint_wingsuit_air_brake"),
        THROW_GRENADE => tap("buttonhint_grenade"),
        PLANT_EXPLOSIVE => tap("buttonhint_plant_explosive"),
        DETONATE_EXPLOSIVE => tap("buttonhint_detonate_explosive"),
        NEXT_WEAPON => tap("buttonhint_next_weapon"),
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
        USE_VEHICLE_MOD if class == Some(VehicleClass::Helicopter) => tap("buttonhint_nitrous"),
        USE_VEHICLE_MOD => ActionLabel::Shown {
            tap: Some("buttonhint_turbo_jump"),
            hold: Some("buttonhint_nitrous"),
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
        Gesture::AfterFlick => Some(Text::Mod(ModText::AfterFlick)),
        Gesture::Flick => Some(Text::Mod(ModText::Flick)),
        Gesture::Touch => Some(Text::Mod(ModText::Touch)),
        Gesture::PressLeftHalf => Some(Text::Mod(ModText::PressLeftHalf)),
        Gesture::PressRightHalf => Some(Text::Mod(ModText::PressRightHalf)),
        Gesture::Yank => Some(Text::Mod(ModText::Yank)),
        Gesture::Strike => Some(Text::Mod(ModText::Strike)),
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
        ModAction::Foregrip => ModText::Foregrip,
        ModAction::Magazine => ModText::Magazine,
        ModAction::Grenade => ModText::Grenade,
        ModAction::ParachuteHandles => ModText::ParachuteHandles,
        ModAction::ShowSummary => ModText::ShowSummary,
        ModAction::Recenter => ModText::Recenter,
        ModAction::DebugPanel => ModText::DebugPanel,
    })
}

/// The mod's own strings: what the game has no string for. English only; a translation would add
/// a column here.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ModText {
    Falling,
    Parachute,
    AfterFlick,
    Flick,
    Touch,
    PressLeftHalf,
    PressRightHalf,
    Yank,
    Strike,
    WithBothGrips,
    WithGrip,
    Turn,
    SeatedCrouch,
    SightCapture,
    Holster,
    Foregrip,
    Magazine,
    Grenade,
    ParachuteHandles,
    ShowSummary,
    Recenter,
    DebugPanel,
}

impl ModText {
    pub const fn english(self) -> &'static str {
        match self {
            ModText::Falling => "FALLING",
            ModText::Parachute => "PARACHUTE",
            ModText::AfterFlick => "FLICK, THEN",
            ModText::Flick => "FLICK",
            ModText::Touch => "TOUCH",
            ModText::PressLeftHalf => "PRESS LEFT",
            ModText::PressRightHalf => "PRESS RIGHT",
            ModText::Yank => "YANK",
            ModText::Strike => "STRIKE",
            ModText::WithBothGrips => "BOTH GRIPS +",
            ModText::WithGrip => "GRIP +",
            ModText::Turn => "Turn",
            ModText::SeatedCrouch => "Crouch (seated)",
            ModText::SightCapture => "Capture Sight Offset",
            ModText::Holster => "At a Holster: Draw or Holster",
            ModText::Foregrip => "At the Foregrip: Steady the Weapon",
            ModText::Magazine => "Belt Pouch to Weapon: Reload",
            ModText::Grenade => "At the Pouch: Grab, Let Go to Throw",
            ModText::ParachuteHandles => "At a Handle: Grab, Pull Down to Steer, Both to Brake",
            ModText::ShowSummary => "Show Controls",
            ModText::Recenter => "Recenter View",
            ModText::DebugPanel => "Mod Debug Panel",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::bindings::tables::{
        coverage::{all_contexts, all_modes, injected},
        describe::{DescribeCtx, Effect, Features, describe},
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
            grenade_throw: true,
            melee: true,
            sight_chord: true,
            trackpad_shows_summary: true,
            parachute_handles: true,
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
                tap: Some("buttonhint_reload"),
                hold: Some("buttonhint_holster_weapon"),
            })
        );
    }

    #[test]
    fn the_wingsuit_cancel_is_the_air_brake() {
        let wingsuit = Mode::Airborne(AirborneVariant::Wingsuit);
        assert_eq!(
            action_label(CANCEL, wingsuit),
            Some(ActionLabel::Shown {
                tap: Some("buttonhint_air_brake"),
                hold: None
            })
        );
    }
}
