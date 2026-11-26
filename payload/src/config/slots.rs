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
    jc3vr_render::stereo::config::STEREO_CONFIG = stereo,
    jc3vr_render::hooks::graphics_engine::config::EXPOSURE_CONFIG = exposure,
    jc3vr_render::vr::FOVEATION_CONFIG = foveation,
    jc3vr_render::far_field::FAR_FIELD_CONFIG = far_field,
    jc3vr_render::hooks::graphics_engine::config::POST_FX_CONFIG = post_fx,
    jc3vr_render::hooks::camera::config::CAMERA_CONFIG = camera,
    jc3vr_gameplay::hooks::input::config::MOVEMENT_CONFIG = movement,
    jc3vr_render::fsr::FSR_CONFIG = fsr,
    jc3vr_hud::hud::config::HUD_CONFIG = hud,
    jc3vr_render::headpose::config::HEADPOSE_CONFIG = headpose,
    jc3vr_gameplay::hooks::character::config::BODY_IK_CONFIG = body_ik,
    jc3vr_render::vr::VR_CONFIG = vr,
    jc3vr_tooling::director::DIRECTOR_CONFIG = director,
    jc3vr_tooling::recorder::RECORDER_CONFIG = recorder,
    jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG = controllers,
    jc3vr_tooling::testing::TESTING_CONFIG = testing,
    jc3vr_core::hooks::online::ONLINE_CONFIG = online,
    jc3vr_gameplay::controllers::body::BODY_CONFIG = body,
    // Fields of a higher layer's group that a lower layer reads.
    jc3vr_render::vr::input::config::INPUT_CONFIG = controllers.input,
    jc3vr_render::vr::input::config::CONTROLLERS_ENABLED = controllers.enabled,
    jc3vr_render::vr::input::config::DOMINANT_HAND = controllers.dominant_hand,
    jc3vr_render::vr::input::config::HAPTICS_ENABLED = controllers.haptics.enabled,
    jc3vr_render::vr::input::config::HAPTICS_INTENSITY = controllers.haptics.intensity,
    jc3vr_gameplay::controllers::scheme::CONTROLLERS_ENABLED = controllers.enabled,
    jc3vr_gameplay::controllers::aim::WORLD_MARKERS_ENABLED = hud.world_markers.enabled,
    jc3vr_render::vr::PANEL_ENABLED = hud.egui_panel.enabled,
    jc3vr_render::vr::PANEL_ON_MIRROR = hud.egui_panel.show_on_mirror,
    jc3vr_render::vr::PANEL_RESOLUTION = hud.egui_panel.resolution,
    jc3vr_render::debug::texture_census::TEXTURE_CENSUS = testing.texture_census,
    jc3vr_render::recorder::tracking::TESTING_ENABLED = testing.enabled,
    jc3vr_render::recorder::tracking::RECORD_HAND_JOINTS = testing.record_hand_joints,
    jc3vr_render::recorder::tracking::TIME_SCALE = director.time_scale,
    jc3vr_core::director::clean_view::CLEAN_VIEW_CONFIG = director.clean_view,
    jc3vr_render::testing::visual::PAUSED = director.paused,
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
