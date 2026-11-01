//! The grenade throw's and the catch's fixed tuning, and the feature switch the catch takes from the
//! configuration ([`GrenadeConfig`]).

use glam::Vec3;

use crate::controllers::{hands::held::HoldPlacement, weapons::config::GrenadeConfig};

/// The grenade throw's tuning with the catch switched on.
pub const GRENADE: GrenadeTuning = GrenadeTuning::new();

/// The physical grenade throw: its hold, its release, its fuse warning, and the catch.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GrenadeTuning {
    /// Where the held grenade's model sits in the hand ([`HoldPlacement`]): upright in the fist,
    /// its fuse head out of the top of the fist past the thumb, its lever along the palm, and its
    /// body out of the palm by the body's radius.
    pub hold: HoldPlacement,
    /// Aim the game's own throw at the point the dominant hand's ray reaches rather than at the
    /// gaze. No controller input binds the game's throw (grenades come from the pouch), so this
    /// matters only to a throw the game starts by other means.
    pub hand_aimed_fallback: bool,
    /// How long before the fuse runs out the held grenade starts to warn, seconds.
    pub warning_s: f32,
    /// Draw a ring at the throwing hand while a grenade is held, turning from green to red as the
    /// fuse runs down.
    pub warning_overlay: bool,
    /// Pulse the throwing hand as the fuse runs down, faster toward the end.
    pub warning_haptics: bool,
    /// The gap between warning pulses when the warning starts, and when the fuse is about to run
    /// out, seconds.
    pub warning_interval_s: [f32; 2],
    /// Catching live grenades.
    pub catch: CatchTuning,
}

impl GrenadeTuning {
    const fn new() -> Self {
        Self {
            hold: HOLD,
            hand_aimed_fallback: true,
            warning_s: 2.0,
            warning_overlay: true,
            warning_haptics: true,
            warning_interval_s: [0.5, 0.08],
            catch: CatchTuning::new(),
        }
    }

    /// The fixed tuning with the switches `config` sets.
    pub const fn of(config: &GrenadeConfig) -> Self {
        Self {
            catch: CatchTuning {
                enabled: config.catch.enabled,
                ..GRENADE.catch
            },
            ..GRENADE
        }
    }
}

/// Catching a live grenade near the player and throwing it on.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CatchTuning {
    /// Let a free hand catch a live grenade with a grip press at it.
    pub enabled: bool,
    /// The radius of a live grenade's grab volume, metres. A grenade in flight moves several
    /// centimetres between two ticks, so it is larger than a pouch's.
    pub radius_m: f32,
    /// How far from the player a live grenade may be to be caught, metres.
    pub range_m: f32,
    /// How long after the player's own throw the grenade cannot be caught, seconds, so that the
    /// hand that threw it does not take it back as it leaves.
    pub own_grace_s: f32,
    /// The fastest a caught grenade is moved toward the hand, metres per second.
    pub max_carry_mps: f32,
}

impl CatchTuning {
    const fn new() -> Self {
        Self {
            enabled: true,
            radius_m: 0.18,
            range_m: 4.0,
            own_grace_s: 0.4,
            max_carry_mps: 25.0,
        }
    }
}

/// The grenade's hold, derived from its model (the grenade weapon's projectile model): the origin
/// is the middle of the round body, the body runs along the model's +Y up to the fuse head, and the
/// lever lies down the body's +Z side. Upright in the fist, the model's +Y runs up the finger tube
/// (the grip's -Z) and its +Z, the lever, into the palm (the right grip's +X). The body's centre is
/// a body radius and a little out of the palm, level with the middle of the palm along the hand,
/// and a little toward the little finger so that the fuse head clears the top of the fist.
pub const HOLD: HoldPlacement = HoldPlacement {
    offset_m: Vec3::new(-(GRENADE_BODY_RADIUS_M + 0.002), 0.01, 0.01),
    axes: [Vec3::NEG_Y, Vec3::NEG_Z, Vec3::X],
};

/// The radius of the grenade model's round body, metres.
pub const GRENADE_BODY_RADIUS_M: f32 = 0.034;
