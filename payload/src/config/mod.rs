//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern, each defined beside the subsystem that owns it. The debug UI reads and
//! writes the whole struct; hooks copy out the field(s) they need at the top of a detour. Live
//! engine-interface state (the current eye, frame counters, the trace arm-flag) does not live here
//! but in the per-subsystem runtime state, such as [`jc3vr_view::stereo`]'s. A field that is
//! configuration-shaped but holds state for the session (a value discovered at runtime, a
//! diagnostic armed for one capture) is `#[serde(skip)]`.
//!
//! The store, the load, and the save are the configuration engine ([`engine`]), which names no
//! subsystem: it holds the configuration as an erased root and works through the
//! [`Schema`](engine::Schema) this module registers for [`Config`] ([`schema`]). Each subsystem
//! reads and changes its own settings through the [`ConfigSlot`](engine::ConfigSlot) it declares
//! beside their type; this module owns the aggregate, binds every slot to its place in it
//! ([`slots`]), and keeps the access paths over the whole configuration ([`CONFIG`] and
//! [`Config::lock_query`]), each of which registers the configuration on first use ([`register`]).
//!
//! The configuration is saved to `settings.toml` in the per-user configuration directory
//! ([`engine::paths`]), which holds only the settings that differ from their defaults
//! ([`format`](mod@format)), so a setting the user never changed follows its default when a later
//! build changes it. It is loaded once at startup, before VR initialization reads it ([`init`]);
//! the load is tolerant ([`engine::load`]), so a file from another build costs at most the settings
//! it gets wrong. A change the user meant goes through an edit guard (a slot's `edit`), which marks
//! the configuration changed; [`tick`] saves it from the game thread's frame once the changes
//! settle, handing the write to a short-lived writer thread, and [`shutdown`] flushes a pending
//! save. A change the payload makes on its own, such as a fallback after a failure, takes a plain
//! lock instead and is not saved for its own sake; one that switches off a setting the user chose
//! goes through [`engine::disable_automatically`], so a later save keeps the user's choice. Every
//! guard locks the one configuration lock, so a thread must not lock it again while it holds one
//! (the [nesting rule](jc3vr_core::config::store#lock-nesting)). See `docs/mod/settings.md`.
//!
//! TOML has no null, so a `None` is left out of the file. An optional setting defaults to `None`:
//! the load then reads a missing key as `None` and a present one as `Some`. A setting whose value is
//! kept while it is switched off is a switch beside the value instead (see
//! [`FsrConfig::sharpen`]).
//!
//! The trace manifest and screenshot sidecar also serialize the capture's configuration.

pub(crate) mod format;
#[cfg(test)]
pub(crate) mod load;
mod schema;
pub(crate) mod slots;

use std::sync::Once;

use jc3vr_body::controllers::body::BodyDimensions;
use jc3vr_core::{config as engine, config::SlotGuard};
use jc3vr_gameplay::{controllers::ControllersConfig, hooks::online::OnlineConfig};
use jc3vr_hud::hud::HudConfig;
use jc3vr_render::hooks::graphics_engine::config::{ExposureConfig, PostFxConfig};
use jc3vr_tooling::{director::DirectorConfig, recorder::RecorderConfig, testing::TestingConfig};
use jc3vr_view::{
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
/// lock.
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
    pub post_fx: PostFxConfig,
    pub camera: CameraConfig,
    pub fsr: FsrConfig,
    pub hud: HudConfig,
    pub headpose: HeadPoseConfig,
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
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            fsr: FsrConfig::new(),
            hud: HudConfig::new(),
            headpose: HeadPoseConfig::new(),
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

impl Default for Config {
    fn default() -> Self {
        Self::new()
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
