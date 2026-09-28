//! The skinning-palette edits of the hand late update, pure: reading and writing a palette entry in
//! either of the layouts the character blocks use, and the forearm swing.
//!
//! A palette entry is a bone's model-space transform times its inverse bind pose, so pre-multiplying
//! an entry by a model-space transform moves every vertex the bone carries by that transform. The
//! hand bones take the hand's whole correction; the forearm bones take a swing about the elbow
//! that follows the corrected wrist, so the forearm stays attached at both ends.

use glam::{Mat4, Quat, Vec3, Vec4};

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
}
