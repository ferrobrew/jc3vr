//! The inventory's, the loadout table's, and the weapon wheel's tuning, read from the shipped
//! `tuning/inventory.toml` ([`INVENTORY`]): the pins, the lost-weapon return, the sockets' offsets,
//! the docked guns' view, the waits of the restore and the table, and the wheel's thresholds.

use std::time::Duration;

use jc3vr_core::{
    config::engine::{Invalid, positive, range, vec3},
    tuning::Tuning,
};
use jc3vr_view::controllers::overlay::primitive::Rgba;
use serde::Deserialize;

use crate::controllers::weapons::{
    inventory::{lost::LostRules, pins::PinSettings, sockets::Offsets},
    wheel::tuning::WheelTuning,
};

jc3vr_core::tuning_file! {
    /// The inventory's tuning file.
    pub static INVENTORY: InventoryTuning = "inventory.toml";
}

/// The inventory's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryTuning {
    /// The pin policy: the weapons the player let go of nearby are kept from the game's despawn
    /// while they are near, the nearest first, and left to the game once far away.
    pub pins: PinSettings,
    /// The lost-weapon return's rules.
    pub lost: LostRules,
    /// Where each socket's weapon sits relative to its bone.
    pub sockets: Offsets,
    pub docked_view: DockedViewTuning,
    pub restore: RestoreTuning,
    pub loadout: LoadoutTuning,
    pub wheel: WheelTuning,
}

impl Tuning for InventoryTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.pins.validate()?;
        self.lost.validate()?;
        self.sockets.validate()?;
        self.docked_view.validate()?;
        self.restore.validate()?;
        self.loadout.validate()?;
        self.wheel.validate()
    }
}

/// The docked guns' in-world diagnostic (`weapons::inventory::docked_view`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockedViewTuning {
    /// How long a submission is drawn without a newer one, milliseconds.
    pub ttl_ms: u64,
    /// The ring's diameter, metres.
    pub ring_m: f32,
    /// The ring's thickness, a share of its radius.
    pub ring_thickness: f32,
    /// The barrel line's length, metres.
    pub barrel_m: f32,
    /// The barrel line's width, metres.
    pub barrel_width_m: f32,
    /// The marks' colour while the gun's model is shown, linear RGBA.
    pub shown: Rgba,
    /// The marks' colour while the gun's model is hidden, linear RGBA.
    pub hidden: Rgba,
    /// The marks' alpha where the body, which a gun turned into the thigh is behind, hides them.
    pub occluded_alpha: f32,
}

impl DockedViewTuning {
    fn validate(&self) -> Result<(), Invalid> {
        if self.ttl_ms == 0 {
            return Err(Invalid(
                "docked_view.ttl_ms must be positive, got 0".to_owned(),
            ));
        }
        positive("docked_view.ring_m", self.ring_m, 1.0)?;
        range("docked_view.ring_thickness", self.ring_thickness, 0.0, 1.0)?;
        positive("docked_view.barrel_m", self.barrel_m, 2.0)?;
        positive("docked_view.barrel_width_m", self.barrel_width_m, 0.1)?;
        for (key, color) in [("shown", self.shown), ("hidden", self.hidden)] {
            for (channel, value) in ["r", "g", "b", "a"].iter().zip(color) {
                range(&format!("docked_view.{key}.{channel}"), value, 0.0, 1.0)?;
            }
        }
        range("docked_view.occluded_alpha", self.occluded_alpha, 0.0, 1.0)
    }
}

/// How long the restore of the carried weapons and the game's gives wait for the engine
/// (`weapons::inventory::restore`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreTuning {
    /// How long a give's spawn is waited for when its definition's entity is loaded, seconds.
    pub give_wait_s: f32,
    /// How long a give's spawn is waited for at most, while its definition's entity is still
    /// streaming in, seconds. Not below `give_wait_s`.
    pub give_wait_loading_s: f32,
    /// How long a spawn is waited for, seconds.
    pub spawn_wait_s: f32,
    /// How long a reset's record waits for the player, seconds: a load screen's length. A record
    /// older than this is taken for a quit to the menu, and a new game does not get it.
    pub reset_record_s: f32,
}

impl RestoreTuning {
    /// [`give_wait_s`](Self::give_wait_s) as a duration.
    pub fn give_wait(&self) -> Duration {
        Duration::from_secs_f32(self.give_wait_s)
    }

    /// [`give_wait_loading_s`](Self::give_wait_loading_s) as a duration.
    pub fn give_wait_loading(&self) -> Duration {
        Duration::from_secs_f32(self.give_wait_loading_s)
    }

    /// [`spawn_wait_s`](Self::spawn_wait_s) as a duration.
    pub fn spawn_wait(&self) -> Duration {
        Duration::from_secs_f32(self.spawn_wait_s)
    }

    /// [`reset_record_s`](Self::reset_record_s) as a duration.
    pub fn reset_record(&self) -> Duration {
        Duration::from_secs_f32(self.reset_record_s)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("restore.give_wait_s", self.give_wait_s, 600.0)?;
        range(
            "restore.give_wait_loading_s",
            self.give_wait_loading_s,
            self.give_wait_s,
            600.0,
        )?;
        positive("restore.spawn_wait_s", self.spawn_wait_s, 600.0)?;
        positive("restore.reset_record_s", self.reset_record_s, 3600.0)
    }
}

/// How long the live loadout table waits for the game (`loadout::live`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadoutTuning {
    /// The longest a reset's completion waits for the slots' spawns to arrive, seconds.
    pub settle_wait_s: f32,
    /// How long a hand expects the game to wield the weapon a grab asked for, seconds.
    pub expect_s: f32,
}

impl LoadoutTuning {
    /// [`settle_wait_s`](Self::settle_wait_s) as a duration.
    pub fn settle_wait(&self) -> Duration {
        Duration::from_secs_f32(self.settle_wait_s)
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("loadout.settle_wait_s", self.settle_wait_s, 600.0)?;
        positive("loadout.expect_s", self.expect_s, 60.0)
    }
}

impl PinSettings {
    fn validate(&self) -> Result<(), Invalid> {
        positive("pins.radius_m", self.radius_m, 1000.0)?;
        range("pins.release_m", self.release_m, self.radius_m, 1000.0)?;
        range("pins.forget_m", self.forget_m, self.release_m, 10_000.0)?;
        if self.max_pinned > self.max_tracked {
            return Err(Invalid(format!(
                "pins.max_pinned must not exceed pins.max_tracked ({}), got {}",
                self.max_tracked, self.max_pinned
            )));
        }
        Ok(())
    }
}

impl LostRules {
    fn validate(&self) -> Result<(), Invalid> {
        positive("lost.lost_depth_m", self.lost_depth_m, 10_000.0)?;
        positive("lost.left_behind_m", self.left_behind_m, 10_000.0)?;
        positive("lost.fast_speed_mps", self.fast_speed_mps, 1000.0)?;
        positive("lost.speed_time_s", self.speed_time_s, 10.0)
    }
}

impl Offsets {
    fn validate(&self) -> Result<(), Invalid> {
        vec3("sockets.back_first", self.back_first, 1.0)?;
        vec3("sockets.back_second", self.back_second, 1.0)?;
        vec3("sockets.hip_left", self.hip_left, 1.0)?;
        vec3("sockets.hip_right", self.hip_right, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(INVENTORY.get().validate(), Ok(()));
    }
}
