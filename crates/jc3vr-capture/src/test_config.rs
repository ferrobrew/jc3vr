//! The configuration root of the crate's tests: the slots the crate's code reads, registered once
//! before any test runs; see [`test_root`](jc3vr_core::config::engine::test_root).

use jc3vr_core::{director::clean_view::CLEAN_VIEW_CONFIG, hooks::online::ONLINE_CONFIG};
use jc3vr_render::{
    debug::texture_census::TEXTURE_CENSUS,
    hooks::graphics_engine::config::{EXPOSURE_CONFIG, POST_FX_CONFIG},
};
use jc3vr_view::{
    fsr::FSR_CONFIG,
    headpose::config::HEADPOSE_CONFIG,
    hooks::camera::config::CAMERA_CONFIG,
    recorder::tracking::{RECORD_HAND_JOINTS, TESTING_ENABLED, TIME_SCALE},
    stereo::config::STEREO_CONFIG,
    vr::{
        FOVEATION_CONFIG, PANEL_CONFIG, VR_CONFIG,
        input::config::{CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_CONFIG, INPUT_CONFIG},
    },
};

use crate::testing::visual::PAUSED;

jc3vr_core::test_root!(
    STEREO_CONFIG,
    FOVEATION_CONFIG,
    CAMERA_CONFIG,
    FSR_CONFIG,
    HEADPOSE_CONFIG,
    VR_CONFIG,
    ONLINE_CONFIG,
    INPUT_CONFIG,
    CONTROLLERS_ENABLED,
    DOMINANT_HAND,
    HAPTICS_CONFIG,
    PANEL_CONFIG,
    TESTING_ENABLED,
    RECORD_HAND_JOINTS,
    TIME_SCALE,
    CLEAN_VIEW_CONFIG,
    EXPOSURE_CONFIG,
    POST_FX_CONFIG,
    TEXTURE_CENSUS,
    PAUSED,
);
