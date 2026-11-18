use std::cell::Cell;

use super::*;

#[test]
fn probe_budget_is_bounded_per_request() {
    let mut budget = (None, 0);
    for invocation in 0..=MAX_INVOCATIONS {
        assert_eq!(reserve_probe(&mut budget, "first"), invocation);
    }
    assert_eq!(reserve_probe(&mut budget, "second"), 0);
}

#[test]
fn wrong_pass_does_not_reserve_diagnostic() {
    let mut budget = (None, 0);
    let reserve = |pass, budget: &mut (Option<String>, usize)| {
        raw_sun_pass(pass).then(|| reserve_probe(budget, "capture"))
    };
    assert_eq!(reserve(RenderPassId::RP_STARS as u8, &mut budget), None);
    assert_eq!(reserve(RenderPassId::RP_SUN as u8, &mut budget), Some(0));
}

#[test]
fn upload_observer_matches_context_ranges_and_overwrites() {
    let _scope = UploadScope::enter(123);
    let rows = [0.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    unsafe { observe_vertex_upload(124usize as *mut c_void, 1, 3, rows.as_ptr(), 2) };
    assert_eq!(UPLOAD_OBSERVATION.with(|v| v.borrow().unwrap().bits), None);
    unsafe { observe_vertex_upload(123usize as *mut c_void, 1, 3, rows.as_ptr(), 2) };
    assert_eq!(
        UPLOAD_OBSERVATION.with(|v| v.borrow().unwrap().bits),
        Some(1.0f32.to_bits())
    );
    unsafe { observe_vertex_upload(123usize as *mut c_void, 1, 4, rows.as_ptr(), 1) };
    assert_eq!(
        UPLOAD_OBSERVATION.with(|v| v.borrow().unwrap().bits),
        Some(0.0f32.to_bits())
    );
    unsafe { observe_vertex_upload(123usize as *mut c_void, 1, 4, std::ptr::null(), 1) };
    assert_eq!(UPLOAD_OBSERVATION.with(|v| v.borrow().unwrap().bits), None);
    assert_eq!(upload_offset(123, 123, 1, 0, 4), None);
    assert_eq!(upload_offset(123, 123, 0, 4, 1), None);
    assert_eq!(upload_offset(123, 123, 1, 4, u32::MAX), None);
}

#[test]
fn upload_scope_restores_nested_observation() {
    assert!(UPLOAD_OBSERVATION.with(|v| v.borrow().is_none()));
    {
        let _outer = UploadScope::enter(1);
        UPLOAD_OBSERVATION.with(|v| v.borrow_mut().as_mut().unwrap().bits = Some(7));
        {
            let _inner = UploadScope::enter(2);
            assert_eq!(
                UPLOAD_OBSERVATION.with(|v| *v.borrow()),
                Some(UploadObservation {
                    context: 2,
                    bits: None,
                    consumed: None,
                    corrected: false,
                })
            );
        }
        assert_eq!(
            UPLOAD_OBSERVATION.with(|v| *v.borrow()),
            Some(UploadObservation {
                context: 1,
                bits: Some(7),
                consumed: None,
                corrected: false,
            })
        );
    }
    assert!(UPLOAD_OBSERVATION.with(|v| v.borrow().is_none()));
}

#[test]
fn far_depth_requires_finite_nonzero_open_shader() {
    assert!(preserve_far_depth_ready(Some(1.0f32.to_bits()), true));
    for value in [0.0f32, -0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(!preserve_far_depth_ready(Some(value.to_bits()), true));
    }
    assert!(!preserve_far_depth_ready(None, true));
    assert!(!preserve_far_depth_ready(Some(1.0f32.to_bits()), false));
}

#[test]
fn far_depth_only_changes_both_z_rows() {
    let mut original = [Vector4::default(); STEREO_CB_TOTAL_ROWS];
    for (index, row) in original.iter_mut().enumerate() {
        row.data = [index as f32, 2.0, 3.0, 4.0];
    }
    for eye in 0..2 {
        let pinned = pin_rows_to_eye(&original, eye);
        let mut corrected = pinned;
        preserve_reprojection_depth(&mut corrected);
        for index in 0..STEREO_CB_TOTAL_ROWS {
            if index == 12 || index == 16 {
                assert_eq!(corrected[index].data, [0.0, 0.0, 1.0, 0.0]);
            } else {
                assert_eq!(corrected[index].data, pinned[index].data);
            }
        }
        corrected = pinned;
        for index in 0..STEREO_CB_TOTAL_ROWS {
            assert_eq!(corrected[index].data, pinned[index].data);
        }
    }
}

#[test]
fn claims_only_the_sun_pass() {
    assert!(is_sun_pass(RenderPassId::RP_SUN as i16));
    for pass in [
        RenderPassId::RP_STARS,
        RenderPassId::RP_MOON,
        RenderPassId::RP_SKYBOX,
        RenderPassId::RP_SKY_GRADIENT,
    ] {
        assert!(!is_sun_pass(pass as i16));
    }
}

#[test]
fn declines_without_collapse_state() {
    let called = Cell::new(false);
    let result = reissue_sun_pass(std::ptr::null(), &|| {
        called.set(true);
        true
    });
    assert_eq!(result, None);
    assert!(!called.get());
}

#[test]
fn reissue_requires_collapse_and_the_camera_scene() {
    assert!(reissue_ready(true, true, false, true));
    assert!(!reissue_ready(false, true, false, true));
    assert!(!reissue_ready(true, false, false, true));
    assert!(!reissue_ready(true, true, true, true));
    assert!(!reissue_ready(true, true, false, false));
}
