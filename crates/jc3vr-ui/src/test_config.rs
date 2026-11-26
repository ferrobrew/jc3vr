//! The configuration root of the crate's tests, registered once before any test runs.
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! this crate, so the crate's tests register a root of their own instead. The debug UI and the VR
//! pages edit the settings of every subsystem, so the root holds every group the aggregate holds,
//! at the same paths and with the same defaults, and binds every slot the payload binds. Its
//! validation runs the same checks the payload's tolerant load runs, with the same resets, so a page
//! test that checks the settings stay valid checks what a load of the saved file would.
//!
//! The root registers itself before the test harness runs; see
//! [`test_root`](jc3vr_core::config::engine::test_root).

use jc3vr_body::{
    controllers::{
        aim::{
            GRAPPLE_SEAT, WORLD_MARKERS_ENABLED, config::AIM_CONFIG, sights::config::SightTable,
        },
        aim_state::AIM_STATE_CONFIG,
        body::{BODY_CONFIG, BodyDimensions, config::BODY_REMAP_CONFIG},
        fingers::config::FINGERS_CONFIG,
        hands::config::{GRAPPLE_PUMP, HANDS_CONFIG, MOUNTED_AIM_BY_HANDS},
        haptics::config::HAPTICS_CONFIG,
        intent::config::BINDINGS_CONFIG,
        locomotion::config::LOCOMOTION_CONFIG,
        overlay::config::OVERLAY_CONFIG,
        roomscale::config::{DRIVING_VIEW_ENABLED, ROOMSCALE_CONFIG, TAKE_OFF_TO_HEAD},
        scheme,
    },
    hooks::{
        animation::canopy::SKIP_REEL_OPENING,
        character::config::{BODY_IK_CONFIG, BodyIkConfig},
        input::{
            config::{MOVEMENT_CONFIG, MovementConfig},
            parachute::{CANOPY_LINE_TURN, CANOPY_LINE_TURN_SCALE},
            wingsuit::WINGSUIT_EVADE_ENABLED,
        },
    },
};
use jc3vr_capture::testing::visual::PAUSED;
use jc3vr_core::{
    config::engine::{
        Invalid,
        load::{Reset, reset_named},
    },
    director::clean_view::CLEAN_VIEW_CONFIG,
    hooks::online::{ONLINE_CONFIG, OnlineConfig},
};
use jc3vr_gameplay::{
    controllers::{
        config::{CONTROLLERS_CONFIG, ControllersConfig, validate},
        summary::config::SUMMARY_CONFIG,
        vehicles::config::VEHICLES_CONFIG,
        weapons::config::WEAPONS_CONFIG,
    },
    grapple::physical::config::GRAPPLE_CONFIG,
};
use jc3vr_hud::hud::config::{HUD_CONFIG, HudConfig};
use jc3vr_physical::controllers::{
    glass::config::GLASS_CONFIG, physical_hands::config::PHYSICAL_HANDS_CONFIG,
};
use jc3vr_render::{
    debug::texture_census::TEXTURE_CENSUS,
    hooks::graphics_engine::config::{
        EXPOSURE_CONFIG, ExposureConfig, POST_FX_CONFIG, PostFxConfig,
    },
};
use jc3vr_tooling::{
    director::{DIRECTOR_CONFIG, DirectorConfig},
    recorder::{RECORDER_CONFIG, RecorderConfig},
    testing::{TESTING_CONFIG, TestingConfig},
};
use jc3vr_view::{
    far_field::{FAR_FIELD_CONFIG, FarFieldConfig},
    fsr::{FSR_CONFIG, FsrConfig},
    headpose::config::{HEADPOSE_CONFIG, HeadPoseConfig, VrTurnConfig},
    hooks::camera::config::{CAMERA_CONFIG, CameraConfig},
    recorder::tracking::{RECORD_HAND_JOINTS, TESTING_ENABLED, TIME_SCALE},
    stereo::config::{STEREO_CONFIG, StereoConfig},
    vr::{
        FOVEATION_CONFIG, FoveationConfig, PANEL_ENABLED, PANEL_ON_MIRROR, PANEL_RESOLUTION,
        VR_CONFIG, VrConfig,
        input::config::{
            CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_ENABLED, HAPTICS_INTENSITY, INPUT_CONFIG,
        },
    },
};

jc3vr_core::test_root! {
    root {
        stereo: StereoConfig = StereoConfig::new(),
        exposure: ExposureConfig = ExposureConfig::new(),
        #[serde(default)]
        foveation: FoveationConfig = FoveationConfig::new(),
        #[serde(default)]
        far_field: FarFieldConfig = FarFieldConfig::new(),
        post_fx: PostFxConfig = PostFxConfig::new(),
        camera: CameraConfig = CameraConfig::new(),
        movement: MovementConfig = MovementConfig::new(),
        fsr: FsrConfig = FsrConfig::new(),
        hud: HudConfig = HudConfig::new(),
        headpose: HeadPoseConfig = HeadPoseConfig::new(),
        body_ik: BodyIkConfig = BodyIkConfig::new(),
        vr: VrConfig = VrConfig::new(),
        director: DirectorConfig = DirectorConfig::new(),
        recorder: RecorderConfig = RecorderConfig::new(),
        #[serde(default)]
        controllers: ControllersConfig = ControllersConfig::new(),
        #[serde(default)]
        testing: TestingConfig = TestingConfig::new(),
        #[serde(default)]
        online: OnlineConfig = OnlineConfig::new(),
        body: BodyDimensions = BodyDimensions::GENERIC_ADULT,
    }
    binds {
        STEREO_CONFIG => stereo,
        EXPOSURE_CONFIG => exposure,
        FOVEATION_CONFIG => foveation,
        FAR_FIELD_CONFIG => far_field,
        POST_FX_CONFIG => post_fx,
        CAMERA_CONFIG => camera,
        MOVEMENT_CONFIG => movement,
        FSR_CONFIG => fsr,
        HUD_CONFIG => hud,
        HEADPOSE_CONFIG => headpose,
        BODY_IK_CONFIG => body_ik,
        VR_CONFIG => vr,
        DIRECTOR_CONFIG => director,
        RECORDER_CONFIG => recorder,
        CONTROLLERS_CONFIG => controllers,
        TESTING_CONFIG => testing,
        ONLINE_CONFIG => online,
        BODY_CONFIG => body,
        INPUT_CONFIG => controllers.input,
        CONTROLLERS_ENABLED => controllers.enabled,
        DOMINANT_HAND => controllers.dominant_hand,
        HAPTICS_ENABLED => controllers.haptics.enabled,
        HAPTICS_INTENSITY => controllers.haptics.intensity,
        scheme::CONTROLLERS_ENABLED => controllers.enabled,
        scheme::DOMINANT_HAND => controllers.dominant_hand,
        LOCOMOTION_CONFIG => controllers.locomotion,
        BINDINGS_CONFIG => controllers.bindings,
        AIM_STATE_CONFIG => controllers.aim_state,
        HANDS_CONFIG => controllers.hands,
        AIM_CONFIG => controllers.aim,
        OVERLAY_CONFIG => controllers.overlay,
        WEAPONS_CONFIG => controllers.weapons,
        BODY_REMAP_CONFIG => controllers.body,
        ROOMSCALE_CONFIG => controllers.roomscale,
        VEHICLES_CONFIG => controllers.vehicles,
        FINGERS_CONFIG => controllers.fingers,
        PHYSICAL_HANDS_CONFIG => controllers.physical_hands,
        SUMMARY_CONFIG => controllers.summary,
        GRAPPLE_CONFIG => controllers.grapple,
        HAPTICS_CONFIG => controllers.haptics,
        GLASS_CONFIG => controllers.glass,
        MOUNTED_AIM_BY_HANDS => controllers.vehicles.mounted.aim_by_hands,
        GRAPPLE_PUMP => controllers.grapple.pump,
        DRIVING_VIEW_ENABLED => controllers.vehicles.driving_view.enabled,
        TAKE_OFF_TO_HEAD => controllers.vehicles.take_off_to_head,
        GRAPPLE_SEAT => controllers.grapple.seat,
        CANOPY_LINE_TURN => controllers.vehicles.canopy.line_turn,
        CANOPY_LINE_TURN_SCALE => controllers.vehicles.canopy.line_turn_scale,
        SKIP_REEL_OPENING => controllers.vehicles.canopy.skip_reel_opening,
        WINGSUIT_EVADE_ENABLED => controllers.vehicles.wingsuit_evade.enabled,
        WORLD_MARKERS_ENABLED => hud.world_markers.enabled,
        PANEL_ENABLED => hud.egui_panel.enabled,
        PANEL_ON_MIRROR => hud.egui_panel.show_on_mirror,
        PANEL_RESOLUTION => hud.egui_panel.resolution,
        TEXTURE_CENSUS => testing.texture_census,
        TESTING_ENABLED => testing.enabled,
        RECORD_HAND_JOINTS => testing.record_hand_joints,
        TIME_SCALE => director.time_scale,
        CLEAN_VIEW_CONFIG => director.clean_view,
        PAUSED => director.paused,
    }
    validate: validate_root,
}

/// The checks the payload's load runs on a loaded configuration, with the same resets.
fn validate_root(root: &mut TestRoot, resets: &mut Vec<Reset>) {
    let fail = |resets: &mut Vec<Reset>, path: &str, error: Invalid| {
        resets.push(Reset {
            path: path.to_owned(),
            reason: error.to_string(),
        });
    };
    if let Err(error) = validate::sights(&root.controllers.aim.sights) {
        root.controllers.aim.sights = SightTable::EMPTY;
        fail(resets, "controllers.aim.sights", error);
    }
    if let Err(error) = reset_named(
        &mut root.controllers,
        &ControllersConfig::new(),
        "controllers",
        validate::controllers,
        resets,
    ) {
        let sights = root.controllers.aim.sights;
        root.controllers = ControllersConfig::new();
        root.controllers.aim.sights = sights;
        fail(resets, "controllers", error);
    }
    if let Err(error) = reset_named(
        &mut root.headpose.vr_turn,
        &VrTurnConfig::new(),
        "headpose.vr_turn",
        validate::vr_turn,
        resets,
    ) {
        root.headpose.vr_turn = VrTurnConfig::new();
        fail(resets, "headpose.vr_turn", error);
    }
    if let Err(error) = reset_named(
        &mut root.hud,
        &HudConfig::new(),
        "hud",
        HudConfig::validate,
        resets,
    ) {
        root.hud = HudConfig::new();
        fail(resets, "hud", error);
    }
    if let Err(error) = root.body.validate() {
        root.body = BodyDimensions::GENERIC_ADULT;
        fail(resets, "body", Invalid(error.to_string()));
    }
}
