//! Each weapon's sight line: where it comes from, and the in-headset tool that tunes it.
//!
//! A gun's derived sight line runs from its current fire point along the barrel. A weapon's sight
//! correction ([`SightOverride`]) moves the line's origin and turns it, and is looked up in order:
//!
//! 1. the per-weapon entry in the config ([`AimConfig::sights`]), which the tuning tool and the
//!    Controllers tab write;
//! 2. the embedded sight table ([`table`]), for an entry of a trusted grade;
//! 3. none: the line runs from the fire point.
//!
//! - [`table`]: the embedded table from `tools/sights/sights.json`;
//! - [`tuning`]: the line-up marker and the capture that solves a weapon's offset from it.
//!
//! [`AimConfig::sights`]: crate::controllers::config::AimConfig::sights

pub mod table;
pub mod tuning;

use glam::Vec2;

use crate::controllers::{
    aim::sights::table::{SightGrade, TableEntry},
    config::{SightOverride, SightTable},
};

/// Where a weapon's sight correction comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SightSource {
    /// The per-weapon entry in the config.
    Config,
    /// The embedded table, with the entry's grade.
    Table(SightGrade),
    /// No correction: the line runs from the fire point along the barrel.
    FirePoint,
}

impl SightSource {
    pub fn label(self) -> String {
        match self {
            SightSource::Config => "config".to_owned(),
            SightSource::Table(grade) => format!("embedded table ({})", grade.label()),
            SightSource::FirePoint => "fire point".to_owned(),
        }
    }
}

/// A weapon's sight correction and where it came from.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ResolvedSight {
    pub source: SightSource,
    /// The correction, or `None` on the fire point.
    pub sight: Option<SightOverride>,
}

impl ResolvedSight {
    /// The sight line's pitch and yaw relative to the barrel, zero on the fire point.
    pub fn angles_deg(&self) -> Vec2 {
        self.sight.map_or(Vec2::ZERO, |s| s.angles_deg)
    }
}

/// The sight correction for `weapon_hash`: the config's entry, then the embedded table's trusted
/// entry, then the fire point.
pub fn resolve(config: &SightTable, weapon_hash: u32) -> ResolvedSight {
    resolve_in(config, table::entries(), weapon_hash)
}

/// The embedded table's entry for `weapon_hash`, of any grade.
pub fn table_entry(weapon_hash: u32) -> Option<&'static TableEntry> {
    find(table::entries(), weapon_hash)
}

fn resolve_in(config: &SightTable, entries: &[TableEntry], weapon_hash: u32) -> ResolvedSight {
    if let Some(sight) = config.get(weapon_hash) {
        return ResolvedSight {
            source: SightSource::Config,
            sight: Some(*sight),
        };
    }
    match find(entries, weapon_hash).filter(|e| e.grade.trusted()) {
        Some(entry) => ResolvedSight {
            source: SightSource::Table(entry.grade),
            sight: Some(entry.sight()),
        },
        None => ResolvedSight {
            source: SightSource::FirePoint,
            sight: None,
        },
    }
}

fn find(entries: &[TableEntry], weapon_hash: u32) -> Option<&TableEntry> {
    entries.iter().find(|e| e.weapon_hash == weapon_hash)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    fn entry(weapon_hash: u32, height: f32, grade: SightGrade) -> TableEntry {
        TableEntry {
            weapon: format!("w{weapon_hash:03}_placeholder"),
            weapon_hash,
            offset_m: Vec3::Y * height,
            angles_deg: Vec2::ZERO,
            grade,
        }
    }

    fn entries() -> Vec<TableEntry> {
        vec![
            entry(1, 0.08, SightGrade::Scoped),
            entry(2, 0.02, SightGrade::Pistol),
            entry(3, 0.09, SightGrade::IronUncertain),
            entry(4, 0.0, SightGrade::Failed),
        ]
    }

    #[test]
    fn the_config_entry_comes_first() {
        let mut config = SightTable::EMPTY;
        let tuned = SightOverride {
            offset_m: Vec3::Y * 0.1,
            ..SightOverride::identity(1)
        };
        config.set(tuned);
        let resolved = resolve_in(&config, &entries(), 1);
        assert_eq!(resolved.source, SightSource::Config);
        assert_eq!(resolved.sight, Some(tuned));
    }

    #[test]
    fn a_trusted_table_entry_comes_next() {
        let entries = entries();
        for (hash, grade) in [(1, SightGrade::Scoped), (2, SightGrade::Pistol)] {
            let resolved = resolve_in(&SightTable::EMPTY, &entries, hash);
            assert_eq!(resolved.source, SightSource::Table(grade));
            assert_eq!(resolved.sight, Some(entries[hash as usize - 1].sight()));
        }
    }

    #[test]
    fn an_untrusted_or_missing_entry_leaves_the_fire_point() {
        for hash in [3, 4, 99] {
            let resolved = resolve_in(&SightTable::EMPTY, &entries(), hash);
            assert_eq!(resolved.source, SightSource::FirePoint);
            assert_eq!(resolved.sight, None);
            assert_eq!(resolved.angles_deg(), Vec2::ZERO);
        }
    }

    #[test]
    fn a_config_entry_overrides_an_untrusted_one() {
        let mut config = SightTable::EMPTY;
        config.set(SightOverride::identity(3));
        let resolved = resolve_in(&config, &entries(), 3);
        assert_eq!(resolved.source, SightSource::Config);
        assert_eq!(resolved.sight, Some(SightOverride::identity(3)));
    }
}
