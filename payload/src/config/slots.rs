//! The slot table: the [`ConfigSlot`] for the whole configuration, and the binding of each
//! subsystem's slot to its group in a [`Config`] when the payload registers the configuration
//! ([`crate::config::register`]).
//!
//! Each subsystem declares the slot for its own settings beside their type (`HUD_CONFIG` beside
//! [`HudConfig`](crate::hud::HudConfig)), so it reads them without naming the aggregate. A
//! subsystem that reads a field of a group owned by a subsystem above it declares a slot onto that
//! field alone (`controllers.input` for the VR runtime's input sampling), so it names neither the
//! group's type nor its owner. A slot locks the whole configuration, as [`crate::config::CONFIG`]
//! does, and derefs to its part. Two guards held at once on one thread deadlock; see the
//! [nesting rule](crate::config::engine::store#lock-nesting).

#[cfg(test)]
use std::ptr;

use crate::config::{
    Config,
    engine::{ConfigSlot, projection},
};

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
    crate::stereo::config::STEREO_CONFIG = stereo,
    crate::hooks::graphics_engine::config::EXPOSURE_CONFIG = exposure,
    crate::vr::FOVEATION_CONFIG = foveation,
    crate::far_field::FAR_FIELD_CONFIG = far_field,
    crate::hooks::graphics_engine::config::POST_FX_CONFIG = post_fx,
    crate::hooks::camera::config::CAMERA_CONFIG = camera,
    crate::hooks::input::config::MOVEMENT_CONFIG = movement,
    crate::fsr::FSR_CONFIG = fsr,
    crate::hud::config::HUD_CONFIG = hud,
    crate::headpose::config::HEADPOSE_CONFIG = headpose,
    crate::hooks::character::config::BODY_IK_CONFIG = body_ik,
    crate::vr::VR_CONFIG = vr,
    crate::director::DIRECTOR_CONFIG = director,
    crate::recorder::RECORDER_CONFIG = recorder,
    crate::controllers::config::CONTROLLERS_CONFIG = controllers,
    crate::testing::TESTING_CONFIG = testing,
    crate::hooks::online::ONLINE_CONFIG = online,
    crate::controllers::body::BODY_CONFIG = body,
    // Fields of a higher layer's group that a lower layer reads.
    crate::vr::input::config::INPUT_CONFIG = controllers.input,
    crate::vr::input::config::CONTROLLERS_ENABLED = controllers.enabled,
    crate::vr::input::config::DOMINANT_HAND = controllers.dominant_hand,
    crate::vr::input::config::HAPTICS_ENABLED = controllers.haptics.enabled,
    crate::vr::input::config::HAPTICS_INTENSITY = controllers.haptics.intensity,
    crate::controllers::scheme::CONTROLLERS_ENABLED = controllers.enabled,
    crate::controllers::aim::WORLD_MARKERS_ENABLED = hud.world_markers.enabled,
    crate::vr::PANEL_ENABLED = hud.egui_panel.enabled,
    crate::vr::PANEL_ON_MIRROR = hud.egui_panel.show_on_mirror,
    crate::vr::PANEL_RESOLUTION = hud.egui_panel.resolution,
    crate::debug::texture_census::TEXTURE_CENSUS = testing.texture_census,
    crate::recorder::tracking::TESTING_ENABLED = testing.enabled,
    crate::recorder::tracking::RECORD_HAND_JOINTS = testing.record_hand_joints,
    crate::recorder::tracking::TIME_SCALE = director.time_scale,
    crate::director::clean_view::CLEAN_VIEW_CONFIG = director.clean_view,
    crate::testing::visual::PAUSED = director.paused,
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
