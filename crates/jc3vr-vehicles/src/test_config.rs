//! The configuration root of the crate's tests, registered once before any test runs.
//!
//! The payload binds every slot to its settings aggregate at startup. That aggregate sits above
//! this crate, so the crate's tests register a root of their own instead: it holds the vehicles
//! group itself, the groups of the layers the vehicle handling builds on (the body-bound
//! controllers groups, the body, the weapons, and the physical hands), the view and core groups read through their slots, and stand-ins for the
//! fields of higher layers' groups that a body slot projects (the grapple's switches and the world
//! markers' switch), all at the paths the payload's aggregate keeps them and with its defaults. A
//! test reads and changes the configuration through the slots exactly as the payload does, and
//! takes it exclusively with [`test_fixture`](jc3vr_core::config::engine::test_fixture).
//!
//! The root registers itself before the test harness runs; see
//! [`test_root`](jc3vr_core::config::engine::test_root).

use jc3vr_body::{
    controllers::{
        aim::{
            WORLD_MARKERS_ENABLED,
            config::{AIM_CONFIG, AimConfig},
        },
        body::{
            BODY_CONFIG, BodyDimensions,
            config::{BODY_REMAP_CONFIG, BodyRemapConfig},
        },
        hands::config::{HANDS_CONFIG, HandsConfig, MOUNTED_AIM_BY_HANDS},
        haptics::config::{HAPTICS_CONFIG, HapticsConfig},
        intent::config::{BINDINGS_CONFIG, BindingsConfig},
        locomotion::config::{LOCOMOTION_CONFIG, LocomotionConfig},
        overlay::config::{OVERLAY_CONFIG, OverlayConfig},
        roomscale::config::{
            DRIVING_VIEW_ENABLED, ROOMSCALE_CONFIG, RoomscaleConfig, TAKE_OFF_TO_HEAD,
        },
        scheme,
    },
    hooks::{
        animation::canopy::SKIP_REEL_OPENING,
        input::{
            parachute::{CANOPY_LINE_TURN, CANOPY_LINE_TURN_SCALE},
            wingsuit::WINGSUIT_EVADE_ENABLED,
        },
    },
};
use jc3vr_core::{
    director::clean_view::{CLEAN_VIEW_CONFIG, CleanViewConfig},
    hooks::online::{ONLINE_CONFIG, OnlineConfig},
};
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::physical_hands::config::{
    PHYSICAL_HANDS_CONFIG, PhysicalHandsConfig,
};
use jc3vr_view::{
    fsr::{FSR_CONFIG, FsrConfig},
    headpose::config::{HEADPOSE_CONFIG, HeadPoseConfig},
    hooks::camera::config::{CAMERA_CONFIG, CameraConfig},
    recorder::tracking::{RECORD_HAND_JOINTS, TESTING_ENABLED, TIME_SCALE},
    stereo::config::{STEREO_CONFIG, StereoConfig},
    vr::{
        FOVEATION_CONFIG, FoveationConfig, PANEL_ENABLED, PANEL_ON_MIRROR, PANEL_RESOLUTION,
        VR_CONFIG, VrConfig,
        input::config::{
            CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_ENABLED, HAPTICS_INTENSITY, INPUT_CONFIG,
            InputConfig,
        },
    },
};
use jc3vr_weapons::controllers::weapons::config::{WEAPONS_CONFIG, WeaponsConfig};

use crate::controllers::vehicles::config::{VEHICLES_CONFIG, VehiclesConfig};

jc3vr_core::test_root! {
    root {
        stereo: StereoConfig = StereoConfig::new(),
        foveation: FoveationConfig = FoveationConfig::new(),
        camera: CameraConfig = CameraConfig::new(),
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
        vr: VrConfig = VrConfig::new(),
        director: Director {
            time_scale: f32 = 1.0,
            #[serde(skip, default = "CleanViewConfig::new")]
            clean_view: CleanViewConfig = CleanViewConfig::new(),
        },
        controllers: Controllers {
            enabled: bool = true,
            dominant_hand: Hand = Hand::Right,
            input: InputConfig = InputConfig::new(),
            locomotion: LocomotionConfig = LocomotionConfig::new(),
            bindings: BindingsConfig = BindingsConfig::new(),
            hands: HandsConfig = HandsConfig::new(),
            aim: AimConfig = AimConfig::new(),
            overlay: OverlayConfig = OverlayConfig::new(),
            body: BodyRemapConfig = BodyRemapConfig::new(),
            roomscale: RoomscaleConfig = RoomscaleConfig::new(),
            haptics: HapticsConfig = HapticsConfig::new(),
            weapons: WeaponsConfig = WeaponsConfig::new(),
            vehicles: VehiclesConfig = VehiclesConfig::new(),
            physical_hands: PhysicalHandsConfig = PhysicalHandsConfig::new(),
        },
        testing: Testing {
            enabled: bool = true,
            record_hand_joints: bool = true,
        },
        online: OnlineConfig = OnlineConfig::new(),
        body: BodyDimensions = BodyDimensions::GENERIC_ADULT,
    }
    binds {
        BODY_CONFIG => body,
        scheme::CONTROLLERS_ENABLED => controllers.enabled,
        scheme::DOMINANT_HAND => controllers.dominant_hand,
        LOCOMOTION_CONFIG => controllers.locomotion,
        BINDINGS_CONFIG => controllers.bindings,
        HANDS_CONFIG => controllers.hands,
        AIM_CONFIG => controllers.aim,
        OVERLAY_CONFIG => controllers.overlay,
        BODY_REMAP_CONFIG => controllers.body,
        ROOMSCALE_CONFIG => controllers.roomscale,
        HAPTICS_CONFIG => controllers.haptics,
        WEAPONS_CONFIG => controllers.weapons,
        VEHICLES_CONFIG => controllers.vehicles,
        PHYSICAL_HANDS_CONFIG => controllers.physical_hands,
        MOUNTED_AIM_BY_HANDS => controllers.vehicles.mounted.aim_by_hands,
        DRIVING_VIEW_ENABLED => controllers.vehicles.driving_view.enabled,
        TAKE_OFF_TO_HEAD => controllers.vehicles.take_off_to_head,
        CANOPY_LINE_TURN => controllers.vehicles.canopy.line_turn,
        CANOPY_LINE_TURN_SCALE => controllers.vehicles.canopy.line_turn_scale,
        SKIP_REEL_OPENING => controllers.vehicles.canopy.skip_reel_opening,
        WINGSUIT_EVADE_ENABLED => controllers.vehicles.wingsuit_evade.enabled,
        WORLD_MARKERS_ENABLED => hud.world_markers.enabled,
        STEREO_CONFIG => stereo,
        FOVEATION_CONFIG => foveation,
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
        TIME_SCALE => director.time_scale,
        CLEAN_VIEW_CONFIG => director.clean_view,
    }
}
