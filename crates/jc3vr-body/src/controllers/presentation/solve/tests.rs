use super::*;

fn arm(side: f32) -> ArmJoints {
    ArmJoints {
        clavicle: Vec3::new(0.08 * side, 1.45, 0.0),
        shoulder: Vec3::new(0.18 * side, 1.45, 0.0),
        elbow: Vec3::new(0.22 * side, 1.18, 0.05),
        wrist: Vec3::new(0.22 * side, 1.0, -0.15),
        hand: Quat::IDENTITY,
        frame: Quat::IDENTITY,
    }
}

fn body() -> UpperBody {
    UpperBody {
        spine: [
            Vec3::new(0.0, 1.05, 0.0),
            Vec3::new(0.0, 1.17, 0.0),
            Vec3::new(0.0, 1.3, 0.01),
            Vec3::new(0.0, 1.5, 0.0),
        ],
        head: Vec3::new(0.0, 1.6, -0.01),
        arms: [arm(-1.0), arm(1.0)],
    }
}

fn still() -> Motion {
    Motion {
        head: None,
        torso: 1.0,
        hands: [None; 2],
        targets: [None; 2],
        shoulders: [None; 2],
        arms: true,
        stretch_forearm: true,
        elbow: None,
    }
}

fn rigid(m: &Mat4) -> bool {
    let (scale, _, _) = m.to_scale_rotation_translation();
    scale.abs_diff_eq(Vec3::ONE, 1e-4)
}

#[test]
fn with_nothing_moved_every_edit_is_the_identity() {
    for edit in solve(&body(), &still()).edits {
        assert!(edit.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{edit:?}");
    }
    let unmoved_head = Motion {
        head: Some(Mat4::IDENTITY),
        ..still()
    };
    for edit in solve(&body(), &unmoved_head).edits {
        assert!(edit.abs_diff_eq(Mat4::IDENTITY, 1e-5), "{edit:?}");
    }
}

#[test]
fn the_chain_swings_its_end_onto_the_target_and_keeps_its_base_and_lengths() {
    let b = body();
    let target = b.head + Vec3::new(0.0, 0.0, -0.04);
    let edits = chain_swing(b.spine, b.head, target, SPINE_SHARES);
    let base = b.spine[0];
    assert!(edits[0].transform_point3(base).abs_diff_eq(base, 1e-6));
    let end = edits[3].transform_point3(b.head);
    let along = (target - base).normalize();
    assert!((end - base).normalize().abs_diff_eq(along, 1e-4), "{end:?}");
    // Every bone keeps its length: each joint carried by the edit below it, and the end.
    let mut joints: Vec<Vec3> = (0..4)
        .map(|i| {
            if i == 0 {
                base
            } else {
                edits[i - 1].transform_point3(b.spine[i])
            }
        })
        .collect();
    joints.push(end);
    let mut original = b.spine.to_vec();
    original.push(b.head);
    for i in 0..4 {
        let before = original[i].distance(original[i + 1]);
        let after = joints[i].distance(joints[i + 1]);
        assert!((before - after).abs() < 1e-5);
    }
    // Each joint turns, the higher ones by more of the swing.
    let angle = |m: &Mat4| rotation_of(m).angle_between(Quat::IDENTITY);
    assert!(angle(&edits[0]) > 0.0);
    assert!(angle(&edits[3]) > angle(&edits[0]));
}

#[test]
fn the_head_moves_with_the_spine_and_turns_by_the_head_s_turn() {
    let b = body();
    let turn = Quat::from_rotation_y(0.3);
    let pivot = b.head + Vec3::new(0.0, 0.02, 0.03);
    let head = head_motion(
        pivot,
        Quat::IDENTITY,
        pivot + Vec3::new(0.02, 0.0, -0.03),
        turn,
    );
    let motion = Motion {
        head: Some(head),
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    let wanted = head.transform_point3(b.head);
    let drawn = edits[Group::Head.index()].transform_point3(b.head);
    assert!(drawn.abs_diff_eq(wanted, 1e-4), "{drawn:?} {wanted:?}");
    let rotation = rotation_of(&edits[Group::Head.index()]);
    assert!(rotation.angle_between(turn) < 1e-4);
    // The spine bent toward it: the neck moved, and the base did not.
    let neck = edits[Group::Sternum.index()].transform_point3(b.spine[3]);
    assert!(neck.distance(b.spine[3]) > 1e-3);
    assert!(
        edits[Group::Spine1.index()]
            .transform_point3(b.spine[0])
            .abs_diff_eq(b.spine[0], 1e-6)
    );
    for edit in &edits {
        assert!(rigid(edit));
    }
}

#[test]
fn without_the_torso_the_spine_stays_and_the_head_s_travel_is_capped() {
    let b = body();
    let head = Mat4::from_translation(Vec3::new(0.0, 0.0, -0.2));
    let motion = Motion {
        head: Some(head),
        torso: 0.0,
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    for group in Group::SPINE {
        assert!(edits[group.index()].abs_diff_eq(Mat4::IDENTITY, 1e-6));
    }
    let moved = edits[Group::Head.index()].transform_point3(b.head) - b.head;
    assert!((moved.length() - MAX_HEAD_RESIDUAL_M).abs() < 1e-5);
}

#[test]
fn the_two_bone_elbow_keeps_the_lengths_and_reproduces_the_drawn_arm() {
    let a = arm(1.0);
    let upper = a.shoulder.distance(a.elbow);
    let fore = a.elbow.distance(a.wrist);
    let elbow = two_bone_elbow(a.shoulder, a.wrist, upper, fore, a.elbow - a.shoulder).unwrap();
    assert!(elbow.abs_diff_eq(a.elbow, 1e-5), "{elbow:?}");
    let wrist = a.wrist + Vec3::new(0.05, 0.08, -0.03);
    let elbow = two_bone_elbow(a.shoulder, wrist, upper, fore, a.elbow - a.shoulder).unwrap();
    assert!((elbow.distance(a.shoulder) - upper).abs() < 1e-5);
    assert!((elbow.distance(wrist) - fore).abs() < 1e-4);
    // Out of reach, the arm straightens toward the wrist.
    let far = a.shoulder + Vec3::new(0.0, 0.0, -2.0);
    let elbow = two_bone_elbow(a.shoulder, far, upper, fore, a.elbow - a.shoulder).unwrap();
    let toward = (far - a.shoulder).normalize();
    assert!((elbow - a.shoulder).normalize().dot(toward) > 0.999);
    // A pole along the reach decides nothing.
    assert!(two_bone_elbow(a.shoulder, wrist, upper, fore, wrist - a.shoulder).is_none());
}

#[test]
fn a_moved_hand_is_met_by_a_resolved_arm_of_the_same_lengths() {
    let b = body();
    let hand =
        Mat4::from_rotation_translation(Quat::from_rotation_x(0.2), Vec3::new(0.03, 0.06, -0.04));
    let motion = Motion {
        hands: [None, Some(hand)],
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    let a = &b.arms[1];
    let [upper, fore, palm] = Group::arm(1).map(|g| edits[g.index()]);
    assert_eq!(palm, hand);
    let shoulder = upper.transform_point3(a.shoulder);
    let elbow = upper.transform_point3(a.elbow);
    let forearm_elbow = fore.transform_point3(a.elbow);
    let forearm_wrist = fore.transform_point3(a.wrist);
    let wrist = hand.transform_point3(a.wrist);
    assert!(shoulder.abs_diff_eq(a.shoulder, 1e-5));
    assert!(elbow.abs_diff_eq(forearm_elbow, 1e-4));
    assert!(
        forearm_wrist.abs_diff_eq(wrist, 1e-4),
        "{forearm_wrist:?} {wrist:?}"
    );
    assert!((elbow.distance(shoulder) - a.shoulder.distance(a.elbow)).abs() < 1e-5);
    // The other arm, its hand unmoved, is left alone.
    for group in Group::arm(0) {
        assert!(edits[group.index()].abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
}

#[test]
fn a_hand_past_the_arm_s_reach_falls_short_on_the_end_of_the_forearm() {
    let b = body();
    let a = &b.arms[1];
    let (upper, fore) = (a.shoulder.distance(a.elbow), a.elbow.distance(a.wrist));
    // The hand thrown far beyond any reach, as a body jumping against the hands in one tick
    // leaves it.
    let hand = Mat4::from_translation(Vec3::new(0.2, -0.3, -0.6));
    let motion = Motion {
        hands: [None, Some(hand)],
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    let [up, fo, palm] = Group::arm(1).map(|g| edits[g.index()]);
    let shoulder = up.transform_point3(a.shoulder);
    let wrist = palm.transform_point3(a.wrist);
    let forearm_wrist = fo.transform_point3(a.wrist);
    assert!(
        forearm_wrist.abs_diff_eq(wrist, 1e-3),
        "{forearm_wrist:?} {wrist:?}"
    );
    let stretch = HANDS.get().reach.max_forearm_stretch;
    assert!(shoulder.distance(wrist) <= upper + fore * stretch + 1e-4);
}

#[test]
fn a_hand_held_still_while_the_torso_bends_keeps_its_place_and_its_arm_s_lengths() {
    let b = body();
    let motion = Motion {
        head: Some(Mat4::from_translation(Vec3::new(0.0, 0.0, -0.04))),
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    let a = &b.arms[0];
    let [upper, fore, palm] = Group::arm(0).map(|g| edits[g.index()]);
    assert_eq!(palm, Mat4::IDENTITY);
    let shoulder = upper.transform_point3(a.shoulder);
    assert!(
        shoulder.distance(a.shoulder) > 1e-3,
        "the shoulder rides the torso"
    );
    assert!(
        edits[Group::Sternum.index()]
            .transform_point3(a.shoulder)
            .abs_diff_eq(shoulder, 1e-5)
    );
    let elbow = upper.transform_point3(a.elbow);
    assert!((elbow.distance(shoulder) - a.shoulder.distance(a.elbow)).abs() < 1e-5);
    assert!(fore.transform_point3(a.wrist).abs_diff_eq(a.wrist, 1e-4));
}

#[test]
fn without_the_arm_solve_the_forearm_swings_about_the_carried_elbow() {
    let b = body();
    let hand = Mat4::from_translation(Vec3::new(0.0, 0.03, 0.0));
    let motion = Motion {
        hands: [Some(hand), None],
        arms: false,
        stretch_forearm: false,
        ..still()
    };
    let edits = solve(&b, &motion).edits;
    let a = &b.arms[0];
    assert!(edits[Group::LeftArm.index()].abs_diff_eq(Mat4::IDENTITY, 1e-6));
    let fore = edits[Group::LeftForeArm.index()];
    assert!(fore.transform_point3(a.elbow).abs_diff_eq(a.elbow, 1e-6));
    let swung = fore.transform_point3(a.wrist) - a.elbow;
    let toward = (hand.transform_point3(a.wrist) - a.elbow).normalize();
    assert!(swung.normalize().abs_diff_eq(toward, 1e-5));
}

fn driven(weight: f32) -> Motion {
    Motion {
        elbow: Some(ElbowDrive {
            up: Vec3::Y,
            palm_weight: elbow::PALM_WEIGHT,
            smoothing_s: elbow::SMOOTHING_S,
            dt: 1.0 / 90.0,
            arms: [
                ArmDrive {
                    weight,
                    previous: None,
                },
                ArmDrive::default(),
            ],
        }),
        ..still()
    }
}

/// The left arm's elbow, shoulder, and wrist as `edits` draw them.
fn left_arm(b: &UpperBody, edits: &Edits) -> (Vec3, Vec3, Vec3) {
    let a = &b.arms[0];
    let [upper, _, palm] = Group::arm(0).map(|g| edits[g.index()]);
    (
        upper.transform_point3(a.elbow),
        upper.transform_point3(a.shoulder),
        palm.transform_point3(a.wrist),
    )
}

#[test]
fn a_driven_arm_bends_toward_the_pole_and_an_undriven_one_is_left_alone() {
    let b = body();
    let solved = solve(&b, &driven(1.0));
    let (elbow, shoulder, wrist) = left_arm(&b, &solved.edits);
    let a = &b.arms[0];
    assert!((elbow.distance(shoulder) - a.shoulder.distance(a.elbow)).abs() < 1e-5);
    assert!(wrist.abs_diff_eq(a.wrist, 1e-6), "the hand stays");
    let reported = solved.elbows[0].expect("the left arm is driven");
    let axis = (wrist - shoulder).normalize();
    let bent = (elbow - shoulder) - axis * (elbow - shoulder).dot(axis);
    assert!(bent.normalize().abs_diff_eq(reported.pole, 1e-4));
    assert_eq!(reported.weight, 1.0);
    assert!(reported.swivel_deg.is_some());
    // The undriven right arm keeps its drawn elbow.
    assert!(solved.elbows[1].is_none());
    for group in Group::arm(1) {
        assert!(solved.edits[group.index()].abs_diff_eq(Mat4::IDENTITY, 1e-5));
    }
}

#[test]
fn the_pole_blends_in_with_the_arm_s_weight() {
    let b = body();
    let angle = |weight: f32| {
        let solved = solve(&b, &driven(weight));
        let (elbow, _, _) = left_arm(&b, &solved.edits);
        elbow.distance(b.arms[0].elbow)
    };
    let (none, half, full) = (angle(0.0), angle(0.5), angle(1.0));
    assert!(none < 1e-5, "{none}");
    assert!(half > 1e-3 && half < full, "{half} {full}");
    // A weight of none reports no pole, so the smoothing starts again.
    assert!(solve(&b, &driven(0.0)).elbows[0].is_none());
}

#[test]
fn the_smoothed_pole_carries_from_the_last_frame() {
    let b = body();
    let target = solve(&b, &driven(1.0)).elbows[0].unwrap().pole;
    let axis = (b.arms[0].wrist - b.arms[0].shoulder).normalize();
    let previous = Quat::from_axis_angle(axis, 1.0) * target;
    let mut motion = driven(1.0);
    motion.elbow.as_mut().unwrap().arms[0].previous = Some(previous);
    let smoothed = solve(&b, &motion).elbows[0].unwrap().pole;
    let expected = elbow::smooth(previous, target, axis, 1.0 / 90.0, elbow::SMOOTHING_S);
    assert!(
        smoothed.abs_diff_eq(expected, 1e-4),
        "{smoothed:?} {expected:?}"
    );
    assert!(smoothed.angle_between(target) > 0.5);
}

#[test]
fn a_nearly_straight_source_arm_does_not_flip_when_the_hand_bends_it() {
    for side in 0..2 {
        for weight in [0.0, 0.2, 1.0] {
            let mut previous: Option<Vec3> = None;
            for step in -20..=20 {
                let mut b = body();
                let shoulder = b.arms[side].shoulder;
                let offset = step as f32 * 0.0001;
                b.arms[side].elbow = shoulder + Vec3::new(offset, 0.0, -0.3);
                b.arms[side].wrist = shoulder + Vec3::new(0.0, 0.0, -0.6);
                let a = b.arms[side];
                let mut motion = driven(weight);
                motion.elbow.as_mut().unwrap().arms[side].weight = weight;
                motion.hands[side] =
                    Some(Mat4::from_translation(Vec3::new(offset * 2.0, 0.0, 0.15)));
                let solved = solve(&b, &motion);
                let [upper, fore, hand] = Group::arm(side).map(|g| solved.edits[g.index()]);
                let elbow = upper.transform_point3(a.elbow);
                let wrist = hand.transform_point3(a.wrist);
                assert!(elbow.is_finite());
                assert!(upper.transform_point3(shoulder).abs_diff_eq(shoulder, 1e-5));
                assert!(fore.transform_point3(a.elbow).abs_diff_eq(elbow, 1e-4));
                assert!(fore.transform_point3(a.wrist).abs_diff_eq(wrist, 1e-4));
                assert!((elbow.distance(shoulder) - a.elbow.distance(shoulder)).abs() < 1e-5);
                assert!((elbow.distance(wrist) - a.elbow.distance(a.wrist)).abs() < 1e-4);
                if let Some(previous) = previous {
                    assert!(elbow.distance(previous) < 0.001, "{side} {weight} {step}");
                }
                previous = Some(elbow);
            }
        }
    }
}

#[test]
fn the_drawn_pole_fades_in_smoothly_with_a_resolved_source_bend() {
    let mut previous = Vec3::X;
    for step in 0..=250 {
        let bend = step as f32 * 0.001;
        let upper = Vec3::new(0.0, bend, (1.0 - bend * bend).sqrt()) * 0.3;
        let drawn = drawn_pole(upper, Vec3::Z * 0.6, Vec3::Z * 0.4);
        let pole = blend_drawn_pole(drawn, Vec3::X, Vec3::Z, 1.0);
        assert!((pole.length() - 1.0).abs() < 1e-5);
        assert!(pole.z.abs() < 1e-5);
        assert!(pole.distance(previous) < 0.02);
        if bend <= MIN_DRAWN_BEND_SIN {
            assert!(pole.abs_diff_eq(Vec3::X, 1e-5));
        } else if bend >= FULL_DRAWN_BEND_SIN {
            assert!(pole.abs_diff_eq(Vec3::Y, 1e-5));
        }
        previous = pole;
    }
}

#[test]
fn the_drawn_bend_is_carried_to_the_target_axis_without_reprojecting_the_upper_arm() {
    let upper = Vec3::new(0.1, 0.0, 0.28);
    let target_axis = Vec3::new(0.8, 0.0, 0.6);
    let drawn = drawn_pole(upper, Vec3::Z * 0.5, target_axis * 0.4).unwrap();
    let expected = Quat::from_rotation_arc(Vec3::Z, target_axis) * Vec3::X;
    assert_eq!(drawn.confidence, 1.0);
    assert!(drawn.direction.abs_diff_eq(expected, 1e-5));
    assert!(drawn.direction.dot(target_axis).abs() < 1e-5);
}

#[test]
fn explicit_flight_targets_close_the_whole_arm_without_a_late_delta() {
    for side in 0..2 {
        for stretch in [false, true] {
            let b = body();
            let a = b.arms[side];
            let target = a.shoulder + Vec3::new(if side == 0 { -0.1 } else { 0.1 }, 0.1, -0.35);
            let mut motion = still();
            motion.stretch_forearm = stretch;
            motion.targets[side] = Some((target, Quat::IDENTITY, 1.0));
            let solved = solve(&b, &motion);
            let [upper, fore, hand] = Group::arm(side).map(|g| solved.edits[g.index()]);
            assert!(hand.transform_point3(a.wrist).abs_diff_eq(target, 1e-5));
            assert!(fore.transform_point3(a.wrist).abs_diff_eq(target, 1e-4));
            assert!(
                upper
                    .transform_point3(a.elbow)
                    .abs_diff_eq(fore.transform_point3(a.elbow), 1e-4)
            );
            assert!(
                (upper
                    .transform_point3(a.elbow)
                    .distance(upper.transform_point3(a.shoulder))
                    - a.elbow.distance(a.shoulder))
                .abs()
                    < 1e-5
            );
        }
    }
}

#[test]
fn canonical_roots_rebase_a_crouched_arm_without_changing_its_lengths() {
    let b = body();
    let a = b.arms[1];
    let shift = Vec3::new(0.04, 0.3, -0.05);
    let mut motion = still();
    motion.shoulders[1] = Some(a.shoulder + shift);
    motion.targets[1] = Some((a.wrist + shift, Quat::IDENTITY, 1.0));
    let edits = solve(&b, &motion).edits;
    let [upper, fore, palm] = Group::arm(1).map(|g| edits[g.index()]);
    assert!(
        upper
            .transform_point3(a.shoulder)
            .abs_diff_eq(a.shoulder + shift, 1e-5)
    );
    assert!(
        upper
            .transform_point3(a.elbow)
            .abs_diff_eq(fore.transform_point3(a.elbow), 1e-4)
    );
    assert!(
        fore.transform_point3(a.wrist)
            .abs_diff_eq(a.wrist + shift, 1e-4)
    );
    assert!(
        palm.transform_point3(a.wrist)
            .abs_diff_eq(a.wrist + shift, 1e-4)
    );
    assert!(rigid(&upper) && rigid(&fore) && rigid(&palm));
}

#[test]
fn an_explicit_target_closes_the_wrist_frame_not_the_hand_bone_frame() {
    let mut b = body();
    b.arms[1].hand = Quat::from_rotation_x(0.7);
    b.arms[1].frame = Quat::from_rotation_y(-0.4);
    let target = Quat::from_rotation_z(0.9);
    let mut motion = still();
    motion.targets[1] = Some((b.arms[1].wrist, target, 1.0));
    let solved = solve(&b, &motion);
    let turn = rotation_of(&solved.edits[Group::RightHand.index()]);
    assert!((turn * b.arms[1].frame).angle_between(target) < 1e-3);
    assert!((turn * b.arms[1].hand).angle_between(target) > 0.1);
}

#[test]
fn a_clavicle_yields_for_a_pinned_handle_without_breaking_either_joint() {
    let b = body();
    let a = b.arms[1];
    let length = a.shoulder.distance(a.elbow) + a.elbow.distance(a.wrist);
    let desired_shoulder = a.clavicle + Quat::from_rotation_z(0.45) * (a.shoulder - a.clavicle);
    let target = desired_shoulder + Vec3::Y * (length - 0.001);
    let mut motion = still();
    motion.stretch_forearm = false;
    motion.targets[1] = Some((target, Quat::IDENTITY, 1.0));
    let edits = solve(&b, &motion).edits;
    let clavicle = edits[Group::RightShoulder.index()];
    let [upper, fore, hand] = Group::arm(1).map(|g| edits[g.index()]);
    assert!(
        clavicle
            .transform_point3(a.clavicle)
            .abs_diff_eq(a.clavicle, 1e-5)
    );
    assert!(clavicle.transform_point3(a.shoulder).distance(a.shoulder) > 0.01);
    assert!(
        clavicle
            .transform_point3(a.shoulder)
            .abs_diff_eq(upper.transform_point3(a.shoulder), 1e-5)
    );
    assert!(
        upper
            .transform_point3(a.elbow)
            .abs_diff_eq(fore.transform_point3(a.elbow), 1e-4)
    );
    assert!(fore.transform_point3(a.wrist).abs_diff_eq(target, 1e-3));
    assert!(hand.transform_point3(a.wrist).abs_diff_eq(target, 1e-4));
    assert!(rigid(&clavicle) && rigid(&upper) && rigid(&fore));
}

#[test]
fn palm_roll_does_not_reverse_a_spread_nearly_extended_arm() {
    for side in 0..2 {
        let b = body();
        let a = b.arms[side];
        let length = a.shoulder.distance(a.elbow) + a.elbow.distance(a.wrist);
        let axis = if side == 0 { Vec3::NEG_X } else { Vec3::X };
        let target = a.shoulder + axis * (length * 0.999);
        let mut previous = None::<Vec3>;
        for step in 0..=120 {
            let mut motion = driven(1.0);
            motion.elbow.as_mut().unwrap().arms[side].weight = 1.0;
            motion.targets[side] =
                Some((target, Quat::from_axis_angle(axis, step as f32 * 0.1), 1.0));
            let solved = solve(&b, &motion);
            let elbow = solved.edits[Group::arm(side)[0].index()].transform_point3(a.elbow);
            if let Some(previous) = previous {
                assert!(elbow.distance(previous) < 1e-4);
            }
            previous = Some(elbow);
        }
    }
}

#[test]
fn the_interpolated_body_blends_every_joint() {
    let a = body();
    let mut b = body();
    b.head += Vec3::Y * 0.1;
    b.arms[1].wrist += Vec3::X * 0.2;
    let half = a.lerp(&b, 0.5);
    assert!((half.head.y - (a.head.y + 0.05)).abs() < 1e-6);
    assert!((half.arms[1].wrist.x - (a.arms[1].wrist.x + 0.1)).abs() < 1e-6);
    assert!(half.is_finite());
}
