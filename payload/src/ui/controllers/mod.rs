//! The Controllers tab: the scheme, the OpenXR input status, live readouts of the published
//! controller frame and of each controller subsystem, the player's body dimensions, and every
//! controller setting. Each subsystem's section is in its own file.

mod aim;
mod aim_state;
mod body;
mod frame;
mod hands;
mod mode;
mod overlay;
mod settings;

use egui::Ui;
use glam::{Quat, Vec3};

use crate::{
    config,
    controllers::{
        aim as controller_aim, aim_state as controller_aim_state, bindings,
        body as controller_body,
        frame::{self as controller_frame, InteractionProfile},
        gamepad_setting,
        hands::{ik as hand_ik, late, tiers},
        mode as controller_mode, overlay as controller_overlay, scheme,
    },
    vr::input::{self as vr_input, ActionsOrigin},
};

pub fn egui_debug_controllers(ui: &mut Ui) {
    // Everything that locks the config is read before the config is locked for editing below.
    let current = scheme::current();
    let selected = scheme::selected();
    let status = vr_input::status();
    let frame = controller_frame::latest();
    let body = controller_body::player_body();
    let detection = controller_mode::current();
    let snapshot = controller_mode::last_snapshot();
    let report = bindings::last_report();
    let aim_state_status = controller_aim_state::status();
    let aim_status = controller_aim::status();
    let gun_counters = controller_aim::gun::counters();
    let setting = gamepad_setting::status();
    let hand_ik = hand_ik::status();
    let hand_tiers = tiers::status();
    let late_status = late::status();
    let overlay_status = controller_overlay::status();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading("Status");
        ui.label(format!(
            "Scheme: {current:?} (auto-selection last chose {selected:?})"
        ));
        ui.label(format!("Action set: {:?}", vr_input::active_action_set()));
        ui.label(match (&status.error, status.attached, status.origin) {
            (Some(error), _, _) => format!("OpenXR input: unavailable: {error}"),
            (None, true, Some(origin)) => {
                format!("OpenXR input: attached ({})", origin_label(origin))
            }
            (None, _, _) => "OpenXR input: no session".to_owned(),
        });
        ui.label(format!(
            "Profiles: left {}, right {}",
            profile_label(status.profiles[0]),
            profile_label(status.profiles[1])
        ));
        let ht = status.hand_tracking;
        ui.label(format!(
            "Hand tracking: extension {}, system {}, trackers {}",
            yes_no(ht.extension_enabled),
            yes_no(ht.system_supported),
            yes_no(ht.trackers_created)
        ));

        ui.separator();
        egui::CollapsingHeader::new("Mode and bindings")
            .default_open(true)
            .show(ui, |ui| {
                mode::mode_readout(ui, detection, snapshot.as_ref(), &report)
            });

        egui::CollapsingHeader::new("Aim state and mental state")
            .default_open(true)
            .show(ui, |ui| aim_state::aim_state_readout(ui, &aim_state_status));

        egui::CollapsingHeader::new("Aiming")
            .default_open(true)
            .show(ui, |ui| {
                aim::aim_readout(ui, &aim_status, gun_counters, &setting)
            });

        egui::CollapsingHeader::new("Hands")
            .default_open(true)
            .show(ui, |ui| {
                hands::hands_readout(ui, &hand_ik, &hand_tiers, &late_status)
            });

        egui::CollapsingHeader::new("Overlay")
            .default_open(true)
            .show(ui, |ui| overlay::overlay_readout(ui, &overlay_status));

        egui::CollapsingHeader::new("Live controller frame")
            .default_open(true)
            .show(ui, |ui| frame::live_frame(ui, frame.as_ref()));

        egui::CollapsingHeader::new("Body dimensions").show(ui, |ui| body::body_readout(ui, &body));

        ui.separator();
        ui.heading("Settings");
        let mut cfg = config::CONFIG.lock();
        settings::settings(ui, &mut cfg);
    });
}

fn vec3_drag(ui: &mut Ui, label: &str, v: &mut Vec3, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(&mut v.x).speed(speed).prefix("x "));
        ui.add(egui::DragValue::new(&mut v.y).speed(speed).prefix("y "));
        ui.add(egui::DragValue::new(&mut v.z).speed(speed).prefix("z "));
    });
}

fn origin_label(origin: ActionsOrigin) -> &'static str {
    match origin {
        ActionsOrigin::Created => "created this session",
        ActionsOrigin::RewrappedAndAttached => "re-wrapped, attached to a new session",
        ActionsOrigin::Rewrapped => "re-wrapped from the previous injection",
    }
}

fn profile_label(profile: InteractionProfile) -> &'static str {
    match profile {
        InteractionProfile::None => "none",
        InteractionProfile::ValveIndex => "Valve Index",
        InteractionProfile::OculusTouch => "Oculus Touch",
        InteractionProfile::Other => "other",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn vec3(v: Vec3) -> String {
    format!("({:+.2}, {:+.2}, {:+.2})", v.x, v.y, v.z)
}

/// Yaw, pitch, and roll in degrees, for OpenXR's axes (+Y up, -Z forward).
fn euler_deg(q: Quat) -> (f32, f32, f32) {
    let (yaw, pitch, roll) = q.to_euler(glam::EulerRot::YXZ);
    (yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees())
}
