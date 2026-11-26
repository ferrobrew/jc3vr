//! Posture and roomscale: the player's posture, crouching, the head fade, and the seat transitions.
//! The automatic posture's detection, the seated eye reference, and the comfort fade's timing are
//! fixed in their modules under [`crate::controllers::roomscale`]. The chase that walks the body
//! after the head is the rig's ([`crate::controllers::body::rig::config::ChaseConfig`]).

use serde::{Deserialize, Serialize};

/// Posture and roomscale locomotion (see [`crate::controllers::roomscale`]). These act on the
/// headset alone, controllers or not, and default on as part of the full gameplay preset. The head
/// height itself is remapped only under [`BodyRemapConfig::remap_head`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// Crouching.
    pub crouch: CrouchConfig,
    /// Fading the view when the head goes deep into geometry the body cannot follow it through.
    pub head_fade: HeadFadeConfig,
    /// How the view behaves while the character enters or exits a vehicle seat.
    pub seat_transition: SeatTransition,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            crouch: CrouchConfig::new(),
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

/// Crouching.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrouchConfig {
    /// Shorten the character's collision capsule while crouched, with the game's own crouching
    /// proxy state. The game hands the proxy state to its own states (swimming, vehicles, the
    /// parachute, ragdoll) whenever they want it.
    pub capsule: bool,
    /// How far (metres, at the character's scale) the head must drop below the standing eye height
    /// before the character counts as crouched, for the capsule and the readouts. It stands again
    /// at [`CROUCH_EXIT_DROP_M`](crate::controllers::roomscale::crouch::CROUCH_EXIT_DROP_M).
    pub enter_drop_m: f32,
}

impl CrouchConfig {
    pub const fn new() -> Self {
        Self {
            capsule: true,
            enter_drop_m: 0.45,
        }
    }
}

impl Default for CrouchConfig {
    fn default() -> Self {
        Self::new()
    }
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
