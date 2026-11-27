//! The test tools' settings.

use jc3vr_core::{
    config::{ConfigSlot, SlotOptions},
    slot_children,
};
use jc3vr_render::debug::texture_census::TEXTURE_CENSUS;
use jc3vr_view::recorder::tracking::{RECORD_HAND_JOINTS, TESTING_ENABLED};
use serde::{Deserialize, Serialize};

/// The capture harness's settings, at `testing` in the settings file; the payload binds the slot at
/// startup. The lower layers read three of its fields through slots of their own.
pub static TESTING_CONFIG: ConfigSlot<TestingConfig> = ConfigSlot::with(
    "testing",
    SlotOptions::new().with_children(slot_children!(TESTING_CONFIG {
        TESTING_ENABLED => enabled,
        RECORD_HAND_JOINTS => record_hand_joints,
        TEXTURE_CENSUS => texture_census,
    })),
);

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
    pub(crate) request_file: bool,
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
