use glam::{Quat, Vec3};
use pretty_assertions::assert_eq;

use super::*;
use crate::testing::visual::{
    environment::WeatherControl,
    overrides::{ReflectionOverride, SunShadowsOverride},
    pose::orientations_match,
};

fn observation(state: &State, ordinal: i64) -> FrameObservation {
    FrameObservation {
        mode: state.mode.unwrap(),
        engine_dimensions: (4096, 2048),
        target_dimensions: (4096, 2048),
        single_pass: state.mode == Some(Mode::SinglePass),
        camera_frozen: true,
        horizon_map_ready: true,
        shadow_override_ready: true,
        shaders_current: true,
        frame_identity: Some(ordinal),
        generation: state.generation,
        pose: None,
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
fn stale_shaders_hold_off_readiness_until_the_rebuild_lands() {
    let mut state = warming(Mode::SinglePass, 2);
    let mut stale = observation(&state, 1);
    stale.shaders_current = false;
    state.complete(stale, true);
    assert_eq!(state.completed_frames, 0);
    state.complete(observation(&state, 2), true);
    let mut stale = observation(&state, 3);
    stale.shaders_current = false;
    state.complete(stale, true);
    assert_eq!(state.completed_frames, 0);
    state.complete(observation(&state, 4), true);
    state.complete(observation(&state, 5), true);
    assert!(state.ready);
}

#[test]
fn single_pass_switch_overrides_survive_the_profile_and_reset_with_it() {
    crate::test_config::register();
    let overrides = VisualOverrides::try_from(
        toml::from_str::<VisualOverridesTable>(
            "[single_pass]\nssr_per_eye = false\nclouds_per_eye = false\n",
        )
        .unwrap(),
    )
    .unwrap();
    let mut config = Snapshot::defaults();
    apply_profile_with_overrides(&mut config, Mode::SinglePass, Some(overrides));
    let sps = config.part(&STEREO_CONFIG).single_pass;
    assert!(sps.enabled && sps.reproject);
    assert!(!sps.ssr_per_eye && !sps.clouds_per_eye);
    apply_profile_with_overrides(&mut config, Mode::SinglePass, None);
    assert!(
        config.part(&STEREO_CONFIG).single_pass.ssr_per_eye
            && config.part(&STEREO_CONFIG).single_pass.clouds_per_eye
    );
}

#[test]
fn reflection_override_runs_the_producer_without_disabling_sun_shadows() {
    crate::test_config::register();
    let mut config = Snapshot::defaults();
    for mode in [
        Mode::LeftOnly,
        Mode::RightOnly,
        Mode::Multipass,
        Mode::SinglePass,
    ] {
        apply_profile_with_overrides(
            &mut config,
            mode,
            Some(VisualOverrides {
                sun_shadows: None,
                screen_space_reflections: Some(ReflectionOverride::Enabled),
                ssao: None,
                ao_volumes: None,
                global_illumination: None,
                fsr: None,
                taa: None,
                foveation: None,
                auto_exposure: None,
                screen_fade: None,
                player_damage: None,
                fsr_mv_correction: None,
                fsr_hybrid_velocity: None,
                fsr_jitter: None,
                anti_aliasing: None,
                fsr_sharpen: None,
                fsr_sharpness: None,
                taa_history_weight: None,
                taa_clip_gamma_rest: None,
                taa_current_sharpness: None,
                taa_motion_history_weight: None,
                headset: None,
                resolution_scale: None,
                stages: None,
                single_pass: Default::default(),
            }),
        );
        assert!(
            !config.part(&STEREO_CONFIG).skip_ssr
                && !config.part(&STEREO_CONFIG).disable_sun_shadows
        );
        apply_profile_with_overrides(&mut config, mode, None);
        assert!(!config.part(&STEREO_CONFIG).skip_ssr);
    }
}

fn shadows_disabled() -> VisualOverrides {
    VisualOverrides {
        sun_shadows: Some(SunShadowsOverride::Disabled),
        screen_space_reflections: None,
        ssao: None,
        ao_volumes: None,
        global_illumination: None,
        fsr: None,
        taa: None,
        foveation: None,
        auto_exposure: None,
        screen_fade: None,
        player_damage: None,
        fsr_mv_correction: None,
        fsr_hybrid_velocity: None,
        fsr_jitter: None,
        anti_aliasing: None,
        fsr_sharpen: None,
        fsr_sharpness: None,
        taa_history_weight: None,
        taa_clip_gamma_rest: None,
        taa_current_sharpness: None,
        taa_motion_history_weight: None,
        headset: None,
        resolution_scale: None,
        stages: None,
        single_pass: Default::default(),
    }
}

#[test]
fn shadow_override_resets_on_new_profile() {
    crate::test_config::register();
    let mut config = Snapshot::defaults();
    let treatment = Some(shadows_disabled());
    apply_profile_with_overrides(&mut config, Mode::Multipass, treatment);
    assert!(config.part(&STEREO_CONFIG).disable_sun_shadows);
    apply_profile_with_overrides(&mut config, Mode::Multipass, None);
    assert!(!config.part(&STEREO_CONFIG).disable_sun_shadows);
}

#[test]
fn shadow_override_readiness_waits_for_settings_and_live_state() {
    assert!(!shadow_state_matches(true, None, false, true));
    assert!(!shadow_state_matches(true, Some(false), false, false));
    assert!(!shadow_state_matches(true, Some(true), false, true));
    assert!(!shadow_state_matches(true, Some(false), true, true));
    assert!(shadow_state_matches(true, Some(false), false, true));
    assert!(!shadow_state_matches(false, None, false, false));
    assert!(!shadow_state_matches(false, Some(false), false, true));
    assert!(!shadow_state_matches(false, Some(true), false, false));
    assert!(shadow_state_matches(false, Some(false), false, false));
    assert!(shadow_state_matches(false, Some(true), true, false));
}

#[test]
fn shadow_override_loss_fails_running_motion_closed() {
    let base = base_pose();
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    let request = state.requested_motion.clone().unwrap();
    let mut observed = observation(&state, 2);
    observed.shadow_override_ready = false;
    state.complete_motion(&request, observed, Some((base, base)), true);
    assert!(!state.ready);
    assert!(state.error.is_some());
    assert!(state.motion.is_none());
}

#[test]
fn shadow_override_readiness_gates_frames_and_restoration() {
    let mut state = warming(Mode::Multipass, 1);
    state.visual_overrides = Some(shadows_disabled());
    state.shadow_sync_required = true;
    let mut observed = observation(&state, 1);
    observed.shadow_override_ready = false;
    state.complete(observed, true);
    assert!(!state.ready);
    state.complete(observation(&state, 2), true);
    assert!(state.ready);
    assert_eq!(
        serde_json::to_value(state.metadata()).unwrap()["visual_overrides"]["sun_shadows"],
        "disabled"
    );
    state.start(Operation::Restoring(active("restore-shadow")));
    assert!(
        state
            .complete_restore(restored_resolution(), Some(false), false, false, false)
            .is_none()
    );
    assert!(
        state
            .complete_restore(restored_resolution(), Some(false), false, false, true)
            .is_some()
    );
    assert!(!state.shadow_sync_required);
}

#[test]
fn horizon_map_readiness_gates_completed_frames() {
    let mut state = warming(Mode::Multipass, 1);
    let mut observed = observation(&state, 1);
    observed.horizon_map_ready = false;
    state.complete(observed, true);
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);
    state.complete(observation(&state, 2), true);
    assert_eq!(state.completed_frames, 1);
    assert!(state.ready);
}

#[test]
fn horizon_maps_ready_requires_two_distinct_non_fallback_slices() {
    let loaded = [[Some(1), Some(3)], [Some(2), Some(4)]];
    assert!(horizon_maps_ready(true, Some(0), loaded));
    assert!(horizon_maps_ready(
        false,
        None,
        [[None::<u32>, None], [None, None]]
    ));
    assert!(!horizon_maps_ready(
        true,
        Some(0),
        [[None, Some(3)], [Some(2), Some(4)]]
    ));
    assert!(!horizon_maps_ready(
        true,
        Some(0),
        [[Some(0), Some(3)], [Some(2), Some(4)]]
    ));
    assert!(!horizon_maps_ready(true, None, loaded));
    assert!(!horizon_maps_ready(
        true,
        Some(0),
        [[Some(1), Some(3)], [Some(1), Some(4)]]
    ));
    assert!(!horizon_maps_ready(
        true,
        Some(0),
        [[Some(1), None], [Some(2), Some(4)]]
    ));
}

#[test]
fn horizon_map_loss_invalidates_completed_frames() {
    let mut state = warming(Mode::Multipass, 1);
    state.complete(observation(&state, 1), true);
    assert!(state.ready);
    let mut observed = observation(&state, 2);
    observed.horizon_map_ready = false;
    state.complete(observed, true);
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);
}

#[test]
fn horizon_map_loss_fails_running_motion_closed() {
    let base = base_pose();
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    assert!(state.motion.as_ref().is_some_and(MotionState::running));
    let request = state.requested_motion.clone().unwrap();
    let mut observed = observation(&state, 2);
    observed.horizon_map_ready = false;
    state.complete_motion(&request, observed, Some((base, base)), true);
    assert!(!state.ready);
    assert!(state.error.is_some());
    assert!(state.motion.is_none());
    assert!(state.requested_motion.is_none());
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
fn profile_states_single_pass_for_every_mode() {
    crate::test_config::register();
    for mode in [
        Mode::LeftOnly,
        Mode::RightOnly,
        Mode::Multipass,
        Mode::ReverseMultipass,
        Mode::SinglePass,
    ] {
        let mut config = Snapshot::defaults();
        apply_profile(&mut config, mode);
        let on = mode == Mode::SinglePass;
        let sps = &config.part(&STEREO_CONFIG).single_pass;
        assert_eq!(sps.enabled, on, "{mode:?}");
    }
}

#[test]
fn profile_keeps_per_eye_native_resolution_across_modes() {
    crate::test_config::register();
    let mut config = Snapshot::defaults();
    config.part_mut(&VR_CONFIG).resolution_scale = 0.75;
    config.part_mut(&POST_FX_CONFIG).skip_dof = true;
    config.part_mut(&POST_FX_CONFIG).dof_no_reproject = false;
    for mode in [
        Mode::LeftOnly,
        Mode::RightOnly,
        Mode::Multipass,
        Mode::ReverseMultipass,
        Mode::SinglePass,
    ] {
        apply_profile(&mut config, mode);
        assert!(config.part(&VR_CONFIG).native_resolution);
        assert_eq!(config.part(&VR_CONFIG).resolution_scale, 0.75);
        assert_eq!(config.part(&VR_CONFIG).freeze_mode, FreezeMode::FullCamera);
        assert!(*config.part(&PAUSED));
        assert_eq!(
            config.part(&FSR_CONFIG).antialiasing,
            jc3vr_view::fsr::AntiAliasing::Engine
        );
        assert!(!config.part(&FOVEATION_CONFIG).enabled);
        assert!(config.part(&EXPOSURE_CONFIG).force);
        assert_eq!(config.part(&EXPOSURE_CONFIG).forced_value, 0.11);
        assert!(
            config.part(&POST_FX_CONFIG).skip_motion_blur
                && config.part(&POST_FX_CONFIG).skip_motion_blur_recon
        );
        assert!(
            !config.part(&POST_FX_CONFIG).skip_dof && config.part(&POST_FX_CONFIG).dof_no_reproject
        );
        assert!(!config.part(&STEREO_CONFIG).skip_ssr && !config.part(&STEREO_CONFIG).skip_gi);
        assert!(
            !config.part(&STEREO_CONFIG).disable_ssao && !config.part(&POST_FX_CONFIG).skip_glare
        );
        assert!(
            config.part(&POST_FX_CONFIG).skip_fade
                && config.part(&POST_FX_CONFIG).skip_player_damage
        );
        assert!(!config.part(&POST_FX_CONFIG).skip_sun_halo);
        let on = mode == Mode::SinglePass;
        let sps = &config.part(&STEREO_CONFIG).single_pass;
        assert_eq!(sps.enabled, on);
        assert_eq!(sps.reproject, on);
        assert_eq!(sps.terrain, on);
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
                .complete_restore(observed, Some(false), false, false, true)
                .is_none()
        );
        assert!(!state.metadata().ready);
    }
    assert!(
        state
            .complete_restore(resolution, Some(true), false, false, true)
            .is_none()
    );
    let completed = state
        .complete_restore(resolution, Some(false), false, false, true)
        .unwrap();
    assert_eq!(completed.id, RequestId::parse("end").unwrap());
    assert!(state.metadata().ready);
    assert!(!state.requires_inline_tail());
    assert!(
        state
            .complete_restore(resolution, Some(false), false, false, true)
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
            .complete_restore(restored_resolution(), Some(false), false, false, true)
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
            .complete_restore(restored_resolution(), Some(false), false, false, true)
            .is_some()
    );
}

#[test]
fn repeated_environment_configures_keep_the_first_saved_world_time() {
    let saved = WorldTimeSnapshot::new(8.25, 1.5, 0.0);
    let mut state = State::new();
    state.record_environment(
        saved,
        EnvironmentMetadata::new(13.5, 0.0, 0.25, WeatherControl::Sunny),
    );
    state.record_environment(
        WorldTimeSnapshot::new(18.0, 2.0, 1.0),
        EnvironmentMetadata::new(18.0, 0.0, 0.2, WeatherControl::Sunny),
    );
    assert_eq!(state.environment.as_ref().unwrap().saved(), saved);
    assert_eq!(state.environment_restore_target(), Some(saved));
    assert_eq!(
        state.metadata().environment,
        Some(EnvironmentMetadata::new(
            18.0,
            0.0,
            0.2,
            WeatherControl::Sunny
        ))
    );
}

#[test]
fn pending_environment_is_kept_until_a_verified_restore() {
    let saved = WorldTimeSnapshot::new(8.25, 1.5, 0.0);
    let mut state = State::new();
    state.environment = Some(EnvironmentControl::pending(saved));
    assert_eq!(state.environment_restore_target(), Some(saved));
    state.environment_restored(EnvironmentMetadata::new(
        8.25,
        1.5,
        0.4,
        WeatherControl::Ambient,
    ));
    assert_eq!(state.environment_restore_target(), None);
    assert_eq!(
        state.environment.as_ref().unwrap().observed(),
        Some(EnvironmentMetadata::new(
            8.25,
            1.5,
            0.4,
            WeatherControl::Ambient
        ))
    );
}

#[test]
fn environment_metadata_serializes_observed_values_and_control() {
    let saved = WorldTimeSnapshot::new(8.25, 1.5, 0.0);
    let mut state = State::new();
    state.record_environment(
        saved,
        EnvironmentMetadata::new(13.5, 0.0, 0.25, WeatherControl::Sunny),
    );
    let ready = serde_json::to_value(state.metadata()).unwrap();
    assert_eq!(
        ready["environment"],
        serde_json::json!({
            "hour": 13.5,
            "day_cycle_speed": 0.0,
            "weather_severity": 0.25,
            "weather_control": "sunny",
        })
    );
    state.environment_restored(EnvironmentMetadata::new(
        8.25,
        1.5,
        0.5,
        WeatherControl::Ambient,
    ));
    let restored = serde_json::to_value(state.metadata()).unwrap();
    assert_eq!(
        restored["environment"]["weather_control"],
        serde_json::json!("ambient")
    );
}

#[test]
fn metadata_tracks_generation_and_profile_without_json_state() {
    let state = warming(Mode::ReverseMultipass, 30);
    let metadata = state.metadata();
    assert_eq!(metadata.mode, Some(Mode::ReverseMultipass));
    assert_eq!(metadata.generation, 7);
    assert_eq!(metadata.profile, DiagnosticProfile::ControlledV3);
    assert_eq!(State::new().metadata().profile, DiagnosticProfile::Native);
}

#[test]
fn restoration_retains_the_original_configuration_until_completion() {
    crate::test_config::register();
    let mut original = Snapshot::defaults();
    original.part_mut(&VR_CONFIG).native_resolution = false;
    let mut state = State::new();
    state.saved = Some(original);
    state.start(Operation::Restoring(active("end-baseline")));
    let resolution = restored_resolution();
    assert!(
        state
            .complete_restore(resolution, None, false, false, true)
            .is_none()
    );
    assert!(!state.metadata().ready);
    assert!(
        !state
            .saved
            .as_ref()
            .unwrap()
            .part(&VR_CONFIG)
            .native_resolution
    );
    state.start(Operation::Configuring(active("configure-baseline")));
    assert!(
        !state
            .saved
            .as_ref()
            .unwrap()
            .part(&VR_CONFIG)
            .native_resolution
    );
    state.start(Operation::Restoring(active("retry-baseline")));
    assert!(
        state
            .complete_restore(resolution, Some(false), false, false, true)
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

fn pose_request(case_id: &str, yaw_deg: f32, pitch_deg: f32, roll_deg: f32) -> PoseRequest {
    PoseRequest::new(
        RequestId::parse(case_id).unwrap(),
        yaw_deg,
        pitch_deg,
        roll_deg,
    )
    .unwrap()
}

fn base_pose() -> PoseValues {
    PoseValues {
        position: Vec3::new(12.5, 2.25, -37.75),
        yaw_deg: 24.0,
        pitch_deg: -11.0,
        roll_deg: 6.0,
    }
}

#[test]
fn pose_requests_compose_offsets_about_the_base_orientation() {
    let base = base_pose();
    for (yaw, pitch, roll) in [
        (30.0, 0.0, 0.0),
        (0.0, -20.0, 0.0),
        (0.0, 0.0, 15.0),
        (12.0, -34.0, 56.0),
        (0.0, 90.0, 0.0),
        (0.0, -90.0, 0.0),
        (45.0, 89.0, -30.0),
    ] {
        let request = pose_request("sweep", yaw, pitch, roll);
        let target = request.target(base);
        let expected = base.orientation()
            * Quat::from_euler(
                pose_control::EULER,
                yaw.to_radians(),
                pitch.to_radians(),
                roll.to_radians(),
            );
        assert_eq!(target.position, base.position);
        assert!(
            orientations_match(target.orientation(), expected),
            "yaw {yaw} pitch {pitch} roll {roll}"
        );
        assert!(pose_matches(target, target));
    }
}

#[test]
fn orientation_matching_tolerates_quaternion_sign_and_rejects_non_finite() {
    let q = Quat::from_euler(pose_control::EULER, 0.4, -0.3, 0.2);
    assert!(orientations_match(q, q));
    assert!(orientations_match(q, -q));
    assert!(!orientations_match(q, Quat::from_rotation_y(0.2)));
    assert!(!orientations_match(
        Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0),
        q
    ));
    assert!(!orientations_match(
        q,
        Quat::from_xyzw(f32::INFINITY, 0.0, 0.0, 1.0)
    ));
}

#[test]
fn pose_translation_moves_the_target_in_anchor_space() {
    let base = PoseValues {
        position: Vec3::new(10.0, 20.0, 30.0),
        yaw_deg: 90.0,
        pitch_deg: 0.0,
        roll_deg: 0.0,
    };
    let request = pose_request("translate", 0.0, 0.0, 0.0)
        .with_translation(TranslationOffset::new(2.0, 3.0, 4.0).unwrap());
    let target = request.target(base);
    let expected = base.position + base.orientation() * Vec3::new(2.0, 3.0, -4.0);
    assert!(target.position.distance(expected) <= 1e-5);
    assert!((target.position - Vec3::new(6.0, 23.0, 28.0)).length() <= 1e-4);
    assert!(orientations_match(target.orientation(), base.orientation()));

    let PoseFrame::Observed(evidence) =
        pose_frame(Some(&request), Some(1), Some((base, target)), Some(target))
    else {
        panic!("a translated pose must be observed");
    };
    assert_eq!(
        evidence.translation,
        TranslationOffset::new(2.0, 3.0, 4.0).unwrap()
    );
    assert_eq!(evidence.anchor, PoseTransform::from(base));
    assert_eq!(evidence.applied, PoseTransform::from(target));
    assert!(request.matches_evidence(&evidence));

    let wrong_position = PoseEvidence {
        applied: PoseTransform::from(base),
        ..evidence.clone()
    };
    assert!(!request.matches_evidence(&wrong_position));
    let wrong_translation = PoseEvidence {
        translation: TranslationOffset::ZERO,
        ..evidence
    };
    assert!(!request.matches_evidence(&wrong_translation));

    let PoseFrame::Apply(reapplied) =
        pose_frame(Some(&request), Some(2), Some((base, base)), Some(base))
    else {
        panic!("a translated pose that did not render must be reapplied");
    };
    assert_eq!(reapplied, target);
    assert!(matches!(
        pose_frame(Some(&request), Some(3), Some((base, base)), Some(target)),
        PoseFrame::Defer
    ));
}

#[test]
fn translated_pose_evidence_matches_the_request_at_world_coordinates() {
    // The euler round trip leaves the anchor quaternion a fraction off unit length, which a 900 m
    // translation magnifies past the position tolerance if the expected position renormalizes it.
    let base = PoseValues {
        position: Vec3::new(-4123.75, 87.5, 6011.25),
        yaw_deg: 2.0,
        pitch_deg: 36.0,
        roll_deg: 128.0,
    };
    let request = pose_request("translate", 0.0, 0.0, 0.0)
        .with_translation(TranslationOffset::new(0.0, -100.0, 900.0).unwrap());
    let target = request.target(base);
    let evidence = request.evidence(base, target);
    assert!(request.matches_evidence(&evidence));

    let shifted = PoseEvidence {
        applied: PoseTransform {
            position: target.position.to_array().map(|value| value + 5e-4),
            ..evidence.applied
        },
        ..evidence
    };
    assert!(!request.matches_evidence(&shifted));
}

#[test]
fn pose_translation_rejects_non_finite_and_out_of_range_values() {
    for (right, up, forward) in [
        (f32::NAN, 0.0, 0.0),
        (0.0, f32::INFINITY, 0.0),
        (5000.5, 0.0, 0.0),
        (0.0, 0.0, -5000.5),
    ] {
        assert!(TranslationOffset::new(right, up, forward).is_err());
    }
    let table: PoseTable = toml::from_str(
        "case_id = 'translate'\nyaw_deg = 0.0\npitch_deg = 0.0\nroll_deg = 0.0\nright = 5000.5\n",
    )
    .unwrap();
    assert!(PoseRequest::try_from(table).is_err());
}

#[test]
fn historical_pose_tables_default_translation_to_zero() {
    let table: PoseTable =
        toml::from_str("case_id = 'legacy'\nyaw_deg = 30.0\npitch_deg = 0.0\nroll_deg = 0.0\n")
            .unwrap();
    let request = PoseRequest::try_from(table).unwrap();
    let base = base_pose();
    let target = request.target(base);
    assert_eq!(target.position, base.position);
    let evidence = request.evidence(base, target);
    assert_eq!(evidence.translation, TranslationOffset::ZERO);
    assert!(request.matches_evidence(&evidence));
}

#[test]
fn pose_targets_keep_the_base_position_and_do_not_accumulate() {
    let base = base_pose();
    let request = pose_request("sweep", 30.0, -12.0, 4.0);
    let first = request.target(base);
    let second = request.target(base);
    assert_eq!(first, second);
    assert_eq!(first.position, base.position);

    let nudged = PoseValues {
        yaw_deg: first.yaw_deg + 2.0,
        ..first
    };
    let PoseFrame::Apply(target) =
        pose_frame(Some(&request), Some(1), Some((base, nudged)), Some(nudged))
    else {
        panic!("a mismatched pose must be reapplied");
    };
    assert_eq!(target.position, base.position);
    assert!(orientations_match(
        target.orientation(),
        first.orientation()
    ));

    let PoseFrame::Observed(evidence) =
        pose_frame(Some(&request), Some(1), Some((base, target)), Some(target))
    else {
        panic!("the applied pose must be observed");
    };
    assert_eq!(evidence.case_id, RequestId::parse("sweep").unwrap());
    assert_eq!(
        evidence.offset,
        OrientationOffset {
            yaw_deg: 30.0,
            pitch_deg: -12.0,
            roll_deg: 4.0,
        }
    );
    assert_eq!(evidence.anchor, PoseTransform::from(base));
    assert_eq!(evidence.applied, PoseTransform::from(target));
}

#[test]
fn an_identity_offset_returns_the_anchor_pose() {
    let base = base_pose();
    let request = pose_request("identity", 0.0, 0.0, 0.0);
    let target = request.target(base);
    assert_eq!(target.position, base.position);
    assert!(orientations_match(target.orientation(), base.orientation()));
    let PoseFrame::Observed(evidence) =
        pose_frame(Some(&request), Some(1), Some((base, target)), Some(target))
    else {
        panic!("an identity offset must be observed");
    };
    assert_eq!(evidence.anchor.position, base.position.to_array());
    assert!(orientations_match(
        Quat::from_array(evidence.anchor.orientation),
        Quat::from_array(evidence.applied.orientation),
    ));
}

#[test]
fn a_pose_request_defers_until_the_requested_pose_has_rendered() {
    let base = base_pose();
    let request = pose_request("sweep", 30.0, 0.0, 0.0);
    assert_eq!(
        pose_frame(Some(&request), None, None, None),
        PoseFrame::Defer
    );
    assert_eq!(
        pose_frame(Some(&request), Some(1), None, Some(base)),
        PoseFrame::Defer
    );
    assert_eq!(
        pose_frame(None, Some(1), Some((base, base)), Some(base)),
        PoseFrame::NotRequested
    );
    let PoseFrame::Apply(target) =
        pose_frame(Some(&request), Some(1), Some((base, base)), Some(base))
    else {
        panic!("the first rendered frame still holds the base pose");
    };
    assert_ne!(target, base);
    let PoseFrame::Observed(_) =
        pose_frame(Some(&request), Some(2), Some((base, target)), Some(target))
    else {
        panic!("the following frame holds the requested pose");
    };
}

#[test]
fn a_frame_without_pose_evidence_cannot_warm_or_publish_evidence() {
    let mut state = warming(Mode::Multipass, 0);
    state.requested_pose = Some(pose_request("sweep", 30.0, 0.0, 0.0));
    state.complete(observation(&state, 1), true);
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);
    assert!(state.metadata().pose.is_none());

    let base = base_pose();
    let request = state.requested_pose.as_ref().unwrap();
    let target = request.target(base);
    let evidence = request.evidence(base, target);
    let mut observed = observation(&state, 2);
    observed.pose = Some(evidence.clone());
    state.complete(observed, true);
    assert!(state.ready);
    assert_eq!(state.metadata().pose, Some(evidence));
}

#[test]
fn deferring_for_an_unapplied_pose_clears_warmup_and_consumes_the_frame() {
    let mut state = warming(Mode::Multipass, 1);
    state.requested_pose = Some(pose_request("sweep", 30.0, 0.0, 0.0));
    let base = base_pose();
    let request = state.requested_pose.as_ref().unwrap();
    let target = request.target(base);
    let mut observed = observation(&state, 1);
    observed.pose = Some(request.evidence(base, target));
    state.complete(observed, true);
    assert!(state.ready);

    state.defer_for_pose(Some(2));
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);
    assert!(state.metadata().pose.is_none());
    assert_eq!(state.last_frame, Some(2));

    state.complete(observation(&state, 2), true);
    assert_eq!(state.completed_frames, 0);
}

#[test]
fn clearing_the_pose_request_drops_its_evidence() {
    let mut state = warming(Mode::Multipass, 1);
    let base = base_pose();
    let request = pose_request("sweep", 30.0, 0.0, 0.0);
    let target = request.target(base);
    state.requested_pose = Some(request.clone());
    state.pose = Some(request.evidence(base, target));
    assert!(state.metadata().pose.is_some());

    state.clear_pose();
    assert!(state.requested_pose.is_none());
    assert!(state.pose.is_none());
    assert!(state.metadata().pose.is_none());

    state.requested_pose = Some(request);
    state.pose = Some(PoseEvidence {
        case_id: RequestId::parse("sweep").unwrap(),
        offset: OrientationOffset {
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            roll_deg: 0.0,
        },
        translation: TranslationOffset::ZERO,
        anchor: PoseTransform::from(base),
        applied: PoseTransform::from(base),
    });
    state.start(Operation::Configuring(active("next")));
    assert!(state.pose.is_none());
}

#[test]
fn completed_frames_reject_evidence_that_does_not_match_the_request() {
    let base = base_pose();
    let request = pose_request("sweep", 30.0, 0.0, 0.0);
    let target = request.target(base);
    let good = request.evidence(base, target);
    let variants = [
        PoseEvidence {
            case_id: RequestId::parse("other").unwrap(),
            ..good.clone()
        },
        PoseEvidence {
            offset: OrientationOffset {
                yaw_deg: 31.0,
                pitch_deg: 0.0,
                roll_deg: 0.0,
            },
            ..good.clone()
        },
        PoseEvidence {
            applied: PoseTransform::from(base),
            ..good.clone()
        },
        PoseEvidence {
            anchor: PoseTransform {
                position: [f32::NAN, 0.0, 0.0],
                ..good.anchor
            },
            ..good.clone()
        },
    ];
    for (index, evidence) in variants.into_iter().enumerate() {
        let mut state = warming(Mode::Multipass, 0);
        state.requested_pose = Some(request.clone());
        let mut observed = observation(&state, 1);
        observed.pose = Some(evidence);
        state.complete(observed, true);
        assert_eq!(state.completed_frames, 0, "variant {index}");
        assert!(!state.ready, "variant {index}");
        assert!(state.metadata().pose.is_none(), "variant {index}");
    }

    let mut state = warming(Mode::Multipass, 0);
    state.requested_pose = Some(request);
    let mut observed = observation(&state, 1);
    observed.pose = Some(good);
    state.complete(observed, true);
    assert!(state.ready);
}

#[test]
fn a_late_live_edit_cannot_acknowledge_the_rendered_pose() {
    let base = base_pose();
    let request = pose_request("sweep", 60.0, 0.0, 0.0);
    let target = request.target(base);
    let older = pose_request("sweep", 30.0, 0.0, 0.0).target(base);

    let PoseFrame::Defer = pose_frame(Some(&request), Some(1), Some((base, older)), Some(target))
    else {
        panic!("a live edit must not acknowledge the frame rendered before it");
    };
    let PoseFrame::Observed(evidence) =
        pose_frame(Some(&request), Some(2), Some((base, target)), Some(target))
    else {
        panic!("the frame that rendered the requested pose must be observed");
    };
    assert_eq!(evidence.applied, PoseTransform::from(target));

    let edited = PoseValues {
        yaw_deg: target.yaw_deg + 5.0,
        ..target
    };
    let PoseFrame::Observed(evidence) =
        pose_frame(Some(&request), Some(3), Some((base, target)), Some(edited))
    else {
        panic!("the rendered pose is the evidence, not the live edit");
    };
    assert_eq!(evidence.applied, PoseTransform::from(target));
    assert_ne!(evidence.applied, PoseTransform::from(edited));
}

#[test]
fn a_missing_render_stamp_fails_closed() {
    let base = base_pose();
    let request = pose_request("sweep", 30.0, 0.0, 0.0);
    let target = request.target(base);
    assert_eq!(
        pose_frame(Some(&request), Some(1), None, Some(target)),
        PoseFrame::Defer
    );
    assert_eq!(
        pose_frame(Some(&request), None, Some((base, target)), Some(target)),
        PoseFrame::Defer
    );
}

#[test]
fn a_timed_out_configure_stops_requesting_and_clearing_its_evidence() {
    let mut state = warming(Mode::Multipass, 1);
    let request = pose_request("sweep", 30.0, 0.0, 0.0);
    let base = base_pose();
    let target = request.target(base);
    let active = active("sweep");
    let deadline = active.started + TIMEOUT;
    state.start(Operation::Configuring(active));
    state.requested_pose = Some(request.clone());
    state.pose = Some(request.evidence(base, target));

    assert!(state.expire(deadline).is_none());
    let failed = state
        .expire(deadline + Duration::from_millis(1))
        .expect("the request times out");
    assert_eq!(failed.id, RequestId::parse("sweep").unwrap());
    assert!(state.requested_pose.is_none());
    assert!(state.pose.is_none());
    assert!(state.metadata().pose.is_none());
    assert!(!state.ready);
    assert!(state.mode.is_some());
    assert!(state.error.as_ref().unwrap().contains("timed out"));
}

#[test]
fn pose_evidence_serializes_the_agreed_wire_shape() {
    let request = pose_request("sweep", 30.0, -10.0, 5.0);
    let base = base_pose();
    let target = request.target(base);
    let evidence = request.evidence(base, target);
    let json = serde_json::to_value(&evidence).unwrap();
    assert_eq!(json["case_id"], "sweep");
    assert_eq!(json["offset"]["yaw_deg"].as_f64(), Some(30.0));
    assert_eq!(json["offset"]["pitch_deg"].as_f64(), Some(-10.0));
    assert_eq!(json["offset"]["roll_deg"].as_f64(), Some(5.0));
    assert_eq!(json["translation"]["right"].as_f64(), Some(0.0));
    assert_eq!(json["translation"]["up"].as_f64(), Some(0.0));
    assert_eq!(json["translation"]["forward"].as_f64(), Some(0.0));
    assert_eq!(json["anchor"]["position"].as_array().unwrap().len(), 3);
    assert_eq!(json["anchor"]["orientation"].as_array().unwrap().len(), 4);
    assert_eq!(json["applied"]["position"].as_array().unwrap().len(), 3);
    assert_eq!(json["applied"]["orientation"].as_array().unwrap().len(), 4);
}

fn motion_request(period_frames: u32) -> MotionRequest {
    MotionRequest::new(
        RequestId::parse("motion-sweep").unwrap(),
        period_frames,
        10.0,
        5.0,
        2.5,
    )
    .unwrap()
}

fn motion_state(mode: Mode, warmup_frames: u32, period_frames: u32) -> State {
    let mut state = warming(mode, warmup_frames);
    let request = motion_request(period_frames);
    state.requested_motion = Some(request.clone());
    state.motion = Some(MotionState::new(request));
    state
}

fn complete_motion_frame(state: &mut State, frame: i64, base: PoseValues, applied: PoseValues) {
    let request = state.requested_motion.clone().unwrap();
    let observed = observation(state, frame);
    state.complete_motion(&request, observed, Some((base, applied)), true);
}

#[test]
fn motion_preroll_holds_the_anchor_before_phase_zero_starts() {
    let base = base_pose();
    let mut state = motion_state(Mode::Multipass, 2, 32);
    for frame in 1..=2 {
        complete_motion_frame(&mut state, frame, base, base);
        let motion = state.motion.as_ref().unwrap();
        assert_eq!(motion.phase(), 0);
        assert!(motion.history().is_empty());
        assert!(!motion.running() || state.ready);
    }
    assert!(state.ready);
    let metadata = state.metadata();
    let motion = metadata.motion.as_ref().unwrap();
    assert_eq!(motion.phase, 0);
    assert_eq!(motion.cycle, 0);
    assert!(motion.history.is_empty());
    assert_eq!(metadata.pose.as_ref().unwrap().offset.yaw_deg, 0.0);
}

#[test]
fn motion_phases_advance_once_per_distinct_completed_frame_and_wrap() {
    let base = base_pose();
    let request = motion_request(32);
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    for phase in 0..32 {
        let target = request.target(base, phase);
        complete_motion_frame(&mut state, phase as i64 + 2, base, target);
        let motion = state.motion.as_ref().unwrap();
        assert_eq!(motion.phase(), (phase + 1) % 32);
        assert_eq!(
            motion.history().len(),
            (phase as usize + 1).min(HISTORY_FRAMES)
        );
        assert_eq!(motion.history().back().unwrap().phase, phase);
        assert_eq!(
            motion.history().back().unwrap().applied,
            PoseTransform::from(target)
        );
    }
    let motion = state.motion.as_ref().unwrap();
    assert_eq!(motion.phase(), 0);
    assert_eq!(motion.cycle(), 1);
}

#[test]
fn a_duplicate_completed_frame_never_advances_motion_phase() {
    let base = base_pose();
    let request = motion_request(32);
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    complete_motion_frame(&mut state, 2, base, request.target(base, 0));
    assert_eq!(state.motion.as_ref().unwrap().phase(), 1);
    complete_motion_frame(&mut state, 2, base, request.target(base, 1));
    assert_eq!(state.motion.as_ref().unwrap().phase(), 1);
    assert_eq!(state.motion.as_ref().unwrap().history().len(), 1);
}

#[test]
fn motion_preroll_tolerates_missing_stamps_but_running_motion_fails_closed() {
    let base = base_pose();
    let mut state = motion_state(Mode::Multipass, 1, 32);
    let request = state.requested_motion.clone().unwrap();
    let mut missing_stamp = observation(&state, 1);
    missing_stamp.frame_identity = None;
    state.complete_motion(&request, missing_stamp, None, true);
    assert!(state.error.is_none());
    assert_eq!(state.completed_frames, 0);
    assert!(!state.ready);

    let mut missing_render = observation(&state, 2);
    missing_render.frame_identity = Some(2);
    state.complete_motion(&request, missing_render, None, true);
    assert!(state.error.is_none());
    assert_eq!(state.completed_frames, 0);

    complete_motion_frame(&mut state, 3, base, base);
    assert!(state.ready);
    assert!(state.motion.as_ref().unwrap().running());

    let mut running_missing_stamp = observation(&state, 4);
    running_missing_stamp.frame_identity = None;
    state.complete_motion(&request, running_missing_stamp, None, true);
    assert!(state.error.as_ref().unwrap().contains("rendered stamp"));
    assert!(!state.ready);
    assert!(state.requested_motion.is_none());
    assert!(state.motion.is_none());
}

#[test]
fn a_mismatched_rendered_pose_fails_motion_closed() {
    let base = base_pose();
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    let wrong = PoseValues {
        yaw_deg: base.yaw_deg + 1.0,
        ..base
    };
    complete_motion_frame(&mut state, 2, base, wrong);
    assert!(
        state
            .error
            .as_ref()
            .unwrap()
            .contains("did not render phase")
    );
    assert!(!state.ready);
    assert!(state.requested_motion.is_none());
    assert!(state.motion.is_none());
    assert!(state.pose.is_none());
}

#[test]
fn restarting_motion_keeps_the_anchor_and_trajectory_but_clears_history() {
    let base = base_pose();
    let request = motion_request(32);
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    for frame in 2..=5 {
        let phase = state.motion.as_ref().unwrap().phase();
        let target = request.target(base, phase);
        complete_motion_frame(&mut state, frame, base, target);
    }
    assert!(state.motion.as_ref().unwrap().cycle() == 0);
    assert!(!state.motion.as_ref().unwrap().history().is_empty());
    let first_phase = request.target(base, 0);

    state.start(Operation::Configuring(active("motion-restart")));
    state.requested_motion = Some(request.clone());
    state.motion = Some(MotionState::new(request.clone()));
    state.warmup_frames = 1;
    complete_motion_frame(&mut state, 10, base, base);
    let motion = state.motion.as_ref().unwrap();
    assert_eq!(motion.phase(), 0);
    assert_eq!(motion.cycle(), 0);
    assert!(motion.history().is_empty());
    assert_eq!(motion.anchor(), Some(base));
    assert_eq!(request.target(base, 0), first_phase);
    assert!(state.ready);
}

#[test]
fn motion_metadata_for_a_capture_uses_the_frame_before_advance() {
    let base = base_pose();
    let request = motion_request(32);
    let mut state = motion_state(Mode::Multipass, 1, 32);
    complete_motion_frame(&mut state, 1, base, base);
    for frame in 2..=34 {
        let phase = state.motion.as_ref().unwrap().phase();
        let target = request.target(base, phase);
        complete_motion_frame(&mut state, frame, base, target);
    }
    let motion = state.motion.as_ref().unwrap();
    assert_eq!(motion.phase(), 1);
    let metadata = motion.metadata(35);
    assert_eq!(metadata.phase, 1);
    assert_eq!(metadata.cycle, 1);
    assert_eq!(metadata.rendered_frame, 35);
    assert_eq!(metadata.history.len(), HISTORY_FRAMES);
    assert_eq!(metadata.history.last().unwrap().rendered_frame, 34);
}

#[test]
fn a_request_is_active_during_warmup_before_it_is_ready() {
    let state = warming(Mode::Multipass, 3);
    assert!(state.is_active());
    assert!(!state.is_ready());
}

#[test]
fn an_ended_request_is_inactive_after_the_mode_is_cleared() {
    let mut state = warming(Mode::Multipass, 3);
    assert!(state.is_active());
    state.mode = None;
    assert!(!state.is_active());
}
