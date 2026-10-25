//! Feature-local controller editors and readouts for the debug UI.

mod aim;
mod aim_state;
mod body;
mod climb;
mod collider_view;
mod diagnostics;
mod explosives;
mod fingers;
mod frame;
mod grab_view;
mod hand_bodies;
mod hands;
mod haptics;
mod inventory;
mod loadout;
mod locomotion;
mod map;
mod mode;
mod overlay;
mod physical_hands;
mod pouch_models;
mod presets;
mod procedural;
mod reload;
mod rig;
mod roomscale;
mod scope;
mod semantic;
mod settings;
mod sights;
mod snatch;
mod stunt;
mod summary;
mod torso;
mod unarmed;
mod vehicles;
mod weapon_selection;
mod weapons;

use egui::Ui;
use glam::{Quat, Vec3};

use crate::{
    config,
    controllers::{
        aim as controller_aim, aim_state as controller_aim_state, bindings,
        body::{self as controller_body, rig as controller_rig, unarmed as controller_unarmed},
        fingers as controller_fingers,
        frame::{self as controller_frame, InteractionProfile},
        gamepad_setting,
        hands::{ik as hand_ik, late, tiers},
        haptics as controller_haptics,
        loadout::{live as loadout_live, sandbox as loadout_sandbox},
        locomotion as controller_locomotion, map as controller_map, mode as controller_mode,
        overlay as controller_overlay, physical_hands as controller_physical_hands,
        roomscale as controller_roomscale, scheme, summary as controller_summary,
        weapons::inventory as controller_inventory,
    },
    hud::{pointer, summary_panel},
    ui::catalog::{FeatureId, FeatureSection},
    vr::input::{self as vr_input, ActionsOrigin},
};

pub(crate) fn feature(ui: &mut Ui, id: FeatureId, section: FeatureSection) {
    match section {
        FeatureSection::Controls => controls(ui, id),
        FeatureSection::Status => status(ui, id),
        FeatureSection::Visualizations => visualizations(ui, id),
    }
}

fn controls(ui: &mut Ui, id: FeatureId) {
    if matches!(id, FeatureId::Calibration | FeatureId::Procedural) {
        ui.label("Calibration and procedural-body options remain in the pause menu's VR pages.");
        return;
    }
    if id == FeatureId::ControllerCapture {
        diagnostics::diagnostics(ui);
        return;
    }
    if id == FeatureId::Sandbox {
        let sandbox = loadout_sandbox::status();
        loadout::controls(ui, &sandbox);
        return;
    }
    // Snapshot providers that read configuration run before the edit guard exists.
    let input_status = vr_input::status();
    let recovery = bindings::recovery_status();
    let mut cfg = config::edit();
    match id {
        FeatureId::Devices => settings::devices(ui, &mut cfg, &input_status, recovery),
        FeatureId::Thresholds => settings::thresholds(ui, &mut cfg),
        FeatureId::Bindings => settings::bindings_settings(ui, &mut cfg),
        FeatureId::MenuPanel => settings::menu_panel(ui, &mut cfg),
        FeatureId::Locomotion => settings::locomotion_settings(ui, &mut cfg),
        FeatureId::Grapple => {
            settings::grapple_settings(ui, &mut cfg);
            hands::anchor_settings(ui, &mut cfg.controllers.hands);
        }
        FeatureId::AnimationIk => settings::animation_settings(ui, &mut cfg),
        FeatureId::Presentation => hands::presentation_settings(ui, &mut cfg.controllers.hands),
        FeatureId::HandTracking => hands::hands_settings(ui, &mut cfg.controllers.hands),
        FeatureId::Fingers => fingers::fingers_settings(ui, &mut cfg.controllers.fingers),
        FeatureId::PhysicalHands => {
            physical_hands::physical_hands_settings(ui, &mut cfg.controllers.physical_hands);
            ui.collapsing("Advanced tuning", |ui| {
                hand_bodies::hand_bodies_settings(ui, &mut cfg.controllers.physical_hands.bodies);
            });
        }
        FeatureId::CharactersDoors => {
            physical_hands::characters_doors_settings(ui, &mut cfg.controllers.physical_hands);
        }
        FeatureId::Snatch => snatch::snatch_settings(ui, &mut cfg.controllers.weapons.snatch),
        FeatureId::Feedback => {
            haptics::haptics_settings(ui, &mut cfg.controllers.haptics);
            ui.separator();
            physical_hands::impact_sounds_settings(ui, &mut cfg.controllers.physical_hands);
        }
        FeatureId::Hold => weapons::hold_feature_settings(ui, &mut cfg.controllers.weapons),
        FeatureId::Support => weapons::support_settings(ui, &mut cfg.controllers.weapons),
        FeatureId::Aim => {
            aim::aim_settings(ui, &mut cfg.controllers);
            let w = &mut cfg.controllers.weapons;
            scope::scope_settings(ui, &mut w.scopes, &mut w.scope);
            scope::ads_settings(ui, &mut w.ads);
            scope::collide_settings(ui, &mut w.collide);
        }
        FeatureId::Reload => weapons::reload_feature_settings(ui, &mut cfg.controllers.weapons),
        FeatureId::Inventory => {
            weapons::body_slots_settings(ui, &mut cfg.controllers.weapons);
            inventory::inventory_settings(ui, &mut cfg.controllers.weapons.inventory);
        }
        FeatureId::Explosives => {
            weapons::explosive_feature_settings(ui, &mut cfg.controllers.weapons)
        }
        FeatureId::WeaponSelection => {
            weapon_selection::selection_settings(ui, &mut cfg.controllers.weapons)
        }
        FeatureId::Climbing => {
            climb::climb_settings(ui, &mut cfg.controllers.physical_hands.climb);
            stunt::stunt_settings(ui, &mut cfg.controllers.physical_hands.stunt);
        }
        FeatureId::Driving
        | FeatureId::VehicleControls
        | FeatureId::Mounted
        | FeatureId::Wingsuit
        | FeatureId::Parachute => {
            vehicles::feature_settings(ui, &mut cfg.controllers.vehicles, id);
        }
        FeatureId::Posture => roomscale::roomscale_settings(ui, &mut cfg.controllers.roomscale),
        FeatureId::RootWalls => rig::rig_settings(ui, &mut cfg.controllers.body.rig),
        FeatureId::Torso => {
            let rig = &mut cfg.controllers.body.rig;
            torso::torso_settings(ui, &mut rig.torso, &mut rig.pelvis);
        }
        FeatureId::Wrist => summary::summary_settings(ui, &mut cfg.controllers.summary),
        FeatureId::Presets => presets::presets(ui, &mut cfg),
        FeatureId::Semantic => {
            ui.checkbox(&mut cfg.controllers.input.self_check, "Input self-check");
        }
        _ => {
            ui.label("No separate controller tuning is available for this feature.");
        }
    }
}

fn status(ui: &mut Ui, id: FeatureId) {
    match id {
        FeatureId::Devices => {
            let current = scheme::current();
            let selected = scheme::selected();
            let status = vr_input::status();
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
        }
        FeatureId::Bindings => {
            let detection = controller_mode::current();
            let snapshot = controller_mode::last_snapshot();
            let report = bindings::last_report();
            mode::mode_readout(ui, detection, snapshot.as_ref(), &report);
        }
        FeatureId::MenuPanel => map::map_readout(
            ui,
            &controller_map::report(),
            pointer::selected_controller(),
            pointer::owns_menu_trigger(),
        ),
        FeatureId::AnimationIk => {
            aim_state::aim_state_readout(ui, &controller_aim_state::status());
            unarmed::unarmed_readout(ui, &controller_unarmed::status(), &hand_ik::status());
        }
        FeatureId::Aim => {
            aim::aim_readout(
                ui,
                &controller_aim::status(),
                controller_aim::gun::counters(),
                &gamepad_setting::status(),
            );
            scope::scope_readout(ui, &scope::ScopeReadout::read());
        }
        FeatureId::Presentation => {
            hands::hands_readout(ui, &hand_ik::status(), &tiers::status(), &late::status())
        }
        FeatureId::Fingers => fingers::fingers_readout(
            ui,
            &controller_fingers::status(),
            vr_input::status().hand_tracking.conforming_motion_range,
        ),
        FeatureId::PhysicalHands => {
            physical_hands::physical_hands_readout(ui, &controller_physical_hands::status());
            ui.collapsing("Diagnostics", |ui| {
                hand_bodies::hand_bodies_readout(ui, &controller_physical_hands::physics::status())
            });
        }
        FeatureId::CharactersDoors => {
            physical_hands::characters_doors_readout(
                ui,
                &controller_physical_hands::status(),
                &controller_physical_hands::vault::status(),
            );
        }
        FeatureId::Climbing => {
            climb::climb_readout(ui, &controller_physical_hands::climb::status());
            stunt::stunt_readout(ui, &controller_physical_hands::stunt::status());
        }
        FeatureId::Hold => weapons::weapons_readout(ui, &weapons::WeaponsReadout::read()),
        FeatureId::Support
        | FeatureId::Reload
        | FeatureId::Explosives
        | FeatureId::Grapple
        | FeatureId::Snatch
        | FeatureId::WeaponSelection => {
            weapons::feature_readout(ui, &weapons::WeaponsReadout::read(), id);
        }
        FeatureId::Inventory => {
            inventory::inventory_readout(
                ui,
                &controller_inventory::status(),
                &controller_inventory::restore::status(),
            );
            ui.separator();
            loadout::live_readout(ui, &loadout_live::status());
        }
        FeatureId::Sandbox => loadout::sandbox_readout(ui, &loadout_sandbox::status()),
        FeatureId::Feedback => {
            haptics::haptics_readout(ui, &controller_haptics::status());
            physical_hands::impact_sounds_readout(ui, &controller_physical_hands::sounds::status());
        }
        FeatureId::Driving
        | FeatureId::VehicleControls
        | FeatureId::Mounted
        | FeatureId::Wingsuit
        | FeatureId::Parachute => {
            vehicles::feature_readout(ui, &vehicles::VehiclesReadout::read(), id);
        }
        FeatureId::Wrist => {
            summary::summary_readout(ui, &controller_summary::status(), &summary_panel::status())
        }
        FeatureId::Locomotion => {
            let stick = controller_locomotion::status();
            let acts = controller_locomotion::acts::status();
            let anchor = controller_locomotion::anchor::status();
            locomotion::locomotion_readout(ui, &stick, &acts, &anchor);
        }
        FeatureId::Posture => roomscale::roomscale_readout(ui, &controller_roomscale::status()),
        FeatureId::RootWalls => rig::rig_readout(ui, &controller_roomscale::status().root),
        FeatureId::Torso => torso::torso_readout(
            ui,
            &controller_rig::torso_status(),
            &controller_rig::skeleton::status(),
        ),
        FeatureId::Procedural => {
            procedural::procedural_readout(ui, &controller_body::procedural::status())
        }
        FeatureId::Calibration => body::body_readout(ui, &controller_body::player()),
        FeatureId::Semantic => {
            semantic::semantic_readout(ui);
            ui.collapsing("Diagnostics", |ui| {
                frame::live_frame(ui, controller_frame::latest().as_ref())
            });
        }
        FeatureId::Thresholds => frame::live_frame(ui, controller_frame::latest().as_ref()),
        _ => {
            ui.label("No separate live snapshot is available for this feature.");
        }
    }
}

fn visualizations(ui: &mut Ui, id: FeatureId) {
    let grab = controller_physical_hands::grab_view::status();
    let collider = controller_physical_hands::collider_view::status();
    let overlay = controller_overlay::status();
    if id == FeatureId::PhysicalHands {
        grab_view::grab_view_readout(ui, &grab);
        collider_view::collider_view_readout(ui, &collider);
        let mut cfg = config::edit();
        grab_view::grab_view_settings(ui, &mut cfg.controllers.physical_hands.grab_view);
        collider_view::collider_view_settings(
            ui,
            &mut cfg.controllers.physical_hands.collider_view,
        );
    } else if id == FeatureId::RootWalls {
        overlay::overlay_readout(ui, &overlay);
        overlay::overlay_settings(ui, &mut config::edit().controllers.overlay);
    } else {
        ui.label("Shared overlay switches: Body root and walls > Visualizations. Grab and collider views: Physical hands and held objects > Visualizations.");
    }
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
