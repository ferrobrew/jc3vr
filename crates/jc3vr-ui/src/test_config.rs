//! The configuration root of the crate's tests, registered once before any test runs.
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! this crate, so the crate's tests register a root of their own instead. The debug UI and the VR
//! pages edit the settings of every subsystem, so the root holds every group the aggregate holds,
//! at the same paths and with the same defaults, and binds every slot the payload binds. Its
//! validation runs the same checks the payload's tolerant load runs, with the same resets, so a page
//! test that checks the settings stay valid checks what a load of the saved file would.
//!
//! The registration runs from the C runtime's initializer table (`.CRT$XCU`), before the test
//! harness's `main`, so a test finds every slot bound whichever test runs first, and no test calls
//! a registration of its own.

use jc3vr_core::{
    config::engine::{
        ConfigRoot, Invalid, Schema,
        format::{Format, Migrated},
        load::{Reset, reset_named},
        projection, store,
    },
    director::clean_view::CLEAN_VIEW_CONFIG,
    hooks::online::{ONLINE_CONFIG, OnlineConfig},
};
use jc3vr_gameplay::{
    controllers::{
        aim::WORLD_MARKERS_ENABLED,
        body::{BODY_CONFIG, BodyDimensions},
        config::{CONTROLLERS_CONFIG, ControllersConfig, SightTable, validate},
        scheme,
    },
    hooks::{
        character::config::{BODY_IK_CONFIG, BodyIkConfig},
        input::config::{MOVEMENT_CONFIG, MovementConfig},
    },
};
use jc3vr_hud::hud::config::{HUD_CONFIG, HudConfig};
use jc3vr_render::{
    debug::texture_census::TEXTURE_CENSUS,
    far_field::{FAR_FIELD_CONFIG, FarFieldConfig},
    fsr::{FSR_CONFIG, FsrConfig},
    headpose::config::{HEADPOSE_CONFIG, HeadPoseConfig, VrTurnConfig},
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
use jc3vr_tooling::{
    director::{DIRECTOR_CONFIG, DirectorConfig},
    recorder::{RECORDER_CONFIG, RecorderConfig},
    testing::{TESTING_CONFIG, TestingConfig},
};
use serde::{Deserialize, Serialize};
use toml::Table;

/// Bind every slot the payload binds to the test root, and register the root's schema.
fn register() {
    STEREO_CONFIG.bind(projection!(TestRoot, stereo));
    EXPOSURE_CONFIG.bind(projection!(TestRoot, exposure));
    FOVEATION_CONFIG.bind(projection!(TestRoot, foveation));
    FAR_FIELD_CONFIG.bind(projection!(TestRoot, far_field));
    POST_FX_CONFIG.bind(projection!(TestRoot, post_fx));
    CAMERA_CONFIG.bind(projection!(TestRoot, camera));
    MOVEMENT_CONFIG.bind(projection!(TestRoot, movement));
    FSR_CONFIG.bind(projection!(TestRoot, fsr));
    HUD_CONFIG.bind(projection!(TestRoot, hud));
    HEADPOSE_CONFIG.bind(projection!(TestRoot, headpose));
    BODY_IK_CONFIG.bind(projection!(TestRoot, body_ik));
    VR_CONFIG.bind(projection!(TestRoot, vr));
    DIRECTOR_CONFIG.bind(projection!(TestRoot, director));
    RECORDER_CONFIG.bind(projection!(TestRoot, recorder));
    CONTROLLERS_CONFIG.bind(projection!(TestRoot, controllers));
    TESTING_CONFIG.bind(projection!(TestRoot, testing));
    ONLINE_CONFIG.bind(projection!(TestRoot, online));
    BODY_CONFIG.bind(projection!(TestRoot, body));
    INPUT_CONFIG.bind(projection!(TestRoot, controllers.input));
    CONTROLLERS_ENABLED.bind(projection!(TestRoot, controllers.enabled));
    DOMINANT_HAND.bind(projection!(TestRoot, controllers.dominant_hand));
    HAPTICS_ENABLED.bind(projection!(TestRoot, controllers.haptics.enabled));
    HAPTICS_INTENSITY.bind(projection!(TestRoot, controllers.haptics.intensity));
    scheme::CONTROLLERS_ENABLED.bind(projection!(TestRoot, controllers.enabled));
    WORLD_MARKERS_ENABLED.bind(projection!(TestRoot, hud.world_markers.enabled));
    PANEL_ENABLED.bind(projection!(TestRoot, hud.egui_panel.enabled));
    PANEL_ON_MIRROR.bind(projection!(TestRoot, hud.egui_panel.show_on_mirror));
    PANEL_RESOLUTION.bind(projection!(TestRoot, hud.egui_panel.resolution));
    TEXTURE_CENSUS.bind(projection!(TestRoot, testing.texture_census));
    TESTING_ENABLED.bind(projection!(TestRoot, testing.enabled));
    RECORD_HAND_JOINTS.bind(projection!(TestRoot, testing.record_hand_joints));
    TIME_SCALE.bind(projection!(TestRoot, director.time_scale));
    CLEAN_VIEW_CONFIG.bind(projection!(TestRoot, director.clean_view));
    PAUSED.bind(projection!(TestRoot, director.paused));
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
    #[serde(default)]
    foveation: FoveationConfig,
    #[serde(default)]
    far_field: FarFieldConfig,
    post_fx: PostFxConfig,
    camera: CameraConfig,
    movement: MovementConfig,
    fsr: FsrConfig,
    hud: HudConfig,
    headpose: HeadPoseConfig,
    body_ik: BodyIkConfig,
    vr: VrConfig,
    director: DirectorConfig,
    recorder: RecorderConfig,
    #[serde(default)]
    controllers: ControllersConfig,
    #[serde(default)]
    testing: TestingConfig,
    #[serde(default)]
    online: OnlineConfig,
    body: BodyDimensions,
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
            hud: HudConfig::new(),
            headpose: HeadPoseConfig::new(),
            body_ik: BodyIkConfig::new(),
            vr: VrConfig::new(),
            director: DirectorConfig::new(),
            recorder: RecorderConfig::new(),
            controllers: ControllersConfig::new(),
            testing: TestingConfig::new(),
            online: OnlineConfig::new(),
            body: BodyDimensions::GENERIC_ADULT,
        }
    }

    fn migrate(_file: &mut Table, _format: Format) -> Vec<Migrated> {
        Vec::new()
    }

    /// The checks the payload's load runs on a loaded configuration, with the same resets.
    fn validate(&mut self, resets: &mut Vec<Reset>) {
        let fail = |resets: &mut Vec<Reset>, path: &str, error: Invalid| {
            resets.push(Reset {
                path: path.to_owned(),
                reason: error.to_string(),
            });
        };
        if let Err(error) = validate::sights(&self.controllers.aim.sights) {
            self.controllers.aim.sights = SightTable::EMPTY;
            fail(resets, "controllers.aim.sights", error);
        }
        if let Err(error) = reset_named(
            &mut self.controllers,
            &ControllersConfig::new(),
            "controllers",
            validate::controllers,
            resets,
        ) {
            let sights = self.controllers.aim.sights;
            self.controllers = ControllersConfig::new();
            self.controllers.aim.sights = sights;
            fail(resets, "controllers", error);
        }
        if let Err(error) = reset_named(
            &mut self.headpose.vr_turn,
            &VrTurnConfig::new(),
            "headpose.vr_turn",
            validate::vr_turn,
            resets,
        ) {
            self.headpose.vr_turn = VrTurnConfig::new();
            fail(resets, "headpose.vr_turn", error);
        }
        if let Err(error) = reset_named(
            &mut self.hud,
            &HudConfig::new(),
            "hud",
            HudConfig::validate,
            resets,
        ) {
            self.hud = HudConfig::new();
            fail(resets, "hud", error);
        }
        if let Err(error) = self.body.validate() {
            self.body = BodyDimensions::GENERIC_ADULT;
            fail(resets, "body", Invalid(error.to_string()));
        }
    }

    fn note_saved(_text: &str) {}
}
