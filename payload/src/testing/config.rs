//! The test tools' settings.

use serde::{Deserialize, Serialize};

use crate::config::engine::ConfigSlot;

/// The capture harness's settings, at `testing` in the settings file; the payload binds the slot at
/// startup.
pub(crate) static TESTING_CONFIG: ConfigSlot<TestingConfig> = ConfigSlot::new("testing");

/// The test tools' settings: the tracking recorder and the request file.
#[derive(Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    /// Whether the tools can be started at all: the tracking recorder (from the debug UI, the
    /// director's key bindings, or the request file) and the request file's other commands.
    /// Nothing runs until it is started, whatever this says.
    pub enabled: bool,
    /// Whether a recording keeps the tracked hand joints, which triple a frame's size.
    pub record_hand_joints: bool,
    /// Whether the request file in the configuration directory is watched, so that a script can
    /// start and stop the tools without the debug UI.
    pub request_file: bool,
    /// Whether to keep a census of the live 2D textures and log it after every resize
    /// (`debug::texture_census`). Diagnostic: once armed, the payload cannot be ejected until the
    /// game restarts.
    #[serde(default)]
    pub texture_census: bool,
}

impl TestingConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            record_hand_joints: true,
            request_file: true,
            texture_census: false,
        }
    }
}

impl Default for TestingConfig {
    fn default() -> Self {
        Self::new()
    }
}
