//! The skinning-palette edits of the hand late update, pure: reading and writing a palette entry in
//! either of the layouts the character blocks use, and the forearm swing.
//!
//! A palette entry is a bone's model-space transform times its inverse bind pose, so pre-multiplying
//! an entry by a model-space transform moves every vertex the bone carries by that transform. The
//! hand bones take the hand's whole correction; the forearm bones take a swing about the elbow
//! that follows the corrected wrist, optionally stretched along the forearm so its far end reaches
//! a hand detached from the arm, so the forearm stays attached at both ends.

use glam::{Mat3, Mat4, Quat, Vec3, Vec4};

/// How a block's palette stores a 3x4 affine matrix (see the layout detection in
/// `crate::hooks::graphics_engine::render_block`).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PaletteLayout {
    /// Four 3-float columns: the images of the X, Y, and Z axes, then the translation.
    Columns,
    /// Three 4-float rows of `[R | t]`.
    Rows,
}

/// Read a palette entry as a transform.
pub fn entry_to_matrix(m: &[f32; 12], layout: PaletteLayout) -> Mat4 {
    match layout {
        PaletteLayout::Columns => Mat4::from_cols(
            Vec4::new(m[0], m[1], m[2], 0.0),
            Vec4::new(m[3], m[4], m[5], 0.0),
            Vec4::new(m[6], m[7], m[8], 0.0),
            Vec4::new(m[9], m[10], m[11], 1.0),
        ),
        PaletteLayout::Rows => Mat4::from_cols(
            Vec4::new(m[0], m[4], m[8], 0.0),
            Vec4::new(m[1], m[5], m[9], 0.0),
            Vec4::new(m[2], m[6], m[10], 0.0),
            Vec4::new(m[3], m[7], m[11], 1.0),
        ),
    }
}

/// Write a transform back into a palette entry in the same layout.
pub fn matrix_to_entry(t: &Mat4, layout: PaletteLayout) -> [f32; 12] {
    let (x, y, z, w) = (t.x_axis, t.y_axis, t.z_axis, t.w_axis);
    match layout {
        PaletteLayout::Columns => [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, w.x, w.y, w.z],
        PaletteLayout::Rows => [x.x, y.x, z.x, w.x, x.y, y.y, z.y, w.y, x.z, y.z, z.z, w.z],
    }
}

/// Pre-multiply a palette entry by `edit`, in place.
pub fn premultiply_entry(m: &mut [f32; 12], layout: PaletteLayout, edit: &Mat4) {
    *m = matrix_to_entry(&(*edit * entry_to_matrix(m, layout)), layout);
}

/// The forearm's swing about the elbow `elbow` that takes the wrist `wrist` toward the corrected
/// wrist `wrist_corrected`: a rotation about the elbow, so the forearm keeps its length and stays
/// on the elbow. The identity for a degenerate forearm.
pub fn forearm_swing(elbow: Vec3, wrist: Vec3, wrist_corrected: Vec3) -> Mat4 {
    let (Some(from), Some(to)) = (
        (wrist - elbow).try_normalize(),
        (wrist_corrected - elbow).try_normalize(),
    ) else {
        return Mat4::IDENTITY;
    };
    Mat4::from_translation(elbow)
        * Mat4::from_quat(Quat::from_rotation_arc(from, to))
        * Mat4::from_translation(-elbow)
}

/// The forearm's transform toward the corrected wrist: the swing about the elbow
/// ([`forearm_swing`]), and with `stretch` a scale along the corrected forearm's axis about the
/// elbow, so the forearm's far end lands on the corrected wrist even when the hand has moved off the
/// end of the arm. The stretch is bounded to [`MAX_FOREARM_STRETCH`] either way.
pub fn forearm_reach(elbow: Vec3, wrist: Vec3, wrist_corrected: Vec3, stretch: bool) -> Mat4 {
    let swing = forearm_swing(elbow, wrist, wrist_corrected);
    let (Some(axis), true) = ((wrist_corrected - elbow).try_normalize(), stretch) else {
        return swing;
    };
    let from = (wrist - elbow).length();
    let to = (wrist_corrected - elbow).length();
    if !(from.is_finite() && to.is_finite()) || from <= f32::EPSILON {
        return swing;
    }
    let ratio = (to / from).clamp(1.0 / MAX_FOREARM_STRETCH, MAX_FOREARM_STRETCH);
    let along = Mat3::IDENTITY
        + Mat3::from_cols(axis * axis.x, axis * axis.y, axis * axis.z) * (ratio - 1.0);
    Mat4::from_translation(elbow) * Mat4::from_mat3(along) * Mat4::from_translation(-elbow) * swing
}

/// The most the forearm stretches or shrinks along its axis, as a ratio of its length.
pub const MAX_FOREARM_STRETCH: f32 = 1.5;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Mat4 {
        Mat4::from_scale_rotation_translation(
            Vec3::ONE,
            Quat::from_euler(glam::EulerRot::YXZ, 0.3, -0.7, 1.1),
            Vec3::new(0.2, 1.4, -0.3),
        )
    }

    #[test]
    fn entries_round_trip_in_both_layouts() {
        let t = sample();
        for layout in [PaletteLayout::Columns, PaletteLayout::Rows] {
            let entry = matrix_to_entry(&t, layout);
            assert!(entry_to_matrix(&entry, layout).abs_diff_eq(t, 1e-6));
        }
    }

    #[test]
    fn the_layouts_place_the_translation_where_the_detection_expects() {
        let t = sample();
        let columns = matrix_to_entry(&t, PaletteLayout::Columns);
        assert_eq!(&columns[9..], &t.w_axis.truncate().to_array());
        let rows = matrix_to_entry(&t, PaletteLayout::Rows);
        assert_eq!([rows[3], rows[7], rows[11]], t.w_axis.truncate().to_array());
    }

    #[test]
    fn premultiplying_applies_the_edit_after_the_entry() {
        let t = sample();
        let edit = Mat4::from_translation(Vec3::new(0.0, 0.1, 0.0));
        let mut entry = matrix_to_entry(&t, PaletteLayout::Rows);
        premultiply_entry(&mut entry, PaletteLayout::Rows, &edit);
        let p = Vec3::new(0.5, -0.2, 0.1);
        let moved = entry_to_matrix(&entry, PaletteLayout::Rows).transform_point3(p);
        assert!(moved.abs_diff_eq(t.transform_point3(p) + Vec3::new(0.0, 0.1, 0.0), 1e-5));
    }

    #[test]
    fn the_swing_keeps_the_elbow_and_aims_the_wrist() {
        let elbow = Vec3::new(0.3, 1.2, 0.0);
        let wrist = elbow + Vec3::new(0.0, 0.0, -0.27);
        let corrected = wrist + Vec3::new(0.05, 0.03, 0.0);
        let swing = forearm_swing(elbow, wrist, corrected);
        assert!(swing.transform_point3(elbow).abs_diff_eq(elbow, 1e-6));
        let swung = swing.transform_point3(wrist);
        assert!(((swung - elbow).length() - 0.27).abs() < 1e-5);
        let direction = (corrected - elbow).normalize();
        assert!((swung - elbow).normalize().abs_diff_eq(direction, 1e-5));
        assert_eq!(forearm_swing(elbow, elbow, corrected), Mat4::IDENTITY);
    }

    #[test]
    fn the_stretch_lands_the_forearm_on_a_detached_wrist() {
        let elbow = Vec3::new(0.3, 1.2, 0.0);
        let wrist = elbow + Vec3::new(0.0, 0.0, -0.25);
        let corrected = wrist + Vec3::new(0.02, 0.0, -0.08);
        let reach = forearm_reach(elbow, wrist, corrected, true);
        assert!(reach.transform_point3(elbow).abs_diff_eq(elbow, 1e-6));
        assert!(reach.transform_point3(wrist).abs_diff_eq(corrected, 1e-5));
        // A point beside the forearm keeps its distance from the axis.
        let side = elbow + Vec3::new(0.03, 0.0, -0.1);
        let moved = reach.transform_point3(side) - elbow;
        let axis = (corrected - elbow).normalize();
        let radial = (moved - axis * moved.dot(axis)).length();
        assert!((radial - 0.03).abs() < 1e-3);
    }

    #[test]
    fn without_the_stretch_the_reach_is_the_swing() {
        let elbow = Vec3::new(0.3, 1.2, 0.0);
        let wrist = elbow + Vec3::new(0.0, 0.0, -0.25);
        let corrected = wrist + Vec3::new(0.0, 0.0, -0.08);
        assert_eq!(
            forearm_reach(elbow, wrist, corrected, false),
            forearm_swing(elbow, wrist, corrected)
        );
    }

    #[test]
    fn the_stretch_is_bounded() {
        let elbow = Vec3::ZERO;
        let wrist = Vec3::NEG_Z * 0.2;
        let far = Vec3::NEG_Z * 1.0;
        let reach = forearm_reach(elbow, wrist, far, true);
        let end = reach.transform_point3(wrist);
        assert!((end.length() - 0.2 * MAX_FOREARM_STRETCH).abs() < 1e-5);
        assert_eq!(forearm_reach(elbow, elbow, far, true), Mat4::IDENTITY);
    }
}
