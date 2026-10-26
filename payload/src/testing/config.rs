//! The test tools' settings.

use serde::{Deserialize, Serialize};

/// The test tools' settings: the tracking recorder, the replay, and the scenario runner.
#[derive(Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    /// Whether the tools can be started at all: from the debug UI, the director's key bindings,
    /// and the request file. Nothing runs until it is started, whatever this says.
    pub enabled: bool,
    /// Whether a recording keeps the tracked hand joints, which triple a frame's size.
    pub record_hand_joints: bool,
    /// Whether recordings use lossless version 4 chunks. This defaults on; turn it off to write
    /// uncompressed version 4 records for comparison.
    #[serde(default = "default_record_compression")]
    pub record_compression: bool,
    /// Where the scenario files are, or empty for the `testing` folder in the configuration
    /// directory. A scenario names its recording relative to its own file.
    pub scenario_dir: String,
    /// Whether the request file in the configuration directory is watched, so that a script can
    /// start and stop the tools without the debug UI.
    pub request_file: bool,
    /// How long a replay waits for the teleport to its start marker to settle before it gives up,
    /// seconds.
    pub teleport_timeout_s: f32,
}

impl TestingConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            record_hand_joints: true,
            record_compression: true,
            scenario_dir: String::new(),
            request_file: true,
            teleport_timeout_s: 60.0,
        }
    }
}

fn default_record_compression() -> bool {
    true
}

impl Default for TestingConfig {
    fn default() -> Self {
        Self::new()
    }
}
