//! A configuration root of the render crate's own tests, holding exactly the settings the crate
//! declares slots for, at the paths the payload's aggregate keeps them, so the crate's tests read
//! and change the configuration without the payload. The defaults are the payload's.

use std::sync::Once;

use jc3vr_core::{
    config::engine::{
        ConfigRoot, Schema,
        format::{Format, Migrated},
        load::Reset,
        projection, store,
    },
    director::clean_view::{CLEAN_VIEW_CONFIG, CleanViewConfig},
    hooks::online::{ONLINE_CONFIG, OnlineConfig},
};
use jc3vr_input::frame::Hand;
use serde::{Deserialize, Serialize};
use toml::Table;

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

/// Bind the crate's slots to the test root and register its schema, once.
pub(crate) fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        STEREO_CONFIG.bind(projection!(TestRoot, stereo));
        EXPOSURE_CONFIG.bind(projection!(TestRoot, exposure));
        FOVEATION_CONFIG.bind(projection!(TestRoot, foveation));
        FAR_FIELD_CONFIG.bind(projection!(TestRoot, far_field));
        POST_FX_CONFIG.bind(projection!(TestRoot, post_fx));
        CAMERA_CONFIG.bind(projection!(TestRoot, camera));
        FSR_CONFIG.bind(projection!(TestRoot, fsr));
        HEADPOSE_CONFIG.bind(projection!(TestRoot, headpose));
        VR_CONFIG.bind(projection!(TestRoot, vr));
        ONLINE_CONFIG.bind(projection!(TestRoot, online));
        INPUT_CONFIG.bind(projection!(TestRoot, controllers.input));
        CONTROLLERS_ENABLED.bind(projection!(TestRoot, controllers.enabled));
        DOMINANT_HAND.bind(projection!(TestRoot, controllers.dominant_hand));
        HAPTICS_ENABLED.bind(projection!(TestRoot, controllers.haptics.enabled));
        HAPTICS_INTENSITY.bind(projection!(TestRoot, controllers.haptics.intensity));
        PANEL_ENABLED.bind(projection!(TestRoot, hud.egui_panel.enabled));
        PANEL_ON_MIRROR.bind(projection!(TestRoot, hud.egui_panel.show_on_mirror));
        PANEL_RESOLUTION.bind(projection!(TestRoot, hud.egui_panel.resolution));
        TESTING_ENABLED.bind(projection!(TestRoot, testing.enabled));
        RECORD_HAND_JOINTS.bind(projection!(TestRoot, testing.record_hand_joints));
        TEXTURE_CENSUS.bind(projection!(TestRoot, testing.texture_census));
        TIME_SCALE.bind(projection!(TestRoot, director.time_scale));
        PAUSED.bind(projection!(TestRoot, director.paused));
        CLEAN_VIEW_CONFIG.bind(projection!(TestRoot, director.clean_view));
        store::register(&SCHEMA);
    });
}

static SCHEMA: Schema = Schema::of::<TestRoot>();

#[derive(Clone, Serialize, Deserialize)]
struct TestRoot {
    stereo: StereoConfig,
    exposure: ExposureConfig,
    foveation: FoveationConfig,
    far_field: FarFieldConfig,
    post_fx: PostFxConfig,
    camera: CameraConfig,
    fsr: FsrConfig,
    headpose: HeadPoseConfig,
    vr: VrConfig,
    online: OnlineConfig,
    controllers: Controllers,
    hud: Hud,
    testing: Testing,
    director: Director,
}

#[derive(Clone, Serialize, Deserialize)]
struct Controllers {
    input: InputConfig,
    enabled: bool,
    dominant_hand: Hand,
    haptics: Haptics,
}

#[derive(Clone, Serialize, Deserialize)]
struct Haptics {
    enabled: bool,
    intensity: f32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Hud {
    egui_panel: EguiPanel,
}

#[derive(Clone, Serialize, Deserialize)]
struct EguiPanel {
    enabled: bool,
    show_on_mirror: bool,
    resolution: (u32, u32),
}

#[derive(Clone, Serialize, Deserialize)]
struct Testing {
    enabled: bool,
    record_hand_joints: bool,
    texture_census: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Director {
    time_scale: f32,
    paused: bool,
    #[serde(skip, default = "CleanViewConfig::new")]
    clean_view: CleanViewConfig,
}

impl ConfigRoot for TestRoot {
    fn defaults() -> Self {
        Self {
            stereo: StereoConfig::new(),
            exposure: ExposureConfig::new(),
            foveation: FoveationConfig::new(),
            far_field: FarFieldConfig::new(),
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            fsr: FsrConfig::new(),
            headpose: HeadPoseConfig::new(),
            vr: VrConfig::new(),
            online: OnlineConfig::new(),
            controllers: Controllers {
                input: InputConfig::new(),
                enabled: true,
                dominant_hand: Hand::Right,
                haptics: Haptics {
                    enabled: true,
                    intensity: 1.0,
                },
            },
            hud: Hud {
                egui_panel: EguiPanel {
                    enabled: false,
                    show_on_mirror: false,
                    resolution: (1280, 960),
                },
            },
            testing: Testing {
                enabled: true,
                record_hand_joints: true,
                texture_census: false,
            },
            director: Director {
                time_scale: 1.0,
                paused: false,
                clean_view: CleanViewConfig::new(),
            },
        }
    }

    fn migrate(_file: &mut Table, _format: Format) -> Vec<Migrated> {
        Vec::new()
    }

    fn validate(&mut self, _resets: &mut Vec<Reset>) {}

    fn note_saved(_text: &str) {}
}
