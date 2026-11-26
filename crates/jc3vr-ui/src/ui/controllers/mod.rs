//! Feature-local controller editors and readouts for the debug UI.

mod aim;
mod aim_state;
mod body_ik;
mod climb;
mod collider_view;
mod estimator;
mod explosives;
mod fingers;
mod frame;
mod glass;
mod grab_view;
mod hand_bodies;
mod hands;
mod haptics;
mod inventory;
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
use jc3vr_body::controllers::{
    aim as controller_aim, aim_state as controller_aim_state, body as controller_body,
    body::{rig as controller_rig, unarmed as controller_unarmed},
    fingers as controller_fingers, gamepad_setting,
    hands::{ik as hand_ik, late, tiers},
    haptics as controller_haptics, locomotion as controller_locomotion, mode as controller_mode,
    roomscale as controller_roomscale, scheme,
};
use jc3vr_gameplay::controllers::{
    bindings, config::CONTROLLERS_CONFIG, summary as controller_summary,
};
use jc3vr_hud::{controllers::map as controller_map, hud::summary_panel};
use jc3vr_input::{frame as controller_frame, frame::InteractionProfile};
use jc3vr_physical::controllers::{
    glass as controller_glass, physical_hands as controller_physical_hands,
};
use jc3vr_render::controllers::overlay::pass as overlay_pass;
use jc3vr_view::vr::input as vr_input;

use crate::{
    status::{
        feedback, hands as hands_status,
        system::{INPUT_ACTIONS, LAYOUT_FINGERPRINT},
    },
    ui::{camera, catalog::FeatureId, dispatch::FeatureBody, status_row::headlines},
};

/// The controller editors for a feature, when it has any. Each editor takes the configuration edit
/// guard itself, after reading any snapshot providers that read configuration.
pub(crate) fn controls(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = match id {
        FeatureId::Devices => |ui, _| settings::devices(ui, &mut CONTROLLERS_CONFIG.edit()),
        FeatureId::OnFoot => {
            |ui, _| settings::locomotion_settings(ui, &mut CONTROLLERS_CONFIG.edit())
        }
        FeatureId::Grapple => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            settings::grapple_settings(ui, &mut c);
            hands::anchor_settings(ui, &mut c.hands);
        },
        FeatureId::Presentation => {
            |ui, _| hands::presentation_settings(ui, &mut CONTROLLERS_CONFIG.edit().hands)
        }
        FeatureId::GripFingers => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            hands::grip_settings(ui, &mut c.hands);
        },
        FeatureId::PhysicalHands => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            physical_hands::physical_hands_settings(ui, &mut c.physical_hands);
            ui.separator();
            glass::glass_settings(ui, &mut c.glass);
        },
        FeatureId::CharactersDoors => |ui, _| {
            physical_hands::characters_doors_settings(
                ui,
                &mut CONTROLLERS_CONFIG.edit().physical_hands,
            );
        },
        FeatureId::Snatch => {
            |ui, _| snatch::snatch_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons.snatch)
        }
        FeatureId::Feedback => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            haptics::haptics_settings(ui, &mut c.haptics);
            ui.separator();
            physical_hands::impact_sounds_settings(ui, &mut c.physical_hands);
        },
        FeatureId::Hold => {
            |ui, _| weapons::hold_feature_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons)
        }
        FeatureId::Support => |ui, _| weapons::support_settings(ui, &mut CONTROLLERS_CONFIG.edit()),
        FeatureId::Aim => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            aim::aim_settings(ui, &mut c);
            let w = &mut c.weapons;
            scope::scope_settings(ui, &mut w.scopes);
            scope::ads_settings(ui, &mut w.ads);
            scope::collide_settings(ui, &mut w.collide);
        },
        FeatureId::Reload => {
            |ui, _| weapons::reload_feature_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons)
        }
        FeatureId::Inventory => {
            |ui, _| inventory::inventory_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons)
        }
        FeatureId::Explosives => {
            |ui, _| weapons::explosive_feature_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons)
        }
        FeatureId::WeaponSelection => {
            |ui, _| weapon_selection::selection_settings(ui, &mut CONTROLLERS_CONFIG.edit().weapons)
        }
        FeatureId::Climbing => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            climb::climb_settings(ui, &mut c.physical_hands.climb);
            stunt::stunt_settings(ui, &mut c.physical_hands.stunt);
        },
        FeatureId::Vehicles => |ui, id| {
            let mut c = CONTROLLERS_CONFIG.edit();
            vehicles::feature_settings(ui, &mut c.vehicles, id);
            settings::vehicle_chord_settings(ui, &mut c);
        },
        FeatureId::Air => |ui, id| {
            vehicles::feature_settings(ui, &mut CONTROLLERS_CONFIG.edit().vehicles, id);
        },
        FeatureId::Posture => {
            |ui, _| roomscale::roomscale_settings(ui, &mut CONTROLLERS_CONFIG.edit().roomscale)
        }
        FeatureId::BodyFit => |ui, _| {
            let mut c = CONTROLLERS_CONFIG.edit();
            rig::rig_settings(ui, &mut c.body);
            estimator::estimator_settings(ui, &mut c);
        },
        FeatureId::Presets => |ui, _| presets::presets(ui, &mut CONTROLLERS_CONFIG.edit()),
        FeatureId::Semantic => |ui, _| {
            ui.checkbox(
                &mut CONTROLLERS_CONFIG.edit().input.self_check,
                "Input self-check",
            );
        },
        _ => return None,
    };
    Some(body)
}

/// The controller readouts for a feature, when it has any. Readouts take no configuration edit
/// guard.
pub(crate) fn status(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = match id {
        FeatureId::Devices => |ui, _| {
            let status = vr_input::status();
            headlines(ui, &[&INPUT_ACTIONS, &LAYOUT_FINGERPRINT]);
            ui.label(format!("Scheme: {:?}", scheme::current()));
            ui.label(format!("Action set: {:?}", vr_input::active_action_set()));
            if let Some(error) = &status.error {
                ui.label(format!("OpenXR input: unavailable: {error}"));
            }
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
            settings::devices_readout(ui, bindings::recovery_status());
            ui.separator();
            let detection = controller_mode::current();
            let snapshot = controller_mode::last_snapshot();
            let report = bindings::last_report();
            mode::mode_readout(ui, detection, snapshot.as_ref(), &report);
            ui.separator();
            map::map_readout(
                ui,
                &controller_map::report(),
                bindings::pointer::selected_controller(),
                bindings::pointer::owns_menu_trigger(),
            );
        },
        FeatureId::AnimationIk => |ui, _| {
            body_ik::body_ik_readout(ui);
            ui.separator();
            aim_state::aim_state_readout(ui, &controller_aim_state::status());
            unarmed::unarmed_readout(ui, &controller_unarmed::status(), &hand_ik::status());
        },
        FeatureId::Aim => |ui, _| {
            aim::aim_readout(
                ui,
                &controller_aim::status(),
                controller_aim::gun::counters(),
                &gamepad_setting::status(),
            );
            scope::scope_readout(ui, &scope::ScopeReadout::read());
        },
        FeatureId::Presentation => {
            |ui, _| hands::hands_readout(ui, &hand_ik::status(), &tiers::status(), &late::status())
        }
        FeatureId::GripFingers => |ui, _| {
            fingers::fingers_readout(
                ui,
                &controller_fingers::status(),
                vr_input::status().hand_tracking.conforming_motion_range,
            )
        },
        FeatureId::PhysicalHands => |ui, _| {
            headlines(
                ui,
                &[
                    &hands_status::PALMS,
                    &hands_status::PUSH_UP,
                    &hands_status::ARTICULATED,
                ],
            );
            physical_hands::physical_hands_readout(ui, &controller_physical_hands::status());
            glass::glass_readout(ui, &controller_glass::status());
            ui.collapsing("Diagnostics", |ui| {
                hand_bodies::hand_bodies_readout(ui, &controller_physical_hands::physics::status())
            });
        },
        FeatureId::CharactersDoors => |ui, _| {
            headlines(ui, &[&hands_status::DOORS, &hands_status::CHARACTERS]);
            physical_hands::characters_doors_readout(ui, &controller_physical_hands::status());
        },
        FeatureId::Climbing => |ui, _| {
            headlines(
                ui,
                &[&hands_status::MOVING_CLIMBS, &hands_status::STUNT_POSITIONS],
            );
            climb::climb_readout(ui, &controller_physical_hands::climb::status());
            stunt::stunt_readout(ui, &controller_physical_hands::stunt::status());
        },
        FeatureId::Hold => |ui, _| weapons::weapons_readout(ui, &weapons::WeaponsReadout::read()),
        FeatureId::Support | FeatureId::Reload | FeatureId::Explosives => |ui, id| {
            weapons::feature_readout(ui, &weapons::WeaponsReadout::read(), id);
        },
        FeatureId::Grapple => |ui, id| {
            weapons::feature_readout(ui, &weapons::WeaponsReadout::read(), id);
            camera::grapple_comfort_readout(ui);
        },
        FeatureId::Inventory => {
            |ui, _| inventory::inventory_view(ui, &inventory::InventoryView::read())
        }
        FeatureId::Feedback => |ui, _| {
            headlines(ui, &[&feedback::HAPTIC_CUES, &feedback::IMPACT_SOUNDS]);
            haptics::haptics_readout(ui, &controller_haptics::status());
            physical_hands::impact_sounds_readout(ui, &controller_physical_hands::sounds::status());
        },
        FeatureId::Vehicles | FeatureId::Air => |ui, id| {
            vehicles::feature_readout(ui, &vehicles::VehiclesReadout::read(), id);
        },
        FeatureId::Wrist => |ui, _| {
            summary::summary_readout(ui, &controller_summary::status(), &summary_panel::status())
        },
        FeatureId::OnFoot => |ui, _| {
            let stick = controller_locomotion::status();
            let anchor = controller_locomotion::anchor::status();
            locomotion::locomotion_readout(ui, &stick, &anchor);
            ui.separator();
            locomotion::strokes_readout(ui, &controller_locomotion::strokes::status());
        },
        FeatureId::Posture => {
            |ui, _| roomscale::roomscale_readout(ui, &controller_roomscale::status())
        }
        FeatureId::BodyFit => |ui, _| {
            procedural::procedural_readout(ui, &controller_body::procedural::status());
            ui.separator();
            procedural::ik_body_readout(ui);
            ui.separator();
            let root = controller_roomscale::status().root;
            rig::rig_readout(ui, &root);
            ui.separator();
            estimator::estimator_readout(ui, &root);
            ui.separator();
            torso::torso_readout(
                ui,
                &controller_rig::torso_status(),
                &controller_rig::skeleton::status(),
            );
        },
        FeatureId::Semantic => |ui, _| {
            semantic::semantic_readout(ui);
            ui.collapsing("Diagnostics", |ui| {
                frame::live_frame(ui, controller_frame::latest().as_ref())
            });
        },
        _ => return None,
    };
    Some(body)
}

/// The world-space overlay, grab view, and collider view readouts, shown in the global
/// visualization drawer.
pub(crate) fn view_readouts(ui: &mut Ui) {
    overlay::overlay_readout(ui, &overlay_pass::status());
    grab_view::grab_view_readout(ui, &controller_physical_hands::grab_view::status());
    collider_view::collider_view_readout(ui, &controller_physical_hands::collider_view::status());
}

fn vec3_drag(ui: &mut Ui, label: &str, v: &mut Vec3, speed: f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(&mut v.x).speed(speed).prefix("x "));
        ui.add(egui::DragValue::new(&mut v.y).speed(speed).prefix("y "));
        ui.add(egui::DragValue::new(&mut v.z).speed(speed).prefix("z "));
    });
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
