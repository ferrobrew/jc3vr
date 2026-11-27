//! The configuration root of the crate's tests: the slots the crate's code reads, registered once
//! before any test runs; see [`test_root`](jc3vr_core::config::test_root).

use jc3vr_core::clean_view::CLEAN_VIEW_CONFIG;
use jc3vr_view::{
    fsr::FSR_CONFIG,
    headpose::config::HEADPOSE_CONFIG,
    hooks::camera::config::CAMERA_CONFIG,
    recorder::{RECORD_HAND_JOINTS, TESTING_ENABLED, TIME_SCALE},
    stereo::config::STEREO_CONFIG,
    vr::{
        FOVEATION_CONFIG, PANEL_CONFIG, VR_CONFIG,
        input::config::{CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_CONFIG, INPUT_CONFIG},
    },
};

use crate::{
    controllers::{
        aim::config::AIM_CONFIG,
        body::{BODY_CONFIG, config::BODY_REMAP_CONFIG},
        hands::config::HANDS_CONFIG,
        intent::config::BINDINGS_CONFIG,
        locomotion::config::LOCOMOTION_CONFIG,
        overlay::config::OVERLAY_CONFIG,
        roomscale::config::{DRIVING_VIEW_CONFIG, ROOMSCALE_CONFIG},
    },
    hooks::input::wingsuit::WINGSUIT_EVADE_CONFIG,
};

jc3vr_core::test_root!(
    STEREO_CONFIG,
    FOVEATION_CONFIG,
    CAMERA_CONFIG,
    FSR_CONFIG,
    HEADPOSE_CONFIG,
    VR_CONFIG,
    INPUT_CONFIG,
    CONTROLLERS_ENABLED,
    DOMINANT_HAND,
    HAPTICS_CONFIG,
    PANEL_CONFIG,
    TESTING_ENABLED,
    RECORD_HAND_JOINTS,
    TIME_SCALE,
    CLEAN_VIEW_CONFIG,
    BODY_CONFIG,
    LOCOMOTION_CONFIG,
    BINDINGS_CONFIG,
    HANDS_CONFIG,
    AIM_CONFIG,
    OVERLAY_CONFIG,
    BODY_REMAP_CONFIG,
    ROOMSCALE_CONFIG,
    DRIVING_VIEW_CONFIG,
    WINGSUIT_EVADE_CONFIG,
);
