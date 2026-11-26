//! Checks for the controller haptics: the strength.

use jc3vr_core::config::engine::{Invalid, range};

use crate::controllers::haptics::HapticsConfig;

pub(super) fn haptics(h: &HapticsConfig) -> Result<(), Invalid> {
    range("haptics.intensity", h.intensity, 0.0, 1.0)
}
