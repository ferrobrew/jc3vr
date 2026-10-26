use super::*;

#[test]
fn identity_hanging_channel_still_has_an_active_runtime_pair() {
    let mut state = Runtime::default();
    let graph = GraphState::Hanging(HangingState::GrappleAttached);
    let tracked = views(Quat::IDENTITY, Vec3::ZERO);
    state
        .update(native(100, graph, Vec3::ZERO), tracked, false)
        .unwrap();
    assert_eq!(
        state
            .update(native(200, graph, Vec3::ZERO), tracked, false)
            .unwrap(),
        Some([Mat4::IDENTITY; 2])
    );
    assert!(state.deltas.is_some());
}

#[test]
fn upside_down_graph_handoff_is_directional_and_does_not_merge_other_hanging_motions() {
    let entry = GraphState::Hanging(HangingState::GrapplePreUpsideDown);
    let idle = GraphState::Hanging(HangingState::GrappleUpsideDown);
    assert!(entry.can_enter());
    assert!(idle.can_enter());
    assert!(entry.continues(idle));
    assert!(!idle.continues(entry));
    for other in [HangingState::GrappleAttached, HangingState::StuntIdle] {
        assert!(!entry.continues(GraphState::Hanging(other)));
        assert!(!GraphState::Hanging(other).continues(idle));
    }
}

#[test]
fn upside_down_camera_handoff_keeps_both_local_components_and_tracking_once() {
    let mut runtime = Runtime::default();
    let tracked = views(Quat::IDENTITY, Vec3::ZERO);
    let entry_local = Trs {
        translation: Vec3::new(0.0, 0.0, -0.6),
        rotation: Quat::from_rotation_x(-1.2),
        ..Trs::IDENTITY
    };
    let idle_local = Trs {
        translation: Vec3::Y * 1.4,
        ..Trs::IDENTITY
    };
    assert!(
        runtime
            .update(sample(1, false, 0.0, entry_local), tracked, false)
            .unwrap()
            .is_none()
    );
    let moving = sample(
        2,
        false,
        0.5,
        Trs {
            translation: Vec3::new(0.0, 1.0, -0.2),
            rotation: Quat::from_rotation_x(-0.2),
            ..Trs::IDENTITY
        },
    );
    assert!(runtime.update(moving, tracked, false).unwrap().is_some());
    let baseline = runtime.baseline;
    let run = runtime.run;
    let head_rotation = Quat::from_rotation_y(0.3);
    let head_translation = Vec3::new(0.1, 0.2, -0.1);
    let tracked = views(head_rotation, head_translation);
    let selected = runtime
        .update(sample(3, true, 0.0, idle_local), tracked, false)
        .unwrap()
        .unwrap();
    let delta = entry_local.matrix().unwrap().inverse() * idle_local.matrix().unwrap();
    let head = Mat4::from_rotation_translation(head_rotation, head_translation);
    assert!(selected[1].abs_diff_eq(delta * head, 0.0001));
    assert_eq!(runtime.baseline, baseline);
    assert_eq!(runtime.run, run);
    assert!(!runtime.outgoing);
    let held = runtime
        .update(sample(4, true, 0.5, idle_local), tracked, false)
        .unwrap()
        .unwrap();
    assert!(held[1].abs_diff_eq(selected[1], 0.0001));
    assert_eq!(runtime.baseline, baseline);
    assert!(
        runtime
            .update(sample(5, false, 0.0, entry_local), tracked, false)
            .unwrap()
            .is_none()
    );
    assert!(runtime.run > run);
    assert_eq!(
        runtime.baseline.unwrap().sample.local.transform,
        entry_local
    );
}

#[test]
fn upside_down_handoff_uses_the_admitted_land_control_until_the_idle_control_contributes() {
    let mut runtime = Runtime::default();
    let tracked = views(Quat::IDENTITY, Vec3::ZERO);
    let now = Instant::now();
    runtime.capture(
        Ok(sample(1, false, 0.0, Trs::IDENTITY)),
        tracked,
        false,
        now,
        0,
    );
    let local = Trs {
        translation: Vec3::X,
        rotation: Quat::from_rotation_x(0.4),
        ..Trs::IDENTITY
    };
    let land = sample(2, false, 0.5, local);
    let outgoing = land.animation.samplers[0];
    assert!(runtime.capture(Ok(land), tracked, false, now, 0).is_some());
    let baseline = runtime.baseline;
    let run = runtime.run;
    for (index, serial) in [3, 3, 4, 4].into_iter().enumerate() {
        let head = views(
            Quat::from_rotation_y(0.1 * (index + 1) as f32),
            Vec3::Y * (index + 1) as f32 * 0.1,
        );
        let previous_pair = runtime.pair;
        let previous_serial = runtime.finalized;
        let selected = runtime
            .capture(Ok(pending(serial, local, outgoing)), head, false, now, 0)
            .unwrap();
        assert!(selected[1].abs_diff_eq(local.matrix().unwrap() * head[1].world, 0.0001));
        assert_eq!(runtime.selected, Some(selected));
        assert!(runtime.pair.is_some());
        if serial == previous_serial {
            assert_eq!(runtime.pair, previous_pair);
        }
        assert_eq!(runtime.baseline, baseline);
        assert_eq!(runtime.run, run);
        assert!(runtime.handback.is_none());
    }
    let head = views(Quat::from_rotation_y(0.6), Vec3::Y * 0.5);
    let selected = runtime
        .capture(Ok(sample(5, true, 0.1, local)), head, false, now, 0)
        .unwrap();
    assert!(selected[1].abs_diff_eq(local.matrix().unwrap() * head[1].world, 0.0001));
    assert_eq!(runtime.baseline, baseline);
    assert_eq!(runtime.run, run);
    assert_eq!(runtime.serial, 5);
    assert_eq!(
        runtime.graph_state,
        Some(GraphState::Hanging(HangingState::GrappleUpsideDown))
    );
}

#[test]
fn pending_handoff_rejects_changed_native_data_at_the_same_finalized_serial() {
    for change in 0..7 {
        let mut runtime = Runtime::default();
        let tracked = views(Quat::IDENTITY, Vec3::ZERO);
        let now = Instant::now();
        runtime.capture(
            Ok(sample(1, false, 0.0, Trs::IDENTITY)),
            tracked,
            false,
            now,
            0,
        );
        let local = Trs {
            translation: Vec3::X,
            ..Trs::IDENTITY
        };
        let land = sample(2, false, 0.5, local);
        let outgoing = land.animation.samplers[0];
        runtime.capture(Ok(land), tracked, false, now, 0);
        assert!(
            runtime
                .capture(Ok(pending(3, local, outgoing)), tracked, false, now, 0)
                .is_some()
        );
        let mut changed = pending(3, local, outgoing);
        match change {
            0 => changed.state = Ok(GraphState::Hanging(HangingState::GrapplePreUpsideDown)),
            1 => changed.animation.state = Some(2),
            2 => {
                changed.animation.playing[0].time = 0.1;
                changed.animation.samplers[0].time = 0.1;
            }
            3 => changed.animation.samplers[1].weight = 0.5,
            4 => changed.pose_buffer += 1,
            5 => changed.local.translation = Vec3::Y,
            _ => changed.root.rotation = Quat::from_rotation_x(0.1),
        }
        assert!(
            runtime
                .capture(Ok(changed), tracked, false, now, 0)
                .is_none()
        );
        assert!(runtime.baseline.is_none());
        assert!(runtime.pair.is_none());
        assert!(runtime.selected.is_none());
        assert!(runtime.observed.is_none());
        assert!(runtime.handback.is_none());
    }
}

#[test]
fn upside_down_states_require_a_contributing_camera_track_before_entry_and_while_live() {
    for idle in [false, true] {
        for control_weight in [false, true] {
            let mut runtime = Runtime::default();
            let mut missing = sample(1, idle, 0.0, Trs::IDENTITY);
            for clip in missing
                .animation
                .playing
                .iter_mut()
                .chain(missing.animation.samplers.iter_mut())
            {
                if control_weight {
                    clip.weight = 0.0;
                } else {
                    clip.camera_track_weight = 0.0;
                }
            }
            assert_eq!(
                runtime.update(missing, views(Quat::IDENTITY, Vec3::ZERO), false),
                Err(Refusal::MissingChannel)
            );
            assert!(runtime.baseline.is_none());
            let now = Instant::now();
            let tracked = views(Quat::IDENTITY, Vec3::ZERO);
            runtime.capture(
                Ok(sample(2, idle, 0.0, Trs::IDENTITY)),
                tracked,
                false,
                now,
                0,
            );
            assert!(
                runtime
                    .capture(
                        Ok(sample(
                            3,
                            idle,
                            0.5,
                            Trs {
                                translation: Vec3::X,
                                ..Trs::IDENTITY
                            },
                        )),
                        tracked,
                        false,
                        now,
                        0,
                    )
                    .is_some()
            );
            let mut lost = sample(4, idle, 0.6, Trs::IDENTITY);
            lost.animation.playing[0].camera_track_weight = 0.0;
            lost.animation.samplers[0].camera_track_weight = 0.0;
            runtime.capture(Ok(lost), tracked, false, now, 0);
            assert!(runtime.baseline.is_none());
            assert!(runtime.pair.is_none());
        }
    }
}

#[test]
fn upside_down_handoff_rejects_reused_finalization_and_changed_source_continuity() {
    for change in 0..5 {
        let mut runtime = Runtime::default();
        let tracked = views(Quat::IDENTITY, Vec3::ZERO);
        let now = Instant::now();
        runtime.capture(
            Ok(sample(1, false, 0.0, Trs::IDENTITY)),
            tracked,
            false,
            now,
            0,
        );
        runtime.capture(
            Ok(sample(2, false, 0.5, Trs::IDENTITY)),
            tracked,
            false,
            now,
            0,
        );
        let mut next = sample(3, true, 0.0, Trs::IDENTITY);
        let epoch = match change {
            0 => {
                next.finalized = 2;
                0
            }
            1 => {
                next.character += 1;
                0
            }
            2 => {
                next.source.producer_owner += 1;
                0
            }
            3 => {
                next.source.camera_bone += 1;
                0
            }
            _ => 1,
        };
        assert!(
            runtime
                .capture(Ok(next), tracked, false, now, epoch)
                .is_none()
        );
        assert!(runtime.pair.is_none());
        assert!(runtime.baseline.is_none());
        assert!(runtime.handback.is_none());
    }
}

fn views(rotation: Quat, translation: Vec3) -> [TrackedView; 2] {
    [TrackedView {
        world: Mat4::from_rotation_translation(rotation, translation),
        tracking_translation: translation,
    }; 2]
}

fn pending(serial: u64, local: Trs, outgoing: Clip) -> NativeSample {
    let mut value = sample(serial, true, 0.0, local);
    value.animation.playing[0].weight = 0.0;
    value.animation.samplers[0].weight = 0.0;
    value.animation.samplers.push(outgoing);
    value
}

fn sample(serial: u64, idle: bool, time: f32, local: Trs) -> NativeSample {
    let graph = GraphState::Hanging(if idle {
        HangingState::GrappleUpsideDown
    } else {
        HangingState::GrapplePreUpsideDown
    });
    let mut value = native(100, graph, local.translation);
    value.local = local;
    value.finalized = serial;
    for clip in value
        .animation
        .playing
        .iter_mut()
        .chain(value.animation.samplers.iter_mut())
    {
        clip.time = time;
        if idle {
            clip.source.instance += 1;
            clip.source.owner += 1;
            clip.source.animation += 1;
            clip.source.binding += 1;
            clip.looping = true;
        }
    }
    value
}
