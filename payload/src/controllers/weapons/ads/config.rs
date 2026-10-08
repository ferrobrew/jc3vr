//! The settings of aiming down a long gun's sights (see
//! [`crate::controllers::weapons::ads`]), held in
//! [`WeaponsConfig::ads`](crate::controllers::config::WeaponsConfig::ads).

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Which eye looks down the sights.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AimingEye {
    /// The eye on the side of the hand that holds the gun.
    #[default]
    HolderSide,
    Left,
    Right,
}

impl AimingEye {
    pub const ALL: [AimingEye; 3] = [AimingEye::HolderSide, AimingEye::Left, AimingEye::Right];

    pub const fn label(self) -> &'static str {
        match self {
            AimingEye::HolderSide => "the gun hand's side",
            AimingEye::Left => "left",
            AimingEye::Right => "right",
        }
    }
}

/// Aiming a long gun down its sights: the sights lined up with the eye, the stock in the shoulder,
/// and a scope's cheek weld. Acts only under the controller scheme, so it defaults on.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdsConfig {
    /// Line a long gun's sight line up with the aiming eye when it is raised to it.
    pub enabled: bool,
    /// Which eye looks down the sights.
    pub eye: AimingEye,
    /// How far each eye sits from the eyes' midpoint, metres.
    pub half_ipd_m: f32,
    /// How far the eye may be off the sight line for the line to start turning onto it, metres.
    pub engage_m: f32,
    /// How far off the sight line the eye is when the line passes through it fully, metres.
    pub lock_m: f32,
    /// How far behind the holding grip, along the sight line, the eye may be, metres.
    pub max_eye_behind_m: f32,
    /// The widest angle between the head's forward and the sight line, degrees: a gun the head
    /// does not look along is not being aimed.
    pub max_view_angle_deg: f32,
    /// How far ahead of the holding grip, along the sight line, the gun turns about to bring the
    /// sights to the eye, metres: about where the front sight is.
    pub pivot_ahead_m: f32,
    /// Seat the stock in the shoulder pocket when the butt comes to it, so the gun pivots about
    /// the shoulder.
    pub shoulder: bool,
    /// How far behind the holding grip the butt is, along the sight line, metres.
    pub butt_behind_grip_m: f32,
    /// The shoulder pocket's offset from the shoulder joint in the torso's frame (+X right, +Y up,
    /// -Z forward), metres, given for the right shoulder and mirrored for the left.
    pub pocket_offset_m: Vec3,
    /// How far from the pocket the butt starts to seat, and how far it is when it is fully seated,
    /// metres.
    pub shoulder_engage_m: f32,
    pub shoulder_lock_m: f32,
    /// Bring a scope's axis onto the eye by moving the gun across it, keeping its direction, so
    /// the scope's view lines up.
    pub cheek_weld: bool,
    /// How far off the scope's axis the eye may be for the weld to start, and how far it is when
    /// the weld is full, metres.
    pub weld_engage_m: f32,
    pub weld_lock_m: f32,
}

impl AdsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            eye: AimingEye::HolderSide,
            half_ipd_m: 0.032,
            engage_m: 0.06,
            lock_m: 0.02,
            max_eye_behind_m: 0.4,
            max_view_angle_deg: 30.0,
            pivot_ahead_m: 0.35,
            shoulder: true,
            butt_behind_grip_m: 0.22,
            pocket_offset_m: Vec3::new(-0.06, -0.04, -0.04),
            shoulder_engage_m: 0.12,
            shoulder_lock_m: 0.04,
            cheek_weld: true,
            weld_engage_m: 0.05,
            weld_lock_m: 0.015,
        }
    }
}

impl Default for AdsConfig {
    fn default() -> Self {
        Self::new()
    }
}
