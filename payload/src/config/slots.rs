//! The slot table: the [`ConfigSlot`] for the whole configuration, and the binding of each group's
//! slot to its field of a [`Config`] when the payload registers the configuration
//! ([`crate::config::register`]).
//!
//! Each subsystem declares the slot for its own settings beside their type (`HUD_CONFIG` beside
//! [`HudConfig`](jc3vr_hud::hud::HudConfig)), so it reads them without naming the aggregate. A group
//! that holds groups or settings a lower layer reads binds their slots relative to its own
//! ([`SlotOptions::with_children`](jc3vr_core::config::engine::SlotOptions::with_children)), so
//! this table lists only the aggregate's own fields. A slot locks the whole configuration, as
//! [`crate::config::CONFIG`] does, and derefs to its part. Two guards held at once on one thread
//! deadlock; see the [nesting rule](jc3vr_core::config::engine::store#lock-nesting).

use jc3vr_core::config::engine::{ConfigSlot, projection};

use crate::config::Config;

/// The whole configuration.
pub(crate) static ROOT: ConfigSlot<Config> = ConfigSlot::new("");

macro_rules! slots {
    ($($slot:path = $field:ident),* $(,)?) => {
        /// Bind every slot. Called once, when the payload registers the configuration.
        pub(crate) fn bind() {
            ROOT.bind(projection!(Config));
            $(
                debug_assert_eq!($slot.path(), stringify!($field));
                $slot.bind(projection!(Config, $field));
            )*
        }
    };
}

// A load runs the slots' checks in this order, so it reports the controller settings' resets
// first, then the VR turn's, the HUD's, and the body's.
slots! {
    jc3vr_view::stereo::config::STEREO_CONFIG = stereo,
    jc3vr_render::hooks::graphics_engine::config::EXPOSURE_CONFIG = exposure,
    jc3vr_view::vr::FOVEATION_CONFIG = foveation,
    jc3vr_render::hooks::graphics_engine::config::POST_FX_CONFIG = post_fx,
    jc3vr_view::hooks::camera::config::CAMERA_CONFIG = camera,
    jc3vr_view::fsr::FSR_CONFIG = fsr,
    jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG = controllers,
    jc3vr_view::headpose::config::HEADPOSE_CONFIG = headpose,
    jc3vr_hud::hud::config::HUD_CONFIG = hud,
    jc3vr_view::vr::VR_CONFIG = vr,
    jc3vr_tooling::director::DIRECTOR_CONFIG = director,
    jc3vr_tooling::recorder::RECORDER_CONFIG = recorder,
    jc3vr_tooling::testing::TESTING_CONFIG = testing,
    jc3vr_core::hooks::online::ONLINE_CONFIG = online,
    jc3vr_body::controllers::body::BODY_CONFIG = body,
}

#[cfg(test)]
mod tests {
    use jc3vr_core::config::engine::mismatched_defaults;

    use crate::config::Config;

    /// A crate's test root defaults each listed slot's part by the slot's declared default or its
    /// type's, so each must be what the aggregate's defaults hold there.
    #[test]
    fn every_slot_defaults_to_the_aggregates_default() {
        crate::config::register();
        assert_eq!(mismatched_defaults(&Config::new()), Vec::<&str>::new());
    }
}
