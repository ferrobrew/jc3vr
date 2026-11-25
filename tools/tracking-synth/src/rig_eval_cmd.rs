//! The `rig-eval` command: its options, and the evaluation it runs and reports.

use std::path::PathBuf;

use body_rig::contact::ContactTuning;
use tracking_synth::rig_eval::{
    self, EvalOptions, Evaluation, PelvisSource, ReplayOptions, ik::IkEvalOptions,
};

use crate::{
    CliError, Command, DEFAULT_EYE_SHARE, DEFAULT_HEIGHT_M, estimate_track, parse_length,
    video_clock,
};

/// What `rig-eval` was asked for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RigEvalRequest {
    pub(crate) path: PathBuf,
    pub(crate) out: PathBuf,
    pub(crate) options: ReplayOptions,
    pub(crate) weights: Option<PathBuf>,
    /// The contact tunings the IK body's replay runs with, once each, when `--ik` asked for it.
    pub(crate) ik: Option<Vec<ContactTuning>>,
}

/// Read `rig-eval`'s options and paths from `args`.
pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut height = None;
    let mut eye_height = None;
    let mut tuning = None;
    let mut source = None;
    let mut weights = None;
    let mut ik = false;
    let mut contacts = Vec::new();
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--weights" => {
                weights = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--weights"))?,
                ));
            }
            "--ik" => ik = true,
            "--contact" => {
                let json = args.next().ok_or(CliError::MissingValue("--contact"))?;
                contacts.push(serde_json::from_str(&json).map_err(CliError::BadTuning)?);
            }
            "--pelvis" => {
                let name = args.next().ok_or(CliError::MissingValue("--pelvis"))?;
                source = Some(PelvisSource::from_name(&name).ok_or(CliError::BadPelvis(name))?);
            }
            "--height" => height = Some(parse_length(args.next(), "--height")?),
            "--eye-height" => eye_height = Some(parse_length(args.next(), "--eye-height")?),
            "--tuning" => {
                let json = args.next().ok_or(CliError::MissingValue("--tuning"))?;
                tuning = Some(serde_json::from_str(&json).map_err(CliError::BadTuning)?);
            }
            option if option.starts_with('-') => {
                return Err(CliError::UnknownOption(option.to_owned()));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    let recorded_body = height.is_none() && eye_height.is_none();
    let recorded_tuning = tuning.is_none();
    let height = height.unwrap_or(DEFAULT_HEIGHT_M);
    let mut eval = EvalOptions::new(height, eye_height.unwrap_or(height * DEFAULT_EYE_SHARE));
    if let Some(tuning) = tuning {
        eval.tuning = tuning;
    }
    let options = ReplayOptions {
        eval,
        source,
        recorded_body,
        recorded_tuning,
    };
    if contacts.is_empty() {
        contacts.push(ContactTuning::new());
    }
    let ik = ik.then_some(contacts);
    if ik.is_some() && weights.is_none() {
        return Err(CliError::IkWithoutWeights);
    }
    match <[PathBuf; 2]>::try_from(paths) {
        Ok([path, out]) => Ok(Command::RigEval(RigEvalRequest {
            path,
            out,
            options,
            weights,
            ik,
        })),
        Err(_) => Err(CliError::RigEvalPaths),
    }
}

/// Run the evaluation `request` asks for, write its CSV, and log its summary, then the IK body's
/// replay with each of its contact tunings.
pub(crate) fn run(request: RigEvalRequest) -> Result<(), CliError> {
    let RigEvalRequest {
        path,
        out,
        options,
        weights,
        ik,
    } = request;
    let bytes = std::fs::read(&path).map_err(|error| CliError::Read {
        path: path.clone(),
        error,
    })?;
    let recording = tracking_format::decode(&bytes).map_err(|error| CliError::Decode {
        path: path.clone(),
        error,
    })?;
    let clock = video_clock(&path);
    let estimate = weights
        .map(|weights| estimate_track(&weights, &recording))
        .transpose()?;
    let evaluation =
        rig_eval::evaluate_replay(&recording, &options, clock.as_ref(), estimate.as_ref())
            .map_err(CliError::RigEval)?;
    report_rig_config(&evaluation);
    let file = std::fs::File::create(&out).map_err(|error| CliError::Write {
        path: out.clone(),
        error,
    })?;
    let mut writer = std::io::BufWriter::new(file);
    rig_eval::write_compared_csv(&evaluation.rows, &mut writer)
        .and_then(|()| std::io::Write::flush(&mut writer))
        .map_err(|error| CliError::Write {
            path: out.clone(),
            error,
        })?;
    let summary = evaluation.summary;
    tracing::info!(
        pelvis = evaluation.source.name(),
        rows = evaluation.rows.len(),
        video = clock.is_some(),
        grounded_s = %format_args!("{:.1}", summary.grounded_s),
        steps = summary.steps,
        driving_s = %format_args!("{:.1}", summary.driving_s),
        turns = summary.turns,
        median_turn_deg = %format_args!("{:.1}", summary.median_turn_deg),
        p90_turn_deg = %format_args!("{:.1}", summary.p90_turn_deg),
        mean_yaw_error_deg = %format_args!("{:.1}", summary.mean_yaw_error_deg),
        mean_chest_error_deg = %format_args!("{:.1}", summary.mean_chest_error_deg),
        mean_offset_m = %format_args!("{:.3}", summary.mean_offset_m),
        max_offset_m = %format_args!("{:.3}", summary.max_offset_m),
        held_lean_s = %format_args!("{:.0}", summary.held_lean_s),
        recenters = summary.recenters,
        "rig-eval: wrote {}",
        out.display()
    );
    if let Some(comparison) = evaluation.comparison {
        tracing::info!(
            rows = comparison.rows,
            mean_body_yaw_diff_deg = %format_args!("{:.2}", comparison.mean_yaw_diff_deg),
            max_body_yaw_diff_deg = %format_args!("{:.2}", comparison.max_yaw_diff_deg),
            phase_agreement = %format_args!("{:.3}", comparison.phase_agreement),
            mean_offset_diff_m = %format_args!("{:.4}", comparison.mean_offset_diff_m),
            "rig-eval: against the recorded rig"
        );
    }
    if let (Some(tunings), Some(estimate)) = (ik, estimate.as_ref()) {
        let mut ik_options = IkEvalOptions::for_body(options.eval.body);
        for (index, contact) in tunings.into_iter().enumerate() {
            ik_options.contact = contact;
            let summary = rig_eval::ik::evaluate_ik(&recording, &ik_options, estimate);
            rig_eval::ik::log_summary(index, &contact, &summary);
        }
    }
    Ok(())
}

/// Say on stderr which calibration and tuning `rig-eval` ran, and which of the recording's notes
/// it could not read.
fn report_rig_config(evaluation: &Evaluation) {
    for error in &evaluation.config_errors {
        tracing::warn!("{error}");
    }
    if evaluation.unplaced_poses > 0 {
        tracing::warn!(
            poses = evaluation.unplaced_poses,
            "rig-eval: some pre-IK poses have no view to place them, and are left out"
        );
    }
    let Some(config) = evaluation.config else {
        return;
    };
    let first = config.first;
    let ran = match (config.body, config.tuning) {
        (true, true) => "the recorded calibration and tuning",
        (true, false) => "the recorded calibration and the command line's tuning",
        (false, true) => "the command line's calibration and the recorded tuning",
        (false, false) => "the command line's calibration and tuning",
    };
    tracing::info!(
        notes = config.notes,
        height_m = first.height_m,
        eye_height_m = first.eye_height_m,
        body_eye_height_m = first.body.eye_height,
        body_hip_height_m = first.body.hip_height,
        "rig-eval: the recording notes the rig's calibration and tuning; running {ran}"
    );
}
