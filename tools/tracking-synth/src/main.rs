//! `tracking-synth`: writes the built-in synthetic tracking recordings, and prints, evaluates,
//! exports, and scans recorded ones.
//!
//! ```text
//! tracking-synth [--out <dir>] [--rate <hz>] [<script>...]
//! tracking-synth --list
//! tracking-synth dump [--json] [--from <time>] [--to <time>] <recording>
//! tracking-synth rig-eval [--pelvis simulated|recorded|pre-ik] [--height <m>] [--eye-height <m>]
//!     [--tuning <json>] [--weights <safetensors>] [--ik [--contact <json>]...] <recording> <out.csv>
//! tracking-synth export-rerun [--from <time>] [--to <time>] [--video <video>]
//!     [--video-height <px>] [--height <m>] [--eye-height <m>] [--tuning <json>] [--no-blueprint]
//!     [--recording-id <id>] [--weights <safetensors>] <recording> -o <out.rrd>
//! tracking-synth estimate-body --weights <safetensors> [--from <time>] [--to <time>] [--lead <s>]
//!     <recording> -o <out.csv>
//! tracking-synth scan-view [--video <video>] [--out <report.md>] <recording>
//! ```
//!
//! With no script named, every built-in script is written. The recordings go to
//! `<dir>/<script>.jc3trk`, by default `target/tracking-synth/` in the repository, for the offline
//! commands to read. `--list` prints each built-in script's name and summary, tab-separated, one
//! per line.
//!
//! `dump` prints any recording's frames and events to stdout, as text or, with `--json`, as JSON
//! lines. `--from` and `--to` limit it to an inclusive stretch; a `<time>` is seconds on the
//! recording's clock (`12.5`) or an OpenXR predicted display time in nanoseconds (`xr:<ns>`), the
//! clock a video recording's sidecar shares.
//!
//! `rig-eval` runs the grounded body rig over a recording and writes a CSV of each grounded frame
//! (`rig_eval`). `--pelvis` picks the rig's inputs: the frames with a simulated pelvis, the rig
//! inputs the payload recorded, or the frames with the pelvis under the game's pre-IK hips; a
//! recording with rig frames defaults to its recorded inputs and compares every row with them.
//! `--tuning` takes the rig's tuning as JSON, any field left out keeping its default, and it and
//! `--height` and `--eye-height` override the calibration and tuning a recording notes. The rest of
//! the rig's tuning, which a recording does not carry, is read from the repository's
//! `crates/jc3vr-body/tuning/rig.toml` each time `rig-eval` or `export-rerun` runs. `--weights`,
//! for `rig-eval` and `export-rerun`, runs EgoPoser over the whole recording first and hands the
//! rig its estimate at the game's cadence, as the payload's rig takes it. `--ik` also replays the
//! IK body's FK pelvis and foot contacts over that estimate (`rig_eval::ik`) and logs its summary,
//! once for each `--contact` tuning given (the contact model's tuning as JSON, any field left out
//! at its default) or once with the defaults. A video sidecar (`frames.jsonl` and `stream.json`)
//! beside the recording adds the video's time to each row.
//!
//! `export-rerun` writes a stretch of a recording as a Rerun recording (`rerun_export`), with the
//! take's video cut to the stretch beside it; it needs the `rerun` feature, on by default.
//! `estimate-body` runs EgoPoser (`body-estimator`) over a stretch from the recording's raw head
//! and grip poses and writes a CSV of its estimate at 60 Hz (`estimate`), resampling from `--lead`
//! seconds (10 by default) before the stretch; the weights are a safetensors file named by
//! `--weights`. `scan-view` scans a take's video for flashes and checks each against the
//! recording's camera (`view_scan`), writing a Markdown report.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::ExitCode,
};

mod rig_eval_cmd;

use tracing_subscriber::EnvFilter;
#[cfg(feature = "rerun")]
use tracking_synth::rerun_export::{self, DEFAULT_VIDEO_HEIGHT, ExportOptions, VideoSource};
use tracking_synth::{
    dump::{self, Bound, DumpFormat, Range},
    estimate,
    rig_eval::{
        EstimateCadence, EstimateTrack, EvalOptions, RigEvalError, RigFile, RigFileError,
        VideoClock,
    },
    script::Script,
    scripts,
    synth::{DEFAULT_RATE_HZ, Options, SynthError},
    view_scan::{self, ScanError, ScanOptions},
};

use crate::rig_eval_cmd::RigEvalRequest;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let command = match parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            tracing::error!("{error}");
            tracing::info!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("{error}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: tracking-synth [--out <dir>] [--rate <hz>] [<script>...] | --list | \
                     dump [--json] [--from <time>] [--to <time>] <recording> | rig-eval \
                     [--pelvis simulated|recorded|pre-ik] [--height <m>] [--eye-height <m>] \
                     [--tuning <json>] [--weights <safetensors>] [--ik [--contact <json>]...] \
                     <recording> <out.csv> | \
                     scan-view [--video <video>] [--out <report.md>] <recording> | \
                     export-rerun [--from <time>] [--to <time>] [--video <video>] [--video-height \
                     <px>] [--height <m>] [--eye-height <m>] [--tuning <json>] [--no-blueprint] [--recording-id <id>] [--weights <safetensors>] <recording> -o \
                     <out.rrd> | estimate-body --weights <safetensors> [--from <time>] [--to \
                     <time>] [--lead <s>] <recording> -o <out.csv>, where <time> is seconds or \
                     xr:<display time ns>";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
enum Command {
    Help,
    List,
    Dump {
        path: PathBuf,
        format: DumpFormat,
        range: Range,
    },
    RigEval(RigEvalRequest),
    ExportRerun {
        path: PathBuf,
        out: PathBuf,
        range: Range,
        video: Option<PathBuf>,
        video_height: Option<u32>,
        blueprint: bool,
        recording_id: Option<String>,
        options: EvalOptions,
        weights: Option<PathBuf>,
    },
    ScanView {
        path: PathBuf,
        video: Option<PathBuf>,
        out: Option<PathBuf>,
    },
    EstimateBody {
        path: PathBuf,
        out: PathBuf,
        weights: PathBuf,
        range: Range,
        lead_s: f64,
    },
    Write {
        out: PathBuf,
        options: Options,
        /// The scripts to write; every built-in one when empty.
        scripts: Vec<String>,
    },
}

/// Why the tool could not do what was asked.
#[derive(Debug)]
enum CliError {
    /// An option the tool does not have.
    UnknownOption(String),
    /// An option given without its value.
    MissingValue(&'static str),
    /// A frame rate that is not a number.
    BadRate(String),
    /// `scan-view` without a recording, or with more than one.
    ScanPath,
    Scan(ScanError),
    /// A `dump` range bound that is neither seconds nor `xr:<ns>`.
    BadTime(String),
    /// A script that is not built in.
    UnknownScript(String),
    /// `dump` without a recording, or with more than one.
    DumpPath,
    /// `rig-eval` without a recording and an output, or with more.
    RigEvalPaths,
    /// A `rig-eval` length that is not a positive number.
    BadLength(String),
    /// `rig-eval` tuning that does not parse.
    BadTuning(serde_json::Error),
    /// A `--pelvis` source the evaluator does not have.
    BadPelvis(String),
    /// A `rig-eval` source the recording cannot feed.
    RigEval(RigEvalError),
    /// The body rig's tuning file could not be read.
    RigFile(RigFileError),
    /// `rig-eval --ik` without the estimator's weights.
    IkWithoutWeights,
    /// `export-rerun` without a recording and an output, or with more.
    ExportPaths,
    /// A `--video-height` that is not a whole number of pixels.
    BadVideoHeight(String),
    /// `export-rerun` in a build without the `rerun` feature.
    #[cfg_attr(feature = "rerun", allow(dead_code))]
    NoRerun,
    #[cfg(feature = "rerun")]
    Export(tracking_synth::rerun_export::ExportError),
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    Decode {
        path: PathBuf,
        error: tracking_format::DecodeError,
    },
    Print(std::io::Error),
    Synth {
        script: &'static str,
        error: SynthError,
    },
    Write {
        path: PathBuf,
        error: std::io::Error,
    },
    /// `estimate-body` without a recording, an output, and the weights, or with more.
    EstimatePaths,
    /// The estimator's weights could not be loaded.
    Weights(body_estimator::WeightsError),
    /// A stretch that selects no frame.
    EmptyRange,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tracking-synth: ")?;
        match self {
            CliError::UnknownOption(option) => write!(f, "there is no option `{option}`"),
            CliError::MissingValue(option) => write!(f, "`{option}` needs a value"),
            CliError::BadRate(value) => write!(f, "the frame rate `{value}` is not a number"),
            CliError::ScanPath => write!(f, "scan-view: name exactly one recording"),
            CliError::Scan(error) => write!(f, "{error}"),
            CliError::BadTime(value) => write!(
                f,
                "dump: the time `{value}` is neither seconds nor `xr:<display time ns>`"
            ),
            CliError::UnknownScript(name) => write!(
                f,
                "there is no built-in script `{name}`; `--list` lists them"
            ),
            CliError::DumpPath => write!(f, "dump: name exactly one recording"),
            CliError::RigEvalPaths => {
                write!(f, "rig-eval: name exactly one recording and one output")
            }
            CliError::BadLength(value) => {
                write!(f, "rig-eval: the length `{value}` is not a positive number")
            }
            CliError::BadTuning(error) => write!(f, "rig-eval: the tuning does not parse: {error}"),
            CliError::BadPelvis(value) => write!(
                f,
                "rig-eval: there is no pelvis source `{value}`; it is one of simulated, recorded, \
                 or pre-ik"
            ),
            CliError::RigEval(error) => write!(f, "{error}"),
            CliError::RigFile(error) => write!(f, "{error}"),
            CliError::IkWithoutWeights => write!(
                f,
                "rig-eval: `--ik` replays the learned estimate, so it needs `--weights`"
            ),
            CliError::ExportPaths => write!(
                f,
                "export-rerun: name exactly one recording, and the output with `-o`"
            ),
            CliError::BadVideoHeight(value) => write!(
                f,
                "export-rerun: the video height `{value}` is not a whole number of pixels"
            ),
            CliError::NoRerun => write!(
                f,
                "export-rerun: this build has no `rerun` feature; build with `--features rerun`"
            ),
            #[cfg(feature = "rerun")]
            CliError::Export(error) => write!(f, "{error}"),
            CliError::Read { path, error } => {
                write!(f, "could not read {}: {error}", path.display())
            }
            CliError::Decode { path, error } => write!(f, "{}: {error}", path.display()),
            CliError::Print(error) => write!(f, "dump: could not print: {error}"),
            CliError::Synth { script, error } => write!(f, "{script}: {error}"),
            CliError::Write { path, error } => {
                write!(f, "could not write {}: {error}", path.display())
            }
            CliError::EstimatePaths => write!(
                f,
                "estimate-body: name exactly one recording, the output with `-o`, and the weights \
                 with `--weights`"
            ),
            CliError::Weights(error) => write!(f, "the body estimator: {error}"),
            CliError::EmptyRange => write!(f, "estimate-body: the stretch selects no frame"),
        }
    }
}

impl std::error::Error for CliError {}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut args = args.peekable();
    if args.peek().map(String::as_str) == Some("dump") {
        args.next();
        return parse_dump(args);
    }
    if args.peek().map(String::as_str) == Some("rig-eval") {
        args.next();
        return rig_eval_cmd::parse(args);
    }
    if args.peek().map(String::as_str) == Some("estimate-body") {
        args.next();
        return parse_estimate_body(args);
    }
    if args.peek().map(String::as_str) == Some("scan-view") {
        args.next();
        return parse_scan_view(args);
    }
    if args.peek().map(String::as_str) == Some("export-rerun") {
        args.next();
        return parse_export_rerun(args);
    }
    let mut out = default_out();
    let mut options = Options::default();
    let mut names = Vec::new();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--list" => return Ok(Command::List),
            "--out" => out = PathBuf::from(args.next().ok_or(CliError::MissingValue("--out"))?),
            "--rate" => {
                let value = args.next().ok_or(CliError::MissingValue("--rate"))?;
                options.rate_hz = value.parse().map_err(|_| CliError::BadRate(value))?;
            }
            option if option.starts_with('-') => {
                return Err(CliError::UnknownOption(option.to_owned()));
            }
            name => names.push(name.to_owned()),
        }
    }
    Ok(Command::Write {
        out,
        options,
        scripts: names,
    })
}

fn parse_dump(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut format = DumpFormat::Text;
    let mut range = Range::default();
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => format = DumpFormat::JsonLines,
            "--from" => range.from = Some(parse_bound(args.next(), "--from")?),
            "--to" => range.to = Some(parse_bound(args.next(), "--to")?),
            option if option.starts_with('-') => {
                return Err(CliError::UnknownOption(option.to_owned()));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    match <[PathBuf; 1]>::try_from(paths) {
        Ok([path]) => Ok(Command::Dump {
            path,
            format,
            range,
        }),
        Err(_) => Err(CliError::DumpPath),
    }
}

fn parse_scan_view(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut video = None;
    let mut out = None;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--video" => {
                video = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--video"))?,
                ));
            }
            "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--out"))?,
                ));
            }
            option if option.starts_with('-') => {
                return Err(CliError::UnknownOption(option.to_owned()));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    match <[PathBuf; 1]>::try_from(paths) {
        Ok([path]) => Ok(Command::ScanView { path, video, out }),
        Err(_) => Err(CliError::ScanPath),
    }
}

fn parse_estimate_body(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut range = Range::default();
    let mut weights = None;
    let mut out = None;
    let mut lead_s = tracking_synth::estimate::DEFAULT_LEAD_S;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => range.from = Some(parse_bound(args.next(), "--from")?),
            "--to" => range.to = Some(parse_bound(args.next(), "--to")?),
            "--weights" => {
                weights = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--weights"))?,
                ));
            }
            "--lead" => lead_s = f64::from(parse_length(args.next(), "--lead")?),
            "-o" | "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("-o"))?,
                ));
            }
            option if option.starts_with('-') => {
                return Err(CliError::UnknownOption(option.to_owned()));
            }
            path => paths.push(PathBuf::from(path)),
        }
    }
    match (<[PathBuf; 1]>::try_from(paths), out, weights) {
        (Ok([path]), Some(out), Some(weights)) => Ok(Command::EstimateBody {
            path,
            out,
            weights,
            range,
            lead_s,
        }),
        _ => Err(CliError::EstimatePaths),
    }
}

fn parse_export_rerun(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut range = Range::default();
    let mut video = None;
    let mut video_height = None;
    let mut blueprint = true;
    let mut recording_id = None;
    let mut out = None;
    let mut height = DEFAULT_HEIGHT_M;
    let mut eye_height = None;
    let mut tuning = None;
    let mut weights = None;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from" => range.from = Some(parse_bound(args.next(), "--from")?),
            "--to" => range.to = Some(parse_bound(args.next(), "--to")?),
            "--weights" => {
                weights = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--weights"))?,
                ));
            }
            "--video" => {
                video = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("--video"))?,
                ));
            }
            "--video-height" => {
                let value = args
                    .next()
                    .ok_or(CliError::MissingValue("--video-height"))?;
                match value.parse::<u32>() {
                    Ok(height) if height >= 2 => video_height = Some(height),
                    _ => return Err(CliError::BadVideoHeight(value)),
                }
            }
            "--no-blueprint" => blueprint = false,
            "--recording-id" => {
                recording_id = Some(
                    args.next()
                        .ok_or(CliError::MissingValue("--recording-id"))?,
                );
            }
            "-o" | "--out" => {
                out = Some(PathBuf::from(
                    args.next().ok_or(CliError::MissingValue("-o"))?,
                ));
            }
            "--height" => height = parse_length(args.next(), "--height")?,
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
    let file = RigFile::load().map_err(CliError::RigFile)?;
    let mut options = EvalOptions::new(
        height,
        eye_height.unwrap_or(height * DEFAULT_EYE_SHARE),
        file,
    );
    if let Some(tuning) = tuning {
        options.tuning = tuning;
    }
    match (<[PathBuf; 1]>::try_from(paths), out) {
        (Ok([path]), Some(out)) => Ok(Command::ExportRerun {
            path,
            out,
            range,
            video,
            video_height,
            blueprint,
            recording_id,
            options,
            weights,
        }),
        _ => Err(CliError::ExportPaths),
    }
}

/// The player's height `rig-eval` assumes without `--height`, metres.
const DEFAULT_HEIGHT_M: f32 = 1.8;

/// The eye height's share of the height without `--eye-height`.
const DEFAULT_EYE_SHARE: f32 = 0.933;

fn parse_length(value: Option<String>, option: &'static str) -> Result<f32, CliError> {
    let value = value.ok_or(CliError::MissingValue(option))?;
    match value.parse::<f32>() {
        Ok(length) if length.is_finite() && length > 0.0 => Ok(length),
        _ => Err(CliError::BadLength(value)),
    }
}

fn parse_bound(value: Option<String>, option: &'static str) -> Result<Bound, CliError> {
    let value = value.ok_or(CliError::MissingValue(option))?;
    value.parse().map_err(|()| CliError::BadTime(value))
}

fn run(command: Command) -> Result<(), CliError> {
    match command {
        Command::Help => {
            tracing::info!("{USAGE}");
            tracing::info!(
                "writes each script to <dir>/<script>.jc3trk at <hz> frames per second (default \
                 {DEFAULT_RATE_HZ}); <dir> defaults to {}",
                default_out().display()
            );
            Ok(())
        }
        Command::List => {
            for script in scripts::all() {
                println!("{}\t{}", script.name, script.summary);
            }
            Ok(())
        }
        Command::Dump {
            path,
            format,
            range,
        } => {
            let bytes = std::fs::read(&path).map_err(|error| CliError::Read {
                path: path.clone(),
                error,
            })?;
            let recording = tracking_format::decode(&bytes)
                .map_err(|error| CliError::Decode { path, error })?;
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            match dump::dump(&recording, format, range, &mut out)
                .and_then(|()| std::io::Write::flush(&mut out))
            {
                // A reader that stops early (`head`) has all it wanted.
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
                result => result.map_err(CliError::Print),
            }
        }
        Command::ScanView { path, video, out } => scan_view(&path, video, out),
        Command::EstimateBody {
            path,
            out,
            weights,
            range,
            lead_s,
        } => estimate_body(&path, &out, &weights, &range, lead_s),
        Command::RigEval(request) => rig_eval_cmd::run(request),
        Command::ExportRerun {
            path,
            out,
            range,
            video,
            video_height,
            blueprint,
            recording_id,
            options,
            weights,
        } => export_rerun(ExportRequest {
            path,
            out,
            range,
            video,
            video_height,
            blueprint,
            recording_id,
            eval: options,
            weights,
        }),
        Command::Write {
            out,
            options,
            scripts: names,
        } => {
            let chosen: Vec<Script> = if names.is_empty() {
                scripts::all()
            } else {
                names
                    .iter()
                    .map(|name| {
                        scripts::named(name).ok_or_else(|| CliError::UnknownScript(name.clone()))
                    })
                    .collect::<Result<_, _>>()?
            };
            std::fs::create_dir_all(&out).map_err(|error| CliError::Write {
                path: out.clone(),
                error,
            })?;
            for script in &chosen {
                write(script, &out, options)?;
            }
            Ok(())
        }
    }
}

fn write(script: &Script, out: &Path, options: Options) -> Result<(), CliError> {
    let bytes = tracking_synth::generate(script, options).map_err(|error| CliError::Synth {
        script: script.name,
        error,
    })?;
    let path = out.join(format!("{}.{}", script.name, tracking_format::EXTENSION));
    std::fs::write(&path, &bytes).map_err(|error| CliError::Write {
        path: path.clone(),
        error,
    })?;
    tracing::info!(
        script = script.name,
        seconds = %format_args!("{:.2}", script.duration_s()),
        kib = bytes.len() / 1024,
        "wrote {}",
        path.display()
    );
    Ok(())
}

/// What `export-rerun` was asked for.
#[cfg_attr(not(feature = "rerun"), allow(dead_code))]
struct ExportRequest {
    path: PathBuf,
    out: PathBuf,
    range: Range,
    video: Option<PathBuf>,
    video_height: Option<u32>,
    blueprint: bool,
    recording_id: Option<String>,
    eval: EvalOptions,
    weights: Option<PathBuf>,
}

#[cfg(feature = "rerun")]
fn export_rerun(request: ExportRequest) -> Result<(), CliError> {
    let ExportRequest {
        path,
        out,
        range,
        video,
        video_height,
        blueprint,
        recording_id,
        eval,
        weights,
    } = request;
    let bytes = std::fs::read(&path).map_err(|error| CliError::Read {
        path: path.clone(),
        error,
    })?;
    let recording = tracking_format::decode(&bytes).map_err(|error| CliError::Decode {
        path: path.clone(),
        error,
    })?;
    drop(bytes);
    let clock = video_clock(&path);
    let estimate = weights
        .map(|weights| estimate_track(&weights, &recording))
        .transpose()?;
    let options = ExportOptions {
        range,
        video: video.map(|path| VideoSource {
            path,
            height: video_height.unwrap_or(DEFAULT_VIDEO_HEIGHT),
        }),
        eval,
        source: path,
        blueprint,
        recording_id,
        estimate,
    };
    let summary = rerun_export::export(&recording, clock.as_ref(), &options, &out)
        .map_err(CliError::Export)?;
    tracing::info!(
        frames = summary.frames,
        events = summary.events,
        rig_rows = summary.rig_rows,
        video_frames = summary.video_frames,
        wall_clock = summary.wall_clock,
        median_wrist_error_m = ?summary.median_wrist_error_m,
        "export-rerun: wrote {}",
        out.display()
    );
    Ok(())
}

#[cfg(not(feature = "rerun"))]
fn export_rerun(_request: ExportRequest) -> Result<(), CliError> {
    Err(CliError::NoRerun)
}

/// Run EgoPoser with the weights at `weights` over the whole of `recording`, for the offline rig.
fn estimate_track(
    weights: &Path,
    recording: &tracking_format::Recording,
) -> Result<EstimateTrack, CliError> {
    let model = body_estimator::EgoPoser::load(weights).map_err(CliError::Weights)?;
    let (track, summary) = EstimateTrack::compute(&recording.frames, &model, EstimateCadence::GAME);
    tracing::info!(
        inferences = summary.inferences,
        weights = %weights.display(),
        "rig-eval: the body estimator ran over the recording"
    );
    Ok(track)
}

/// Run EgoPoser over a stretch of the recording at `path` and write its estimate to `out` as a CSV.
fn estimate_body(
    path: &Path,
    out: &Path,
    weights: &Path,
    range: &Range,
    lead_s: f64,
) -> Result<(), CliError> {
    let model = body_estimator::EgoPoser::load(weights).map_err(CliError::Weights)?;
    let bytes = std::fs::read(path).map_err(|error| CliError::Read {
        path: path.to_owned(),
        error,
    })?;
    let recording = tracking_format::decode(&bytes).map_err(|error| CliError::Decode {
        path: path.to_owned(),
        error,
    })?;
    drop(bytes);
    let (from, to) = range
        .resolve(&recording.frames)
        .ok_or(CliError::EmptyRange)?;
    let last = recording.frames.last().map_or(0, |frame| frame.time_ns);
    let stretch = (from as f64 / 1.0e9, to.min(last) as f64 / 1.0e9);
    let (rows, summary) = estimate::estimate(&recording.frames, &model, stretch, lead_s);
    let write = |error| CliError::Write {
        path: out.to_owned(),
        error,
    };
    let mut file = std::io::BufWriter::new(std::fs::File::create(out).map_err(write)?);
    estimate::write_csv(&rows, &mut file).map_err(write)?;
    let per_inference_ms = if summary.inferences > 0 {
        summary.inference.as_secs_f64() * 1.0e3 / summary.inferences as f64
    } else {
        0.0
    };
    tracing::info!(
        rows = rows.len(),
        per_inference_ms = %format_args!("{per_inference_ms:.3}"),
        "estimate-body: wrote {}",
        out.display()
    );
    Ok(())
}

/// The video sidecar beside a take's recording, when there is one.
/// Scan the take's video beside `path` (or `video`) and write the report beside it (or to `out`).
fn scan_view(path: &Path, video: Option<PathBuf>, out: Option<PathBuf>) -> Result<(), CliError> {
    let take = path.parent().unwrap_or(Path::new("."));
    let video = video.unwrap_or_else(|| take.join("video.mkv"));
    let out = out.unwrap_or_else(|| take.join("view-scan.md"));
    let bytes = std::fs::read(path).map_err(|error| CliError::Read {
        path: path.to_owned(),
        error,
    })?;
    let recording = tracking_format::decode(&bytes).map_err(|error| CliError::Decode {
        path: path.to_owned(),
        error,
    })?;
    let clock = video_clock(path);
    if clock.is_none() {
        tracing::warn!(
            "scan-view: the take has no video sidecar; the video's clock is taken as the \
             recording's"
        );
    }
    tracing::info!("scan-view: scanning {}", video.display());
    let scan = view_scan::scan(&recording, clock.as_ref(), &ScanOptions::new(video))
        .map_err(CliError::Scan)?;
    let file = std::fs::File::create(&out).map_err(|error| CliError::Write {
        path: out.clone(),
        error,
    })?;
    let mut writer = std::io::BufWriter::new(file);
    view_scan::write_report(&scan, path, &mut writer)
        .and_then(|()| std::io::Write::flush(&mut writer))
        .map_err(|error| CliError::Write {
            path: out.clone(),
            error,
        })?;
    let behind = scan
        .flashes
        .iter()
        .filter(|flash| {
            flash
                .camera
                .as_ref()
                .is_some_and(view_scan::CameraNear::behind)
        })
        .count();
    tracing::info!(
        frames = scan.frames,
        flashes = scan.flashes.len(),
        behind_head = behind,
        "scan-view: wrote {}",
        out.display()
    );
    Ok(())
}

fn video_clock(recording: &Path) -> Option<VideoClock> {
    let dir = recording.parent()?;
    let frames = std::fs::read_to_string(dir.join("frames.jsonl")).ok()?;
    let stream = std::fs::read_to_string(dir.join("stream.json")).ok()?;
    VideoClock::parse(&frames, &stream)
}

/// Where the synthetic recordings go by default: `target/tracking-synth/` in the repository this tool
/// was built from, beside the build output and as disposable.
fn default_out() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // The tool's manifest is two levels below the repository's root.
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .join("target/tracking-synth")
}
