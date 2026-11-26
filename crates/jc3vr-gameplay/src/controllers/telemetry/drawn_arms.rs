//! The arm joints of Rico's drawn body as recording events and debug log lines: the shoulders,
//! elbows, and wrists where the presentation's edited bone palette draws them, in model space, so a
//! recording can compare what rendered with the pose the solves left.

use std::time::{Duration, Instant};

use glam::Vec3;
use jc3gi::graphics_engine::render_block::Matrix3x4;
use jc3vr_render::{
    hooks::graphics_engine::palette_edit::PaletteLayout,
    recorder::tracking::events::{self, EventKind},
    stereo,
};
use parking_lot::Mutex;
use tracking_format::event::{Diagnostic, Label};

use crate::controllers::hands::palette::entry_to_matrix;

/// The drawn arm joints, each joint's bind point carried through its edited palette entry, or
/// `None` when an entry is missing or a point is not finite.
pub(crate) fn drawn_arm_joints(
    entries: &[Matrix3x4],
    layout: PaletteLayout,
    bones: [[usize; 3]; 2],
    bind_points: [[Vec3; 3]; 2],
) -> Option<[[Vec3; 3]; 2]> {
    let mut joints = [[Vec3::ZERO; 3]; 2];
    for side in 0..2 {
        for joint in 0..3 {
            let entry = entries.get(bones[side][joint])?;
            let matrix = entry_to_matrix(&entry.m, layout);
            let bind = bind_points[side][joint];
            let point = matrix.transform_point3(bind);
            if !(matrix.is_finite() && bind.is_finite() && point.is_finite()) {
                return None;
            }
            joints[side][joint] = point;
        }
    }
    Some(joints)
}

/// Note the drawn arm joints of the palette sequence `sequence`, once per sequence, on the first
/// eye's draw during gameplay.
pub(crate) fn sample_drawn_arms(sequence: u64, dtf: f32, joints: [[Vec3; 3]; 2]) {
    if stereo::draw_index() != 0
        || !jc3vr_core::hooks::in_gameplay()
        || !dtf.is_finite()
        || !joints.iter().flatten().all(|point| point.is_finite())
    {
        return;
    }
    let collecting = events::collecting();
    let trace_enabled = tracing::enabled!(target: "controllers", tracing::Level::DEBUG);
    if !collecting && !trace_enabled {
        return;
    }
    let now = Instant::now();
    let trace = {
        let mut sampled = DRAWN.lock();
        if sampled.sequence.is_some_and(|last| sequence <= last) {
            return;
        }
        sampled.sequence = Some(sequence);
        let trace = trace_enabled
            && sampled
                .traced_at
                .is_none_or(|last| now.saturating_duration_since(last) >= DRAWN_TRACE_INTERVAL);
        if trace {
            sampled.traced_at = Some(now);
        }
        trace
    };
    let [
        [left_shoulder, left_elbow, left_wrist],
        [right_shoulder, right_elbow, right_wrist],
    ] = joints.map(|arm| arm.map(|point| point.to_array()));
    if trace {
        tracing::debug!(
            target: "controllers",
            stage = "post_presentation_palette",
            space = "model",
            sequence,
            dtf,
            left_shoulder_m = ?left_shoulder,
            left_elbow_m = ?left_elbow,
            left_wrist_m = ?left_wrist,
            right_shoulder_m = ?right_shoulder,
            right_elbow_m = ?right_elbow,
            right_wrist_m = ?right_wrist,
            "drawn arm joints"
        );
    }
    if collecting {
        events::note_with(|| {
            EventKind::Diagnostic(Box::new(Diagnostic {
                source: Label::Borrowed("presentation"),
                hand: None,
                what: Label::Borrowed("drawn arm joints"),
                detail: Some(Label::Owned(format!(
                    "stage=post_presentation_palette space=model sequence={sequence} dtf={dtf:.6} \
                     left_shoulder_m={left_shoulder:?} left_elbow_m={left_elbow:?} left_wrist_m={left_wrist:?} \
                     right_shoulder_m={right_shoulder:?} right_elbow_m={right_elbow:?} right_wrist_m={right_wrist:?}"
                ))),
            }))
        });
    }
}

const DRAWN_TRACE_INTERVAL: Duration = Duration::from_millis(500);

struct DrawnSample {
    sequence: Option<u64>,
    traced_at: Option<Instant>,
}

static DRAWN: Mutex<DrawnSample> = Mutex::new(DrawnSample {
    sequence: None,
    traced_at: None,
});

#[cfg(test)]
mod tests {
    use glam::{Mat4, Quat};

    use super::*;
    use crate::controllers::hands::palette::matrix_to_entry;

    #[test]
    fn drawn_joints_use_bind_points_in_both_layouts() {
        let bind = [[Vec3::X, Vec3::Y, Vec3::Z], [-Vec3::X, -Vec3::Y, -Vec3::Z]];
        let bones = [[0, 1, 2], [3, 4, 5]];
        let edits: [Mat4; 6] = std::array::from_fn(|i| {
            Mat4::from_scale_rotation_translation(
                Vec3::splat(1.0 + i as f32 * 0.1),
                Quat::from_rotation_z(i as f32 * 0.2),
                Vec3::new(i as f32, 2.0, -3.0),
            )
        });
        for layout in [PaletteLayout::Rows, PaletteLayout::Columns] {
            let entries = edits.map(|edit| Matrix3x4 {
                m: matrix_to_entry(&edit, layout),
            });
            let drawn = drawn_arm_joints(&entries, layout, bones, bind).unwrap();
            for side in 0..2 {
                for joint in 0..3 {
                    let expected = edits[bones[side][joint]].transform_point3(bind[side][joint]);
                    assert!(drawn[side][joint].abs_diff_eq(expected, 1e-5));
                }
            }
        }
    }

    #[test]
    fn drawn_joints_reject_incomplete_or_non_finite_palettes() {
        let layout = PaletteLayout::Columns;
        let identity = Matrix3x4 {
            m: matrix_to_entry(&Mat4::IDENTITY, layout),
        };
        let bones = [[0, 1, 2], [3, 4, 5]];
        let bind = [[Vec3::ZERO; 3]; 2];
        assert!(drawn_arm_joints(&[identity; 5], layout, bones, bind).is_none());
        let mut entries = [identity; 6];
        entries[4].m[0] = f32::NAN;
        assert!(drawn_arm_joints(&entries, layout, bones, bind).is_none());
        let mut bad_bind = bind;
        bad_bind[0][1].x = f32::INFINITY;
        assert!(drawn_arm_joints(&[identity; 6], layout, bones, bad_bind).is_none());
    }
}
