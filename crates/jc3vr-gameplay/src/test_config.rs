//! The configuration root of the crate's tests, registered once before any test runs.
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! this crate, so the crate's tests register a root of their own instead: it holds the gameplay
//! groups themselves (the controllers, the body, the movement, and the body IK settings), the
//! render and core groups the gameplay code reads through their slots, and stand-ins for the fields
//! of higher layers' groups that a gameplay slot projects (the world markers' switch), all at the
//! paths the payload's aggregate keeps them and with its defaults. A test reads and changes the
//! configuration through the slots exactly as the payload does, and takes it exclusively with
//! [`test_fixture`](jc3vr_core::config::engine::test_fixture).
//!
//! The registration runs from the C runtime's initializer table (`.CRT$XCU`), before the test
//! harness's `main`, so a test finds every slot bound whichever test runs first, and no test calls
//! a registration of its own.

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
use jc3vr_render::{
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
        },
    },
};
use serde::{Deserialize, Serialize};
use toml::Table;

use crate::{
    controllers::{
        aim::WORLD_MARKERS_ENABLED,
        body::{BODY_CONFIG, BodyDimensions},
        config::{CONTROLLERS_CONFIG, ControllersConfig},
        scheme,
    },
    hooks::{
        character::config::{BODY_IK_CONFIG, BodyIkConfig},
        input::config::{MOVEMENT_CONFIG, MovementConfig},
    },
};

/// Bind every slot the crate's code reads to the test root, and register the root's schema.
fn register() {
    CONTROLLERS_CONFIG.bind(projection!(TestRoot, controllers));
    BODY_CONFIG.bind(projection!(TestRoot, body));
    MOVEMENT_CONFIG.bind(projection!(TestRoot, movement));
    BODY_IK_CONFIG.bind(projection!(TestRoot, body_ik));
    scheme::CONTROLLERS_ENABLED.bind(projection!(TestRoot, controllers.enabled));
    WORLD_MARKERS_ENABLED.bind(projection!(TestRoot, hud.world_markers.enabled));
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
}

/// The C runtime calls each pointer in the `.CRT$XCU` section before `main`.
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static REGISTER_BEFORE_TESTS: extern "C" fn() = {
    extern "C" fn register_before_tests() {
        register();
    }
    register_before_tests
};

static SCHEMA: Schema = Schema::of::<TestRoot>();

#[derive(Clone, Serialize, Deserialize)]
struct TestRoot {
    stereo: StereoConfig,
    exposure: ExposureConfig,
    foveation: FoveationConfig,
    far_field: FarFieldConfig,
    post_fx: PostFxConfig,
    camera: CameraConfig,
    movement: MovementConfig,
    fsr: FsrConfig,
    hud: Hud,
    headpose: HeadPoseConfig,
    body_ik: BodyIkConfig,
    vr: VrConfig,
    director: Director,
    controllers: ControllersConfig,
    testing: Testing,
    online: OnlineConfig,
    body: BodyDimensions,
}

#[derive(Clone, Serialize, Deserialize)]
struct Hud {
    egui_panel: EguiPanel,
    world_markers: WorldMarkers,
}

#[derive(Clone, Serialize, Deserialize)]
struct EguiPanel {
    enabled: bool,
    show_on_mirror: bool,
    resolution: (u32, u32),
}

#[derive(Clone, Serialize, Deserialize)]
struct WorldMarkers {
    enabled: bool,
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
            movement: MovementConfig::new(),
            fsr: FsrConfig::new(),
            hud: Hud {
                egui_panel: EguiPanel {
                    enabled: false,
                    show_on_mirror: false,
                    resolution: (1280, 960),
                },
                world_markers: WorldMarkers { enabled: true },
            },
            headpose: HeadPoseConfig::new(),
            body_ik: BodyIkConfig::new(),
            vr: VrConfig::new(),
            director: Director {
                time_scale: 1.0,
                paused: false,
                clean_view: CleanViewConfig::new(),
            },
            controllers: ControllersConfig::new(),
            testing: Testing {
                enabled: true,
                record_hand_joints: true,
                texture_census: false,
            },
            online: OnlineConfig::new(),
            body: BodyDimensions::GENERIC_ADULT,
        }
    }

    fn migrate(_file: &mut Table, _format: Format) -> Vec<Migrated> {
        Vec::new()
    }

    fn validate(&mut self, _resets: &mut Vec<Reset>) {}

    fn note_saved(_text: &str) {}
}
