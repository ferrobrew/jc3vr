//! Checks for the controller haptics: the strength.

use jc3vr_body::controllers::haptics::HapticsConfig;
use jc3vr_core::config::engine::{Invalid, range};

pub(super) fn haptics(h: &HapticsConfig) -> Result<(), Invalid> {
    range("haptics.intensity", h.intensity, 0.0, 1.0)
}
