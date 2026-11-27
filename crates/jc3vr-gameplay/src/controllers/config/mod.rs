//! The motion-controller configuration: the master switch and one group per subsystem
//! (`docs/mod/input/controllers.md`), each holding the settings a player
//! sets for it.
//!
//! `ControllersConfig::new()` ships as the full gameplay preset (the debug UI's
//! `presets::Preset::Controllers`), so that hand aiming, roomscale, weapon handling, and vehicle
//! steering (including the wingsuit's arm steering and the parachute's handles) all work out of the
//! box, and the debug UI's `presets::preset_modified` stays false for the defaults.
//!
//! Stick turning is not configured here: the controller scheme's right stick drives the same body
//! yaw as the gamepad's look input, so it shares
//! [`VrTurnConfig`](jc3vr_view::headpose::config::VrTurnConfig) (`headpose.vr_turn`) rather than
//! keeping a second copy of the smooth and snap settings.
//!
//! Each group's type and its slot live beside the subsystem that owns them (`WEAPONS_CONFIG` beside
//! [`WeaponsConfig`] in [`jc3vr_weapons::controllers::weapons::config`], `controllers.enabled` in
//! [`jc3vr_view::vr::input::config`]), and every subsystem reads its own group through its slot.
//! This module only assembles the groups into the `controllers` table of the settings file, binds
//! their slots wherever its own is bound, and has a load run the range and consistency checks in
//! [`validate`] over the whole table.

pub mod validate;

use jc3vr_body::controllers::{
    aim::config::{AIM_CONFIG, AimConfig},
    body::config::{BODY_REMAP_CONFIG, BodyRemapConfig},
    hands::config::{HANDS_CONFIG, HandsConfig},
    intent::config::{BINDINGS_CONFIG, BindingsConfig},
    locomotion::config::{LOCOMOTION_CONFIG, LocomotionConfig},
    overlay::config::{OVERLAY_CONFIG, OverlayConfig},
    roomscale::config::{ROOMSCALE_CONFIG, RoomscaleConfig},
    scheme,
};
use jc3vr_core::{
    config::{ConfigSlot, SlotOptions},
    slot_children,
};
use jc3vr_input::frame::Hand;
use jc3vr_physical::controllers::{
    glass::{GlassConfig, config::GLASS_CONFIG},
    physical_hands::{PhysicalHandsConfig, config::PHYSICAL_HANDS_CONFIG},
};
use jc3vr_vehicles::vehicles::config::{VEHICLES_CONFIG, VehiclesConfig};
use jc3vr_view::vr::input::config::{
    CONTROLLERS_ENABLED, DOMINANT_HAND, HAPTICS_CONFIG, HapticsConfig, INPUT_CONFIG, InputConfig,
};
use jc3vr_weapons::controllers::weapons::config::WEAPONS_CONFIG;
pub use jc3vr_weapons::controllers::weapons::config::WeaponsConfig;
use serde::{Deserialize, Serialize};

/// The controller settings, at `controllers` in the settings file; the payload binds the slot at
/// startup, which binds each group's slot to its field, and a load resets the settings that fail
/// their checks ([`validate::reset_invalid`]).
pub static CONTROLLERS_CONFIG: ConfigSlot<ControllersConfig> = ConfigSlot::with(
    "controllers",
    SlotOptions::new()
        .with_children(slot_children!(CONTROLLERS_CONFIG {
            CONTROLLERS_ENABLED => enabled,
            DOMINANT_HAND => dominant_hand,
            INPUT_CONFIG => input,
            LOCOMOTION_CONFIG => locomotion,
            BINDINGS_CONFIG => bindings,
            HANDS_CONFIG => hands,
            AIM_CONFIG => aim,
            OVERLAY_CONFIG => overlay,
            WEAPONS_CONFIG => weapons,
            BODY_REMAP_CONFIG => body,
            ROOMSCALE_CONFIG => roomscale,
            VEHICLES_CONFIG => vehicles,
            PHYSICAL_HANDS_CONFIG => physical_hands,
            HAPTICS_CONFIG => haptics,
            GLASS_CONFIG => glass,
        }))
        .validated(validate::reset_invalid),
);

/// All motion-controller settings, added to `config::Config` as `controllers`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllersConfig {
    /// Master switch. Off, the mod reads no controller input and publishes no controller frame, so
    /// every controller subsystem is inert and the gamepad scheme is in effect. The OpenXR actions
    /// are still created and attached with the session, because attachment is possible only once
    /// per session and turning this on later must not require a new one.
    pub enabled: bool,
    /// The hand that holds and fires the guns. The other hand fires the grapple. Mirroring swaps
    /// the bindings and the dual-wield pistols' attach bones; a single gun stays on the model's
    /// right hand, and the grapple gauntlet on the model's left arm. The hand roles follow
    /// [`ControllersConfig::acting_hand`], which differs from this until a restart after a change.
    pub dominant_hand: Hand,
    /// Raw input interpretation: the derived press thresholds and hand tracking.
    pub input: InputConfig,
    /// On-foot stick locomotion.
    pub locomotion: LocomotionConfig,
    /// The controller-to-game bindings.
    pub bindings: BindingsConfig,
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
    /// The palms' collision with the world.
    pub physical_hands: PhysicalHandsConfig,
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
            hands: HandsConfig::new(),
            aim: AimConfig::new(),
            overlay: OverlayConfig::new(),
            weapons: WeaponsConfig::new(),
            body: BodyRemapConfig::new(),
            roomscale: RoomscaleConfig::new(),
            vehicles: VehiclesConfig::new(),
            physical_hands: PhysicalHandsConfig::new(),
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
