//! The test tools' settings.

use serde::{Deserialize, Serialize};

/// The test tools' settings: the tracking recorder and the replay.
#[derive(Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    /// Whether the tools can be started at all: from the debug UI, the director's key bindings,
    /// and the request file. Nothing runs until it is started, whatever this says.
    pub enabled: bool,
    /// Whether a recording keeps the tracked hand joints, which triple a frame's size.
    pub record_hand_joints: bool,
    /// Whether the request file in the configuration directory is watched, so that a script can
    /// start and stop the tools without the debug UI.
    pub request_file: bool,
    /// How long a replay waits for the teleport to its start marker to settle before it gives up,
    /// seconds.
    pub teleport_timeout_s: f32,
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
            teleport_timeout_s: 60.0,
            texture_census: false,
        }
    }
}

impl Default for TestingConfig {
    fn default() -> Self {
        Self::new()
    }
}
