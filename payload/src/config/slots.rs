//! The slot table: a [`ConfigSlot`] for the whole configuration and one for each of its groups,
//! bound to their places in a [`Config`] when the payload registers the configuration
//! ([`crate::config::register`]).
//!
//! A slot locks the whole configuration, as [`crate::config::CONFIG`] does, and derefs to its
//! group, so a subsystem can read its own settings without naming the aggregate. Two guards held
//! at once on one thread deadlock; see the
//! [nesting rule](crate::config::engine::store#lock-nesting).

#[cfg(test)]
use std::ptr;

use crate::{
    config::{
        Config,
        engine::{ConfigSlot, projection},
    },
    controllers::{ControllersConfig, body::BodyDimensions},
    director::DirectorConfig,
    far_field::FarFieldConfig,
    fsr::FsrConfig,
    headpose::HeadPoseConfig,
    hooks::{
        camera::CameraConfig,
        character::BodyIkConfig,
        graphics_engine::config::{ExposureConfig, PostFxConfig},
        input::MovementConfig,
        online::OnlineConfig,
    },
    hud::HudConfig,
    recorder::RecorderConfig,
    stereo::config::StereoConfig,
    testing::TestingConfig,
    vr::{FoveationConfig, VrConfig},
};

/// The whole configuration.
pub(crate) static ROOT: ConfigSlot<Config> = ConfigSlot::new("");

macro_rules! slots {
    ($($name:ident: $ty:ty = $field:ident),* $(,)?) => {
        $(
            #[doc = concat!("The `", stringify!($field), "` group.")]
            pub(crate) static $name: ConfigSlot<$ty> = ConfigSlot::new(stringify!($field));
        )*

        /// Bind every slot. Called once, when the payload registers the configuration.
        pub(crate) fn bind() {
            ROOT.bind(projection!(Config));
            $($name.bind(projection!(Config, $field));)*
        }

        /// Check that each slot is bound to the field it is named after, and that its path is
        /// that field's key in `groups`, the configuration's top-level table.
        #[cfg(test)]
        fn check_projections(config: &mut Config, groups: &toml::Table) {
            let root: *const Config = ROOT.project_mut(config);
            assert!(ptr::eq(root, config));
            $(
                let projected: *const $ty = $name.project_mut(config);
                assert!(
                    ptr::eq(projected, &config.$field),
                    "the `{}` slot projects another field",
                    stringify!($field)
                );
                assert!(groups.contains_key($name.path()), "{}", $name.path());
            )*
        }
    };
}

slots! {
    STEREO: StereoConfig = stereo,
    EXPOSURE: ExposureConfig = exposure,
    FOVEATION: FoveationConfig = foveation,
    FAR_FIELD: FarFieldConfig = far_field,
    POST_FX: PostFxConfig = post_fx,
    CAMERA: CameraConfig = camera,
    MOVEMENT: MovementConfig = movement,
    FSR: FsrConfig = fsr,
    HUD: HudConfig = hud,
    HEADPOSE: HeadPoseConfig = headpose,
    BODY_IK: BodyIkConfig = body_ik,
    VR: VrConfig = vr,
    DIRECTOR: DirectorConfig = director,
    RECORDER: RecorderConfig = recorder,
    CONTROLLERS: ControllersConfig = controllers,
    TESTING: TestingConfig = testing,
    ONLINE: OnlineConfig = online,
    BODY: BodyDimensions = body,
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
