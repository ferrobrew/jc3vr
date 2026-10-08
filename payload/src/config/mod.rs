//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads and writes the whole struct; hooks copy out the
//! field(s) they need at the top of a detour. Live engine-interface state (the current eye, frame
//! counters, the trace arm-flag) does not live here; see [`crate::stereo::StereoState`] and the
//! per-subsystem runtime statics. A field that is configuration-shaped but holds state for the
//! session (a value discovered at runtime, a diagnostic armed for one capture) is `#[serde(skip)]`.
//!
//! The configuration is saved to `settings.toml` in the per-user configuration directory
//! ([`paths`]). It is loaded once at startup, before VR initialization reads it ([`init`]); the
//! load is tolerant ([`load`]), so a file from another build costs at most the settings it gets
//! wrong. A change the user meant goes through [`edit`], which marks the configuration changed;
//! [`tick`] saves it from the game thread's frame once the changes settle, handing the write to a
//! short-lived writer thread, and [`shutdown`] flushes a pending save. A change the payload makes on
//! its own, such as a fallback after a failure, takes the plain [`CONFIG`] lock instead and is not
//! saved for its own sake; one that switches off a setting the user chose goes through
//! [`disable_automatically`], so a later save keeps the user's choice. See
//! `docs/mod/settings.md`.
//!
//! TOML has no null, so a `None` is left out of the file. An optional setting defaults to `None`:
//! the load then reads a missing key as `None` and a present one as `Some`. A setting that is on by
//! default with a value is a switch beside the value instead (see
//! [`FsrConfig::sharpen`](crate::fsr::FsrConfig::sharpen)).
//!
//! The trace manifest and screenshot sidecar also serialize the capture's configuration.
//!
//! The sub-structs are defined in their owning modules:
//! [`StereoConfig`](crate::stereo::config::StereoConfig) and
//! [`SinglePassConfig`](crate::stereo::config::SinglePassConfig) in `stereo::config`,
//! [`ExposureConfig`](crate::hooks::graphics_engine::config::ExposureConfig) and
//! [`PostFxConfig`](crate::hooks::graphics_engine::config::PostFxConfig) in
//! `hooks::graphics_engine::config`,
//! [`FarFieldConfig`](crate::far_field::FarFieldConfig) in `far_field`,
//! [`FoveationConfig`](crate::vr::FoveationConfig) in `vr::config`,
//! [`CameraConfig`](crate::hooks::camera::CameraConfig) in `hooks::camera::config`,
//! [`BodyIkConfig`](crate::hooks::character::BodyIkConfig) in `hooks::character::config`,
//! [`MovementConfig`](crate::hooks::input::MovementConfig) in `hooks::input::config`,
//! [`FsrConfig`](crate::fsr::FsrConfig) in `fsr::config`,
//! [`HudConfig`](crate::hud::HudConfig) in `hud::config`,
//! [`HeadPoseConfig`](crate::headpose::HeadPoseConfig) in `headpose::config`,
//! [`VrConfig`](crate::vr::VrConfig) in `vr::config`, and
//! [`DirectorConfig`](crate::director::DirectorConfig) in `director`,
//! [`RecorderConfig`](crate::recorder::RecorderConfig) in `recorder`,
//! [`ControllersConfig`](crate::controllers::ControllersConfig) in `controllers::config`,
//! [`PauseMenuConfig`](crate::pause_menu::PauseMenuConfig) in `pause_menu::config`,
//! [`TestingConfig`](crate::testing::TestingConfig) in `testing::config`, and
//! [`BodyDimensions`](crate::controllers::body::BodyDimensions) in `controllers::body`.

mod automatic;
pub(crate) mod load;
pub(crate) mod paths;
mod persist;
pub(crate) mod validate;
pub(crate) mod write;

use std::ops::{Deref, DerefMut};

pub(crate) use automatic::Switch;
use parking_lot::{Mutex, MutexGuard};
pub(crate) use persist::{SaveStatus, init, shutdown, status, tick};
use serde::{Deserialize, Serialize};

use crate::{
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
    },
    hud::HudConfig,
    pause_menu::PauseMenuConfig,
    recorder::RecorderConfig,
    stereo::config::StereoConfig,
    testing::TestingConfig,
    vr::{FoveationConfig, VrConfig},
};

/// The global runtime configuration. Cheap to lock (uncontended `parking_lot::Mutex`); read it at the
/// top of a hook and release before doing engine work.
///
/// Lock it directly to read, and for a change the payload makes on its own (a fallback, a value
/// discovered at runtime). A change the user meant (a debug UI control, a pause menu item, a
/// hotkey, a calibration) goes through [`edit`] instead, so that it is saved.
pub static CONFIG: Mutex<Config> = Mutex::new(Config::new());

/// Snapshot the whole config (for the trace manifest / bulk UI reads).
pub fn get() -> Config {
    CONFIG.lock().clone()
}

/// Lock the configuration for a change the user meant. The guard derefs to the [`Config`]; when it
/// drops, the configuration is marked changed and [`tick`] saves it once the changes settle.
/// Holding the guard through a frame of debug UI without changing anything is harmless: the save
/// compares the serialized text with the file's and writes nothing when they match.
///
/// Like a plain lock of [`CONFIG`], it must not be held while locking the VR runtime state.
pub(crate) fn edit() -> ConfigEdit {
    let mut guard = CONFIG.lock();
    let before = automatic::observe(&mut guard);
    ConfigEdit { guard, before }
}

/// Switch `switch` off because the payload cannot keep it on (a fallback after a failure, or a
/// teardown), without saving that as the user's choice: the live configuration changes at once,
/// while a save keeps the value the user chose until the user changes the switch through [`edit`].
pub(crate) fn disable_automatically(switch: Switch) {
    let mut config = CONFIG.lock();
    automatic::disable(&mut config, switch);
}

/// The guard [`edit`] returns.
pub(crate) struct ConfigEdit {
    guard: MutexGuard<'static, Config>,
    before: automatic::Observed,
}

impl Deref for ConfigEdit {
    type Target = Config;

    fn deref(&self) -> &Config {
        &self.guard
    }
}

impl DerefMut for ConfigEdit {
    fn deref_mut(&mut self) -> &mut Config {
        &mut self.guard
    }
}

impl Drop for ConfigEdit {
    fn drop(&mut self) {
        automatic::settle(&mut self.guard, &self.before);
        persist::note_edit();
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub stereo: StereoConfig,
    pub exposure: ExposureConfig,
    #[serde(default)]
    pub foveation: FoveationConfig,
    #[serde(default)]
    pub far_field: FarFieldConfig,
    pub post_fx: PostFxConfig,
    pub camera: CameraConfig,
    pub movement: MovementConfig,
    pub fsr: FsrConfig,
    pub hud: HudConfig,
    pub headpose: HeadPoseConfig,
    pub body_ik: BodyIkConfig,
    pub vr: VrConfig,
    pub director: DirectorConfig,
    pub recorder: RecorderConfig,
    #[serde(default)]
    pub controllers: ControllersConfig,
    #[serde(default)]
    pub pause_menu: PauseMenuConfig,
    #[serde(default)]
    pub testing: TestingConfig,
    /// The player's body dimensions; see [`crate::controllers::body`].
    pub body: BodyDimensions,
}
impl Config {
    pub const fn new() -> Self {
        Self {
            stereo: StereoConfig::new(),
            exposure: ExposureConfig::new(),
            foveation: FoveationConfig::new(),
            far_field: FarFieldConfig::new(),
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            movement: MovementConfig::new(),
            fsr: FsrConfig::new(),
            hud: HudConfig::new(),
            headpose: HeadPoseConfig::new(),
            body_ik: BodyIkConfig::new(),
            vr: VrConfig::new(),
            director: DirectorConfig::new(),
            recorder: RecorderConfig::new(),
            controllers: ControllersConfig::new(),
            pause_menu: PauseMenuConfig::new(),
            testing: TestingConfig::new(),
            body: BodyDimensions::GENERIC_ADULT,
        }
    }

    /// Lock the global config, run `f` against it, and return the result -- the terse read path for
    /// hooks: `Config::lock_query(|c| c.post_fx.skip_sun_halo)`. The lock is held only for `f`.
    pub fn lock_query<R>(f: impl FnOnce(&Config) -> R) -> R {
        f(&CONFIG.lock())
    }
}

#[cfg(test)]
mod tests;
