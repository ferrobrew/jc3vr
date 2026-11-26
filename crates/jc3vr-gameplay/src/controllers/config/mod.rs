//! The motion-controller configuration: the master switch and one sub-struct per
//! subsystem of the plan of record (`docs/mod/input/controllers-and-roomscale.md`), each holding its
//! toggle and the tuning beside it.
//!
//! `ControllersConfig::new()` ships as the full gameplay preset
//! (`presets::Preset::Controllers`),
//! so that hand aiming, roomscale, weapon handling, vehicle steering (including the wingsuit's arm
//! steering and the parachute's handles), and the debug gizmos all work, and show, out of the box.
//! `presets::preset_modified` checked against that preset stays false for the defaults.
//!
//! Stick turning is not configured here: the controller scheme's right stick drives the same body
//! yaw as the gamepad's look input, so it shares
//! [`VrTurnConfig`](jc3vr_view::headpose::config::VrTurnConfig) (`headpose.vr_turn`) rather than keeping
//! a second copy of the smooth and snap settings.
//!
//! Each group's type and its slot live beside the subsystem that owns them (`WEAPONS_CONFIG` beside
//! [`WeaponsConfig`] in [`jc3vr_weapons::controllers::weapons::config`], `controllers.enabled` in
//! [`jc3vr_body::controllers::scheme`]), and every subsystem reads its own group through its slot. This
//! module only assembles the groups into the `controllers` table of the settings file, and the
//! payload's load runs the range and consistency checks in [`validate`] over the whole table.

pub mod validate;

use jc3vr_body::controllers::{
    aim::config::AimConfig, aim_state::AimStateConfig, body::config::BodyRemapConfig,
    fingers::config::FingersConfig, hands::config::HandsConfig, haptics::HapticsConfig,
    intent::config::BindingsConfig, locomotion::config::LocomotionConfig,
    overlay::config::OverlayConfig, roomscale::config::RoomscaleConfig, scheme,
};
use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_grapple::grapple::physical::PhysicalGrappleConfig;
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::{glass::GlassConfig, physical_hands::PhysicalHandsConfig};
use jc3vr_vehicles::controllers::vehicles::config::VehiclesConfig;
use jc3vr_view::vr::input::config::InputConfig;
pub use jc3vr_weapons::controllers::weapons::config::WeaponsConfig;
use serde::{Deserialize, Serialize};

use crate::controllers::summary::SummaryConfig;

/// The controller settings, at `controllers` in the settings file; the payload binds the slot at
/// startup.
pub static CONTROLLERS_CONFIG: ConfigSlot<ControllersConfig> = ConfigSlot::new("controllers");

/// All motion-controller settings, added to `config::Config` as `controllers`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllersConfig {
    /// Master switch. Off, the mod reads no controller input and publishes no controller frame, so
    /// every controller subsystem is inert and the gamepad scheme is in effect. The OpenXR actions
    /// are still created and attached with the session, because attachment is possible only once
    /// per session and turning this on later must not require a new one.
    pub enabled: bool,
    /// The hand that holds and fires the guns. The other hand fires the grapple. Mirroring swaps the
    /// bindings and the dual-wield pistols' attach bones; a single gun stays on the model's right
    /// hand, and the grapple gauntlet on the model's left arm. The hand roles follow
    /// [`ControllersConfig::acting_hand`], which differs from this until a restart after a change.
    pub dominant_hand: Hand,
    /// Raw input interpretation: the derived press thresholds and hand tracking.
    pub input: InputConfig,
    /// On-foot stick locomotion.
    pub locomotion: LocomotionConfig,
    /// The controller-to-game bindings.
    pub bindings: BindingsConfig,
    /// The aim-state pin and the mental-state cuts.
    pub aim_state: AimStateConfig,
    /// Controller-owned hands.
    pub hands: HandsConfig,
    /// Per-hand aiming.
    pub aim: AimConfig,
    /// The world-space overlay layer's readouts.
    pub overlay: OverlayConfig,
    /// Physical weapon handling.
    pub weapons: WeaponsConfig,
    /// Body scale remapping (see [`jc3vr_body::controllers::body`]).
    pub body: BodyRemapConfig,
    /// Posture and roomscale locomotion.
    pub roomscale: RoomscaleConfig,
    /// Vehicles, mounted guns, the wingsuit, and the parachute.
    pub vehicles: VehiclesConfig,
    /// Finger curls on the rendered hands.
    pub fingers: FingersConfig,
    /// The palms' collision with the world.
    pub physical_hands: PhysicalHandsConfig,
    /// The controls summary, and the game's own button hints.
    pub summary: SummaryConfig,
    /// The physical grapple: the reel, the tether retract, and the hang swing worked by the grapple
    /// hand.
    pub grapple: PhysicalGrappleConfig,
    /// Controller vibration: the master switch and strength, the category switches, and the
    /// game's rumble.
    pub haptics: HapticsConfig,
    /// Breaking window glass with a palm or a held gun.
    pub glass: GlassConfig,
}

impl ControllersConfig {
    /// The dominant hand the mod's hand roles follow: the gun hand, the grapple's ray, the
    /// readouts on the wrists, the mirrored sticks and levers, and the bindings' reading of the
    /// buttons. While controller bindings are attached it is the hand they were suggested for,
    /// since OpenXR keeps those until the game restarts and the buttons stay laid out for it; a
    /// change of [`ControllersConfig::dominant_hand`] takes effect for the roles at the same
    /// restart as for the buttons, so the two never disagree.
    #[inline]
    pub fn acting_hand(&self) -> Hand {
        scheme::acting_hand_for(self.dominant_hand)
    }

    pub const fn new() -> Self {
        Self {
            enabled: true,
            dominant_hand: Hand::Right,
            input: InputConfig::new(),
            locomotion: LocomotionConfig::new(),
            bindings: BindingsConfig::new(),
            aim_state: AimStateConfig::new(),
            hands: HandsConfig::new(),
            aim: AimConfig::new(),
            overlay: OverlayConfig::new(),
            weapons: WeaponsConfig::new(),
            body: BodyRemapConfig::new(),
            roomscale: RoomscaleConfig::new(),
            vehicles: VehiclesConfig::new(),
            fingers: FingersConfig::new(),
            physical_hands: PhysicalHandsConfig::new(),
            summary: SummaryConfig::new(),
            grapple: PhysicalGrappleConfig::new(),
            haptics: HapticsConfig::new(),
            glass: GlassConfig::new(),
        }
    }
}

impl Default for ControllersConfig {
    fn default() -> Self {
        Self::new()
    }
}
