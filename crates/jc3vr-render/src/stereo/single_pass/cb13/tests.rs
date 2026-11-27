use glam::{DMat4, DVec3, EulerRot, Mat4, Quat, Vec3, Vec4};
use jc3gi::types::math::Vector4;
use jc3vr_view::vr::{EyeRenderParams, Fov, OffAxisProjection};
use test_case::test_case;

use crate::stereo::single_pass::cb13::{
    MEYE_ROW_BASE, STEREO_CB_TOTAL_ROWS, clip_origin_row, pin_rows_to_eye, relative_reprojection,
    write_meye,
};

#[test_case(10_000.0, 0; "left at ten thousand")]
#[test_case(10_000.0, 1; "right at ten thousand")]
#[test_case(100_000.0, 0; "left at one hundred thousand")]
#[test_case(100_000.0, 1; "right at one hundred thousand")]
fn relative_reprojection_matches_direct_eye_relative_projection(origin: f32, eye: usize) {
    let fixture = fixture(origin, eye);
    let old = full_vp_reprojection(&fixture);
    let relative = relative_matrix(&fixture);
    let mut old_error = 0.0_f64;
    let mut relative_error = 0.0_f64;
    let mut direct_error = 0.0_f64;
    for point in points(&fixture) {
        let center_clip = fixture.center_offset_vp * (point - fixture.center_position).extend(1.0);
        let direct = fixture.eye_offset_vp * (point - fixture.eye_position).extend(1.0);
        let reference = fixture.eye_offset_vp.as_dmat4()
            * (point.as_dvec3() - fixture.eye_position.as_dvec3()).extend(1.0);
        let reference_ndc = reference.truncate() / reference.w;
        old_error = old_error.max((ndc(old * center_clip) - reference_ndc).abs().max_element());
        relative_error = relative_error.max(
            (ndc(relative * center_clip) - reference_ndc)
                .abs()
                .max_element(),
        );
        direct_error = direct_error.max((ndc(direct) - reference_ndc).abs().max_element());
        assert!(
            (ndc(relative * center_clip) - ndc(direct))
                .abs()
                .max_element()
                < 2.0e-6,
            "origin={origin}, eye={eye}, point={point:?}",
        );
    }
    eprintln!(
        "origin={origin}, eye={eye}: max NDC error old={old_error:.9e}, relative={relative_error:.9e}, direct={direct_error:.9e}"
    );
    assert!(
        old_error > 1.0e-4,
        "the fixture must expose the f32 full-VP defect"
    );
    assert!(relative_error < 2.0e-6);
    assert!(relative_error * 100.0 < old_error);
}

#[test]
fn relative_translation_uses_the_stored_camera_positions() {
    let fixture = fixture(100_000.0, 0);
    let actual_displacement = fixture.eye_position.as_dvec3() - fixture.center_position.as_dvec3();
    assert!((actual_displacement - fixture.requested_offset.as_dvec3()).length() > 1.0e-3);
    let wrong = (fixture.eye_offset_vp.as_dmat4()
        * DMat4::from_translation(-fixture.requested_offset.as_dvec3())
        * fixture.center_offset_vp.as_dmat4().inverse())
    .as_mat4();
    let relative = relative_matrix(&fixture);
    let point = points(&fixture)[0];
    let center_clip = fixture.center_offset_vp * (point - fixture.center_position).extend(1.0);
    let direct = fixture.eye_offset_vp * (point - fixture.eye_position).extend(1.0);
    assert!(
        (ndc(relative * center_clip) - ndc(direct))
            .abs()
            .max_element()
            < 2.0e-6
    );
    assert!((ndc(wrong * center_clip) - ndc(direct)).abs().max_element() > 1.0e-3);
}

#[test]
fn coincident_views_reproject_to_identity() {
    let fixture = fixture(100_000.0, 0);
    let matrix = relative_reprojection(
        fixture.center_offset_vp.as_dmat4().inverse(),
        fixture.center_position,
        fixture.center_offset_vp,
        fixture.center_position,
    );
    assert!(
        (matrix - Mat4::IDENTITY)
            .to_cols_array()
            .into_iter()
            .all(|value| value.abs() < 1.0e-7)
    );
}

#[test]
fn writing_reprojection_preserves_per_eye_rows_and_row_orientation() {
    let mut rows = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    for (index, row) in rows.iter_mut().take(MEYE_ROW_BASE).enumerate() {
        row.data = [index as f32 + 0.125, 2.0, -3.0, 4.0];
    }
    for eye in 0..2 {
        rows[clip_origin_row(eye)].data = [10.0 + eye as f32, 20.0, 30.0, 1.0];
    }
    let original = rows;
    let mut matrices = [Mat4::IDENTITY; 2];
    for (eye, matrix) in matrices.iter_mut().enumerate() {
        *matrix = relative_matrix(&fixture(100_000.0, eye));
        write_meye(&mut rows, eye, *matrix);
    }
    for index in 0..MEYE_ROW_BASE {
        assert_eq!(rows[index].data, original[index].data);
    }
    let clip = Vec4::new(0.2, -0.4, 0.01, 1.7);
    for (eye, matrix) in matrices.into_iter().enumerate() {
        let uploaded = Mat4::from_cols_array_2d(&std::array::from_fn(|r| {
            rows[MEYE_ROW_BASE + eye * 4 + r].data
        }))
        .transpose();
        assert_eq!(uploaded, matrix);
        assert!((uploaded * clip - matrix * clip).abs().max_element() < 1.0e-6);
        let pinned = pin_rows_to_eye(&rows, eye);
        for slot in 0..2 {
            for r in 0..5 {
                assert_eq!(pinned[slot * 5 + r].data, rows[eye * 5 + r].data);
            }
            for r in 0..4 {
                assert_eq!(
                    pinned[MEYE_ROW_BASE + slot * 4 + r].data,
                    rows[MEYE_ROW_BASE + eye * 4 + r].data,
                );
            }
            assert_eq!(
                pinned[clip_origin_row(slot)].data,
                original[clip_origin_row(eye)].data
            );
        }
    }
}

struct Fixture {
    center_position: Vec3,
    eye_position: Vec3,
    requested_offset: Vec3,
    center_world: Mat4,
    center_offset_vp: Mat4,
    eye_offset_vp: Mat4,
}

fn fixture(origin: f32, eye: usize) -> Fixture {
    let center_position = Vec3::new(origin + 0.125, -0.37 * origin, 0.83 * origin);
    let center_world = Mat4::from_rotation_translation(
        Quat::from_euler(EulerRot::YXZ, 0.73, -0.29, 0.12),
        center_position,
    );
    let side = if eye == 0 { -1.0 } else { 1.0 };
    let projection = OffAxisProjection::new(
        Fov {
            left: -0.95 + side * 0.06,
            right: 0.90 + side * 0.06,
            up: 0.88,
            down: -0.81,
        },
        0.1,
        38_400.0,
    );
    let params = EyeRenderParams {
        projection_standard: projection.standard_depth,
        projection_reverse_z: projection.reverse_z,
        local_offset: Vec3::new(side * 0.032, 0.0013, -0.0021),
        orientation_delta: Quat::from_euler(EulerRot::YXZ, side * 0.085, 0.006, -side * 0.009),
        jitter: None,
    };
    let center_projection = OffAxisProjection::new(
        Fov {
            left: -1.05,
            right: 1.05,
            up: 0.98,
            down: -0.98,
        },
        0.1,
        38_400.0,
    );
    let mut center_view = center_world.inverse();
    center_view.w_axis = Vec4::W;
    let mut eye_view = params.resolve(center_world).inverse();
    eye_view.w_axis = Vec4::W;
    let requested_offset = params.world_offset(center_world);
    Fixture {
        center_position,
        eye_position: center_position + requested_offset,
        requested_offset,
        center_world,
        center_offset_vp: Mat4::from(center_projection.reverse_z) * center_view,
        eye_offset_vp: Mat4::from(params.projection_reverse_z) * eye_view,
    }
}

fn points(fixture: &Fixture) -> [Vec3; 6] {
    [
        Vec3::new(0.08, -0.04, -0.35),
        Vec3::new(-0.2, 0.1, -0.7),
        Vec3::new(0.3, 0.5, -2.0),
        Vec3::new(-3.0, -2.0, -10.0),
        Vec3::new(40.0, 20.0, -100.0),
        Vec3::new(-300.0, 200.0, -1000.0),
    ]
    .map(|point| fixture.center_world.transform_point3(point))
}

fn full_vp_reprojection(fixture: &Fixture) -> Mat4 {
    let center_full = fixture.center_offset_vp * Mat4::from_translation(-fixture.center_position);
    let eye_full = fixture.eye_offset_vp * Mat4::from_translation(-fixture.eye_position);
    (eye_full.as_dmat4() * center_full.as_dmat4().inverse()).as_mat4()
}

fn relative_matrix(fixture: &Fixture) -> Mat4 {
    relative_reprojection(
        fixture.center_offset_vp.as_dmat4().inverse(),
        fixture.center_position,
        fixture.eye_offset_vp,
        fixture.eye_position,
    )
}

fn ndc(clip: Vec4) -> DVec3 {
    clip.truncate().as_dvec3() / f64::from(clip.w)
}
