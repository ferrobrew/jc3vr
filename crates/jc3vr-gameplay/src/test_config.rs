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
//! The root registers itself before the test harness runs; see
//! [`test_root`](jc3vr_core::config::engine::test_root).

use jc3vr_core::{
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

use crate::{
    controllers::{
        aim::{GRAPPLE_SEAT, WORLD_MARKERS_ENABLED, config::AIM_CONFIG},
        aim_state::AIM_STATE_CONFIG,
        bindings::config::BINDINGS_CONFIG,
        body::{BODY_CONFIG, BodyDimensions, config::BODY_REMAP_CONFIG},
        config::{CONTROLLERS_CONFIG, ControllersConfig},
        fingers::config::FINGERS_CONFIG,
        glass::config::GLASS_CONFIG,
        hands::config::{GRAPPLE_PUMP, HANDS_CONFIG, MOUNTED_AIM_BY_HANDS},
        haptics::config::HAPTICS_CONFIG,
        locomotion::config::LOCOMOTION_CONFIG,
        overlay::config::OVERLAY_CONFIG,
        physical_hands::config::PHYSICAL_HANDS_CONFIG,
        roomscale::config::{DRIVING_VIEW_ENABLED, ROOMSCALE_CONFIG, TAKE_OFF_TO_HEAD},
        scheme,
        summary::config::SUMMARY_CONFIG,
        vehicles::config::VEHICLES_CONFIG,
        weapons::config::WEAPONS_CONFIG,
    },
    grapple::physical::config::GRAPPLE_CONFIG,
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

jc3vr_core::test_root! {
    root {
        stereo: StereoConfig = StereoConfig::new(),
        exposure: ExposureConfig = ExposureConfig::new(),
        foveation: FoveationConfig = FoveationConfig::new(),
        far_field: FarFieldConfig = FarFieldConfig::new(),
        post_fx: PostFxConfig = PostFxConfig::new(),
        camera: CameraConfig = CameraConfig::new(),
        movement: MovementConfig = MovementConfig::new(),
        fsr: FsrConfig = FsrConfig::new(),
        hud: Hud {
            egui_panel: EguiPanel {
                enabled: bool = false,
                show_on_mirror: bool = false,
                resolution: (u32, u32) = (1280, 960),
            },
            world_markers: WorldMarkers {
                enabled: bool = true,
            },
        },
        headpose: HeadPoseConfig = HeadPoseConfig::new(),
        body_ik: BodyIkConfig = BodyIkConfig::new(),
        vr: VrConfig = VrConfig::new(),
        director: Director {
            time_scale: f32 = 1.0,
            paused: bool = false,
            #[serde(skip, default = "CleanViewConfig::new")]
            clean_view: CleanViewConfig = CleanViewConfig::new(),
        },
        controllers: ControllersConfig = ControllersConfig::new(),
        testing: Testing {
            enabled: bool = true,
            record_hand_joints: bool = true,
            texture_census: bool = false,
        },
        online: OnlineConfig = OnlineConfig::new(),
        body: BodyDimensions = BodyDimensions::GENERIC_ADULT,
    }
    binds {
        CONTROLLERS_CONFIG => controllers,
        BODY_CONFIG => body,
        MOVEMENT_CONFIG => movement,
        BODY_IK_CONFIG => body_ik,
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
