//! The slot table: the [`ConfigSlot`] for the whole configuration, and the binding of each
//! subsystem's slot to its group in a [`Config`] when the payload registers the configuration
//! ([`crate::config::register`]).
//!
//! Each subsystem declares the slot for its own settings beside their type (`HUD_CONFIG` beside
//! [`HudConfig`](jc3vr_hud::hud::HudConfig)), so it reads them without naming the aggregate. A
//! subsystem that reads a field of a group owned by a subsystem above it declares a slot onto that
//! field alone (`controllers.input` for the VR runtime's input sampling), so it names neither the
//! group's type nor its owner. A slot locks the whole configuration, as [`crate::config::CONFIG`]
//! does, and derefs to its part. Two guards held at once on one thread deadlock; see the
//! [nesting rule](jc3vr_core::config::engine::store#lock-nesting).

#[cfg(test)]
use std::ptr;

use jc3vr_core::config::engine::{ConfigSlot, projection};

use crate::config::Config;

/// The whole configuration.
pub(crate) static ROOT: ConfigSlot<Config> = ConfigSlot::new("");

macro_rules! slots {
    ($($slot:path = $($field:ident).+),* $(,)?) => {
        /// Bind every slot. Called once, when the payload registers the configuration.
        pub(crate) fn bind() {
            ROOT.bind(projection!(Config));
            $($slot.bind(projection!(Config, $($field).+));)*
        }

        /// Check that each slot is bound to the field it is listed with, that its path names that
        /// field, and that a group's path is a key of `settings`, the configuration's table.
        #[cfg(test)]
        fn check_projections(config: &mut Config, settings: &toml::Table) {
            let root: *const Config = ROOT.project_mut(config);
            assert!(ptr::eq(root, config));
            $(
                let projected: *const _ = $slot.project_mut(config);
                let path = [$(stringify!($field)),+].join(".");
                assert!(
                    ptr::eq(projected, &config.$($field).+),
                    "the `{path}` slot projects another field"
                );
                assert_eq!($slot.path(), path);
                // A nested field may be session state the file leaves out (`#[serde(skip)]`).
                assert!(path.contains('.') || settings.contains_key(&path), "{path}");
            )*
        }
    };
}

slots! {
    jc3vr_view::stereo::config::STEREO_CONFIG = stereo,
    jc3vr_render::hooks::graphics_engine::config::EXPOSURE_CONFIG = exposure,
    jc3vr_view::vr::FOVEATION_CONFIG = foveation,
    jc3vr_render::hooks::graphics_engine::config::POST_FX_CONFIG = post_fx,
    jc3vr_view::hooks::camera::config::CAMERA_CONFIG = camera,
    jc3vr_view::fsr::FSR_CONFIG = fsr,
    jc3vr_hud::hud::config::HUD_CONFIG = hud,
    jc3vr_view::headpose::config::HEADPOSE_CONFIG = headpose,
    jc3vr_view::vr::VR_CONFIG = vr,
    jc3vr_tooling::director::DIRECTOR_CONFIG = director,
    jc3vr_tooling::recorder::RECORDER_CONFIG = recorder,
    jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG = controllers,
    jc3vr_tooling::testing::TESTING_CONFIG = testing,
    jc3vr_core::hooks::online::ONLINE_CONFIG = online,
    jc3vr_body::controllers::body::BODY_CONFIG = body,
    // The controllers' groups, each declared by the subsystem that owns it.
    jc3vr_body::controllers::scheme::DOMINANT_HAND = controllers.dominant_hand,
    jc3vr_body::controllers::locomotion::config::LOCOMOTION_CONFIG = controllers.locomotion,
    jc3vr_body::controllers::intent::config::BINDINGS_CONFIG = controllers.bindings,
    jc3vr_body::controllers::aim_state::AIM_STATE_CONFIG = controllers.aim_state,
    jc3vr_body::controllers::hands::config::HANDS_CONFIG = controllers.hands,
    jc3vr_body::controllers::aim::config::AIM_CONFIG = controllers.aim,
    jc3vr_body::controllers::overlay::config::OVERLAY_CONFIG = controllers.overlay,
    jc3vr_weapons::controllers::weapons::config::WEAPONS_CONFIG = controllers.weapons,
    jc3vr_body::controllers::body::config::BODY_REMAP_CONFIG = controllers.body,
    jc3vr_body::controllers::roomscale::config::ROOMSCALE_CONFIG = controllers.roomscale,
    jc3vr_vehicles::controllers::vehicles::config::VEHICLES_CONFIG = controllers.vehicles,
    jc3vr_body::controllers::fingers::config::FINGERS_CONFIG = controllers.fingers,
    jc3vr_physical::controllers::physical_hands::config::PHYSICAL_HANDS_CONFIG = controllers.physical_hands,
    jc3vr_gameplay::controllers::summary::config::SUMMARY_CONFIG = controllers.summary,
    jc3vr_grapple::grapple::physical::config::GRAPPLE_CONFIG = controllers.grapple,
    jc3vr_body::controllers::haptics::config::HAPTICS_CONFIG = controllers.haptics,
    jc3vr_physical::controllers::glass::config::GLASS_CONFIG = controllers.glass,
    // Fields of a controllers group that a subsystem below the group's owner reads.
    jc3vr_body::controllers::hands::config::MOUNTED_AIM_BY_HANDS = controllers.vehicles.mounted.aim_by_hands,
    jc3vr_body::controllers::hands::config::GRAPPLE_PUMP = controllers.grapple.pump,
    jc3vr_body::controllers::roomscale::config::DRIVING_VIEW_ENABLED = controllers.vehicles.driving_view.enabled,
    jc3vr_body::controllers::roomscale::config::TAKE_OFF_TO_HEAD = controllers.vehicles.take_off_to_head,
    jc3vr_body::controllers::aim::GRAPPLE_SEAT = controllers.grapple.seat,
    jc3vr_body::hooks::input::parachute::CANOPY_LINE_TURN = controllers.vehicles.canopy.line_turn,
    jc3vr_body::hooks::input::parachute::CANOPY_LINE_TURN_SCALE = controllers.vehicles.canopy.line_turn_scale,
    jc3vr_body::hooks::animation::canopy::SKIP_REEL_OPENING = controllers.vehicles.canopy.skip_reel_opening,
    jc3vr_body::hooks::input::wingsuit::WINGSUIT_EVADE_ENABLED = controllers.vehicles.wingsuit_evade.enabled,
    // Fields of a higher layer's group that a lower layer reads.
    jc3vr_view::vr::input::config::INPUT_CONFIG = controllers.input,
    jc3vr_view::vr::input::config::CONTROLLERS_ENABLED = controllers.enabled,
    jc3vr_view::vr::input::config::DOMINANT_HAND = controllers.dominant_hand,
    jc3vr_view::vr::input::config::HAPTICS_ENABLED = controllers.haptics.enabled,
    jc3vr_view::vr::input::config::HAPTICS_INTENSITY = controllers.haptics.intensity,
    jc3vr_body::controllers::scheme::CONTROLLERS_ENABLED = controllers.enabled,
    jc3vr_body::controllers::aim::WORLD_MARKERS_ENABLED = hud.world_markers.enabled,
    jc3vr_view::vr::PANEL_ENABLED = hud.egui_panel.enabled,
    jc3vr_view::vr::PANEL_ON_MIRROR = hud.egui_panel.show_on_mirror,
    jc3vr_view::vr::PANEL_RESOLUTION = hud.egui_panel.resolution,
    jc3vr_render::debug::texture_census::TEXTURE_CENSUS = testing.texture_census,
    jc3vr_view::recorder::tracking::TESTING_ENABLED = testing.enabled,
    jc3vr_view::recorder::tracking::RECORD_HAND_JOINTS = testing.record_hand_joints,
    jc3vr_view::recorder::tracking::TIME_SCALE = director.time_scale,
    jc3vr_core::director::clean_view::CLEAN_VIEW_CONFIG = director.clean_view,
    jc3vr_capture::testing::visual::PAUSED = director.paused,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_projects_the_field_it_is_named_after() {
        crate::config::register();
        let settings: toml::Table =
            toml::from_str(&toml::to_string(&Config::new()).unwrap()).unwrap();
        check_projections(&mut Config::new(), &settings);
    }
}
