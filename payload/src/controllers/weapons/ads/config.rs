//! The settings of aiming down a long gun's sights (see
//! [`crate::controllers::weapons::ads`]), held in
//! [`WeaponsConfig::ads`](crate::controllers::config::WeaponsConfig::ads).

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
/// and a scope's cheek weld. Acts only under the controller scheme, so it defaults on. The fixed
/// distances are in [`crate::controllers::weapons::ads::solve`].
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdsConfig {
    /// Line a long gun's sight line up with the aiming eye when it is raised to it.
    pub enabled: bool,
    /// Which eye looks down the sights.
    pub eye: AimingEye,
    /// Seat the stock in the shoulder pocket when the butt comes to it, so the gun pivots about
    /// the shoulder.
    pub shoulder: bool,
    /// Bring a scope's axis onto the eye by moving the gun across it, keeping its direction, so
    /// the scope's view lines up.
    pub cheek_weld: bool,
}

impl AdsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            eye: AimingEye::HolderSide,
            shoulder: true,
            cheek_weld: true,
        }
    }
}

impl Default for AdsConfig {
    fn default() -> Self {
        Self::new()
    }
}
