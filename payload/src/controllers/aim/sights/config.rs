//! The sight-line settings: the per-weapon sight corrections and the in-headset tuning tool,
//! re-exported from [`crate::controllers::config`].

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// The in-headset sight tuning tool (see [`crate::controllers::aim::sights::tuning`]): a marker
/// placed ahead of the head, and a capture that stores the held weapon's sight offset that makes
/// its sight line pass through the marker.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SightTuningConfig {
    /// Offer the tuning tool: the line-up marker, the capture, and the capture chord (both grips
    /// held and the dominant A pressed, on foot, while the marker is placed).
    pub enabled: bool,
    /// How far ahead of the head the marker is placed, metres.
    pub marker_distance_m: f32,
}

impl SightTuningConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            marker_distance_m: 10.0,
        }
    }
}

impl Default for SightTuningConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// A correction to one weapon type's sight line, keyed by the weapon's name hash. The derived
/// sight line runs from the gun's current fire point along the fire point's barrel axis; the
/// correction moves its origin and turns its direction, both in the fire point's frame.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SightOverride {
    /// The weapon type's name hash
    /// ([`WeaponBase::m_WeaponNameHash`](jc3gi::equipment::weapon::WeaponBase::m_WeaponNameHash)).
    pub weapon_hash: u32,
    /// The sight's origin relative to the fire point, in the fire point's frame (+Y up, -Z along
    /// the barrel), metres.
    pub offset_m: Vec3,
    /// The sight line's pitch (positive up) and yaw (positive left) relative to the barrel axis,
    /// degrees.
    pub angles_deg: Vec2,
}

impl SightOverride {
    /// No correction for `weapon_hash`: the derived sight line as it is.
    pub const fn identity(weapon_hash: u32) -> Self {
        Self {
            weapon_hash,
            offset_m: Vec3::ZERO,
            angles_deg: Vec2::ZERO,
        }
    }
}

/// The per-weapon sight corrections: a small fixed table, so that the configuration stays `Copy`.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SightTable {
    pub entries: [Option<SightOverride>; SightTable::CAPACITY],
}

impl SightTable {
    /// How many weapons can carry a correction. The embedded sight table covers the weapons it
    /// trusts, so this holds only the weapons tuned or corrected by hand: room for every weapon the
    /// embedded table does not trust, in one session. Serde derives arrays up to this length.
    pub const CAPACITY: usize = 32;

    pub const EMPTY: SightTable = SightTable {
        entries: [None; SightTable::CAPACITY],
    };

    /// The correction for `weapon_hash`, if any.
    pub fn get(&self, weapon_hash: u32) -> Option<&SightOverride> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.weapon_hash == weapon_hash)
    }

    /// Set the correction for its weapon, replacing any it has. Returns whether the table holds it
    /// afterwards; `false` means the table is full.
    pub fn set(&mut self, sight: SightOverride) -> bool {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.weapon_hash == sight.weapon_hash)
        {
            *entry = sight;
            return true;
        }
        match self.entries.iter_mut().find(|entry| entry.is_none()) {
            Some(slot) => {
                *slot = Some(sight);
                true
            }
            None => false,
        }
    }

    /// Remove the correction for `weapon_hash`.
    pub fn remove(&mut self, weapon_hash: u32) {
        for entry in &mut self.entries {
            if entry.is_some_and(|e| e.weapon_hash == weapon_hash) {
                *entry = None;
            }
        }
    }
}

impl Default for SightTable {
    fn default() -> Self {
        Self::EMPTY
    }
}
