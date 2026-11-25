//! Rico's recorded pose (from version 8): the body's bones before the HumanIK solves and as they
//! rendered, each as a skeleton in the character's frame, the pre-IK sample's HumanIK position
//! targets, and Rico's heading from the final pose's character transform.
//!
//! Each stage is an entity under `world/pose` whose transform is the character's model-to-world
//! transform, so its bones are logged in model space as the sample holds them.

use glam::Vec3;
use rerun::{
    Arrows3D, Color, LineStrips3D, Points3D, RecordingStreamResult, Rgba32, Transform3D,
    TransformAxes3D,
};
use tracking_format::{
    event::{EventKind, IkGoal, PoseSample, PoseStage},
    model::Pose,
};

use crate::rerun_export::{Context, scene::rotation};

/// Log the pose samples within `(from, to)`.
pub(crate) fn log(context: &Context<'_>, (from, to): (u64, u64)) -> RecordingStreamResult<()> {
    let mut axes = false;
    for event in &context.recording.events {
        if !(from..=to).contains(&event.time_ns) {
            continue;
        }
        let EventKind::Pose(sample) = &event.kind else {
            continue;
        };
        context.set_time(event.time_ns);
        if !axes && sample.stage == PoseStage::Final {
            context
                .rec
                .log_static("world/pose/final", &TransformAxes3D::new(0.25))?;
            axes = true;
        }
        log_sample(context, sample)?;
    }
    Ok(())
}

/// The bones' segments, parent to child, in the bones' own space: one for each bone whose parent
/// is in the list and both are present.
pub(crate) fn bone_segments(bones: &[Option<Pose>], parents: &[Option<usize>]) -> Vec<[Vec3; 2]> {
    bones
        .iter()
        .zip(parents)
        .filter_map(|(bone, parent)| {
            let child = bone.as_ref()?.position;
            let parent = bones.get((*parent)?)?.as_ref()?.position;
            Some([parent, child])
        })
        .collect()
}

const PRE_IK: Color = Color(Rgba32::from_rgb(150, 150, 150));
const FINAL: Color = Color(Rgba32::from_rgb(255, 170, 60));
const TARGET: Color = Color(Rgba32::from_rgb(255, 60, 200));
const HEADING: Color = Color(Rgba32::from_rgb(255, 140, 0));

/// How long the heading arrow is, metres.
const HEADING_M: f32 = 0.6;

fn log_sample(context: &Context<'_>, sample: &PoseSample) -> RecordingStreamResult<()> {
    let rec = context.rec;
    let stage = sample.stage.name();
    let base = format!("world/pose/{stage}");
    let color = match sample.stage {
        PoseStage::PreIk => PRE_IK,
        PoseStage::Final => FINAL,
    };
    rec.log(
        base.as_str(),
        &Transform3D::from_translation_rotation(
            context.world(sample.character.position),
            rotation(sample.character.orientation),
        ),
    )?;
    let segments: Vec<[[f32; 3]; 2]> = bone_segments(&sample.bones, sample.set.parents())
        .into_iter()
        .map(|[a, b]| [a.to_array(), b.to_array()])
        .collect();
    rec.log(
        format!("{base}/bones"),
        &LineStrips3D::new(segments)
            .with_colors([color])
            .with_radii([0.012]),
    )?;
    let (joints, names): (Vec<[f32; 3]>, Vec<&str>) = sample
        .bones
        .iter()
        .zip(sample.set.names())
        .filter_map(|(bone, name)| Some((bone.as_ref()?.position.to_array(), *name)))
        .unzip();
    rec.log(
        format!("{base}/joints"),
        &Points3D::new(joints)
            .with_radii([0.015])
            .with_colors([color])
            .with_labels(names)
            .with_show_labels(false),
    )?;
    match sample.stage {
        PoseStage::PreIk => {
            let (targets, labels): (Vec<[f32; 3]>, Vec<String>) = sample
                .targets
                .iter()
                .filter_map(|target| match target.goal {
                    IkGoal::Position(position) => Some((
                        position.to_array(),
                        format!(
                            "effector {} reach {:.2} pull {:.2}",
                            target.effector, target.reach, target.pull
                        ),
                    )),
                    IkGoal::Rotation { .. } => None,
                })
                .unzip();
            rec.log(
                format!("{base}/ik_targets"),
                &Points3D::new(targets)
                    .with_radii([0.02])
                    .with_colors([TARGET])
                    .with_labels(labels)
                    .with_show_labels(false),
            )
        }
        PoseStage::Final => {
            let forward = sample.character.orientation * Vec3::NEG_Z;
            let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
            rec.log(
                "world/rico/heading",
                &Arrows3D::from_vectors([(flat * HEADING_M).to_array()])
                    .with_colors([HEADING])
                    .with_radii([0.02]),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::Quat;
    use tracking_format::event::{BODY_BONE_PARENTS, BODY_BONES};

    use super::*;

    fn at(position: Vec3) -> Option<Pose> {
        Some(Pose {
            position,
            orientation: Quat::IDENTITY,
        })
    }

    #[test]
    fn every_present_bone_with_a_present_parent_is_a_segment() {
        let bones: Vec<Option<Pose>> = (0..BODY_BONES.len())
            .map(|i| at(Vec3::new(0.0, i as f32, 0.0)))
            .collect();
        let segments = bone_segments(&bones, &BODY_BONE_PARENTS);
        // Every bone but the hips has a parent.
        assert_eq!(segments.len(), BODY_BONES.len() - 1);
        // The left hand hangs from the left forearm.
        assert!(segments.contains(&[Vec3::new(0.0, 9.0, 0.0), Vec3::new(0.0, 10.0, 0.0)]));
    }

    #[test]
    fn a_missing_bone_drops_its_own_segment_and_its_childrens() {
        let mut bones: Vec<Option<Pose>> = (0..BODY_BONES.len())
            .map(|i| at(Vec3::new(i as f32, 0.0, 0.0)))
            .collect();
        // Without the left forearm, neither the arm to it nor the hand from it is drawn.
        bones[9] = None;
        let segments = bone_segments(&bones, &BODY_BONE_PARENTS);
        assert_eq!(segments.len(), BODY_BONES.len() - 3);
        assert!(segments.iter().all(|[a, b]| a.x != 9.0 && b.x != 9.0));
    }
}
