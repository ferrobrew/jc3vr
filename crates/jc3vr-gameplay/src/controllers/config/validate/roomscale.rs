//! Checks for posture and roomscale: the crouch.

use jc3vr_body::controllers::roomscale::config::RoomscaleConfig;
use jc3vr_core::config::engine::{Invalid, nonnegative};

/// Check the roomscale settings. Paths are relative to `controllers.roomscale`.
pub(super) fn roomscale(r: &RoomscaleConfig) -> Result<(), Invalid> {
    nonnegative("roomscale.crouch.enter_drop_m", r.crouch.enter_drop_m, 10.0)
}
