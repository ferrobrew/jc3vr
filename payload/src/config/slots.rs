//! The slot table: the [`ConfigSlot`] for the whole configuration, and the binding of each
//! subsystem's slot to its group in a [`Config`] when the payload registers the configuration
//! ([`crate::config::register`]).
//!
//! Each subsystem declares the slot for its own settings beside their type (`HUD_CONFIG` beside
//! [`HudConfig`](crate::hud::HudConfig)), so it reads them without naming the aggregate. A slot
//! locks the whole configuration, as [`crate::config::CONFIG`] does, and derefs to its group. Two
//! guards held at once on one thread deadlock; see the
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
    ($($slot:path = $field:ident),* $(,)?) => {
        /// Bind every slot. Called once, when the payload registers the configuration.
        pub(crate) fn bind() {
            ROOT.bind(projection!(Config));
            $($slot.bind(projection!(Config, $field));)*
        }

        /// Check that each slot is bound to the field it is listed with, and that its path is
        /// that field's key in `groups`, the configuration's top-level table.
        #[cfg(test)]
        fn check_projections(config: &mut Config, groups: &toml::Table) {
            let root: *const Config = ROOT.project_mut(config);
            assert!(ptr::eq(root, config));
            $(
                let projected: *const _ = $slot.project_mut(config);
                assert!(
                    ptr::eq(projected, &config.$field),
                    "the `{}` slot projects another field",
                    stringify!($field)
                );
                assert_eq!($slot.path(), stringify!($field));
                assert!(groups.contains_key($slot.path()), "{}", $slot.path());
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slot_projects_the_field_it_is_named_after() {
        crate::config::register();
        let groups: toml::Table =
            toml::from_str(&toml::to_string(&Config::new()).unwrap()).unwrap();
        check_projections(&mut Config::new(), &groups);
    }
}
