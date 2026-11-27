//! The pure core's tuning that the player's settings do not hold ([`CoreTuning`]): the grounded
//! rig's level gaze and head speed, the standing height's sampling, the yaw's hand reach and
//! turning readout, and the contact model's fixed rules.
//!
//! The values live in the body crate's `tuning/rig.toml`, under `core`, which the body crate loads
//! and validates and the offline evaluator in `tools/tracking-synth` reads from the repository. The
//! pelvis, step, and yaw tuning ([`crate::RigTuning`]) and the contact model's
//! ([`crate::contact::ContactTuning`]) are the player's settings, not the file's.

use crate::{
    contact::ContactFileTuning, height::HeightTuning, rig::GroundedTuning, yaw::YawFileTuning,
};

/// The pure core's tuning from the file.
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize),
    serde(deny_unknown_fields)
)]
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct CoreTuning {
    pub grounded: GroundedTuning,
    pub height: HeightTuning,
    pub yaw: YawFileTuning,
    pub contact: ContactFileTuning,
}

/// The repository's `rig.toml`'s `core` table, for the crate's tests.
#[cfg(test)]
pub(crate) fn shipped() -> &'static CoreTuning {
    use std::{path::Path, sync::LazyLock};

    static SHIPPED: LazyLock<CoreTuning> = LazyLock::new(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../jc3vr-body/tuning/rig.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("rig tuning: {}: {error}", path.display()));
        let mut file: toml::Table = toml::from_str(&text)
            .unwrap_or_else(|error| panic!("rig tuning: {}: {error}", path.display()));
        file.remove("core")
            .unwrap_or_else(|| panic!("rig tuning: {}: no `core` table", path.display()))
            .try_into()
            .unwrap_or_else(|error| panic!("rig tuning: {}: {error}", path.display()))
    });
    &SHIPPED
}
