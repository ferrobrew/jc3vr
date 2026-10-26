use super::*;

fn views() -> [TrackedView; 2] {
    [TrackedView {
        world: Mat4::IDENTITY,
        tracking_translation: Vec3::ZERO,
    }; 2]
}

fn timed(serial: u64, graph: GraphState, time: f32, position: Vec3) -> NativeSample {
    let mut value = native(100, graph, position);
    value.finalized = serial;
    value.animation.playing[0].time = time;
    value.animation.samplers[0].time = time;
    value
}

fn all_cases() -> [GraphState; 5] {
    [
        GraphState::GroundInto,
        GraphState::Landing(LandingState::Hard),
        GraphState::Recovery(RecoveryState::GetUp),
        GraphState::FloorPickup,
        GraphState::Hanging(HangingState::GrappleAttached),
    ]
}

#[test]
fn every_case_rebases_same_state_restart_on_the_same_pose_buffer() {
    for graph in all_cases() {
        let mut runtime = Runtime::default();
        for run in 0..3 {
            let serial = run * 3 + 1;
            assert!(
                runtime
                    .update(
                        timed(serial, graph, 0.0, Vec3::Y * run as f32),
                        views(),
                        false
                    )
                    .unwrap()
                    .is_none()
            );
            assert!(
                runtime
                    .update(timed(serial + 1, graph, 0.8, Vec3::X), views(), false)
                    .unwrap()
                    .is_some()
            );
        }
        assert_eq!(runtime.serial, 6);
    }
}

#[test]
fn native_forward_wrap_keeps_baseline_but_unexplained_reverse_restarts() {
    let graph = GraphState::Hanging(HangingState::GrappleAttached);
    let mut runtime = Runtime::default();
    let looping = |serial, time, cycles, position| {
        let mut value = timed(serial, graph, time, position);
        for clip in value
            .animation
            .playing
            .iter_mut()
            .chain(value.animation.samplers.iter_mut())
        {
            clip.looping = true;
            clip.overflows = cycles;
        }
        value
    };
    runtime
        .update(looping(1, 1.8, 0, Vec3::ZERO), views(), false)
        .unwrap();
    runtime
        .update(looping(2, 1.9, 0, Vec3::X), views(), false)
        .unwrap();
    let run = runtime.run;
    assert!(
        runtime
            .update(looping(3, 0.1, 1, Vec3::Y), views(), false)
            .unwrap()
            .is_some()
    );
    assert_eq!(runtime.run, run);
    assert!(
        runtime
            .update(looping(4, 0.0, 1, Vec3::Z), views(), false)
            .unwrap()
            .is_none()
    );
    assert!(runtime.run > run);
    assert_eq!(
        runtime.baseline.unwrap().sample.local.transform.translation,
        Vec3::Z
    );
}

#[test]
fn native_outgoing_sources_survive_interrupts_and_are_not_weighted_twice() {
    for graph in all_cases() {
        let mut runtime = Runtime::default();
        runtime
            .update(timed(1, graph, 0.0, Vec3::ZERO), views(), false)
            .unwrap();
        runtime
            .update(timed(2, graph, 0.5, Vec3::X), views(), false)
            .unwrap();
        for (serial, weight) in [(3, 0.75), (4, 0.25)] {
            let mut tail = timed(serial, graph, 0.6 + serial as f32 * 0.1, Vec3::X * weight);
            tail.state = Err(Refusal::OrdinaryOrUnsupportedState);
            let mut incoming = clip(0.1);
            incoming.source.instance += serial;
            incoming.source.owner += serial;
            incoming.source.binding += serial;
            tail.animation.playing = vec![incoming];
            tail.animation.samplers[0].weight = weight;
            tail.animation.samplers.push(incoming);
            let pair = runtime.update(tail, views(), false).unwrap().unwrap();
            assert!(
                pair[1]
                    .w_axis
                    .truncate()
                    .abs_diff_eq(Vec3::X * weight, 0.0001)
            );
            assert!(runtime.outgoing);
        }
        let mut done = timed(5, graph, 1.2, Vec3::ZERO);
        done.state = Err(Refusal::OrdinaryOrUnsupportedState);
        done.animation.samplers[0].camera_track_weight = 0.0;
        done.animation.playing[0].camera_track_weight = 0.0;
        assert_eq!(
            runtime.update(done, views(), false),
            Err(Refusal::NativeTailComplete)
        );
    }
}

#[test]
fn missing_or_invalid_source_time_and_stale_serial_clear_capture() {
    for change in 0..6 {
        let mut runtime = Runtime::default();
        let now = Instant::now();
        runtime.capture(
            Ok(timed(1, GraphState::FloorPickup, 0.0, Vec3::ZERO)),
            views(),
            false,
            now,
            0,
        );
        runtime.capture(
            Ok(timed(2, GraphState::FloorPickup, 0.1, Vec3::X)),
            views(),
            false,
            now,
            0,
        );
        let mut next = timed(3, GraphState::FloorPickup, 0.2, Vec3::Y);
        match change {
            0 => next.animation.samplers[0].time = f32::NAN,
            1 => next.animation.samplers[0].weight = f32::INFINITY,
            2 => next.animation.samplers.clear(),
            3 => next.finalized = 2,
            4 => next.source.producer_owner += 1,
            _ => next.source.skeleton_resource += 1,
        }
        assert!(runtime.capture(Ok(next), views(), false, now, 0).is_none());
        assert!(runtime.selected.is_none());
        assert!(runtime.handback.is_none());
        assert!(runtime.continuity.is_none());
    }
}

#[test]
fn same_state_binding_replacement_restarts_without_reusing_the_baseline() {
    let graph = GraphState::FloorPickup;
    let mut runtime = Runtime::default();
    runtime
        .update(timed(1, graph, 0.0, Vec3::ZERO), views(), false)
        .unwrap();
    runtime
        .update(timed(2, graph, 0.5, Vec3::X), views(), false)
        .unwrap();
    let mut replaced = timed(3, graph, 0.6, Vec3::Y);
    for clip in replaced
        .animation
        .playing
        .iter_mut()
        .chain(replaced.animation.samplers.iter_mut())
    {
        clip.source.binding += 1;
    }
    assert!(runtime.update(replaced, views(), false).unwrap().is_none());
    assert_eq!(
        runtime.baseline.unwrap().sample.local.transform.translation,
        Vec3::Y
    );
}

#[test]
fn native_tail_composes_the_current_head_transform_once() {
    let graph = GraphState::FloorPickup;
    let mut runtime = Runtime::default();
    runtime
        .update(timed(1, graph, 0.0, Vec3::ZERO), views(), false)
        .unwrap();
    runtime
        .update(timed(2, graph, 0.5, Vec3::X), views(), false)
        .unwrap();
    let mut tail = timed(3, graph, 0.6, Vec3::X * 0.5);
    tail.state = Err(Refusal::OrdinaryOrUnsupportedState);
    tail.animation.playing[0].weight = 0.5;
    tail.animation.samplers[0].weight = 0.5;
    let head = Mat4::from_rotation_translation(Quat::from_rotation_y(0.4), Vec3::Y * 0.3);
    let tracked = [TrackedView {
        world: head,
        tracking_translation: Vec3::Y * 0.3,
    }; 2];
    let pair = runtime.update(tail, tracked, true).unwrap().unwrap();
    let expected = Mat4::from_translation(Vec3::X * 0.5) * head;
    assert!(pair[1].abs_diff_eq(expected, 0.0001));
    assert_eq!(pair[0], pair[1]);
}
