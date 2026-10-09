//! Named in-headset test presets: bundles of controller feature switches for bringing the
//! subsystems up in stages (input only, then hands and aim, weapon handling, roomscale, and
//! vehicles) or dropping back to the gamepad baseline. A preset changes switches only; tuning,
//! sight calibration, and optional preferences stay as they are.

use crate::controllers::config::{
    ControllersConfig, GunReadout, Posture, SchemeSetting, SeatTransition,
};

/// The named controller switch presets shown by the Controllers tab.
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
    c.hands.presentation.enabled = hands;
    c.hands.suppress_secondary_ik = hands;
    c.hands.rigid_grip = hands;
    c.aim.gun = hands;
    c.aim.grapple = hands;
    c.aim.sight_tuning.enabled = false;
    c.overlay.enabled = hands;
    c.overlay.gun_readout = GunReadout::None;
    c.overlay.grapple_highlight = hands;
    c.overlay.grapple_beam = hands;
    c.overlay.hide_game_reticle = hands;
    c.weapons.holsters.enabled = weapons;
    c.weapons.foregrip.enabled = weapons;
    c.weapons.block_fire_during_actions = weapons;
    c.weapons.recoil.enabled = weapons;
    c.weapons.recoil.suppress_camera_kick = weapons;
    c.haptics.enabled = weapons;
    c.haptics.game_rumble = weapons;
    c.haptics.categories.weapons = weapons;
    c.haptics.categories.traversal = weapons;
    c.weapons.dual_wield.split = weapons;
    c.weapons.dual_wield.place_by_handedness = weapons;
    c.weapons.dual_wield.independent_draw = weapons;
    c.weapons.reload.enabled = weapons;
    c.weapons.grenade_throw = weapons;
    c.weapons.grenade.warning_overlay = weapons;
    c.weapons.grenade.warning_haptics = weapons;
    c.weapons.c4_throw = weapons;
    c.weapons.wrist_detonator = weapons;
    c.weapons.c4.detonator.overlay = weapons;
    c.weapons.scopes = weapons;
    c.weapons.scope.suppress_precision_aim = weapons;
    c.weapons.scope.reticle = weapons;
    c.weapons.auto_switch_when_empty = !weapons;
    c.body.remap_head = room;
    c.body.remap_hands = hands;
    c.roomscale.posture = if room { Posture::Auto } else { Posture::Seated };
    c.roomscale.chase.enabled = room;
    c.roomscale.crouch.visual = room;
    c.roomscale.crouch.capsule = room;
    c.roomscale.head_fade.enabled = room;
    c.roomscale.seat_transition = SeatTransition::Ride;
    c.vehicles.driver_weapons_by_hand = vehicles;
    c.vehicles.passenger_body_by_hand = vehicles;
    c.vehicles.mounted.aim_by_hands = vehicles;
    c.vehicles.mounted.anchor_hands = vehicles;
    c.vehicles.mounted.grip_handles = vehicles;
    c.vehicles.mounted.recoil = vehicles;
    c.vehicles.physical.wheel.enabled = vehicles;
    c.vehicles.physical.handlebars.enabled = vehicles;
    c.vehicles.physical.cyclic.enabled = vehicles;
    c.vehicles.physical.collective.enabled = vehicles;
    c.vehicles.physical.flight_stick.enabled = vehicles;
    c.vehicles.physical.throttle.enabled = vehicles;
    c.vehicles.physical.flight.twist_rudder = vehicles;
    c.vehicles.physical.flight.afterburner_gate = vehicles;
    c.vehicles.physical.buttons.horn = vehicles;
    c.vehicles.physical.buttons.nitrous_button = vehicles;
    c.vehicles.physical.buttons.twist_nitrous = vehicles;
    c.vehicles.physical.buttons.twist_throttle = vehicles;
    c.vehicles.arm_steering.wingsuit = vehicles;
    c.vehicles.driving_view.enabled = vehicles;
    c.vehicles.parachute_handles.enabled = vehicles;
    c.fingers.enabled = hands;
    c.grapple.reel.enabled = hands;
    c.grapple.tether.enabled = hands;
    c.grapple.swing.pump = hands;
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

#[cfg(test)]
mod tests {
    use glam::{Vec2, Vec3};

    use super::*;
    use crate::{config::Config, controllers::config::SightOverride};

    #[test]
    fn presets_preserve_tuning_and_calibration() {
        let mut config = Config::new();
        config.controllers.hands.grip.position_m = Vec3::new(0.12, -0.08, 0.04);
        config.controllers.aim.sights.set(SightOverride {
            weapon_hash: 99,
            offset_m: Vec3::new(0.0, 0.02, -0.01),
            angles_deg: Vec2::new(2.0, 3.0),
        });
        config.headpose.vr_turn.snap_angle_deg = 47.0;
        let grip = config.controllers.hands.grip;
        let sight = config.controllers.aim.sights.get(99).copied();
        let turn = config.headpose.vr_turn.snap_angle_deg;
        apply_preset(&mut config.controllers, Preset::VehiclesAndAdvanced);
        assert_eq!(config.controllers.hands.grip, grip);
        assert_eq!(config.controllers.aim.sights.get(99).copied(), sight);
        assert_eq!(config.headpose.vr_turn.snap_angle_deg, turn);
    }

    #[test]
    fn preset_modified_reports_switch_differences_without_tuning_differences() {
        let mut c = ControllersConfig::new();
        apply_preset(&mut c, Preset::VehiclesAndAdvanced);
        assert!(!preset_modified(&c, Preset::VehiclesAndAdvanced));
        c.hands.grip.position_m.x += 0.1;
        assert!(!preset_modified(&c, Preset::VehiclesAndAdvanced));
        c.vehicles.physical.wheel.enabled = false;
        assert!(preset_modified(&c, Preset::VehiclesAndAdvanced));
    }

    #[test]
    fn defaults_are_the_full_gameplay_preset() {
        // `ControllersConfig::new()` ships as the full "Vehicles and advanced" preset, so that
        // wingsuit and parachute arm steering, roomscale, and every other subsystem work out of
        // the box.
        assert!(!preset_modified(
            &ControllersConfig::new(),
            Preset::VehiclesAndAdvanced
        ));
    }

    #[test]
    fn gamepad_baseline_turns_off_controller_and_roomscale_features() {
        let mut c = ControllersConfig::new();
        apply_preset(&mut c, Preset::GamepadBaseline);
        assert!(!c.enabled);
        assert_eq!(c.scheme, SchemeSetting::Gamepad);
        assert!(!c.roomscale.chase.enabled);
        assert!(!c.roomscale.crouch.visual);
        assert!(!c.roomscale.crouch.capsule);
        assert!(!c.roomscale.head_fade.enabled);
        assert!(!c.body.remap_head);
        assert!(!c.body.remap_hands);
    }

    #[test]
    fn preset_stages_enable_the_required_support() {
        for preset in Preset::ALL {
            let mut c = Config::new().controllers;
            apply_preset(&mut c, preset);
            let hands = !matches!(preset, Preset::InputOnly | Preset::GamepadBaseline);
            assert_eq!(c.bindings.yank.enabled, hands);
            assert_eq!(c.body.remap_hands, hands);
            assert_eq!(c.overlay.enabled, hands);
        }
    }

    #[test]
    fn every_owned_switch_marks_the_selected_preset_modified() {
        let baseline = Config::new().controllers;
        let mut leaves = Vec::new();
        collect_leaves(
            &serde_json::to_value(baseline).unwrap(),
            String::new(),
            &mut leaves,
        );
        let mut owned = 0;
        for preset in Preset::ALL {
            let mut expected = baseline;
            apply_preset(&mut expected, preset);
            let expected_value = serde_json::to_value(expected).unwrap();
            for path in &leaves {
                let mut changed_value = expected_value.clone();
                let leaf = changed_value.pointer_mut(path).unwrap();
                if let Some(value) = leaf.as_bool() {
                    *leaf = (!value).into();
                } else if leaf.is_string() {
                    // Obtain another valid enum variant from a different preset.
                    let alternative = Preset::ALL.into_iter().find_map(|other| {
                        let mut c = baseline;
                        apply_preset(&mut c, other);
                        let value = serde_json::to_value(c).unwrap();
                        let alternative = value.pointer(path).unwrap();
                        (alternative != leaf).then(|| alternative.clone())
                    });
                    let Some(alternative) = alternative else {
                        continue;
                    };
                    *leaf = alternative;
                } else {
                    continue;
                }
                let changed: ControllersConfig =
                    serde_json::from_value(changed_value.clone()).unwrap();
                let mut reapplied = changed;
                apply_preset(&mut reapplied, preset);
                let reapplied = serde_json::to_value(reapplied).unwrap();
                let is_owned = reapplied.pointer(path) != changed_value.pointer(path);
                assert_eq!(
                    preset_modified(&changed, preset),
                    is_owned,
                    "{preset:?}: {path}"
                );
                owned += usize::from(is_owned);
            }
        }
        assert!(owned > 300);
    }

    fn collect_leaves(value: &serde_json::Value, path: String, leaves: &mut Vec<String>) {
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                collect_leaves(value, format!("{path}/{key}"), leaves);
            }
        } else {
            leaves.push(path);
        }
    }
}
