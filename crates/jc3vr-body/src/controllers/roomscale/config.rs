//! Posture and roomscale: the player's posture, the head fade, and the seat transitions.
//! The automatic posture's detection, the seated eye reference, the crouch, and the comfort fade's timing are
//! fixed in their modules under [`crate::controllers::roomscale`]. The chase that walks the body
//! after the head is the rig's ([`crate::controllers::body::rig::config::ChaseConfig`]).

use jc3vr_core::config::engine::ConfigSlot;
use serde::{Deserialize, Serialize};

/// The posture and roomscale settings, at `controllers.roomscale` in the settings file; the payload
/// binds the slot at startup.
pub static ROOMSCALE_CONFIG: ConfigSlot<RoomscaleConfig> = ConfigSlot::new("controllers.roomscale");

/// Whether the driver's view moves to a standard place behind the wheel, at
/// `controllers.vehicles.driving_view.enabled` in the settings file: a field of the vehicles' group,
/// which the cockpit composition reads. The payload binds the slot at startup.
pub static DRIVING_VIEW_ENABLED: ConfigSlot<bool> =
    ConfigSlot::new("controllers.vehicles.driving_view.enabled");

/// Whether the body turns to the head's heading when the boost wingsuit takes off from the ground,
/// at `controllers.vehicles.take_off_to_head` in the settings file: a field of the vehicles' group,
/// which the roomscale tick reads. The payload binds the slot at startup.
pub static TAKE_OFF_TO_HEAD: ConfigSlot<bool> =
    ConfigSlot::new("controllers.vehicles.take_off_to_head");

/// Posture and roomscale locomotion (see [`crate::controllers::roomscale`]). These act on the
/// headset alone, controllers or not, and default on as part of the full gameplay preset. The head
/// height itself is remapped only under [`BodyRemapConfig::remap_head`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// Fading the view when the head goes deep into geometry the body cannot follow it through.
    pub head_fade: HeadFadeConfig,
    /// How the view behaves while the character enters or exits a vehicle seat.
    pub seat_transition: SeatTransition,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            head_fade: HeadFadeConfig::new(),
            seat_transition: SeatTransition::Ride,
        }
    }
}

impl Default for RoomscaleConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The player's posture.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Posture {
    /// Seated or standing by the head's height above the floor
    /// ([`AUTO_POSTURE`](crate::controllers::roomscale::posture::AUTO_POSTURE)), seated until the
    /// head has been located.
    #[default]
    Auto,
    /// The seated eye reference ([`crate::controllers::roomscale::seated`]) maps to the character's standing eye
    /// height, the body does not follow the head about the room, and a button toggles crouching.
    Seated,
    /// STAGE space: the true floor height, physical crouching, and the body following the head
    /// about the room. A saved `Roomscale` loads as this.
    #[serde(alias = "Roomscale")]
    Standing,
}

/// The head-through-geometry fade.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeadFadeConfig {
    /// On by default, as part of the full gameplay preset: it darkens the view for every player
    /// whose head enters a wall. The depths are
    /// [`HEAD_FADE_START_DEPTH_M`](crate::controllers::body::rig::collision::probe::HEAD_FADE_START_DEPTH_M)
    /// and [`HEAD_FADE_FULL_DEPTH_M`](crate::controllers::body::rig::collision::probe::HEAD_FADE_FULL_DEPTH_M).
    pub enabled: bool,
}

impl HeadFadeConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for HeadFadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// How the view behaves while the character enters or exits a vehicle seat.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SeatTransition {
    /// Re-base the cockpit the moment the seat locks. The entry animation plays behind a fade that
    /// clears at the lock; the exit plays live, and the view re-bases its yaw and height once the
    /// character is out.
    Instant,
    /// Fade out when the entry animation takes the camera, and fade in seated once the re-based
    /// view has settled. The exit also plays behind a fade.
    Fade,
    /// Keep the head anchored through the whole animation with nothing hidden. The default until
    /// the other modes are proven, because it is the mod's existing behaviour.
    #[default]
    Ride,
}
