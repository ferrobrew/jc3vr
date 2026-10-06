//! The Controllers tab: the scheme, the OpenXR input status, live readouts of the published
//! controller frame and of each controller subsystem (the native map, finger curls, the physical
//! hands, weapon handling, scopes, vehicles and flight, the controls summary, posture, roomscale, and the semantic
//! actions with the input self-check included),
//! the player's body dimensions, and every controller setting. Each subsystem's section is in its own file.

mod aim;
mod aim_state;
mod alignment;
mod body;
mod diagnostics;
mod explosives;
mod fingers;
mod frame;
mod hands;
mod locomotion;
mod map;
mod mode;
mod overlay;
mod physical_hands;
mod presets;
mod procedural;
mod reload;
mod roomscale;
mod scope;
mod semantic;
mod settings;
mod sights;
mod stance;
mod summary;
mod vehicles;
mod weapons;

use egui::Ui;
use glam::{Quat, Vec3};

use crate::{
    config,
    controllers::{
        aim as controller_aim, aim_state as controller_aim_state, bindings,
        body::{self as controller_body, alignment as controller_alignment},
        fingers as controller_fingers,
        frame::{self as controller_frame, InteractionProfile},
        gamepad_setting,
        hands::{ik as hand_ik, late, tiers},
        locomotion as controller_locomotion, map as controller_map, mode as controller_mode,
        overlay as controller_overlay, physical_hands as controller_physical_hands,
        roomscale as controller_roomscale, scheme, summary as controller_summary,
    },
    hud::{pointer, summary_panel},
    vr::input::{self as vr_input, ActionsOrigin},
};

pub fn egui_debug_controllers(ui: &mut Ui) {
    // Everything that locks the config is read before the config is locked for editing below.
    let current = scheme::current();
    let selected = scheme::selected();
    let status = vr_input::status();
    let frame = controller_frame::latest();
    let body = controller_body::player();
    let detection = controller_mode::current();
    let snapshot = controller_mode::last_snapshot();
    let report = bindings::last_report();
    let map_report = controller_map::report();
    let map_pointer = pointer::selected_controller();
    let map_menu_trigger = pointer::owns_menu_trigger();
    let aim_state_status = controller_aim_state::status();
    let aim_status = controller_aim::status();
    let gun_counters = controller_aim::gun::counters();
    let setting = gamepad_setting::status();
    let hand_ik = hand_ik::status();
    let hand_tiers = tiers::status();
    let late_status = late::status();
    let fingers_status = controller_fingers::status();
    let physical_hands_status = controller_physical_hands::status();
    let overlay_status = controller_overlay::status();
    let roomscale_status = controller_roomscale::status();
    let stance_status = controller_roomscale::stance::status();
    let unarmed_status = controller_roomscale::stance::unarmed::status();
    let alignment_status = controller_alignment::status();
    let procedural_status = controller_body::procedural::status();
    let locomotion_status = (
        controller_locomotion::status(),
        controller_locomotion::acts::status(),
        controller_locomotion::anchor::status(),
    );
    let weapons_readout = weapons::WeaponsReadout::read();
    let scope_readout = scope::ScopeReadout::read();
    let vehicles_readout = vehicles::VehiclesReadout::read();
    let summary_status = controller_summary::status();
    let panel_status = summary_panel::status();

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

        egui::CollapsingHeader::new("Native map")
            .default_open(true)
            .show(ui, |ui| {
                map::map_readout(ui, &map_report, map_pointer, map_menu_trigger)
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

        egui::CollapsingHeader::new("Finger curls")
            .default_open(true)
            .show(ui, |ui| {
                fingers::fingers_readout(
                    ui,
                    &fingers_status,
                    status.hand_tracking.conforming_motion_range,
                )
            });

        egui::CollapsingHeader::new("Physical hands")
            .default_open(true)
            .show(ui, |ui| {
                physical_hands::physical_hands_readout(ui, &physical_hands_status)
            });

        egui::CollapsingHeader::new("Weapon handling")
            .default_open(true)
            .show(ui, |ui| weapons::weapons_readout(ui, &weapons_readout));

        egui::CollapsingHeader::new("Scopes")
            .default_open(true)
            .show(ui, |ui| scope::scope_readout(ui, &scope_readout));

        egui::CollapsingHeader::new("Vehicles and flight")
            .default_open(true)
            .show(ui, |ui| vehicles::vehicles_readout(ui, &vehicles_readout));

        egui::CollapsingHeader::new("Overlay")
            .default_open(true)
            .show(ui, |ui| overlay::overlay_readout(ui, &overlay_status));

        egui::CollapsingHeader::new("Controls summary")
            .default_open(true)
            .show(ui, |ui| {
                summary::summary_readout(ui, &summary_status, &panel_status)
            });

        egui::CollapsingHeader::new("Posture and roomscale")
            .default_open(true)
            .show(ui, |ui| roomscale::roomscale_readout(ui, &roomscale_status));

        egui::CollapsingHeader::new("Stick locomotion and view anchor")
            .default_open(true)
            .show(ui, |ui| {
                let (stick, acts, anchor) = &locomotion_status;
                locomotion::locomotion_readout(ui, stick, acts, anchor)
            });

        egui::CollapsingHeader::new("Procedural body")
            .default_open(true)
            .show(ui, |ui| {
                procedural::procedural_readout(ui, &procedural_status)
            });

        egui::CollapsingHeader::new("Standing stance")
            .default_open(true)
            .show(ui, |ui| {
                stance::stance_readout(ui, &stance_status, &unarmed_status, &hand_ik)
            });

        egui::CollapsingHeader::new("Body alignment")
            .default_open(true)
            .show(ui, |ui| alignment::alignment_readout(ui, &alignment_status));

        egui::CollapsingHeader::new("Semantic actions").show(ui, semantic::semantic_readout);

        egui::CollapsingHeader::new("Live controller frame")
            .default_open(true)
            .show(ui, |ui| frame::live_frame(ui, frame.as_ref()));

        egui::CollapsingHeader::new("Body dimensions").show(ui, |ui| body::body_readout(ui, &body));

        egui::CollapsingHeader::new("Diagnostic capture").show(ui, diagnostics::diagnostics);

        ui.separator();
        ui.heading("Settings");
        let mut cfg = config::edit();
        settings::settings(ui, &mut cfg);
        presets::presets(ui, &mut cfg);
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
