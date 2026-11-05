//! Named in-headset test presets: the controller feature switches all on, as they ship, or all off
//! for the head-tracked gamepad baseline. A preset changes switches only; tuning, sight
//! calibration, and optional preferences stay as they are.

use crate::controllers::config::{ControllersConfig, GunReadout, Posture, SeatTransition};

/// The named controller switch presets shown by the Tools page.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Preset {
    /// Every controller feature on, as `ControllersConfig::new()` ships.
    Controllers,
    /// Every controller and roomscale feature off: the head-tracked gamepad game.
    GamepadBaseline,
}

impl Preset {
    pub const ALL: [Self; 2] = [Self::Controllers, Self::GamepadBaseline];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Controllers => "Controllers",
            Self::GamepadBaseline => "Gamepad baseline",
        }
    }
}

/// Apply feature switches without changing tuning, calibration, or optional preferences.
pub fn apply_preset(c: &mut ControllersConfig, preset: Preset) {
    let on = preset == Preset::Controllers;
    c.enabled = on;
    c.input.hand_tracking = on;
    c.bindings.enabled = on;
    c.bindings.yank.enabled = on;
    c.aim_state.skip_arm_ik = on;
    c.hands.ik = on;
    c.hands.late_update = on;
    c.hands.presentation.enabled = on;
    c.hands.rigid_grip = on;
    c.aim.gun = on;
    c.aim.grapple = on;
    c.aim.sight_tuning.enabled = false;
    c.overlay.enabled = on;
    c.overlay.gun_readout = GunReadout::None;
    c.overlay.grapple_highlight = on;
    c.overlay.grapple_beam = on;
    c.overlay.hide_game_reticle = on;
    c.weapons.foregrip.enabled = on;
    c.weapons.recoil.enabled = on;
    c.haptics.enabled = on;
    c.haptics.game_rumble = on;
    c.weapons.reload.enabled = on;
    c.weapons.grenade_throw = on;
    c.weapons.c4_throw = on;
    c.weapons.wrist_detonator = on;
    c.weapons.c4.detonator.overlay = on;
    c.weapons.scopes = on;
    c.weapons.auto_switch_when_empty = !on;
    c.body.remap_head = on;
    c.body.remap_hands = on;
    c.roomscale.posture = if on { Posture::Auto } else { Posture::Seated };
    c.body.rig.steps.enabled = on;
    c.roomscale.crouch.capsule = on;
    c.roomscale.head_fade.enabled = on;
    c.roomscale.seat_transition = SeatTransition::Ride;
    c.vehicles.driver_weapons_by_hand = on;
    c.vehicles.passenger_body_by_hand = on;
    c.vehicles.mounted.aim_by_hands = on;
    c.vehicles.mounted.grip_handles = on;
    c.vehicles.physical.wheel.enabled = on;
    c.vehicles.physical.handlebars = on;
    c.vehicles.physical.sticks = on;
    c.vehicles.physical.levers = on;
    c.vehicles.physical.twist_rudder = on;
    c.vehicles.physical.nitrous_button = on;
    c.vehicles.physical.bars_twist = on;
    c.vehicles.arm_steering.wingsuit = on;
    c.vehicles.driving_view.enabled = on;
    c.vehicles.parachute_handles.enabled = on;
    c.vehicles.ripcord.enabled = on;
    c.fingers.enabled = on;
    c.grapple.pull_to_reel = on;
    c.grapple.pump = on;
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
        apply_preset(&mut config.controllers, Preset::Controllers);
        assert_eq!(config.controllers.hands.grip, grip);
        assert_eq!(config.controllers.aim.sights.get(99).copied(), sight);
        assert_eq!(config.headpose.vr_turn.snap_angle_deg, turn);
    }

    #[test]
    fn preset_modified_reports_switch_differences_without_tuning_differences() {
        let mut c = ControllersConfig::new();
        apply_preset(&mut c, Preset::Controllers);
        assert!(!preset_modified(&c, Preset::Controllers));
        c.hands.grip.position_m.x += 0.1;
        assert!(!preset_modified(&c, Preset::Controllers));
        c.vehicles.physical.wheel.enabled = false;
        assert!(preset_modified(&c, Preset::Controllers));
    }

    #[test]
    fn defaults_are_the_full_gameplay_preset() {
        // `ControllersConfig::new()` ships as the full "Controllers" preset, so that wingsuit and
        // parachute arm steering, roomscale, and every other subsystem work out of the box.
        assert!(!preset_modified(
            &ControllersConfig::new(),
            Preset::Controllers
        ));
    }

    #[test]
    fn gamepad_baseline_turns_off_controller_and_roomscale_features() {
        let mut c = ControllersConfig::new();
        apply_preset(&mut c, Preset::GamepadBaseline);
        assert!(!c.enabled);
        assert!(!c.body.rig.steps.enabled);
        assert!(!c.roomscale.crouch.capsule);
        assert!(!c.roomscale.head_fade.enabled);
        assert!(!c.body.remap_head);
        assert!(!c.body.remap_hands);
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
        assert!(owned > 100);
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
