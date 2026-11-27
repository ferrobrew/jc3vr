//! The settings one input tick reads, copied out of the configuration once at its start, so that
//! the tick reads one consistent set and takes the configuration's lock once.

use jc3vr_body::controllers::{
    body::{BODY_CONFIG, BodyDimensions},
    hands::config::GripOffset,
    intent::config::BindingsConfig,
    locomotion::config::LocomotionDirection,
    overlay::config::OverlayConfig,
};
use jc3vr_input::frame::Hand;
use jc3vr_view::headpose::config::{HEADPOSE_CONFIG, VrTurnConfig};

use crate::controllers::config::{CONTROLLERS_CONFIG, ControllersConfig};

/// The settings of one input tick.
#[derive(Copy, Clone)]
pub(crate) struct Settings {
    /// The controllers' master switch.
    pub(crate) enabled: bool,
    /// The hand the hand roles follow ([`ControllersConfig::acting_hand`]).
    pub(crate) acting: Hand,
    pub(crate) bindings: BindingsConfig,
    /// The stick turn, whose deadzones decide when the turn stick is at rest.
    pub(crate) vr_turn: VrTurnConfig,
    /// The player's body, which the wingsuit gestures measure the arms against.
    pub(crate) dimensions: BodyDimensions,
    pub(crate) locomotion_direction: LocomotionDirection,
    /// Where the grip sits in each hand, which the wingsuit gestures place the hands by.
    pub(crate) grip_offset: GripOffset,
    pub(crate) overlay: OverlayConfig,
    /// A stick click cycles the weapons.
    pub(crate) stick_click_cycles: bool,
    /// The sight tuning's capture chord is live.
    pub(crate) sight_tuning: bool,
    /// The grounded rig's committed steps walk Rico through the game's animation.
    pub(crate) steps: bool,
    /// The physical reload: magazines by hand and the belt reload.
    pub(crate) reload: bool,
    /// The weapon wheel.
    pub(crate) wheel: bool,
    /// The parachute's physical handles.
    pub(crate) parachute_handles: bool,
    /// The wingsuit's arm steering.
    pub(crate) arm_steering: bool,
}

impl Settings {
    /// The settings as they are now.
    pub(crate) fn snapshot() -> Self {
        let mut settings = CONTROLLERS_CONFIG.query(Self::of);
        settings.vr_turn = HEADPOSE_CONFIG.query(|headpose| headpose.vr_turn);
        settings.dimensions = *BODY_CONFIG.lock();
        settings
    }

    /// The settings `config` holds, with the stick turn and the body at their defaults.
    pub(crate) fn of(config: &ControllersConfig) -> Self {
        Self {
            enabled: config.enabled,
            acting: config.acting_hand(),
            bindings: config.bindings,
            vr_turn: VrTurnConfig::new(),
            dimensions: BodyDimensions::default(),
            locomotion_direction: config.locomotion.direction,
            grip_offset: config.hands.grip,
            overlay: config.overlay,
            stick_click_cycles: config.weapons.holsters.stick_click_cycles,
            sight_tuning: config.aim.sight_tuning.enabled,
            steps: config.body.rig.steps.enabled,
            reload: config.weapons.reload.enabled,
            wheel: config.weapons.wheel.enabled,
            parachute_handles: config.vehicles.parachute_handles.enabled,
            arm_steering: config.vehicles.arm_steering.wingsuit,
        }
    }
}
