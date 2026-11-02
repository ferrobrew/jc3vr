//! Which weapons the true reload reloads, and how: what each loads, where, and how its action is
//! worked. Pure.
//!
//! A weapon is reloaded by hand when it is wielded in the hands, has a magazine limit, is not one
//! of the game's untimed reloads, and is of a class with something to load: the pistols, SMGs,
//! rifles, shotguns, sniper rifles, grenade and rocket launchers, the mortar, and the revolvers.
//! The minigun, the mounted guns, and the special weapons keep the game's reload. The wells table
//! ([`crate::controllers::weapons::reload::wells`]) says what a weapon loads, where its well is
//! in the gun's model frame, and how its action is worked; a weapon it lacks takes its class's
//! defaults and a well at the configured offset from the holding wrist.

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponClass;

use crate::controllers::weapons::reload::{
    chamber::ActionKind,
    wells::{WellEntry, WellKind},
};

/// What a weapon loads.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LoadKind {
    /// A magazine, ejected and replaced whole.
    Magazine,
    /// One shell per load.
    Shell,
    /// A round into the muzzle.
    Rocket,
    /// A revolver's cylinder, loaded whole.
    Cylinder,
}

impl LoadKind {
    pub const fn label(self) -> &'static str {
        match self {
            LoadKind::Magazine => "magazine",
            LoadKind::Shell => "shell",
            LoadKind::Rocket => "rocket",
            LoadKind::Cylinder => "cylinder",
        }
    }

    /// Whether the load can be ejected: a magazine or a cylinder's spent rounds.
    pub const fn ejects(self) -> bool {
        matches!(self, LoadKind::Magazine | LoadKind::Cylinder)
    }

    /// Whether a round of this kind seated in a gun whose magazine is `out` first pushes the old
    /// magazine out: a magazine or a cylinder seated while the gun still holds one.
    pub const fn swaps(self, out: bool) -> bool {
        self.ejects() && !out
    }

    /// Whether the gun shows its round part on its own once no round is in the hand: a magazine
    /// while it is in the gun, and a rocket while the launcher holds one, which the launcher's own
    /// update shows and hides with its magazine.
    pub const fn part_shown(self, out: bool, magazine: i32) -> bool {
        match self {
            LoadKind::Rocket => magazine > 0,
            LoadKind::Magazine | LoadKind::Shell | LoadKind::Cylinder => !out,
        }
    }
}

/// Where a gun takes its rounds, in the gun's model frame, or in the holding wrist's frame for a
/// weapon with no table entry.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum WellFrame {
    Gun {
        mouth: Vec3,
        axis: Vec3,
    },
    /// The configured offset from the wrist, the axis up through the hand.
    Wrist {
        offset: Vec3,
    },
}

/// How a weapon is reloaded.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReloadPlan {
    pub load: LoadKind,
    pub action: ActionKind,
    pub well: WellFrame,
    /// The action's handle in the gun's model frame, when known.
    pub handle: Option<Vec3>,
    pub travel_m: f32,
    /// For a weapon whose round is a separate model part (a magazine, the UPU-210's cartridge,
    /// the Hydra's pod, or a launcher's rocket), the top of the part from its origin along
    /// [`part_axis`](Self::part_axis), metres.
    pub part_top_m: Option<f32>,
    /// The direction the round part travels into the gun, in the part's own frame, which is the
    /// gun's model frame for the round parts the table measures: the well's insertion axis, or
    /// +Y without one.
    pub part_axis: Vec3,
    /// A launcher: a grenade launcher, a rocket launcher, or the mortar. Its round is caught by
    /// the well at the launcher capture radius and angle, since a launcher's well is large and
    /// awkward to reach with the other hand.
    pub launcher: bool,
}

/// The facts about a weapon the plan is made from.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WeaponFacts {
    /// The weapon's class, as its raw value.
    pub class: i32,
    /// The magazine size; negative means no limit.
    pub capacity: i32,
    /// The weapon has unlimited magazines or an untimed reload.
    pub exempt: bool,
}

/// The plan for a weapon, or `None` for one that keeps the game's reload.
pub fn plan(
    facts: WeaponFacts,
    entry: Option<&WellEntry>,
    fallback_well: Vec3,
    default_travel_m: f32,
) -> Option<ReloadPlan> {
    if facts.exempt || facts.capacity <= 0 {
        return None;
    }
    let (class_load, class_action) = class_defaults(facts.class)?;
    let entry = entry.filter(|e| e.grade.trusted());
    let load = match entry.map(|e| e.kind) {
        Some(WellKind::None) => return None,
        Some(WellKind::Magazine) => LoadKind::Magazine,
        Some(WellKind::Shell) => LoadKind::Shell,
        Some(WellKind::Rocket) => LoadKind::Rocket,
        Some(WellKind::Cylinder) => LoadKind::Cylinder,
        None => class_load,
    };
    let action = entry.map_or(class_action, |e| e.action.into());
    let well = match entry {
        Some(e) => match (e.mouth_m, e.insert_axis.and_then(Vec3::try_normalize)) {
            (Some(mouth), Some(axis)) if mouth.is_finite() => WellFrame::Gun { mouth, axis },
            _ => WellFrame::Wrist {
                offset: fallback_well,
            },
        },
        None => WellFrame::Wrist {
            offset: fallback_well,
        },
    };
    Some(ReloadPlan {
        load,
        action,
        well,
        handle: entry.and_then(|e| e.handle_m).filter(|h| h.is_finite()),
        travel_m: entry
            .and_then(|e| e.travel_m)
            .filter(|t| t.is_finite() && *t > 0.0)
            .unwrap_or(default_travel_m),
        part_top_m: entry
            .filter(|e| e.magazine_part && matches!(load, LoadKind::Magazine | LoadKind::Rocket))
            .and_then(|e| e.part_top_m)
            .filter(|t| t.is_finite()),
        part_axis: match well {
            WellFrame::Gun { axis, .. } => axis,
            WellFrame::Wrist { .. } => Vec3::Y,
        },
        launcher: is_launcher(facts.class),
    })
}

/// Whether the class is a launcher's: a grenade launcher, a rocket launcher, or the mortar.
fn is_launcher(class: i32) -> bool {
    [
        WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER,
        WeaponClass::E_WEAPON_CLASS_RPG,
        WeaponClass::E_WEAPON_CLASS_MORTAR,
    ]
    .into_iter()
    .any(|c| c as i32 == class)
}

/// The load and action of a class, or `None` for a class that keeps the game's reload.
fn class_defaults(class: i32) -> Option<(LoadKind, ActionKind)> {
    let defaults = [
        (
            WeaponClass::E_WEAPON_CLASS_PISTOL,
            LoadKind::Magazine,
            ActionKind::Slide,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_SMG,
            LoadKind::Magazine,
            ActionKind::ChargingHandle,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
            LoadKind::Magazine,
            ActionKind::ChargingHandle,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_SHOTGUN,
            LoadKind::Shell,
            ActionKind::None,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_SNIPER,
            LoadKind::Magazine,
            ActionKind::Bolt,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER,
            LoadKind::Magazine,
            ActionKind::None,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_RPG,
            LoadKind::Rocket,
            ActionKind::None,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_MORTAR,
            LoadKind::Rocket,
            ActionKind::None,
        ),
        (
            WeaponClass::E_WEAPON_CLASS_REVOLVER,
            LoadKind::Cylinder,
            ActionKind::None,
        ),
    ];
    defaults
        .into_iter()
        .find(|(c, ..)| *c as i32 == class)
        .map(|(_, load, action)| (load, action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::reload::wells::{self, TableAction, WellGrade};

    const WRIST: Vec3 = Vec3::new(0.0, -0.08, -0.06);

    fn facts(class: WeaponClass) -> WeaponFacts {
        WeaponFacts {
            class: class as i32,
            capacity: 30,
            exempt: false,
        }
    }

    fn entry(kind: WellKind, action: TableAction, grade: WellGrade) -> WellEntry {
        WellEntry {
            weapon: "w000_test".to_owned(),
            weapon_hash: 1,
            kind,
            mouth_m: Some(Vec3::new(0.0, -0.05, -0.16)),
            insert_axis: Some(Vec3::Y * 2.0),
            magazine_part: true,
            part_top_m: Some(0.05),
            action,
            handle_m: Some(Vec3::new(0.0, 0.06, 0.1)),
            travel_m: Some(0.07),
            grade,
        }
    }

    #[test]
    fn the_minigun_specials_and_unlimited_magazines_keep_the_games_reload() {
        assert!(
            plan(
                facts(WeaponClass::E_WEAPON_CLASS_MINIGUN),
                None,
                WRIST,
                0.06
            )
            .is_none()
        );
        assert!(
            plan(
                facts(WeaponClass::E_WEAPON_CLASS_SPECIAL),
                None,
                WRIST,
                0.06
            )
            .is_none()
        );
        let unlimited = WeaponFacts {
            capacity: -1,
            ..facts(WeaponClass::E_WEAPON_CLASS_PISTOL)
        };
        assert!(plan(unlimited, None, WRIST, 0.06).is_none());
        let exempt = WeaponFacts {
            exempt: true,
            ..facts(WeaponClass::E_WEAPON_CLASS_PISTOL)
        };
        assert!(plan(exempt, None, WRIST, 0.06).is_none());
        let nothing = entry(WellKind::None, TableAction::None, WellGrade::Default);
        assert!(
            plan(
                facts(WeaponClass::E_WEAPON_CLASS_PISTOL),
                Some(&nothing),
                WRIST,
                0.06
            )
            .is_none()
        );
    }

    #[test]
    fn a_weapon_without_an_entry_takes_its_class_defaults_at_the_wrist() {
        let p = plan(facts(WeaponClass::E_WEAPON_CLASS_PISTOL), None, WRIST, 0.06).unwrap();
        assert_eq!(p.load, LoadKind::Magazine);
        assert_eq!(p.action, ActionKind::Slide);
        assert_eq!(p.well, WellFrame::Wrist { offset: WRIST });
        assert_eq!(p.travel_m, 0.06);
        let p = plan(facts(WeaponClass::E_WEAPON_CLASS_SNIPER), None, WRIST, 0.06).unwrap();
        assert_eq!(p.action, ActionKind::Bolt);
    }

    #[test]
    fn a_trusted_entry_places_the_well_on_the_gun() {
        let e = entry(WellKind::Shell, TableAction::Pump, WellGrade::Mesh);
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_SHOTGUN),
            Some(&e),
            WRIST,
            0.06,
        )
        .unwrap();
        assert_eq!(p.load, LoadKind::Shell);
        assert_eq!(p.action, ActionKind::Pump);
        assert_eq!(
            p.well,
            WellFrame::Gun {
                mouth: e.mouth_m.unwrap(),
                axis: Vec3::Y
            }
        );
        assert_eq!(p.travel_m, 0.07);

        let failed = entry(WellKind::Shell, TableAction::Pump, WellGrade::Failed);
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_SHOTGUN),
            Some(&failed),
            WRIST,
            0.06,
        )
        .unwrap();
        assert_eq!(p.action, ActionKind::None);
        assert_eq!(p.well, WellFrame::Wrist { offset: WRIST });
    }

    #[test]
    fn the_upu_210_is_a_launcher_whose_cartridge_rides_the_hand_with_no_action() {
        let upu = wells::lookup(0xE00F_0B63).expect("the UPU-210 is in the wells table");
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER),
            Some(upu),
            WRIST,
            0.06,
        )
        .unwrap();
        assert!(p.launcher);
        assert_eq!(p.load, LoadKind::Magazine);
        assert_eq!(p.action, ActionKind::None);
        assert!(p.part_top_m.is_some());
        assert_eq!(p.part_axis, Vec3::Y);
    }

    #[test]
    fn a_rocket_launcher_carries_its_rocket_along_the_tube() {
        let e = WellEntry {
            insert_axis: Some(Vec3::Z),
            ..entry(WellKind::Rocket, TableAction::None, WellGrade::Part)
        };
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_RPG),
            Some(&e),
            WRIST,
            0.06,
        )
        .unwrap();
        assert!(p.launcher);
        assert_eq!(p.load, LoadKind::Rocket);
        assert_eq!(p.part_top_m, Some(0.05));
        assert_eq!(p.part_axis, Vec3::Z);
        let rifle = plan(
            facts(WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE),
            None,
            WRIST,
            0.06,
        )
        .unwrap();
        assert!(!rifle.launcher);
        assert_eq!(rifle.part_axis, Vec3::Y);
    }

    #[test]
    fn a_rocket_part_shows_with_the_launchers_rounds_and_a_magazine_while_it_is_in() {
        assert!(LoadKind::Rocket.part_shown(false, 1));
        assert!(!LoadKind::Rocket.part_shown(false, 0));
        assert!(LoadKind::Magazine.part_shown(false, 0));
        assert!(!LoadKind::Magazine.part_shown(true, 5));
    }

    #[test]
    fn only_a_magazine_or_a_cylinder_still_in_is_pushed_out() {
        for load in [LoadKind::Magazine, LoadKind::Cylinder] {
            assert!(load.swaps(false), "{load:?}");
            assert!(!load.swaps(true), "{load:?}");
        }
        for load in [LoadKind::Shell, LoadKind::Rocket] {
            assert!(!load.swaps(false), "{load:?}");
            assert!(!load.swaps(true), "{load:?}");
        }
    }
}
