//! Controller feature presets that preserve tuning and optional preferences.

use crate::controllers::config::{
    ControllersConfig, GunReadout, Posture, SchemeSetting, SeatTransition,
};

/// The named controller switch presets shown by the testing UI.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Preset {
    InputOnly,
    HandsAndAim,
    WeaponHandling,
    Roomscale,
    VehiclesAndAdvanced,
    GamepadBaseline,
}

impl Preset {
    pub const ALL: [Self; 6] = [
        Self::InputOnly,
        Self::HandsAndAim,
        Self::WeaponHandling,
        Self::Roomscale,
        Self::VehiclesAndAdvanced,
        Self::GamepadBaseline,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::InputOnly => "Input only",
            Self::HandsAndAim => "Hands and aim",
            Self::WeaponHandling => "Weapon handling",
            Self::Roomscale => "Roomscale",
            Self::VehiclesAndAdvanced => "Vehicles and advanced",
            Self::GamepadBaseline => "Gamepad baseline",
        }
    }
}

/// Apply feature switches without changing tuning, calibration, or optional preferences.
pub fn apply_preset(c: &mut ControllersConfig, preset: Preset) {
    let input = preset != Preset::GamepadBaseline;
    let hands = input && preset != Preset::InputOnly;
    let weapons = matches!(
        preset,
        Preset::WeaponHandling | Preset::Roomscale | Preset::VehiclesAndAdvanced
    );
    let room = matches!(preset, Preset::Roomscale | Preset::VehiclesAndAdvanced);
    let vehicles = preset == Preset::VehiclesAndAdvanced;
    c.enabled = input;
    c.scheme = if input {
        SchemeSetting::Controllers
    } else {
        SchemeSetting::Gamepad
    };
    c.gamepad_semantics = input;
    c.input.hand_tracking = input;
    c.bindings.enabled = input;
    c.bindings.yank.enabled = hands;
    c.aim_state.pin = hands;
    c.aim_state.pin_grapple = hands;
    c.aim_state.skip_arm_ik = hands;
    c.aim_state.neutral_aim_point = hands;
    c.aim_state.clear_wall_block = hands;
    c.aim_state.mental_state_cuts = hands;
    c.aim_state.without_controllers = false;
    c.hands.ik = hands;
    c.hands.late_update = hands;
    c.hands.suppress_secondary_ik = hands;
    c.aim.gun = hands;
    c.aim.grapple = hands;
    c.aim.sight_tuning.enabled = false;
    c.overlay.enabled = hands;
    c.overlay.gun_readout = GunReadout::None;
    c.overlay.grapple_highlight = hands;
    c.overlay.grapple_beam = hands;
    c.overlay.ghost_hands = hands;
    c.overlay.grab_volumes = hands;
    c.overlay.debug_gizmos = hands;
    c.weapons.holsters.enabled = weapons;
    c.weapons.foregrip.enabled = weapons;
    c.weapons.block_fire_during_actions = weapons;
    c.weapons.recoil.enabled = weapons;
    c.weapons.recoil.suppress_camera_kick = weapons;
    c.weapons.haptics.enabled = weapons;
    c.weapons.haptics.game_rumble = weapons;
    c.weapons.haptics.grapple = weapons;
    c.weapons.dual_wield.split = weapons;
    c.weapons.dual_wield.place_by_handedness = weapons;
    c.weapons.reload_gesture = weapons;
    c.weapons.grenade_throw = weapons;
    c.weapons.grenade.warning_overlay = weapons;
    c.weapons.grenade.warning_haptics = weapons;
    c.weapons.melee = weapons;
    c.weapons.melee_strike.pointing_targets = weapons;
    c.weapons.scopes = weapons;
    c.weapons.scope.suppress_precision_aim = weapons;
    c.weapons.scope.reticle = weapons;
    c.body.remap_head = room;
    c.body.remap_hands = hands;
    c.roomscale.posture = if room { Posture::Auto } else { Posture::Seated };
    c.roomscale.chase.enabled = room;
    c.roomscale.crouch.visual = room;
    c.roomscale.crouch.capsule = room;
    c.roomscale.head_fade.enabled = room;
    c.roomscale.seat_transition = SeatTransition::Ride;
    c.vehicles.driver_weapons_by_hand = vehicles;
    c.vehicles.mounted.aim_by_hands = vehicles;
    c.vehicles.mounted.anchor_hands = vehicles;
    c.vehicles.physical.wheel.enabled = vehicles;
    c.vehicles.physical.handlebars.enabled = vehicles;
    c.vehicles.physical.cyclic.enabled = vehicles;
    c.vehicles.physical.collective.enabled = vehicles;
    c.vehicles.physical.flight_stick.enabled = vehicles;
    c.vehicles.physical.throttle.enabled = vehicles;
    c.vehicles.arm_steering.wingsuit = vehicles;
    c.vehicles.arm_steering.parachute = vehicles;
    c.fingers.enabled = hands;
}

/// Whether applying the preset changes any of its owned switches.
pub fn preset_modified(config: &ControllersConfig, preset: Preset) -> bool {
    let mut expected = *config;
    apply_preset(&mut expected, preset);
    match (serde_json::to_value(config), serde_json::to_value(expected)) {
        (Ok(actual), Ok(expected)) => actual != expected,
        _ => true,
    }
}
