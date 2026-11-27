//! The configuration root of the crate's tests: the slots the crate's code reads, registered once
//! before any test runs; see [`test_root`](jc3vr_core::config::test_root).

use jc3vr_body::controllers::body::BODY_CONFIG;
use jc3vr_gameplay::{controllers::config::CONTROLLERS_CONFIG, hooks::online::ONLINE_CONFIG};
use jc3vr_hud::hud::config::HUD_CONFIG;
use jc3vr_render::hooks::graphics_engine::config::{EXPOSURE_CONFIG, POST_FX_CONFIG};
use jc3vr_tooling::{
    director::DIRECTOR_CONFIG, recorder::RECORDER_CONFIG, testing::TESTING_CONFIG,
};
use jc3vr_view::{
    fsr::FSR_CONFIG,
    headpose::config::HEADPOSE_CONFIG,
    hooks::camera::config::CAMERA_CONFIG,
    stereo::config::STEREO_CONFIG,
    vr::{FOVEATION_CONFIG, VR_CONFIG},
};

jc3vr_core::test_root!(
    STEREO_CONFIG,
    EXPOSURE_CONFIG,
    FOVEATION_CONFIG,
    POST_FX_CONFIG,
    CAMERA_CONFIG,
    FSR_CONFIG,
    HUD_CONFIG,
    HEADPOSE_CONFIG,
    VR_CONFIG,
    DIRECTOR_CONFIG,
    RECORDER_CONFIG,
    CONTROLLERS_CONFIG,
    TESTING_CONFIG,
    ONLINE_CONFIG,
    BODY_CONFIG,
);
