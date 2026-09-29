//! Every controller setting except those with their own section's file.

use egui::Ui;

use crate::{
    config,
    controllers::{
        config::{LocomotionDirection, SchemeSetting},
        frame::Hand,
    },
    headpose::config::VrTurnMode,
    ui::controllers::{aim, hands, overlay, roomscale},
};

pub(super) fn settings(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.enabled, "Enabled").on_hover_text(
        "Read controller input and publish the controller frame. Off, every controller subsystem \
         is inert and the gamepad scheme is in effect.",
    );
    ui.horizontal(|ui| {
        ui.label("Scheme");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Auto, "Auto")
            .on_hover_text("Switch on the most recent input.");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Controllers, "Controllers");
        ui.selectable_value(&mut c.scheme, SchemeSetting::Gamepad, "Gamepad");
    });
    ui.horizontal(|ui| {
        ui.label("Dominant hand");
        ui.selectable_value(&mut c.dominant_hand, Hand::Right, "Right");
        ui.selectable_value(&mut c.dominant_hand, Hand::Left, "Left");
    });

    egui::CollapsingHeader::new("Input").show(ui, |ui| {
        ui.add(
            egui::Slider::new(&mut c.input.trigger_press.press, 0.05..=1.0).text("Trigger press"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.trigger_press.release, 0.0..=1.0)
                .text("Trigger release"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.squeeze_press.press, 0.05..=1.0).text("Squeeze press"),
        );
        ui.add(
            egui::Slider::new(&mut c.input.squeeze_press.release, 0.0..=1.0)
                .text("Squeeze release"),
        );
        ui.add(egui::Slider::new(&mut c.input.stick_deadzone, 0.0..=0.9).text("Stick deadzone"));
        ui.checkbox(&mut c.input.hand_tracking, "Hand tracking")
            .on_hover_text("Locate the hand skeleton through XR_EXT_hand_tracking when offered.");
    });

    egui::CollapsingHeader::new("Bindings").show(ui, |ui| {
        let b = &mut c.bindings;
        ui.checkbox(&mut b.enabled, "Inject bindings").on_hover_text(
            "Map the controllers to the game's actions while the controller scheme is in effect.",
        );
        ui.add(egui::Slider::new(&mut b.move_deadzone, 0.0..=0.6).text("Move stick deadzone"));
        ui.add(egui::Slider::new(&mut b.trigger_deadzone, 0.0..=0.5).text("Trigger deadzone"));
        ui.add(egui::Slider::new(&mut b.long_press_s, 0.2..=1.5).text("Commlink long press (s)"));
        ui.add(egui::Slider::new(&mut b.evade_flick, 0.3..=1.0).text("Wingsuit evade flick"));
        ui.add(egui::Slider::new(&mut b.crouch_flick, 0.3..=1.0).text("Seated crouch flick"));
        ui.add(egui::Slider::new(&mut c.mode.debounce_ticks, 1..=6).text("Mode debounce (ticks)"));
    });

    egui::CollapsingHeader::new("Locomotion and turning").show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Move relative to");
            ui.selectable_value(
                &mut c.locomotion.direction,
                LocomotionDirection::Head,
                "Head",
            );
            ui.selectable_value(
                &mut c.locomotion.direction,
                LocomotionDirection::OffHand,
                "Off-hand",
            );
        });
        // Turning is shared with the gamepad's look input (`headpose.vr_turn`).
        let turn = &mut cfg.headpose.vr_turn;
        ui.horizontal(|ui| {
            ui.label("Turn");
            ui.selectable_value(&mut turn.mode, VrTurnMode::Smooth, "Smooth");
            ui.selectable_value(&mut turn.mode, VrTurnMode::Snap, "Snap");
        });
        ui.add(egui::Slider::new(&mut turn.smooth_scale, 0.1..=3.0).text("Smooth turn scale"));
        ui.add(egui::Slider::new(&mut turn.snap_angle_deg, 10.0..=90.0).text("Snap angle (°)"));
    });

    let c = &mut cfg.controllers;
    egui::CollapsingHeader::new("Body and aiming").show(ui, |ui| {
        let a = &mut c.aim_state;
        ui.checkbox(&mut a.pin, "Aim-state pin (weapon)").on_hover_text(
            "Keep the body in the no-aim animation family while firing still works.",
        );
        ui.checkbox(&mut a.pin_grapple, "Aim-state pin (grapple)").on_hover_text(
            "Also clear the grapple aim: the body stops turning to face the hook on a held tether.",
        );
        ui.checkbox(&mut a.skip_arm_ik, "Skip the game's arm aim IK");
        ui.checkbox(&mut a.neutral_aim_point, "Neutral body aim point").on_hover_text(
            "Hold the point the body turns toward ahead of the body. The weapons keep their own aim.",
        );
        ui.add_enabled(
            a.neutral_aim_point,
            egui::Slider::new(&mut a.neutral_aim_distance_m, 5.0..=100.0)
                .text("Neutral point distance (m)"),
        );
        ui.checkbox(&mut a.clear_wall_block, "Clear the wall-penetration block");
        ui.checkbox(&mut a.mental_state_cuts, "Mental-state cuts").on_hover_text(
            "No automatic draw when combat starts and no automatic holster when it ends.",
        );
        ui.checkbox(&mut a.without_controllers, "Apply the above without controllers")
            .on_hover_text("For testing with a gamepad: changes the body for every player.");
        ui.checkbox(&mut c.body.remap_head, "Remap head height")
            .on_hover_text("Moves the view for every player, controllers or not.");
        ui.checkbox(&mut c.body.remap_hands, "Remap hand reach");
        ui.checkbox(&mut c.fingers.enabled, "Finger curls");
    });

    egui::CollapsingHeader::new("Aiming").show(ui, |ui| aim::aim_settings(ui, c));

    egui::CollapsingHeader::new("Hands").show(ui, |ui| hands::hands_settings(ui, &mut c.hands));

    egui::CollapsingHeader::new("Overlay")
        .show(ui, |ui| overlay::overlay_settings(ui, &mut c.overlay));

    egui::CollapsingHeader::new("Weapons").show(ui, |ui| {
        let w = &mut c.weapons;
        ui.checkbox(&mut w.holsters, "Holsters");
        ui.checkbox(&mut w.foregrip, "Foregrip");
        ui.checkbox(&mut w.dual_wield_split, "Dual-wield split");
        ui.checkbox(&mut w.reload_gesture, "Reload gesture");
        ui.checkbox(&mut w.grenade_throw, "Physical grenade throw");
        ui.checkbox(&mut w.melee, "Physical melee");
        ui.checkbox(&mut w.scopes, "Scope lenses");
        ui.checkbox(&mut w.recoil.enabled, "Recoil");
        ui.add_enabled_ui(w.recoil.enabled, |ui| {
            ui.indent("recoil", |ui| {
                ui.add(egui::Slider::new(&mut w.recoil.kick_scale, 0.0..=3.0).text("Kick"));
                ui.add(
                    egui::Slider::new(&mut w.recoil.spring_stiffness, 10.0..=2000.0)
                        .logarithmic(true)
                        .text("Spring stiffness"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.spring_damping_ratio, 0.1..=2.0)
                        .text("Damping ratio"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.two_hand_stiffness_scale, 1.0..=5.0)
                        .text("Two-hand stiffness"),
                );
                ui.add(
                    egui::Slider::new(&mut w.recoil.two_hand_kick_scale, 0.0..=1.0)
                        .text("Two-hand kick"),
                );
            });
        });
        ui.checkbox(&mut w.haptics.enabled, "Haptics");
        ui.add_enabled(
            w.haptics.enabled,
            egui::Slider::new(&mut w.haptics.intensity, 0.0..=1.0).text("Haptic intensity"),
        );
    });

    egui::CollapsingHeader::new("Posture and roomscale").show(ui, |ui| {
        roomscale::roomscale_settings(ui, &mut c.roomscale);
    });

    egui::CollapsingHeader::new("Vehicles and flight").show(ui, |ui| {
        let v = &mut c.vehicles;
        ui.checkbox(&mut v.physical_controls, "Physical vehicle controls");
        ui.checkbox(&mut v.driver_weapons_by_hand, "Driver weapons aim by hand");
        ui.checkbox(&mut v.wingsuit_arm_steering, "Wingsuit arm steering");
        ui.checkbox(&mut v.parachute_arm_steering, "Parachute arm steering");
    });
}
