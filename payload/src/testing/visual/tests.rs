use pretty_assertions::assert_eq;

use super::*;

fn observation(state: &State, ordinal: i64) -> FrameObservation {
    FrameObservation {
        mode: state.mode.unwrap(),
        engine_dimensions: (4096, 2048),
        target_dimensions: (4096, 2048),
        single_pass: state.mode == Some(Mode::SinglePass),
        camera_frozen: true,
        frame_identity: Some(ordinal),
        generation: state.generation,
    }
}

fn warming(mode: Mode, frames: u32) -> State {
    let mut state = State::new();
    state.mode = Some(mode);
    state.generation = 7;
    state.warmup_frames = frames;
    state
}

#[test]
fn warmup_counts_only_distinct_completed_frames() {
    let mut state = warming(Mode::Multipass, 3);
    state.complete(observation(&state, 1), true);
    state.complete(observation(&state, 1), true);
    assert_eq!(state.completed_frames, 1);
    assert!(!state.ready);
    state.complete(observation(&state, 2), true);
    assert!(!state.ready);
    state.complete(observation(&state, 3), true);
    assert!(state.ready);
}

#[test]
fn stale_generations_cannot_warm_a_new_mode() {
    let mut state = warming(Mode::RightOnly, 1);
    let mut old = observation(&state, 1);
    old.generation -= 1;
    state.complete(old, true);
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);
}

#[test]
fn resolution_pipeline_camera_and_clock_must_match() {
    for mismatch in 0..7 {
        let mut state = warming(Mode::SinglePass, 2);
        state.complete(observation(&state, 1), true);
        let mut observed = observation(&state, 2);
        let paused = mismatch != 4;
        match mismatch {
            0 => observed.engine_dimensions.0 /= 2,
            1 => observed.single_pass = false,
            2 => observed.mode = Mode::Multipass,
            3 => observed.camera_frozen = false,
            4 => {}
            5 => observed.frame_identity = None,
            6 => {
                observed.engine_dimensions = (0, 0);
                observed.target_dimensions = (0, 0);
            }
            _ => unreachable!(),
        }
        state.complete(observed, paused);
        assert!(!state.ready, "mismatch {mismatch}");
        assert_eq!(state.completed_frames, 0, "mismatch {mismatch}");
        state.complete(observation(&state, 3), true);
        assert!(!state.ready);
        state.complete(observation(&state, 4), true);
        assert!(state.ready);
    }
}

#[test]
fn zero_warmup_still_requires_a_completed_matching_frame() {
    let mut state = warming(Mode::LeftOnly, 0);
    assert!(!state.ready);
    state.complete(observation(&state, 1), true);
    assert!(state.ready);
}

#[test]
fn failed_reload_never_becomes_ready() {
    let mut state = warming(Mode::SinglePass, 1);
    state.error = Some("shader reload: no graphics engine".to_owned());
    for ordinal in 1..=3 {
        state.complete(observation(&state, ordinal), true);
    }
    assert!(!state.ready);
    assert_eq!(state.completed_frames, 0);
}

#[test]
fn profile_keeps_per_eye_native_resolution_across_modes() {
    let mut config = Config::new();
    config.vr.resolution_scale = 0.75;
    for mode in [
        Mode::LeftOnly,
        Mode::RightOnly,
        Mode::Multipass,
        Mode::ReverseMultipass,
        Mode::SinglePass,
    ] {
        apply_profile(&mut config, mode);
        assert!(config.vr.native_resolution);
        assert_eq!(config.vr.resolution_scale, 0.75);
        assert_eq!(config.vr.freeze_mode, FreezeMode::FullCamera);
        assert!(config.director.paused);
        assert!(!config.fsr.enabled);
        assert!(!config.foveation.enabled);
        assert!(!config.far_field.enabled);
        assert!(config.exposure.force);
        assert_eq!(config.exposure.forced_value, 0.11);
        assert!(config.post_fx.skip_motion_blur && config.post_fx.skip_motion_blur_recon);
        assert!(config.post_fx.skip_dof && config.stereo.skip_ssr && config.stereo.skip_gi);
        assert!(config.stereo.disable_ssao && config.post_fx.skip_glare);
        assert!(config.post_fx.skip_fade && config.post_fx.skip_player_damage);
        assert!(config.post_fx.skip_sun_halo);
        let on = mode == Mode::SinglePass;
        let sps = &config.stereo.single_pass;
        assert_eq!(sps.enabled, on);
        assert_eq!(sps.dual_eye, on);
        assert_eq!(sps.double_wide, on);
        assert_eq!(sps.collapse, on);
        assert_eq!(sps.reproject, on);
        assert_eq!(sps.terrain, on);
        assert_eq!(sps.tree_impostors, on);
        assert_eq!(sps.bark && sps.foliage && sps.occluder, on);
        assert!(!sps.patch_dryrun);
    }
}

fn active(id: &str) -> Active {
    Active {
        id: RequestId::parse(id).unwrap(),
        started: Instant::now(),
    }
}

fn restored_resolution() -> ResolutionProgress {
    ResolutionProgress {
        engine: Some((1920, 1080)),
        desired: Some((1920, 1080)),
        pending: false,
    }
}

#[test]
fn restoration_starts_protected_before_configuration_changes() {
    let mut state = State::new();
    assert!(state.start(Operation::Restoring(active("end"))).is_none());
    assert!(state.requires_inline_tail());
    assert!(!state.metadata().ready);
    assert!(state.contains_id(&RequestId::parse("end").unwrap()));
}

#[test]
fn restoration_requires_serviced_resize_and_restored_clock() {
    let mut state = State::new();
    state.start(Operation::Restoring(active("end")));
    let resolution = restored_resolution();
    for observed in [
        ResolutionProgress {
            pending: true,
            ..resolution
        },
        ResolutionProgress {
            desired: Some((3840, 1080)),
            ..resolution
        },
        ResolutionProgress {
            engine: None,
            ..resolution
        },
        ResolutionProgress {
            engine: Some((0, 0)),
            desired: Some((0, 0)),
            pending: false,
        },
    ] {
        assert!(
            state
                .complete_restore(observed, Some(false), false, false)
                .is_none()
        );
        assert!(!state.metadata().ready);
    }
    assert!(
        state
            .complete_restore(resolution, Some(true), false, false)
            .is_none()
    );
    let completed = state
        .complete_restore(resolution, Some(false), false, false)
        .unwrap();
    assert_eq!(completed.id, RequestId::parse("end").unwrap());
    assert!(state.metadata().ready);
    assert!(!state.requires_inline_tail());
    assert!(
        state
            .complete_restore(resolution, Some(false), false, false)
            .is_none()
    );
}

#[test]
fn restore_reload_failure_retains_protection_and_cannot_acknowledge_success() {
    let mut state = State::new();
    state.start(Operation::Restoring(active("end")));
    let failed = state
        .fail_operation("shader reload: no graphics engine".to_owned())
        .unwrap();
    assert_eq!(failed.id, RequestId::parse("end").unwrap());
    assert!(state.requires_inline_tail());
    assert!(state.contains_id(&failed.id));
    assert!(!state.metadata().ready);
    assert!(
        state
            .complete_restore(restored_resolution(), Some(false), false, false)
            .is_none()
    );
    assert!(
        state
            .fail_operation("visual control: another failure".to_owned())
            .is_none()
    );
    assert!(state.requires_inline_tail());
}

#[test]
fn restoring_requests_time_out_once_without_releasing_protection() {
    let mut state = State::new();
    let active = active("end");
    let deadline = active.started + TIMEOUT;
    state.start(Operation::Restoring(active));
    assert!(state.expire(deadline).is_none());
    assert!(state.expire(deadline + Duration::from_millis(1)).is_some());
    assert!(state.error.as_ref().unwrap().contains("restoration"));
    assert!(state.requires_inline_tail());
    assert!(state.expire(deadline + TIMEOUT).is_none());
    assert!(!state.metadata().ready);
}

#[test]
fn configure_and_end_supersede_restoration_without_orphaning_its_id() {
    for next in [
        Operation::Configuring(active("configure")),
        Operation::Restoring(active("retry")),
    ] {
        let mut state = State::new();
        state.start(Operation::Restoring(active("end")));
        let (superseded, metadata) = state.start(next).unwrap();
        assert_eq!(superseded.id, RequestId::parse("end").unwrap());
        assert!(!metadata.ready);
        assert!(metadata.error.unwrap().contains("superseded"));
        assert_ne!(state.operation.id(), Some(&superseded.id));
        assert!(state.error.is_none());
    }
}

#[test]
fn retry_after_failed_restoration_does_not_report_failure_twice() {
    let mut state = State::new();
    state.start(Operation::Restoring(active("end")));
    assert!(
        state
            .fail_operation("shader reload: failed".to_owned())
            .is_some()
    );
    assert!(state.start(Operation::Restoring(active("retry"))).is_none());
    assert!(state.error.is_none());
    assert!(state.requires_inline_tail());
    assert!(
        state
            .complete_restore(restored_resolution(), Some(false), false, false)
            .is_some()
    );
}

#[test]
fn metadata_tracks_generation_and_profile_without_json_state() {
    let state = warming(Mode::ReverseMultipass, 30);
    let metadata = state.metadata();
    assert_eq!(metadata.mode, Some(Mode::ReverseMultipass));
    assert_eq!(metadata.generation, 7);
    assert_eq!(metadata.profile, DiagnosticProfile::ControlledV1);
    assert_eq!(State::new().metadata().profile, DiagnosticProfile::Native);
}

#[test]
fn restoration_retains_the_original_configuration_until_completion() {
    let mut original = Config::new();
    original.vr.native_resolution = false;
    let mut state = State::new();
    state.saved = Some(original);
    state.start(Operation::Restoring(active("end-baseline")));
    let resolution = restored_resolution();
    assert!(
        state
            .complete_restore(resolution, None, false, false)
            .is_none()
    );
    assert!(!state.metadata().ready);
    assert!(!state.saved.as_ref().unwrap().vr.native_resolution);
    state.start(Operation::Configuring(active("configure-baseline")));
    assert!(!state.saved.as_ref().unwrap().vr.native_resolution);
    state.start(Operation::Restoring(active("retry-baseline")));
    assert!(
        state
            .complete_restore(resolution, Some(false), false, false)
            .is_some()
    );
    assert!(state.saved.is_none());
}

#[test]
fn duplicate_ids_include_queued_configuring_restoring_and_failed_restores() {
    let mut state = State::new();
    let queued = RequestId::parse("queued-id").unwrap();
    state.pending.push_back(Request::End { id: queued.clone() });
    assert!(state.contains_id(&queued));
    assert!(!state.contains_id(&RequestId::parse("unused-id").unwrap()));
    for operation in [
        Operation::Configuring(active("configure-id")),
        Operation::Restoring(active("restore-id")),
        Operation::RestoreFailed(RequestId::parse("failed-id").unwrap()),
    ] {
        let id = operation.id().unwrap().clone();
        state.operation = operation;
        assert!(state.contains_id(&id));
        assert!(state.contains_id(&queued));
    }
}
