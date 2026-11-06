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

use glam::{Quat, Vec3};
use jc3gi::equipment::weapon::WeaponClass;

use crate::{
    controllers::weapons::reload::{
        chamber::ActionKind,
        wells::{RoundEntry, WellEntry, WellKind},
    },
    hash::hashlittle,
};

/// The most rounds a hand holds at once: a revolver's cylinder.
pub const MAX_HELD_ROUNDS: usize = 6;

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

    /// Whether the well of a gun of this kind, its magazine `out` or not, is occupied: a magazine or
    /// a cylinder still in the gun, which has to leave before a fresh one can seat.
    pub const fn well_occupied(self, out: bool) -> bool {
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
    /// For a magazine whose gun has a spare magazine model that depicts it, the top of that model
    /// centred across it, in the model's own frame, metres: what the magazine in the hand is
    /// drawn with.
    pub spare_top_m: Option<Vec3>,
    /// For a shell or a cylinder, which the game has no model of a loose round for, the stand-in
    /// model the round in the hand is drawn with.
    pub round: Option<RoundModel>,
    /// A launcher: a grenade launcher, a rocket launcher, or the mortar. Its round is caught by
    /// the well at the launcher capture radius and angle, since a launcher's well is large and
    /// awkward to reach with the other hand.
    pub launcher: bool,
    /// A launcher of one round (the UPU-210's cartridge, the Hydra's pod, a rocket launcher, or
    /// the mortar), whose spent round does not hold the well: a fresh one seats in its place. A
    /// launcher with a magazine of several rounds, the Bildt Negotiator, has its empty magazine
    /// knocked out, ejected, or pulled first, as a gun's is, which is how the game's own reload of
    /// it swaps its magazine.
    pub spent_round_replaced: bool,
}

impl ReloadPlan {
    /// The model a round taken from the pouch is drawn with in the hand, and how it lies: the
    /// gun's spare magazine model for a magazine that has one, the stand-in round model for a
    /// shell or a cylinder, or `None`, which leaves the round to the gun's own part, if any.
    pub fn hand_round(&self) -> Option<HandRound> {
        match (self.spare_top_m, self.round) {
            (Some(top), _) => Some(HandRound {
                model: HandModel::Spare,
                top,
                axis: self.part_axis,
                in_gun: Quat::IDENTITY,
            }),
            (None, Some(round)) => Some(HandRound {
                model: HandModel::Round(round),
                top: round.top,
                axis: round.axis,
                in_gun: Quat::from_rotation_arc(round.axis, self.part_axis),
            }),
            (None, None) => None,
        }
    }

    /// The plan with its action worked by hand only while `manual_action` is on. Off, every action
    /// is [`ActionKind::None`], so the gun chambers its first round as its magazine seats, as a
    /// launcher or a revolver does, and nothing waits on a slide, a charging handle, a bolt, or a
    /// pump.
    pub fn with_manual_action(self, manual_action: bool) -> Self {
        if manual_action {
            self
        } else {
            Self {
                action: ActionKind::None,
                ..self
            }
        }
    }
}

/// The stand-in model of a loose round, from the wells table ([`RoundEntry`]): a casing, or a
/// ring of them for a revolver's cylinder.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RoundModel {
    /// The compiled-name hash the game's model cache holds the model under.
    pub name_hash: u32,
    /// The round's feed end, centred across it, in the model's own frame, metres.
    pub top: Vec3,
    /// The direction the round travels into the gun, in the model's own frame, unit length.
    pub axis: Vec3,
    pub radius_m: f32,
    /// How many rounds the hand holds, `1..=MAX_HELD_ROUNDS`.
    pub count: usize,
    /// The ring the rounds stand on around the group's axis, metres; zero for one round.
    pub ring_m: f32,
}

impl RoundModel {
    /// The model from a table entry, or `None` when the entry has no complete, finite model.
    pub fn from_entry(entry: &RoundEntry) -> Option<Self> {
        let axis = entry.round_axis?.try_normalize()?;
        let top = entry.round_top_m.filter(|t| t.is_finite())?;
        let radius_m = entry.round_radius_m.filter(|r| r.is_finite() && *r > 0.0)?;
        let ring_m = entry.round_ring_m.filter(|r| r.is_finite() && *r >= 0.0)?;
        let count = usize::try_from(entry.round_count?).ok()?;
        Some(Self {
            name_hash: hashlittle(entry.round_model.as_ref()?.as_bytes()),
            top,
            axis,
            radius_m,
            count: count.clamp(1, MAX_HELD_ROUNDS),
            ring_m,
        })
    }

    /// How far the group reaches from its axis: from the palm to the group's axis, for a group
    /// whose side lies in the palm.
    pub fn half_width_m(&self) -> f32 {
        self.ring_m + self.radius_m
    }

    /// Each round's offset from the group's axis, in the model's own frame: evenly round the ring,
    /// square to the axis.
    pub fn offsets(&self) -> impl Iterator<Item = Vec3> + use<> {
        let across = self.axis.any_orthonormal_vector();
        let (axis, ring, count) = (self.axis, self.ring_m, self.count);
        (0..count).map(move |i| {
            let turn = std::f32::consts::TAU * i as f32 / count as f32;
            Quat::from_axis_angle(axis, turn) * across * ring
        })
    }
}

/// What draws a round in the hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum HandModel {
    /// The gun's spare magazine model.
    Spare,
    /// The stand-in round model.
    Round(RoundModel),
}

/// A round in the hand drawn by an instance of a model separate from the gun.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandRound {
    pub model: HandModel,
    /// The round's feed end, centred across it, in the model's own frame, metres.
    pub top: Vec3,
    /// The direction the round travels into the gun, in the model's own frame, unit length.
    pub axis: Vec3,
    /// The model's turn into the gun's model frame as it lies in the well: none for a spare
    /// magazine, whose frame is the gun's magazine part's, and for a stand-in round, its axis onto
    /// the well's.
    pub in_gun: Quat,
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
        spare_top_m: entry
            .filter(|_| load == LoadKind::Magazine)
            .and_then(|e| e.spare_top_m)
            .filter(|t| t.is_finite()),
        round: entry
            .filter(|_| matches!(load, LoadKind::Shell | LoadKind::Cylinder))
            .and_then(|e| RoundModel::from_entry(&e.round)),
        launcher: is_launcher(facts.class),
        spent_round_replaced: is_launcher(facts.class) && facts.capacity == 1,
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
    use TableAction::{Bolt, ChargingHandle, None as NoAction, Pump, Slide};
    use WellKind::{Cylinder, Magazine, None as Nothing, Rocket, Shell};
    use jc3gi::equipment::weapon::WeaponClass::{
        E_WEAPON_CLASS_ASSAULT_RIFLE, E_WEAPON_CLASS_GRENADE_LAUNCHER, E_WEAPON_CLASS_MORTAR,
        E_WEAPON_CLASS_PISTOL, E_WEAPON_CLASS_RPG, E_WEAPON_CLASS_SHOTGUN, E_WEAPON_CLASS_SMG,
        E_WEAPON_CLASS_SNIPER, E_WEAPON_CLASS_SPECIAL,
    };

    use super::*;
    use crate::controllers::weapons::reload::wells::{self, RoundEntry, TableAction, WellGrade};

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
            part_length_m: Some(0.1),
            spare_top_m: Some(Vec3::new(0.0, 0.06, 0.01)),
            round: RoundEntry::default(),
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
    fn only_a_magazine_is_drawn_with_the_spare_magazine_model() {
        let rifle = plan(
            facts(WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE),
            Some(&entry(
                WellKind::Magazine,
                TableAction::ChargingHandle,
                WellGrade::Part,
            )),
            WRIST,
            0.06,
        )
        .unwrap();
        assert_eq!(rifle.spare_top_m, Some(Vec3::new(0.0, 0.06, 0.01)));
        let launcher = plan(
            facts(WeaponClass::E_WEAPON_CLASS_RPG),
            Some(&entry(WellKind::Rocket, TableAction::None, WellGrade::Part)),
            WRIST,
            0.06,
        )
        .unwrap();
        assert_eq!(launcher.spare_top_m, None);
    }

    #[test]
    fn the_table_draws_pistol_and_smg_magazines_but_not_the_upu_210s_cartridge() {
        let spare = |name: &str| {
            wells::entries()
                .iter()
                .find(|e| e.weapon == name)
                .unwrap_or_else(|| panic!("{name} is in the wells table"))
                .spare_top_m
        };
        for name in [
            "w011_pistol_u_pozhar_98",
            "w031_smg_prizrak_u4",
            "w111_assault_rifle_up_39",
        ] {
            assert!(spare(name).is_some(), "{name}");
        }
        assert_eq!(spare("w123_grenade_launcher_upu_210"), None);
        assert_eq!(spare("w141_rpg_uvk_13"), None);
    }

    fn round_entry(count: u32, ring_m: f32) -> RoundEntry {
        RoundEntry {
            round_model: Some("models/test/casing.lod".to_owned()),
            round_top_m: Some(Vec3::Z * 0.035),
            round_axis: Some(Vec3::Z),
            round_radius_m: Some(0.009),
            round_count: Some(count),
            round_ring_m: Some(ring_m),
        }
    }

    #[test]
    fn a_shell_is_drawn_with_the_stand_in_round_turned_onto_the_well() {
        let e = WellEntry {
            spare_top_m: None,
            magazine_part: false,
            round: round_entry(1, 0.0),
            ..entry(WellKind::Shell, TableAction::Pump, WellGrade::Mesh)
        };
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_SHOTGUN),
            Some(&e),
            WRIST,
            0.06,
        )
        .unwrap();
        let round = p.round.unwrap();
        assert_eq!(round.name_hash, hashlittle(b"models/test/casing.lod"));
        assert_eq!(round.count, 1);
        assert_eq!(round.offsets().collect::<Vec<_>>(), [Vec3::ZERO]);
        assert!((round.half_width_m() - 0.009).abs() < 1e-6);
        let hand = p.hand_round().unwrap();
        assert_eq!(hand.model, HandModel::Round(round));
        assert_eq!(hand.top, Vec3::Z * 0.035);
        // The casing goes in along its own +Z, which lies along the well's +Y in the gun.
        assert!((hand.in_gun * hand.axis - p.part_axis).length() < 1e-6);
    }

    #[test]
    fn a_revolvers_rounds_stand_side_by_side_round_the_cylinders_axis() {
        let e = WellEntry {
            spare_top_m: None,
            magazine_part: false,
            insert_axis: Some(Vec3::NEG_Z),
            round: round_entry(6, 0.013),
            ..entry(WellKind::Cylinder, TableAction::None, WellGrade::Mesh)
        };
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_REVOLVER),
            Some(&e),
            WRIST,
            0.06,
        )
        .unwrap();
        let round = p.round.unwrap();
        let offsets: Vec<Vec3> = round.offsets().collect();
        assert_eq!(offsets.len(), 6);
        for (i, offset) in offsets.iter().enumerate() {
            // Each round's axis is on the ring, square to the group's axis.
            assert!((offset.length() - 0.013).abs() < 1e-6, "{i}");
            assert!(offset.dot(round.axis).abs() < 1e-6, "{i}");
            // Neighbours are a sixth of a turn apart, so the ring's chord equals its radius.
            let next = offsets[(i + 1) % 6];
            assert!((offset.distance(next) - 0.013).abs() < 1e-5, "{i}");
        }
        assert!((round.half_width_m() - 0.022).abs() < 1e-6);
        let hand = p.hand_round().unwrap();
        assert!((hand.in_gun * hand.axis - Vec3::NEG_Z).length() < 1e-6);
    }

    #[test]
    fn a_round_entry_without_a_whole_model_draws_nothing() {
        let mut partial = round_entry(1, 0.0);
        partial.round_top_m = None;
        assert_eq!(RoundModel::from_entry(&partial), None);
        assert_eq!(RoundModel::from_entry(&RoundEntry::default()), None);
        let mut many = round_entry(40, 0.0);
        many.round_axis = Some(Vec3::Z * 3.0);
        let many = RoundModel::from_entry(&many).unwrap();
        assert_eq!(many.count, MAX_HELD_ROUNDS);
        assert_eq!(many.axis, Vec3::Z);
        // A magazine is never drawn with a stand-in round, nor a shell with a spare magazine.
        let rifle = WellEntry {
            round: round_entry(1, 0.0),
            ..entry(
                WellKind::Magazine,
                TableAction::ChargingHandle,
                WellGrade::Part,
            )
        };
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE),
            Some(&rifle),
            WRIST,
            0.06,
        )
        .unwrap();
        assert_eq!(p.round, None);
        assert_eq!(p.hand_round().unwrap().model, HandModel::Spare);
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
    fn the_hydras_pod_is_a_launcher_magazine_drawn_from_its_own_part() {
        let hydra = wells::lookup(3_070_823_687).expect("the Hydra is in the wells table");
        let p = plan(
            facts(WeaponClass::E_WEAPON_CLASS_RPG),
            Some(hydra),
            WRIST,
            0.06,
        )
        .unwrap();
        assert!(p.launcher);
        assert_eq!(p.load, LoadKind::Magazine);
        assert!(p.load.ejects());
        assert_eq!(p.action, ActionKind::None);
        assert!(p.part_top_m.is_some());
        assert_eq!(p.part_axis, Vec3::NEG_Z);
    }

    /// Every weapon in the wells table, with what the game says about its reload: its class, its
    /// magazine size, and whether its reload is exempt (untimed, or of the unlimited ammunition
    /// kind). The reload audit in `docs/mod/input/weapon-reloads.md` lists the same weapons.
    struct Audited {
        weapon: &'static str,
        class: WeaponClass,
        capacity: i32,
        exempt: bool,
        kind: WellKind,
        action: TableAction,
    }

    const fn audited(
        weapon: &'static str,
        class: WeaponClass,
        capacity: i32,
        exempt: bool,
        kind: WellKind,
        action: TableAction,
    ) -> Audited {
        Audited {
            weapon,
            class,
            capacity,
            exempt,
            kind,
            action,
        }
    }

    const AUDIT: &[Audited] = &[
        audited(
            "w011_pistol_u_pozhar_98",
            E_WEAPON_CLASS_PISTOL,
            16,
            false,
            Magazine,
            Slide,
        ),
        audited(
            "w012_pistol_cs_spectre_mark_v",
            E_WEAPON_CLASS_PISTOL,
            14,
            false,
            Magazine,
            Slide,
        ),
        audited(
            "w013_pistol_balloon_head",
            E_WEAPON_CLASS_PISTOL,
            16,
            true,
            Magazine,
            Slide,
        ),
        audited(
            "w014_handgun_little_general",
            E_WEAPON_CLASS_PISTOL,
            12,
            false,
            Cylinder,
            NoAction,
        ),
        audited(
            "w021_handgun_peacekeeper",
            E_WEAPON_CLASS_PISTOL,
            12,
            false,
            Cylinder,
            NoAction,
        ),
        audited(
            "w031_smg_prizrak_u4",
            E_WEAPON_CLASS_SMG,
            40,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w032_smg_cs_wraith_2000",
            E_WEAPON_CLASS_SMG,
            60,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w033_smg_cs_9",
            E_WEAPON_CLASS_SMG,
            36,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w111_assault_rifle_up_39",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            35,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w112_assault_rifle_cs_predator",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            30,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w117_assault_rifle_csv",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            40,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w118_assault_rifle_up_61",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            25,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w119_molten_bavarium_spitter",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            60,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w121_grenade_launcher_cs_bildt_negotiator",
            E_WEAPON_CLASS_GRENADE_LAUNCHER,
            3,
            false,
            Magazine,
            NoAction,
        ),
        audited(
            "w123_grenade_launcher_upu_210",
            E_WEAPON_CLASS_GRENADE_LAUNCHER,
            1,
            false,
            Magazine,
            NoAction,
        ),
        audited(
            "w131_machine_gun_urga_vdova_89",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            80,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w132_machine_gun_capstone_a_92",
            E_WEAPON_CLASS_ASSAULT_RIFLE,
            60,
            false,
            Magazine,
            ChargingHandle,
        ),
        audited(
            "w141_rpg_uvk_13",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w141_rpg_uvk_13_mm110",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w142_rpg_capstone_bloodhound",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w143_rpg_fire_leach",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w144_rpg_capstone_hydra",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Magazine,
            NoAction,
        ),
        audited(
            "w146_rpg_javelin",
            E_WEAPON_CLASS_RPG,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w147_rpg_guitar_case",
            E_WEAPON_CLASS_RPG,
            1,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w151_shotgun_u_rys_96",
            E_WEAPON_CLASS_SHOTGUN,
            6,
            false,
            Shell,
            Pump,
        ),
        audited(
            "w153_shotgun_automat_12",
            E_WEAPON_CLASS_SHOTGUN,
            14,
            false,
            Magazine,
            NoAction,
        ),
        audited(
            "w155_shotgun_u_rys_12",
            E_WEAPON_CLASS_SHOTGUN,
            4,
            false,
            Shell,
            NoAction,
        ),
        audited(
            "w161_sniper_rifle_u_sv_45",
            E_WEAPON_CLASS_SNIPER,
            4,
            false,
            Magazine,
            Bolt,
        ),
        audited(
            "w162_sniper_rifle_cs111_archangel",
            E_WEAPON_CLASS_SNIPER,
            6,
            false,
            Magazine,
            Bolt,
        ),
        audited(
            "w163_sniper_rifle_final_argument",
            E_WEAPON_CLASS_SNIPER,
            6,
            false,
            Magazine,
            Bolt,
        ),
        audited(
            "w173_ragnarok",
            E_WEAPON_CLASS_SPECIAL,
            10,
            false,
            Nothing,
            NoAction,
        ),
        audited(
            "w176_stupka_210",
            E_WEAPON_CLASS_MORTAR,
            1,
            false,
            Rocket,
            NoAction,
        ),
        audited(
            "w177_typhoon",
            E_WEAPON_CLASS_SPECIAL,
            -1,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w178_balloon",
            E_WEAPON_CLASS_SPECIAL,
            0,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w179_namaste",
            E_WEAPON_CLASS_SPECIAL,
            0,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w701_missile_strike",
            E_WEAPON_CLASS_SPECIAL,
            1,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w701_missile_strike_ai",
            E_WEAPON_CLASS_SPECIAL,
            1,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w702_emp",
            E_WEAPON_CLASS_SPECIAL,
            1,
            true,
            Nothing,
            NoAction,
        ),
        audited(
            "w703_sungun",
            E_WEAPON_CLASS_SPECIAL,
            -1,
            false,
            Nothing,
            NoAction,
        ),
        audited("w704_m488", E_WEAPON_CLASS_RPG, 1, false, Rocket, NoAction),
    ];

    #[test]
    fn every_weapon_in_the_table_loads_what_the_audit_says() {
        let entries = wells::entries();
        for e in entries {
            assert!(
                AUDIT.iter().any(|a| a.weapon == e.weapon),
                "{} is in the wells table but not in the audit",
                e.weapon
            );
        }
        for a in AUDIT {
            let e = entries
                .iter()
                .find(|e| e.weapon == a.weapon)
                .unwrap_or_else(|| {
                    panic!("{} is in the audit but not in the wells table", a.weapon)
                });
            assert_eq!(e.kind, a.kind, "{}", a.weapon);
            assert_eq!(e.action, a.action, "{}", a.weapon);
            let facts = WeaponFacts {
                class: a.class as i32,
                capacity: a.capacity,
                exempt: a.exempt,
            };
            let Some(p) = plan(facts, Some(e), WRIST, 0.06) else {
                assert!(
                    a.exempt
                        || a.capacity <= 0
                        || a.kind == WellKind::None
                        || class_defaults(facts.class).is_none(),
                    "{}: no plan",
                    a.weapon
                );
                continue;
            };
            assert!(
                !a.exempt && a.kind != WellKind::None,
                "{}: planned",
                a.weapon
            );
            // Every reloaded weapon's well sits on the gun, not at the wrist.
            assert!(matches!(p.well, WellFrame::Gun { .. }), "{}", a.weapon);
            let launcher = matches!(
                a.class,
                WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER
                    | WeaponClass::E_WEAPON_CLASS_RPG
                    | WeaponClass::E_WEAPON_CLASS_MORTAR
            );
            assert_eq!(p.launcher, launcher, "{}", a.weapon);
            assert_eq!(
                p.spent_round_replaced,
                launcher && a.capacity == 1,
                "{}",
                a.weapon
            );
            // A round the hand carries is drawn from the gun's spare magazine model, its own
            // round part, or for a shell or a cylinder the casing the gun ejects, except where the
            // game has none of these (see the audit).
            let drawn = p.hand_round().is_some() || p.part_top_m.is_some();
            let undrawn = a.weapon == "w153_shotgun_automat_12";
            assert_eq!(drawn, !undrawn, "{}", a.weapon);
            // One push loads one shell, or a revolver's whole cylinder, and the hand shows as many.
            let rounds = p.round.map(|r| r.count);
            let expected = match a.kind {
                Shell => Some(1),
                Cylinder => Some(MAX_HELD_ROUNDS),
                _ => None,
            };
            assert_eq!(rounds, expected, "{}", a.weapon);
        }
    }

    #[test]
    fn without_the_manual_action_no_action_is_worked_by_hand() {
        for (class, action) in [
            (WeaponClass::E_WEAPON_CLASS_PISTOL, TableAction::Slide),
            (WeaponClass::E_WEAPON_CLASS_SNIPER, TableAction::Bolt),
            (WeaponClass::E_WEAPON_CLASS_SHOTGUN, TableAction::Pump),
        ] {
            let e = entry(WellKind::Magazine, action, WellGrade::Mesh);
            let p = plan(facts(class), Some(&e), WRIST, 0.06).unwrap();
            assert_ne!(p.action, ActionKind::None, "{class:?}");
            assert_eq!(p.with_manual_action(true), p);
            let cocked = p.with_manual_action(false);
            assert_eq!(cocked.action, ActionKind::None, "{class:?}");
            assert_eq!(cocked.load, p.load);
            assert_eq!(cocked.well, p.well);
        }
    }

    #[test]
    fn only_a_magazine_or_a_cylinder_still_in_occupies_the_well() {
        for load in [LoadKind::Magazine, LoadKind::Cylinder] {
            assert!(load.well_occupied(false), "{load:?}");
            assert!(!load.well_occupied(true), "{load:?}");
        }
        for load in [LoadKind::Shell, LoadKind::Rocket] {
            assert!(!load.well_occupied(false), "{load:?}");
            assert!(!load.well_occupied(true), "{load:?}");
        }
    }
}
