//! Checks for the controller haptics: the strength.

use crate::controllers::{
    config::validate::{Invalid, range},
    haptics::HapticsConfig,
};

pub(super) fn haptics(h: &HapticsConfig) -> Result<(), Invalid> {
    range("haptics.intensity", h.intensity, 0.0, 1.0)
}
