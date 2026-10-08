//! Checks for the finger-curl mapping: the curl ranges, the per-joint bend and rest-offset tables,
//! the splay, and the grip poses' blend.

use crate::{
    config::validate::{Invalid, nonnegative, range},
    controllers::fingers::config::FingersConfig,
};

/// Check the finger settings. Paths are relative to `controllers.fingers`.
pub(super) fn fingers(c: &FingersConfig) -> Result<(), Invalid> {
    for (index, finger_range) in c.curl_ranges.into_iter().enumerate() {
        range(
            &format!("fingers.curl_ranges[{index}].open"),
            finger_range.open,
            -2.0,
            2.0,
        )?;
        range(
            &format!("fingers.curl_ranges[{index}].closed"),
            finger_range.closed,
            -2.0,
            2.0,
        )?;
    }
    for (index, value) in c.finger_bend_deg.into_iter().enumerate() {
        range(
            &format!("fingers.finger_bend_deg[{index}]"),
            value,
            0.0,
            360.0,
        )?;
    }
    for (index, value) in c.thumb_bend_deg.into_iter().enumerate() {
        range(
            &format!("fingers.thumb_bend_deg[{index}]"),
            value,
            0.0,
            360.0,
        )?;
    }
    for (finger, offsets) in c.rest_offset_deg.into_iter().enumerate() {
        for (joint, value) in offsets.into_iter().enumerate() {
            range(
                &format!("fingers.rest_offset_deg[{finger}][{joint}]"),
                value,
                -180.0,
                180.0,
            )?;
        }
    }
    range(
        "fingers.thumb_spread_deg",
        c.thumb_spread_deg,
        -180.0,
        180.0,
    )?;
    nonnegative("fingers.splay_scale", c.splay_scale, 10.0)?;
    range("fingers.max_splay_deg", c.max_splay_deg, 0.0, 180.0)?;
    nonnegative("fingers.smoothing_s", c.smoothing_s, 60.0)?;
    range("fingers.grip.light_share", c.grip.light_share, 0.0, 1.0)?;
    range("fingers.grip.firm_squeeze", c.grip.firm_squeeze, 0.01, 1.0)?;
    nonnegative("fingers.grip.blend_s", c.grip.blend_s, 10.0)?;
    range(
        "fingers.grip.max_opposition_deg",
        c.grip.max_opposition_deg,
        0.0,
        90.0,
    )
}
