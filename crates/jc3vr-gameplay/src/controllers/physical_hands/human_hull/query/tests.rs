use super::*;

fn output(distance: f32, normal: [f32; 3]) -> hkcdGskGetClosestPointOutput {
    hkcdGskGetClosestPointOutput {
        m_distance: hkVector4f {
            data: [distance; 4],
        },
        m_normalInA: hkVector4f {
            data: [normal[0], normal[1], normal[2], 0.0],
        },
        m_pointAinA: hkVector4f { data: [0.0; 4] },
    }
}

#[test]
fn runtime_call_adapter_preserves_fault_status_and_unwritten_output() {
    let mut input: hkcdGskGetClosestPointInput = unsafe { std::mem::zeroed() };
    input.m_handlePenetration = hkcdGskGetClosestPointInput::HANDLE_PENETRATION;
    let fault = invoke_distance(&input, Mat3::IDENTITY, 0.1, 0.1, |_, _| {
        Err(ClearanceUnavailableReason::QueryIncomplete)
    });
    assert!(matches!(
        fault,
        Err(ClearanceUnavailableReason::QueryIncomplete)
    ));
    assert!(matches!(
        invoke_distance(&input, Mat3::IDENTITY, 0.1, 0.1, |cache, _| {
            assert_eq!(cache.m_vertexIds, [0; 4]);
            assert_eq!(cache.m_dimAb, hkcdGskCache::INITIAL_DIMENSION);
            Ok(0)
        }),
        Err(ClearanceUnavailableReason::QueryRejected)
    ));
    assert!(matches!(
        invoke_distance(&input, Mat3::IDENTITY, 0.1, 0.1, |_, result| {
            *result = output(100.0, [0.0, 1.0, 0.0]);
            Ok(3)
        }),
        Err(ClearanceUnavailableReason::QueryRejected)
    ));
}

#[test]
fn native_status_is_required_even_when_outputs_look_clear() {
    let output = output(100.0, [0.0, 1.0, 0.0]);
    for status in [2, 3, 4, 5, 6, 7, -1, 99] {
        assert!(matches!(
            accepted_output(status, &output, Mat3::IDENTITY, 0.1, 0.1),
            Err(ClearanceUnavailableReason::QueryRejected)
        ));
    }
    for status in [0, 1] {
        assert!(accepted_output(status, &output, Mat3::IDENTITY, 0.1, 0.1).is_ok());
    }
}

#[test]
fn native_output_faults_and_unwritten_outputs_are_not_clear() {
    for distance in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            accepted_output(
                0,
                &output(distance, [0.0, 1.0, 0.0]),
                Mat3::IDENTITY,
                0.1,
                0.1
            )
            .is_err()
        );
    }
    assert!(accepted_output(0, &output(1.0, [0.0; 3]), Mat3::IDENTITY, 0.1, 0.1).is_err());
    assert!(accepted_output(0, &output(1.0, [f32::NAN; 3]), Mat3::IDENTITY, 0.1, 0.1).is_err());
    assert!(accepted_output(0, &output(1.0, [0.0, 1.0, 0.0]), Mat3::IDENTITY, -0.1, 0.1).is_err());
}

#[test]
fn native_core_distance_subtracts_both_radii_once() {
    let contact =
        accepted_output(0, &output(0.5, [0.0, 1.0, 0.0]), Mat3::IDENTITY, 0.2, 0.3).unwrap();
    assert!(contact.gap.abs() < 1.0e-6);
}

#[test]
fn native_normal_is_rotated_to_world_support_direction() {
    let rotation = Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let contact = accepted_output(0, &output(0.5, [1.0, 0.0, 0.0]), rotation, 0.2, 0.3).unwrap();
    assert!(contact.normal.abs_diff_eq(Vec3::Y, 1.0e-5));
}

#[test]
fn unchanged_ground_contact_allows_authored_width_on_actual_floor() {
    for gap in [0.0, -0.001, -0.01] {
        let floor = Contact {
            gap,
            normal: Vec3::Y,
        };
        assert_eq!(compare_contacts(floor, floor), Ok(()));
    }
}

#[test]
fn existing_wall_or_ceiling_contact_never_excuses_expansion() {
    for normal in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y] {
        let contact = Contact { gap: 0.0, normal };
        assert_eq!(
            compare_contacts(contact, contact),
            Err(ClearanceUnavailableReason::Obstructed)
        );
    }
}

#[test]
fn new_floor_contact_deeper_contact_or_changed_normal_blocks() {
    let floor = Contact {
        gap: 0.0,
        normal: Vec3::Y,
    };
    for existing in [
        Contact { gap: 0.02, ..floor },
        Contact {
            gap: -0.005,
            ..floor
        },
        Contact {
            normal: Vec3::X,
            ..floor
        },
    ] {
        assert_eq!(
            compare_contacts(floor, existing),
            Err(ClearanceUnavailableReason::Obstructed)
        );
    }
    assert_eq!(
        compare_contacts(
            Contact {
                gap: -0.03,
                ..floor
            },
            Contact {
                gap: -0.03,
                ..floor
            }
        ),
        Err(ClearanceUnavailableReason::Obstructed)
    );
}

#[test]
fn unknown_composite_query_is_not_an_empty_clear_result() {
    assert!(require_composite_kind(hknpShapeType::COMPRESSED_MESH as u32).is_ok());
    assert!(require_composite_kind(hknpShapeType::COMPRESSED_HEIGHT_FIELD as u32).is_ok());
    assert_eq!(
        require_composite_kind(u32::MAX),
        Err(ClearanceUnavailableReason::UnsupportedShape)
    );
}

#[test]
fn native_key_page_rejects_empty_unsupported_and_invalid_counts() {
    assert_eq!(
        accepted_page(0, KEY_PAGE, true),
        Err(ClearanceUnavailableReason::UnsupportedShape)
    );
    assert_eq!(
        accepted_page(-1, KEY_PAGE, false),
        Err(ClearanceUnavailableReason::QueryIncomplete)
    );
    assert_eq!(
        accepted_page(KEY_PAGE as i32 + 1, KEY_PAGE, false),
        Err(ClearanceUnavailableReason::QueryIncomplete)
    );
    assert_eq!(accepted_page(0, KEY_PAGE, false), Ok(0));
    assert_eq!(accepted_page(KEY_PAGE as i32, KEY_PAGE, true), Ok(KEY_PAGE));
}

#[test]
fn budget_exhaustion_is_explicit() {
    let mut budget = Budget { remaining: 1 };
    assert_eq!(budget.take(), Ok(()));
    assert_eq!(
        budget.take(),
        Err(ClearanceUnavailableReason::BudgetExhausted)
    );
}
