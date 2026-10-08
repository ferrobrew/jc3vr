//! The motion-controller configuration: the master switch, the scheme, and one sub-struct per
//! subsystem of the plan of record (`docs/mod/input/controllers-and-roomscale.md`), each holding its
//! toggle and the tuning beside it.
//!
//! `ControllersConfig::new()` ships as the full gameplay preset
//! ([`presets::Preset::VehiclesAndAdvanced`](crate::controllers::presets::Preset::VehiclesAndAdvanced)),
//! so that hand aiming, roomscale, weapon handling, vehicle steering (including the wingsuit's arm
//! steering and the parachute's handles), and the debug gizmos all work, and show, out of the box.
//! `presets::preset_modified` checked against that preset stays false for the defaults.
//!
//! Stick turning is not configured here: the controller scheme's right stick drives the same body
//! yaw as the gamepad's look input, so it shares
//! [`VrTurnConfig`](crate::headpose::config::VrTurnConfig) (`headpose.vr_turn`) rather than keeping
//! a second copy of the smooth and snap settings.
//!
//! One file per subsystem cluster: [`input`] (raw input, locomotion, the mode detector, and the
//! bindings), [`aim`] (controller-owned hands and per-hand aiming), [`overlay`], [`weapons`],
//! [`body`], and [`roomscale`].

mod aim;
mod body;
mod input;
mod overlay;
mod roomscale;
mod weapons;

use serde::{Deserialize, Serialize};

pub use self::{
    aim::{AimConfig, AimStateConfig, GripOffset, HandsConfig},
    body::BodyRemapConfig,
    input::{BindingsConfig, InputConfig, LocomotionConfig, LocomotionDirection, ModeConfig},
    overlay::{GunReadout, OverlayConfig},
    roomscale::{
        AutoPostureConfig, ChaseConfig, FadeConfig, HeadFadeConfig, Posture, RoomscaleConfig,
        SeatTransition, SeatedReferenceConfig,
    },
    weapons::WeaponsConfig,
};
pub use crate::controllers::aim::sights::config::{SightOverride, SightTable, SightTuningConfig};
use crate::{
    controllers::{
        fingers::config::FingersConfig, frame::Hand, haptics::HapticsConfig,
        physical_hands::PhysicalHandsConfig, summary::SummaryConfig,
        vehicles::config::VehiclesConfig,
    },
    grapple::physical::PhysicalGrappleConfig,
};

/// All motion-controller settings, added to [`crate::config::Config`] as `controllers`.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ControllersConfig {
    /// Master switch. Off, the mod reads no controller input and publishes no controller frame, so
    /// every controller subsystem is inert and the gamepad scheme is in effect. The OpenXR actions
    /// are still created and attached with the session, because attachment is possible only once
    /// per session and turning this on later must not require a new one.
    pub enabled: bool,
    /// Which control scheme is in effect (see [`SchemeSetting`]).
    pub scheme: SchemeSetting,
    /// The hand that holds and fires the guns. The other hand fires the grapple. Mirroring swaps the
    /// bindings and the dual-wield pistols' attach bones; a single gun stays on the model's right
    /// hand, and the grapple gauntlet on the model's left arm. The hand roles follow
    /// [`ControllersConfig::acting_hand`], which differs from this until a restart after a change.
    pub dominant_hand: Hand,
    /// Pin the game's "the gamepad is the active input device" setting to the gamepad while the
    /// controller scheme is in effect, so that the game applies its gamepad rules (aim assist,
    /// tap-style melee) and shows gamepad prompts. The setting is restored when the scheme ends.
    pub gamepad_semantics: bool,
    /// Raw input interpretation: the derived press thresholds and hand tracking.
    pub input: InputConfig,
    /// On-foot stick locomotion.
    pub locomotion: LocomotionConfig,
    /// The per-tick mode detector.
    pub mode: ModeConfig,
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
    /// Body scale remapping (see [`crate::controllers::body`]).
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
}

impl ControllersConfig {
    /// The dominant hand the mod's hand roles follow: the gun hand, the grapple's ray, the
    /// readouts on the wrists, the mirrored sticks and levers, and the bindings' reading of the
    /// buttons. While controller bindings are attached it is the hand they were suggested for,
    /// since OpenXR keeps those until the game restarts and the buttons stay laid out for it; a
    /// change of [`ControllersConfig::dominant_hand`] takes effect for the roles at the same
    /// restart as for the buttons, so the two never disagree.
    pub fn acting_hand(&self) -> Hand {
        acting_hand(self.dominant_hand, crate::vr::input::bound_hand())
    }

    pub const fn new() -> Self {
        Self {
            enabled: true,
            scheme: SchemeSetting::Controllers,
            dominant_hand: Hand::Right,
            gamepad_semantics: true,
            input: InputConfig::new(),
            locomotion: LocomotionConfig::new(),
            mode: ModeConfig::new(),
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
        }
    }
}

impl Default for ControllersConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Which control scheme is in effect: the controller scheme (hand aiming, controller bindings,
/// controller-owned hands) or the gamepad scheme (gaze aiming, the game's own gamepad and keyboard
/// input, animated hands).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SchemeSetting {
    /// Switch on the most recent input: controller input selects the controller scheme, and
    /// gamepad, keyboard, or mouse input selects the gamepad scheme.
    #[default]
    Auto,
    /// Always the controller scheme, even when no controller is present.
    Controllers,
    /// Always the gamepad scheme.
    Gamepad,
}

/// The hand the roles follow for a configured `dominant` hand and the hand the attached bindings
/// were suggested for, `bound`, if any.
fn acting_hand(dominant: Hand, bound: Option<Hand>) -> Hand {
    bound.unwrap_or(dominant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roles_follow_the_bound_hand_until_a_restart() {
        assert_eq!(acting_hand(Hand::Left, None), Hand::Left);
        assert_eq!(acting_hand(Hand::Left, Some(Hand::Right)), Hand::Right);
        assert_eq!(acting_hand(Hand::Right, Some(Hand::Right)), Hand::Right);
    }

    #[test]
    fn the_sight_table_keys_by_weapon_hash() {
        let mut table = SightTable::EMPTY;
        assert!(table.set(SightOverride::identity(7)));
        let raised = SightOverride {
            offset_m: glam::Vec3::Y * 0.05,
            ..SightOverride::identity(7)
        };
        assert!(table.set(raised));
        assert_eq!(table.entries.iter().flatten().count(), 1);
        assert_eq!(table.get(7), Some(&raised));
        table.remove(7);
        assert!(table.get(7).is_none());
    }

    #[test]
    fn a_full_sight_table_refuses_a_new_weapon() {
        let mut table = SightTable::EMPTY;
        for hash in 0..SightTable::CAPACITY as u32 {
            assert!(table.set(SightOverride::identity(hash)));
        }
        assert!(!table.set(SightOverride::identity(1000)));
        assert!(table.set(SightOverride::identity(3)));
    }
}
