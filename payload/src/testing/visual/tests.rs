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

#[test]
fn metadata_tracks_generation_and_profile_without_json_state() {
    let state = warming(Mode::ReverseMultipass, 30);
    let metadata = state.metadata();
    assert_eq!(metadata.mode, Some(Mode::ReverseMultipass));
    assert_eq!(metadata.generation, 7);
    assert_eq!(metadata.profile, DiagnosticProfile::ControlledV1);
    assert_eq!(State::new().metadata().profile, DiagnosticProfile::Native);
}
