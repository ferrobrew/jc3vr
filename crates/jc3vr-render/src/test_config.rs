//! A configuration root of the render crate's own tests, holding exactly the settings the crate
//! declares slots for, at the paths the payload's aggregate keeps them, so the crate's tests read
//! and change the configuration without the payload. The defaults are the payload's.
//!
//! The root registers itself before the test harness runs (see
//! [`test_root`](jc3vr_core::config::engine::test_root)); the explicit `register` calls some tests
//! still make find it registered.

use jc3vr_core::{
    director::clean_view::{CLEAN_VIEW_CONFIG, CleanViewConfig},
    hooks::online::{ONLINE_CONFIG, OnlineConfig},
};
use jc3vr_input::frame::Hand;

use crate::{
    debug::texture_census::TEXTURE_CENSUS,
    far_field::{FAR_FIELD_CONFIG, FarFieldConfig},
    fsr::{FSR_CONFIG, FsrConfig},
    headpose::config::{HEADPOSE_CONFIG, HeadPoseConfig},
    hooks::{
        camera::config::{CAMERA_CONFIG, CameraConfig},
        graphics_engine::config::{EXPOSURE_CONFIG, ExposureConfig, POST_FX_CONFIG, PostFxConfig},
    },
    recorder::tracking::{RECORD_HAND_JOINTS, TESTING_ENABLED, TIME_SCALE},
    stereo::config::{STEREO_CONFIG, StereoConfig},
    testing::visual::PAUSED,
    vr::{
        FOVEATION_CONFIG, FoveationConfig, PANEL_ENABLED, PANEL_ON_MIRROR, PANEL_RESOLUTION,
        VR_CONFIG, VrConfig,
        input::config::{
            CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_ENABLED, HAPTICS_INTENSITY, INPUT_CONFIG,
            InputConfig,
        },
    },
};

jc3vr_core::test_root! {
    root {
        stereo: StereoConfig = StereoConfig::new(),
        exposure: ExposureConfig = ExposureConfig::new(),
        foveation: FoveationConfig = FoveationConfig::new(),
        far_field: FarFieldConfig = FarFieldConfig::new(),
        post_fx: PostFxConfig = PostFxConfig::new(),
        camera: CameraConfig = CameraConfig::new(),
        fsr: FsrConfig = FsrConfig::new(),
        headpose: HeadPoseConfig = HeadPoseConfig::new(),
        vr: VrConfig = VrConfig::new(),
        online: OnlineConfig = OnlineConfig::new(),
        controllers: Controllers {
            input: InputConfig = InputConfig::new(),
            enabled: bool = true,
            dominant_hand: Hand = Hand::Right,
            haptics: Haptics {
                enabled: bool = true,
                intensity: f32 = 1.0,
            },
        },
        hud: Hud {
            egui_panel: EguiPanel {
                enabled: bool = false,
                show_on_mirror: bool = false,
                resolution: (u32, u32) = (1280, 960),
            },
        },
        testing: Testing {
            enabled: bool = true,
            record_hand_joints: bool = true,
            texture_census: bool = false,
        },
        director: Director {
            time_scale: f32 = 1.0,
            paused: bool = false,
            #[serde(skip, default = "CleanViewConfig::new")]
            clean_view: CleanViewConfig = CleanViewConfig::new(),
        },
    }
    binds {
        STEREO_CONFIG => stereo,
        EXPOSURE_CONFIG => exposure,
        FOVEATION_CONFIG => foveation,
        FAR_FIELD_CONFIG => far_field,
        POST_FX_CONFIG => post_fx,
        CAMERA_CONFIG => camera,
        FSR_CONFIG => fsr,
        HEADPOSE_CONFIG => headpose,
        VR_CONFIG => vr,
        ONLINE_CONFIG => online,
        INPUT_CONFIG => controllers.input,
        CONTROLLERS_ENABLED => controllers.enabled,
        DOMINANT_HAND => controllers.dominant_hand,
        HAPTICS_ENABLED => controllers.haptics.enabled,
        HAPTICS_INTENSITY => controllers.haptics.intensity,
        PANEL_ENABLED => hud.egui_panel.enabled,
        PANEL_ON_MIRROR => hud.egui_panel.show_on_mirror,
        PANEL_RESOLUTION => hud.egui_panel.resolution,
        TESTING_ENABLED => testing.enabled,
        RECORD_HAND_JOINTS => testing.record_hand_joints,
        TEXTURE_CENSUS => testing.texture_census,
        TIME_SCALE => director.time_scale,
        PAUSED => director.paused,
        CLEAN_VIEW_CONFIG => director.clean_view,
    }
}
