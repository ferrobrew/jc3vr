//! Source evidence remains available without executing any existing UI page or game access.

use crate::ui::inventory::{FeatureId, SourceFile, Surface};

macro_rules! debug_sources {
    ($($path:literal => [$($feature:ident),+]),+ $(,)?) => {
        &[$(SourceFile {
            path: concat!("payload/src/ui/", $path),
            text: include_str!(concat!("../", $path)),
            surface: Surface::Debug,
            destinations: &[$(FeatureId::$feature),+],
        }),+]
    };
}

pub(crate) const DEBUG_SOURCES: &[SourceFile] = debug_sources! {
    "mod.rs" => [Session, MenuPanel, Hud],
    "camera.rs" => [HeadView, AnimationIk, Locomotion, Grapple],
    "diagnostics.rs" => [RenderDiagnostics, RenderCapture],
    "director.rs" => [DirectorSafety, DirectorSpawn, Video, Keyboard],
    "environment.rs" => [Environment],
    "game.rs" => [Locomotion, Swimming, Wingsuit, Parachute, Driving, DirectorSpawn, Engine],
    "hud.rs" => [Hud, Previews, MenuPanel],
    "performance.rs" => [Performance],
    "previews.rs" => [Previews],
    "status.rs" => [AllStatus],
    "testing.rs" => [TrackingCapture, Replay],
    "vr.rs" => [Runtime, Resolution, HeadView, AnimationIk],
    "controllers/mod.rs" => [Session, Devices, Bindings],
    "controllers/aim.rs" => [Aim],
    "controllers/aim_state.rs" => [AnimationIk],
    "controllers/body.rs" => [Calibration],
    "controllers/climb.rs" => [Climbing],
    "controllers/collider_view.rs" => [PhysicalHands, CharactersDoors],
    "controllers/diagnostics.rs" => [ControllerCapture],
    "controllers/explosives.rs" => [Explosives],
    "controllers/fingers.rs" => [Fingers],
    "controllers/frame.rs" => [Semantic],
    "controllers/grab_view.rs" => [Hold, Snatch, PhysicalHands],
    "controllers/hand_bodies.rs" => [PhysicalHands],
    "controllers/hands.rs" => [HandTracking, Presentation, Grapple],
    "controllers/haptics.rs" => [Feedback],
    "controllers/inventory.rs" => [Inventory],
    "controllers/loadout.rs" => [Inventory, Sandbox],
    "controllers/locomotion.rs" => [Locomotion],
    "controllers/map.rs" => [MenuPanel],
    "controllers/mode.rs" => [Bindings],
    "controllers/overlay.rs" => [PhysicalHands, HandTracking, Hold, RootWalls, Snatch],
    "controllers/physical_hands.rs" => [PhysicalHands, CharactersDoors, Feedback],
    "controllers/pouch_models.rs" => [Inventory],
    "controllers/presets.rs" => [Presets],
    "controllers/procedural.rs" => [Procedural],
    "controllers/reload.rs" => [Reload],
    "controllers/rig.rs" => [RootWalls],
    "controllers/roomscale.rs" => [Posture],
    "controllers/scope.rs" => [Aim],
    "controllers/semantic.rs" => [Semantic],
    "controllers/settings.rs" => [Devices, Thresholds, Bindings, MenuPanel, Grapple, Locomotion, AnimationIk],
    "controllers/sights.rs" => [Aim],
    "controllers/stunt.rs" => [Climbing],
    "controllers/summary.rs" => [Wrist],
    "controllers/torso.rs" => [Torso],
    "controllers/unarmed.rs" => [PhysicalHands],
    "controllers/vehicles.rs" => [Driving, VehicleControls, Mounted, Wingsuit, Parachute],
    "controllers/weapons.rs" => [Hold, Support, Snatch, Reload, Inventory, Explosives, WeaponSelection],
    "render/mod.rs" => [Stereo, Resolution, Graphics],
    "render/capture.rs" => [RenderCapture],
    "render/corrections.rs" => [Stereo],
    "render/far_field.rs" => [Graphics],
    "render/passes.rs" => [RenderDiagnostics],
    "render/post_fx.rs" => [Stereo, RenderDiagnostics],
    "render/resolution.rs" => [Resolution],
    "render/single_pass.rs" => [Graphics],
    "render/stereo.rs" => [Stereo, Resolution],
};

macro_rules! pause_sources {
    ($($path:literal => [$($feature:ident),+]),+ $(,)?) => {
        &[$(SourceFile {
            path: concat!("payload/src/", $path),
            text: include_str!(concat!("../../", $path)),
            surface: Surface::Pause,
            destinations: &[$(FeatureId::$feature),+],
        }),+]
    };
}

pub(crate) const PAUSE_SOURCES: &[SourceFile] = pause_sources! {
    "controllers/body/menu/mod.rs" => [Calibration],
    "controllers/body/menu/pages.rs" => [Calibration, Procedural, Presentation, Torso],
    "controllers/menu/controls/mod.rs" => [Devices, Wingsuit, Parachute],
    "controllers/menu/controls/sections.rs" => [Devices, Locomotion, Aim, Hold, Reload, WeaponSelection, Support, Feedback, VehicleControls, Parachute],
    "controllers/menu/comfort.rs" => [HeadView, Posture, RootWalls, Locomotion, Driving],
    "controllers/menu/display.rs" => [Resolution, Hud],
};
