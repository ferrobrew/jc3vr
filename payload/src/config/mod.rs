//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads and writes the whole struct; hooks copy out the
//! field(s) they need at the top of a detour. Live engine-interface state (the current eye, frame
//! counters, the trace arm-flag) does not live here; see [`jc3vr_view::stereo::StereoState`] and the
//! per-subsystem runtime statics. A field that is configuration-shaped but holds state for the
//! session (a value discovered at runtime, a diagnostic armed for one capture) is `#[serde(skip)]`.
//!
//! The store, the load, and the save are the configuration engine ([`engine`]), which names no
//! subsystem: it holds the configuration as an erased root and works through the
//! [`Schema`](engine::Schema) this module registers for [`Config`] ([`schema`]). Each subsystem
//! reads and changes its own settings through the [`ConfigSlot`](engine::ConfigSlot) it declares
//! beside their type; this module owns the aggregate, binds every slot to its place in it
//! ([`slots`]), and keeps the access paths over the whole configuration ([`CONFIG`] and [`get`]),
//! each of which registers the configuration on first use ([`register`]).
//!
//! The configuration is saved to `settings.toml` in the per-user configuration directory
//! ([`paths`]), which holds only the settings that differ from their defaults ([`format`]), so a
//! setting the user never changed follows its default when a later build changes it. It is loaded
//! once at startup, before VR initialization reads it ([`init`]); the load is tolerant ([`load`]),
//! so a file from another build costs at most the settings it gets wrong. A change the user meant
//! goes through an edit guard (a slot's `edit`), which marks the configuration
//! changed; [`tick`] saves it from the game thread's frame once the changes settle, handing the
//! write to a short-lived writer thread, and [`shutdown`] flushes a pending save. A change the
//! payload makes on its own, such as a fallback after a failure, takes a plain lock instead and is
//! not saved for its own sake; one that switches off a setting the user chose goes through
//! [`engine::disable_automatically`], so a later save keeps the user's choice. Every guard locks
//! the one configuration lock, so a thread must not lock it again while it holds one (the
//! [nesting rule](jc3vr_core::config::engine::store#lock-nesting)). See `docs/mod/settings.md`.
//!
//! TOML has no null, so a `None` is left out of the file. An optional setting defaults to `None`:
//! the load then reads a missing key as `None` and a present one as `Some`. A setting whose value is
//! kept while it is switched off is a switch beside the value instead (see
//! [`FsrConfig::sharpen`](jc3vr_view::fsr::FsrConfig::sharpen)).
//!
//! The trace manifest and screenshot sidecar also serialize the capture's configuration.
//!
//! The sub-structs are defined in their owning modules:
//! [`StereoConfig`](jc3vr_view::stereo::config::StereoConfig) and
//! [`SinglePassConfig`](jc3vr_view::stereo::config::SinglePassConfig) in `stereo::config`,
//! [`ExposureConfig`](jc3vr_render::hooks::graphics_engine::config::ExposureConfig) and
//! [`PostFxConfig`](jc3vr_render::hooks::graphics_engine::config::PostFxConfig) in
//! `hooks::graphics_engine::config`,
//! [`FarFieldConfig`](jc3vr_view::far_field::FarFieldConfig) in `far_field`,
//! [`FoveationConfig`](jc3vr_view::vr::FoveationConfig) in `vr::config`,
//! [`CameraConfig`](jc3vr_view::hooks::camera::CameraConfig) in `hooks::camera::config`,
//! [`BodyIkConfig`](jc3vr_body::hooks::character::BodyIkConfig) in `hooks::character::config`,
//! [`MovementConfig`](jc3vr_body::hooks::input::MovementConfig) in `hooks::input::config`,
//! [`OnlineConfig`](jc3vr_core::hooks::online::OnlineConfig) in `hooks::online`,
//! [`FsrConfig`](jc3vr_view::fsr::FsrConfig) in `fsr::config`,
//! [`HudConfig`](jc3vr_hud::hud::HudConfig) in `hud::config`,
//! [`HeadPoseConfig`](jc3vr_view::headpose::HeadPoseConfig) in `headpose::config`,
//! [`VrConfig`](jc3vr_view::vr::VrConfig) in `vr::config`, and
//! [`DirectorConfig`](jc3vr_tooling::director::DirectorConfig) in `director`,
//! [`RecorderConfig`](jc3vr_tooling::recorder::RecorderConfig) in `recorder`,
//! [`ControllersConfig`](jc3vr_gameplay::controllers::ControllersConfig) in `controllers::config`,
//! [`TestingConfig`](jc3vr_tooling::testing::TestingConfig) in `testing::config`, and
//! [`BodyDimensions`](jc3vr_body::controllers::body::BodyDimensions) in `controllers::body`.

pub(crate) mod format;
pub(crate) mod load;
mod schema;
pub(crate) mod slots;

use std::sync::Once;

use jc3vr_body::{
    controllers::body::BodyDimensions,
    hooks::{character::BodyIkConfig, input::MovementConfig},
};
use jc3vr_core::{
    config::{engine, engine::SlotGuard},
    hooks::online::OnlineConfig,
};
use jc3vr_gameplay::controllers::ControllersConfig;
use jc3vr_hud::hud::HudConfig;
use jc3vr_render::hooks::graphics_engine::config::{ExposureConfig, PostFxConfig};
use jc3vr_tooling::{director::DirectorConfig, recorder::RecorderConfig, testing::TestingConfig};
use jc3vr_view::{
    far_field::FarFieldConfig,
    fsr::FsrConfig,
    headpose::HeadPoseConfig,
    hooks::camera::CameraConfig,
    stereo::config::StereoConfig,
    vr::{FoveationConfig, VrConfig},
};
use serde::{Deserialize, Serialize};

/// The global runtime configuration. Cheap to lock (an uncontended `parking_lot::Mutex`); read it
/// at the top of a hook and release before doing engine work.
///
/// Lock it directly to read, and for a change the payload makes on its own (a fallback, a value
/// discovered at runtime). A change the user meant (a debug UI control, a pause menu item, a
/// hotkey, a calibration) goes through an edit guard instead, so that it is saved.
pub(crate) static CONFIG: ConfigLock = ConfigLock(());

/// The lock over the whole configuration; see [`CONFIG`].
pub(crate) struct ConfigLock(());

impl ConfigLock {
    /// Lock the whole configuration. The guard derefs to the [`Config`].
    pub(crate) fn lock(&self) -> SlotGuard<Config> {
        register();
        slots::ROOT.lock()
    }
}

/// Load the settings file into the configuration. Called once at startup, before anything reads
/// the configuration for initialization; see [`engine::init`].
pub(crate) fn init() {
    register();
    engine::init();
}

/// Save the configuration if the user's changes have settled; see [`engine::tick`].
pub(crate) fn tick() {
    register();
    engine::tick();
}

/// Stop saving and write a pending change; see [`engine::shutdown`]. Returns `false` when a writer
/// did not finish in time, in which case the payload must stay mapped.
#[must_use = "a writer that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    register();
    engine::shutdown()
}

/// Bind the slot table and register the configuration's schema with the store, once. Every access
/// path in this module calls it first, so the configuration reads as its defaults from the first
/// lock, as it did when it was a constant.
pub(crate) fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        slots::bind();
        engine::register(&schema::SCHEMA);
    });
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
    pub testing: TestingConfig,
    #[serde(default)]
    pub online: OnlineConfig,
    /// The player's body dimensions; see [`jc3vr_body::controllers::body`].
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
            testing: TestingConfig::new(),
            online: OnlineConfig::new(),
            body: BodyDimensions::GENERIC_ADULT,
        }
    }

    /// Lock the global config, run `f` against it, and return the result -- the terse read path for
    /// hooks: `Config::lock_query(|c| c.post_fx.skip_sun_halo)`. The lock is held only for `f`.
    pub fn lock_query<R>(f: impl FnOnce(&Config) -> R) -> R {
        f(&CONFIG.lock())
    }
}

/// The test binary registers the configuration before `main` runs, as the payload's startup does
/// before anything reads it, so a test that reads a subsystem's slot finds it bound whichever test
/// runs first. The C runtime calls each pointer in the `.CRT$XCU` section before `main`.
#[cfg(test)]
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static REGISTER_BEFORE_TESTS: extern "C" fn() = {
    extern "C" fn register_before_tests() {
        register();
    }
    register_before_tests
};

/// Take the configuration for a test: other tests that take it wait, and the configuration and the
/// edit marks are restored when the fixture drops.
#[cfg(test)]
pub(crate) fn test_fixture() -> engine::TestFixture {
    register();
    engine::test_fixture()
}

#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;
