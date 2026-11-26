use std::sync::OnceLock;

use glam::{Quat, Vec3};
use serde::Deserialize;

use crate::controllers::weapons::reload::{
    plan::{LoadKind, ReloadPlan, RoundModel},
    wells::{self, WellKind},
};

pub(super) struct Shape {
    pub axis: Vec3,
    pub segments: Vec<(Vec3, Vec3)>,
}

pub(super) fn shape(name_hash: u32, plan: &ReloadPlan) -> Shape {
    let kind = match plan.load {
        LoadKind::Magazine => WellKind::Magazine,
        LoadKind::Shell => WellKind::Shell,
        LoadKind::Rocket => WellKind::Rocket,
        LoadKind::Cylinder => WellKind::Cylinder,
    };
    let templates = templates();
    let measured = templates
        .iter()
        .find(|entry| entry.weapon_hash == name_hash && entry.kind == kind)
        .unwrap_or_else(|| templates.iter().find(|entry| entry.kind == kind).unwrap());
    let axis = plan.part_axis.try_normalize().unwrap_or(Vec3::Y);
    let rotation = Quat::from_rotation_arc(measured.axis.normalize(), axis);
    let round = plan.round.or_else(|| {
        wells::lookup(measured.weapon_hash).and_then(|entry| RoundModel::from_entry(&entry.round))
    });
    let offsets = round.map_or_else(
        || vec![Vec3::ZERO],
        |round| {
            let rotation = Quat::from_rotation_arc(round.axis, axis);
            round.offsets().map(|offset| rotation * offset).collect()
        },
    );
    Shape {
        axis,
        segments: offsets
            .into_iter()
            .flat_map(|offset| {
                measured
                    .segments
                    .iter()
                    .map(move |[a, b]| (rotation * *a + offset, rotation * *b + offset))
            })
            .collect(),
    }
}

#[derive(Deserialize)]
struct Measured {
    weapon_hash: u32,
    kind: WellKind,
    axis: Vec3,
    segments: Vec<[Vec3; 2]>,
}

fn templates() -> &'static [Measured] {
    static TABLE: OnceLock<Vec<Measured>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(include_str!("measured.json")).unwrap())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::controllers::weapons::reload::{chamber::ActionKind, plan::WellFrame};

    fn plan(load: LoadKind) -> ReloadPlan {
        ReloadPlan {
            load,
            action: ActionKind::None,
            well: WellFrame::Wrist { offset: Vec3::ZERO },
            handle: None,
            travel_m: 0.05,
            part_top_m: None,
            part_axis: Vec3::Y,
            spare_top_m: None,
            round: None,
            launcher: false,
            spent_round_replaced: false,
        }
    }

    #[test]
    fn every_supported_table_entry_has_a_measured_nondegenerate_silhouette() {
        let mut hashes = BTreeSet::new();
        let mut counts = [0; 4];
        for measured in templates() {
            assert!(hashes.insert(measured.weapon_hash));
            let entry = wells::lookup(measured.weapon_hash).unwrap();
            assert!(entry.grade.trusted());
            assert_eq!(entry.kind, measured.kind);
            assert!(measured.axis.is_normalized());
            assert!(measured.segments.len() >= 12);
            let mut back = f32::INFINITY;
            let mut front = f32::NEG_INFINITY;
            for [a, b] in &measured.segments {
                assert!(a.is_finite() && b.is_finite());
                assert!(a.distance(*b) > 1.0e-6);
                back = back.min(a.dot(measured.axis)).min(b.dot(measured.axis));
                front = front.max(a.dot(measured.axis)).max(b.dot(measured.axis));
            }
            assert!(front.abs() < 0.001);
            assert!(back < -0.02);
            if entry.magazine_part {
                assert!((front - back - entry.part_length_m.unwrap()).abs() < 0.001);
            }
            let load = match entry.kind {
                WellKind::Magazine => LoadKind::Magazine,
                WellKind::Rocket => LoadKind::Rocket,
                WellKind::Shell => LoadKind::Shell,
                WellKind::Cylinder => LoadKind::Cylinder,
                WellKind::None => panic!(),
            };
            let shape = shape(
                entry.weapon_hash,
                &ReloadPlan {
                    part_axis: measured.axis,
                    ..plan(load)
                },
            );
            assert!(!shape.segments.is_empty());
            assert!(
                shape
                    .segments
                    .iter()
                    .all(|(a, b)| a.is_finite() && b.is_finite())
            );
            let kind = match entry.kind {
                WellKind::Magazine => 0,
                WellKind::Rocket => 1,
                WellKind::Shell => 2,
                WellKind::Cylinder => 3,
                WellKind::None => panic!(),
            };
            counts[kind] += 1;
        }
        assert_eq!(counts, [20, 7, 2, 2]);
        for entry in wells::entries()
            .iter()
            .filter(|entry| entry.grade.trusted() && entry.kind != WellKind::None)
        {
            assert!(hashes.contains(&entry.weapon_hash), "{}", entry.weapon);
        }
    }

    #[test]
    fn missing_runtime_models_and_missing_table_metadata_still_produce_volume() {
        for load in [
            LoadKind::Magazine,
            LoadKind::Rocket,
            LoadKind::Shell,
            LoadKind::Cylinder,
        ] {
            for axis in [
                Vec3::X,
                Vec3::Y,
                -Vec3::Z,
                Vec3::new(1.0, 2.0, 3.0).normalize(),
            ] {
                let plan = ReloadPlan {
                    part_axis: axis,
                    ..plan(load)
                };
                let shape = shape(0, &plan);
                assert!(shape.segments.len() >= 12);
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for (a, b) in shape.segments {
                    min = min.min(a).min(b);
                    max = max.max(a).max(b);
                }
                assert!((max - min).min_element() > 0.005);
            }
        }
    }

    #[test]
    fn a_cylinder_keeps_all_six_measured_rounds_instead_of_one_ring() {
        let entry = wells::entries()
            .iter()
            .find(|entry| entry.kind == WellKind::Cylinder)
            .unwrap();
        let plan = plan(LoadKind::Cylinder);
        let shape = shape(entry.weapon_hash, &plan);
        let cylinder = templates()
            .iter()
            .find(|entry| entry.kind == WellKind::Cylinder)
            .unwrap();
        assert_eq!(shape.segments.len(), cylinder.segments.len() * 6);
    }
}
