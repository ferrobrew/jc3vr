use super::*;
use crate::hooks::camera::authored::{
    Status,
    composition::{InvalidPair, Pair, compose_view},
    status,
};

#[test]
fn production_is_explicitly_unavailable() {
    assert_eq!(status(), Status::Unavailable);
}

#[test]
fn root_local_inverse_cancels_without_erasing_the_local_delta() {
    let root = trs(
        Vec3::new(2.0, -1.0, 3.0),
        Quat::from_rotation_y(0.7),
        Vec3::ONE,
    );
    let (scale, rotation, translation) = root
        .matrix()
        .unwrap()
        .inverse()
        .to_scale_rotation_translation();
    let local = trs(translation, rotation, scale);
    let pair = Pair::new(baseline().sample, sample(2, root, local), baseline()).unwrap();
    let compensated = pair
        .deltas(
            DeltaRequest::RootCompensatedRelativeToBaseline,
            RootTreatment::ComposeWithAuthoredLocal,
        )
        .unwrap();
    assert_matrix(compensated[1], Mat4::IDENTITY);
    let authored = pair
        .deltas(
            DeltaRequest::AuthoredLocalRelativeToBaseline,
            RootTreatment::SeparateFromAuthoredLocal,
        )
        .unwrap();
    assert_matrix(authored[1], local.matrix().unwrap());
    assert!(authored[1].w_axis.truncate().length() > 1.0);
}

#[test]
fn noncommuting_full_trs_baseline_and_head_compose_in_explicit_order() {
    let origin = Baseline {
        run: 30,
        sample: sample(
            1,
            trs(Vec3::X, Quat::from_rotation_y(0.4), Vec3::splat(1.2)),
            trs(
                Vec3::Y,
                Quat::from_rotation_x(0.3),
                Vec3::new(0.9, 1.1, 1.0),
            ),
        ),
    };
    let current = sample(
        2,
        trs(Vec3::Z, Quat::from_rotation_z(-0.6), Vec3::splat(0.8)),
        trs(
            Vec3::X * 0.2,
            Quat::from_rotation_y(0.9),
            Vec3::new(1.0, 0.8, 1.1),
        ),
    );
    let pair = Pair::new(origin.sample, current, origin).unwrap();
    let delta = pair
        .deltas(
            DeltaRequest::RootCompensatedRelativeToBaseline,
            RootTreatment::ComposeWithAuthoredLocal,
        )
        .unwrap()[1];
    let expected = (origin.sample.root.transform.matrix().unwrap()
        * origin.sample.local.transform.matrix().unwrap())
    .inverse()
        * current.root.transform.matrix().unwrap()
        * current.local.transform.matrix().unwrap();
    assert_matrix(delta, expected);
    let base = trs(Vec3::Y * 2.0, Quat::from_rotation_x(-0.2), Vec3::ONE);
    let head = trs(Vec3::Z * 0.1, Quat::from_rotation_z(0.5), Vec3::ONE);
    let view = compose_view(base, delta, head).unwrap();
    assert_matrix(
        view,
        base.matrix().unwrap() * expected * head.matrix().unwrap(),
    );
    let doubled = view * head.matrix().unwrap();
    assert!((view.x_axis - doubled.x_axis).length() > 0.1);
    let reversed = head.matrix().unwrap() * expected * base.matrix().unwrap();
    assert!((view.w_axis - reversed.w_axis).length() > 0.1);
}

#[test]
fn unknown_or_incompatible_root_semantics_cannot_select_a_delta() {
    let pair = Pair::new(
        baseline().sample,
        sample(2, Trs::IDENTITY, Trs::IDENTITY),
        baseline(),
    )
    .unwrap();
    for request in [
        DeltaRequest::AuthoredLocalRelativeToBaseline,
        DeltaRequest::RootCompensatedRelativeToBaseline,
    ] {
        assert_eq!(
            pair.deltas(request, RootTreatment::Unknown),
            Err(InvalidPair::RootTreatmentUnknown)
        );
    }
    assert_eq!(
        pair.deltas(
            DeltaRequest::RootCompensatedRelativeToBaseline,
            RootTreatment::SeparateFromAuthoredLocal,
        ),
        Err(InvalidPair::RootTreatmentMismatch),
    );
}

#[test]
fn one_current_local_pose_with_two_roots_is_not_a_previous_current_pair() {
    let current = sample(2, Trs::IDENTITY, Trs::IDENTITY);
    let mut pseudo_previous = current;
    pseudo_previous.root.stamp.finalized = 1;
    assert_eq!(
        Pair::new(pseudo_previous, current, baseline()),
        Err(InvalidPair::MixedRootLocalTime),
    );
    assert_eq!(
        Pair::new(current, current, baseline()),
        Err(InvalidPair::RepeatedOrReversedSample),
    );
}

#[test]
fn identity_and_epoch_changes_invalidate_a_pair() {
    for character_change in [true, false] {
        let mut current = sample(2, Trs::IDENTITY, Trs::IDENTITY);
        if character_change {
            current.root.stamp.character += 1;
        } else {
            current.root.stamp.continuity += 1;
        }
        current.local.stamp = current.root.stamp;
        assert_eq!(
            Pair::new(baseline().sample, current, baseline()),
            Err(if character_change {
                InvalidPair::CharacterChanged
            } else {
                InvalidPair::ContinuityChanged
            }),
        );
    }
}

#[test]
fn malformed_full_transforms_are_rejected() {
    for invalid in [
        trs(Vec3::splat(f32::NAN), Quat::IDENTITY, Vec3::ONE),
        trs(Vec3::ZERO, Quat::from_xyzw(0.0, 0.0, 0.0, 0.0), Vec3::ONE),
        trs(Vec3::ZERO, Quat::IDENTITY, Vec3::ZERO),
    ] {
        assert_eq!(
            Pair::new(
                baseline().sample,
                sample(2, invalid, Trs::IDENTITY),
                baseline()
            ),
            Err(InvalidPair::InvalidTransform),
        );
    }
}
