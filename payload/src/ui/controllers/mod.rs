//! Feature-local controller editors and readouts for the debug UI.

mod aim;
mod aim_state;
mod body_ik;
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
        haptics as controller_haptics, locomotion as controller_locomotion, map as controller_map,
        mode as controller_mode, overlay as controller_overlay,
        physical_hands::{self as controller_physical_hands, collider_view::ColliderViewConfig},
        roomscale as controller_roomscale, scheme, summary as controller_summary,
    },
    hud::{pointer, summary_panel},
    ui::{camera, catalog::FeatureId, dispatch::FeatureBody},
    vr::input::{self as vr_input, ActionsOrigin},
};

/// The controller editors for a feature, when it has any. Each editor takes the configuration edit
/// guard itself, after reading any snapshot providers that read configuration.
pub(crate) fn controls(id: FeatureId) -> Option<FeatureBody> {
    let body: FeatureBody = match id {
        FeatureId::ControllerCapture => |ui, _| diagnostics::diagnostics(ui),
        FeatureId::Devices => |ui, _| settings::devices(ui, &mut config::edit()),
        FeatureId::OnFoot => |ui, _| settings::locomotion_settings(ui, &mut config::edit()),
        FeatureId::Grapple => |ui, _| {
            let mut cfg = config::edit();
            settings::grapple_settings(ui, &mut cfg);
            hands::anchor_settings(ui, &mut cfg.controllers.hands);
        },
        FeatureId::AnimationIk => |ui, _| settings::animation_settings(ui, &mut config::edit()),
        FeatureId::Presentation => {
            |ui, _| hands::presentation_settings(ui, &mut config::edit().controllers.hands)
        }
        FeatureId::GripFingers => |ui, _| {
            let mut cfg = config::edit();
            hands::grip_settings(ui, &mut cfg.controllers.hands);
            fingers::fingers_settings(ui, &mut cfg.controllers.fingers);
        },
        FeatureId::PhysicalHands => |ui, _| {
            physical_hands::physical_hands_settings(
                ui,
                &mut config::edit().controllers.physical_hands,
            );
        },
        FeatureId::CharactersDoors => |ui, _| {
            physical_hands::characters_doors_settings(
                ui,
                &mut config::edit().controllers.physical_hands,
            );
        },
        FeatureId::Snatch => {
            |ui, _| snatch::snatch_settings(ui, &mut config::edit().controllers.weapons.snatch)
        }
        FeatureId::Feedback => |ui, _| {
            let mut cfg = config::edit();
            haptics::haptics_settings(ui, &mut cfg.controllers.haptics);
            ui.separator();
            physical_hands::impact_sounds_settings(ui, &mut cfg.controllers.physical_hands);
        },
        FeatureId::Hold => {
            |ui, _| weapons::hold_feature_settings(ui, &mut config::edit().controllers.weapons)
        }
        FeatureId::Support => {
            |ui, _| weapons::support_settings(ui, &mut config::edit().controllers)
        }
        FeatureId::Aim => |ui, _| {
            let mut cfg = config::edit();
            aim::aim_settings(ui, &mut cfg.controllers);
            let w = &mut cfg.controllers.weapons;
            scope::scope_settings(ui, &mut w.scopes);
            scope::ads_settings(ui, &mut w.ads);
            scope::collide_settings(ui, &mut w.collide);
        },
        FeatureId::Reload => {
            |ui, _| weapons::reload_feature_settings(ui, &mut config::edit().controllers.weapons)
        }
        FeatureId::Inventory => {
            |ui, _| inventory::inventory_settings(ui, &mut config::edit().controllers.weapons)
        }
        FeatureId::Explosives => {
            |ui, _| weapons::explosive_feature_settings(ui, &mut config::edit().controllers.weapons)
        }
        FeatureId::WeaponSelection => |ui, _| {
            weapon_selection::selection_settings(ui, &mut config::edit().controllers.weapons)
        },
        FeatureId::Climbing => |ui, _| {
            let mut cfg = config::edit();
            climb::climb_settings(ui, &mut cfg.controllers.physical_hands.climb);
            stunt::stunt_settings(ui, &mut cfg.controllers.physical_hands.stunt);
        },
        FeatureId::Vehicles | FeatureId::Air => |ui, id| {
            vehicles::feature_settings(ui, &mut config::edit().controllers.vehicles, id);
        },
        FeatureId::Posture => {
            |ui, _| roomscale::roomscale_settings(ui, &mut config::edit().controllers.roomscale)
        }
        FeatureId::BodyFit => |ui, _| rig::rig_settings(ui, &mut config::edit().controllers.body),
        FeatureId::Wrist => {
            |ui, _| summary::summary_settings(ui, &mut config::edit().controllers.summary)
        }
        FeatureId::Presets => |ui, _| presets::presets(ui, &mut config::edit()),
        FeatureId::Semantic => |ui, _| {
            ui.checkbox(
                &mut config::edit().controllers.input.self_check,
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
            ui.label(format!("Scheme: {:?}", scheme::current()));
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
                pointer::selected_controller(),
                pointer::owns_menu_trigger(),
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
            physical_hands::physical_hands_readout(
                ui,
                &controller_physical_hands::status(),
                &controller_physical_hands::vault::status(),
            );
            ui.collapsing("Diagnostics", |ui| {
                hand_bodies::hand_bodies_readout(ui, &controller_physical_hands::physics::status())
            });
        },
        FeatureId::CharactersDoors => |ui, _| {
            physical_hands::characters_doors_readout(ui, &controller_physical_hands::status());
        },
        FeatureId::Climbing => |ui, _| {
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
            let acts = controller_locomotion::acts::status();
            let anchor = controller_locomotion::anchor::status();
            locomotion::locomotion_readout(ui, &stick, &acts, &anchor);
        },
        FeatureId::Posture => {
            |ui, _| roomscale::roomscale_readout(ui, &controller_roomscale::status())
        }
        FeatureId::BodyFit => |ui, _| {
            procedural::procedural_readout(ui, &controller_body::procedural::status());
            ui.separator();
            rig::rig_readout(ui, &controller_roomscale::status().root);
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

/// What the collider view draws, edited by the global visualization drawer on a copy of the saved
/// configuration.
pub(crate) fn collider_view_settings(ui: &mut Ui, collider: &mut ColliderViewConfig) {
    collider_view::collider_view_settings(ui, collider);
}

/// The world-space overlay, grab view, and collider view readouts, shown in the global
/// visualization drawer.
pub(crate) fn view_readouts(ui: &mut Ui) {
    overlay::overlay_readout(ui, &controller_overlay::status());
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
