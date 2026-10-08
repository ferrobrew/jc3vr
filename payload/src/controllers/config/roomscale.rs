//! Posture and roomscale locomotion: the player's posture, the automatic posture's detection, the
//! seated eye reference, the roomscale chase loop, crouching, and the comfort fades.

use serde::{Deserialize, Serialize};

use crate::controllers::roomscale::stance::StanceConfig;

/// Posture and roomscale locomotion (see [`crate::controllers::roomscale`]). These act on the
/// headset alone, controllers or not, and default on as part of the full gameplay preset. The head
/// height itself is remapped only under [`BodyRemapConfig::remap_head`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomscaleConfig {
    /// The player's posture.
    pub posture: Posture,
    /// In the automatic posture, the smallest play area (both sides, metres) that counts as a
    /// room-scale setup. A runtime reports a standing-only setup as no bounds or a very small
    /// rectangle.
    pub roomscale_min_extent_m: f32,
    /// In the automatic posture, how the head's height above the floor tells a seated player from
    /// a standing one.
    pub auto_posture: AutoPostureConfig,
    /// In the seated posture, how the head height that maps to the character's eyes follows the
    /// player.
    pub seated_reference: SeatedReferenceConfig,
    /// The body chasing the physical head in the roomscale posture.
    pub chase: ChaseConfig,
    /// Crouching.
    pub crouch: CrouchConfig,
    /// Fading the view when the head goes deep into geometry the body cannot follow it through.
    pub head_fade: HeadFadeConfig,
    /// How fast the comfort fade darkens and clears.
    pub fade: FadeConfig,
    /// How the view behaves while the character enters or exits a vehicle seat.
    pub seat_transition: SeatTransition,
    /// Under [`SeatTransition::Fade`], how long the view stays dark after the seat locks, so that
    /// the re-based view settles before it clears, seconds.
    pub seat_fade_hold_s: f32,
    /// Keeping the standing stance square with the player's.
    pub stance: StanceConfig,
}

impl RoomscaleConfig {
    pub const fn new() -> Self {
        Self {
            posture: Posture::Auto,
            roomscale_min_extent_m: 1.0,
            auto_posture: AutoPostureConfig::new(),
            seated_reference: SeatedReferenceConfig::new(),
            chase: ChaseConfig::new(),
            crouch: CrouchConfig::new(),
            head_fade: HeadFadeConfig::new(),
            fade: FadeConfig::new(),
            seat_transition: SeatTransition::Ride,
            seat_fade_hold_s: 0.3,
            stance: StanceConfig::new(),
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
    /// Seated or standing by the head's height above the floor ([`AutoPostureConfig`]), seated
    /// until the head has been located; standing becomes roomscale when the runtime reports a
    /// room-scale boundary.
    #[default]
    Auto,
    /// The seated eye reference ([`SeatedReferenceConfig`]) maps to the character's standing eye
    /// height, roomscale translation is off, and a button toggles crouching.
    Seated,
    /// STAGE space: the true floor height, roomscale translation off, physical crouching.
    Standing,
    /// STAGE space with the roomscale chase loop on.
    Roomscale,
}

/// The automatic posture's seated and standing detection
/// ([`crate::controllers::roomscale::posture::PostureClassifier`]). The thresholds sit between the
/// calibrated seated eye height (or an estimate from the standing eye height) and the standing eye
/// height; the dwell times decide how long the head must stay past them.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoPostureConfig {
    /// How long a seated player's head must stay at standing height before the posture becomes
    /// standing, seconds. Short: a seated player cannot raise the head that far without standing
    /// up.
    pub stand_after_s: f32,
    /// How long a standing player's head must stay at seated height before the posture becomes
    /// seated, seconds. Long, so that a standing player crouching behind cover keeps the physical
    /// crouch.
    pub sit_after_s: f32,
    /// Count the sit wait only while a standing player's head is steady at a chair's height: a
    /// head at floor level (sitting or kneeling on the floor of the play area) or one that bobs
    /// (a crouch peeking out of cover) stays standing.
    pub chair_height_only: bool,
}

impl AutoPostureConfig {
    pub const fn new() -> Self {
        Self {
            stand_after_s: 1.0,
            sit_after_s: 20.0,
            chair_height_only: true,
        }
    }
}

impl Default for AutoPostureConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The seated eye reference ([`crate::controllers::roomscale::seated`]): the head height above the
/// floor that the seated posture puts at the character's eyes. It starts at the calibrated seated
/// eye height, a manual recenter while seated captures the head's height, and the adaptive
/// baseline moves it toward how the player actually sits.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SeatedReferenceConfig {
    /// Move the reference toward the median head height over the window. On by default: without it
    /// a reference that does not match how the player sits holds the view above or below the
    /// character's eyes until the next manual recenter.
    pub adaptive: bool,
    /// The window the median is taken over, seconds. Head movement held for less than half of it
    /// (a lean, a duck, a look around) does not move the median.
    pub window_s: f32,
    /// The fastest the adaptive baseline moves the reference, metres per second.
    pub rate_mps: f32,
    /// How far the median must be from the reference before the adaptive baseline moves it,
    /// metres.
    pub deadband_m: f32,
}

impl SeatedReferenceConfig {
    pub const fn new() -> Self {
        Self {
            adaptive: true,
            window_s: 120.0,
            rate_mps: 0.01,
            deadband_m: 0.02,
        }
    }
}

impl Default for SeatedReferenceConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The roomscale chase loop: in the roomscale posture, the body walks toward the physical head.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ChaseConfig {
    /// On by default, as part of the full gameplay preset: it moves the character from head
    /// motion, which the headset alone produces.
    pub enabled: bool,
    /// Horizontal head offsets from the body below this many metres are left alone. A chase that
    /// has started runs until the offset is below half of it.
    pub deadzone_m: f32,
    /// The fastest the chase moves the body, metres per second.
    pub max_speed_mps: f32,
    /// How quickly the chase closes the offset: the chase speed is the offset divided by this,
    /// seconds, before the speed limit. A few simulation ticks, so that the body does not overshoot
    /// the head.
    pub time_constant_s: f32,
    /// The shortest single-tick jump of the body, metres, that counts as a teleport the game made
    /// without announcing it. The threshold also grows with the body's speed.
    pub warp_min_m: f32,
}

impl ChaseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            deadzone_m: 0.10,
            max_speed_mps: 3.0,
            time_constant_s: 0.15,
            warp_min_m: 8.0,
        }
    }
}

impl Default for ChaseConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Crouching.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrouchConfig {
    /// Fold the body with the lowered head through the body IK: the feet stay planted and the hips
    /// drop. On by default, as part of the full gameplay preset: it changes the body for every
    /// player who ducks.
    pub visual: bool,
    /// Shorten the character's collision capsule while crouched, with the game's own crouching
    /// proxy state. Requires [`visual`](Self::visual). The game hands the proxy state to its own
    /// states (swimming, vehicles, the parachute, ragdoll) whenever they want it.
    pub capsule: bool,
    /// How much of the head's drop the hips take, `0..=1`. A squat lowers the hips nearly as far as
    /// the head; the spine takes the rest.
    pub hips_share: f32,
    /// How far (metres, at the character's scale) the head must drop below the standing eye height
    /// before the character counts as crouched, for the capsule and the readouts.
    pub enter_drop_m: f32,
    /// How far the head must come back up (metres below the standing eye height) before the
    /// character counts as standing again.
    pub exit_drop_m: f32,
    /// In the seated posture, how far (metres, at the character's scale) the crouch toggle lowers
    /// the eyes.
    pub seated_drop_m: f32,
}

impl CrouchConfig {
    pub const fn new() -> Self {
        Self {
            visual: true,
            capsule: true,
            hips_share: 0.8,
            enter_drop_m: 0.45,
            exit_drop_m: 0.30,
            seated_drop_m: 0.6,
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
    /// whose head enters a wall.
    pub enabled: bool,
    /// How deep (metres) past the obstruction the head may go before the view starts to darken.
    pub start_depth_m: f32,
    /// How deep (metres) past the obstruction the view is fully dark.
    pub full_depth_m: f32,
}

impl HeadFadeConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            start_depth_m: 0.05,
            full_depth_m: 0.25,
        }
    }
}

impl Default for HeadFadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The comfort fade's timing, shared by every source (the head in geometry, the seat transitions).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FadeConfig {
    /// How long a fade from clear to fully dark takes, seconds.
    pub out_s: f32,
    /// How long a fade from fully dark to clear takes, seconds.
    pub in_s: f32,
}

impl FadeConfig {
    pub const fn new() -> Self {
        Self {
            out_s: 0.15,
            in_s: 0.3,
        }
    }
}

impl Default for FadeConfig {
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
