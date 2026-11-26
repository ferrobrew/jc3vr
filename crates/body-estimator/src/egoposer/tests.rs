//! Golden tests against the original PyTorch model. Those that need the weights read their path
//! from `JC3VR_EGOPOSER_WEIGHTS` and skip without it; the input pipeline's tests always run.

use super::*;
use crate::{
    egoposer::network::network_input,
    features::{HandVisibility, SparseFrame, WINDOW_FRAMES, WINDOW_POSES},
    fixture::{Fixture, max_abs_diff},
    tracking::GripOffset,
};

/// The environment variable naming the weights (a safetensors file).
const WEIGHTS_VAR: &str = "JC3VR_EGOPOSER_WEIGHTS";

/// The largest error tolerated, relative to the largest magnitude in the reference tensor.
const RELATIVE_TOLERANCE: f64 = 1e-4;

fn weights() -> Option<EgoPoser> {
    let Some(path) = std::env::var_os(WEIGHTS_VAR) else {
        eprintln!(
            "skipping: set {WEIGHTS_VAR} to EgoPoser's weights (a safetensors file) to run this test"
        );
        return None;
    };
    Some(EgoPoser::load(&path).unwrap_or_else(|e| panic!("{e}")))
}

/// A window of the fixture's own features, so a network test does not depend on the pipeline.
fn fixture_window(fixture: &Fixture, window: usize) -> SparseWindow {
    let features = fixture.tensor(&format!("w{window}.features"));
    SparseWindow {
        features: std::array::from_fn(|f| std::array::from_fn(|k| features[f * 54 + k])),
        visibility: HandVisibility::ALWAYS,
    }
}

/// Compare `ours` with the fixture's `name`, report the errors, and fail past the tolerance.
fn check(fixture: &Fixture, name: &str, ours: &[f32]) {
    check_against(name, ours, &fixture.tensor(name));
}

/// Compare `ours` with `reference`, named `name` in the report.
fn check_against(name: &str, ours: &[f32], reference: &[f32]) {
    let error = max_abs_diff(ours, reference);
    let scale = reference
        .iter()
        .map(|v| f64::from(v.abs()))
        .fold(0.0, f64::max)
        .max(1e-6);
    eprintln!(
        "{name}: max abs error {error:.2e}, relative to the largest value {:.2e}",
        error / scale
    );
    assert!(
        error / scale < RELATIVE_TOLERANCE,
        "{name}: max abs error {error} against a scale of {scale}"
    );
}

#[test]
fn the_network_input_matches_the_reference_normalisation() {
    let fixture = Fixture::load();
    let input = network_input(&fixture_window(&fixture, 0));
    check(&fixture, "w0.normalized", &input);
}

#[test]
fn the_masks_zero_only_the_hidden_hand() {
    let fixture = Fixture::load();
    let mut visibility = HandVisibility::ALWAYS;
    visibility.left[3] = false;
    let open = network_input(&fixture_window(&fixture, 0));
    let masked = network_input(&fixture_window(&fixture, 0).with_visibility(visibility));
    let row = |x: &[f32], frame: usize| x[frame * 60..(frame + 1) * 60].to_vec();
    let (a, b) = (row(&open, 3), row(&masked, 3));
    for k in 0..60 {
        let hidden = (6..12).contains(&k)
            || (24..30).contains(&k)
            || (39..42).contains(&k)
            || (48..51).contains(&k);
        assert_eq!(b[k], if hidden { 0.0 } else { a[k] }, "value {k}");
    }
    assert_eq!(row(&open, 4), row(&masked, 4));
}

#[test]
fn every_stage_matches_the_reference() {
    let Some(model) = weights() else { return };
    let fixture = Fixture::load();
    assert!(
        !model.estimates_shape(),
        "the fixture's checkpoint has no shape head"
    );
    let (output, trace) = model.infer_traced(&fixture_window(&fixture, 0));
    check(&fixture, "w0.normalized", &trace.normalized);
    check(&fixture, "w0.embedded", &trace.embedded);
    assert_eq!(trace.layers.len(), 3);
    for (i, layer) in trace.layers[..2].iter().enumerate() {
        check(&fixture, &format!("w0.layer{i}"), layer);
    }
    // The last layer computes the last token alone.
    let last_token = &fixture.tensor("w0.layer2")[39 * 256..];
    check_against("w0.layer2's last token", &trace.layers[2], last_token);
    check(&fixture, "w0.root_orient", &output.root_orient);
    check(&fixture, "w0.pose_body", &output.pose_body);
}

#[test]
fn every_window_matches_the_reference() {
    let Some(model) = weights() else { return };
    let fixture = Fixture::load();
    for window in 0..fixture.windows() {
        let output = model.infer(&fixture_window(&fixture, window));
        check(
            &fixture,
            &format!("w{window}.root_orient"),
            &output.root_orient,
        );
        check(&fixture, &format!("w{window}.pose_body"), &output.pose_body);
    }
}

#[test]
fn hidden_hands_match_the_reference() {
    let Some(model) = weights() else { return };
    let fixture = Fixture::load();
    let (left, right) = (fixture.tensor("w0.fov_l"), fixture.tensor("w0.fov_r"));
    let visibility = HandVisibility {
        left: std::array::from_fn(|i| left[i] > 0.5),
        right: std::array::from_fn(|i| right[i] > 0.5),
    };
    assert!(visibility.left.iter().take(WINDOW_FRAMES).any(|v| !v));
    let output = model.infer(&fixture_window(&fixture, 0).with_visibility(visibility));
    check(&fixture, "w0.fov_root_orient", &output.root_orient);
    check(&fixture, "w0.fov_pose_body", &output.pose_body);
}

#[test]
fn yaws_from_tracking_match_the_reference_end_to_end() {
    let Some(model) = weights() else { return };
    let fixture = Fixture::load();
    for window in 0..fixture.windows() {
        let poses: Vec<SparseFrame> = fixture
            .tracked_frames(window)
            .iter()
            .map(|frame| SparseFrame::from_tracking(frame, &GripOffset::INDEX_BEST_GUESS))
            .collect();
        let poses: &[SparseFrame; WINDOW_POSES] = poses.as_slice().try_into().unwrap();
        let ours = model.infer(&SparseWindow::from_poses(poses));
        let reference = EgoPoserOutput {
            root_orient: fixture
                .tensor(&format!("w{window}.root_orient"))
                .try_into()
                .unwrap(),
            pose_body: fixture
                .tensor(&format!("w{window}.pose_body"))
                .try_into()
                .unwrap(),
            betas: None,
        };
        let degrees = |a: f64, b: f64| (a - b).sin().atan2((a - b).cos()).abs().to_degrees();
        let pelvis = degrees(ours.pelvis_yaw(), reference.pelvis_yaw());
        let chest = degrees(ours.chest_yaw(), reference.chest_yaw());
        eprintln!(
            "window {window}: yaw error from tracking, pelvis {pelvis:.2e} deg, chest {chest:.2e} deg"
        );
        assert!(pelvis < 0.01 && chest < 0.01);
    }
}

#[test]
fn errors_name_the_weights_and_the_cause() {
    let missing = EgoPoser::load("/nonexistent/egoposer.safetensors").unwrap_err();
    assert!(matches!(missing.kind, WeightsErrorKind::Read(_)));
    assert!(missing.to_string().starts_with(
        "egoposer weights at /nonexistent/egoposer.safetensors: could not read the file: "
    ));
    let garbage = EgoPoser::from_bytes(b"not a safetensors file").unwrap_err();
    assert!(matches!(garbage.kind, WeightsErrorKind::Parse(_)));
    assert!(
        garbage
            .to_string()
            .starts_with("egoposer weights: not a safetensors file: ")
    );
}
