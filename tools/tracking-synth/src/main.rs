//! `tracking-synth`: writes the built-in synthetic tracking recordings.
//!
//! ```text
//! tracking-synth [--out <dir>] [--rate <hz>] [<script>...]
//! tracking-synth --list
//! tracking-synth dump [--json] [--from <time>] [--to <time>] <recording>
//! tracking-synth rig-eval [--height <m>] [--eye-height <m>] [--tuning <json>] <recording> <out.csv>
//! ```
//!
//! With no script named, every built-in script is written. The recordings go to `<dir>/<script>.jc3trk`,
//! by default `testing/recordings/synthetic/` in the repository, where the synthetic scenarios look
//! for them. `--list` prints each built-in script's name and summary, tab-separated, one per line.
//! `dump` prints any recording's frames and events to stdout, as text or, with `--json`, as JSON
//! lines. `--from` and `--to` limit it to an inclusive stretch; a `<time>` is seconds on the
//! recording's clock (`12.5`) or an OpenXR predicted display time in nanoseconds (`xr:<ns>`), the
//! clock a video recording's sidecar shares. `rig-eval` runs the grounded body rig over a recording
//! and writes a CSV of each grounded frame (`rig_eval`); `--tuning` takes the rig's tuning as JSON,
//! any field left out keeping its default, and a video sidecar (`frames.jsonl` and `stream.json`)
//! beside the recording adds the video's time to each row.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::ExitCode,
};

use tracing_subscriber::EnvFilter;
use tracking_synth::{
    dump::{self, Bound, DumpFormat, Range},
    rig_eval::{self, EvalOptions, VideoClock},
    script::Script,
    scripts,
    synth::{DEFAULT_RATE_HZ, Options, SynthError},
};

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
                     [--height <m>] [--eye-height <m>] [--tuning <json>] <recording> <out.csv>, \
                     where <time> is seconds or xr:<display time ns>";

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
    RigEval {
        path: PathBuf,
        out: PathBuf,
        options: EvalOptions,
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
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tracking-synth: ")?;
        match self {
            CliError::UnknownOption(option) => write!(f, "there is no option `{option}`"),
            CliError::MissingValue(option) => write!(f, "`{option}` needs a value"),
            CliError::BadRate(value) => write!(f, "the frame rate `{value}` is not a number"),
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
            CliError::Read { path, error } => {
                write!(f, "dump: could not read {}: {error}", path.display())
            }
            CliError::Decode { path, error } => write!(f, "dump: {}: {error}", path.display()),
            CliError::Print(error) => write!(f, "dump: could not print: {error}"),
            CliError::Synth { script, error } => write!(f, "{script}: {error}"),
            CliError::Write { path, error } => {
                write!(f, "could not write {}: {error}", path.display())
            }
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
        return parse_rig_eval(args);
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

fn parse_rig_eval(mut args: impl Iterator<Item = String>) -> Result<Command, CliError> {
    let mut height = DEFAULT_HEIGHT_M;
    let mut eye_height = None;
    let mut tuning = None;
    let mut paths = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
    let mut options = EvalOptions::new(height, eye_height.unwrap_or(height * DEFAULT_EYE_SHARE));
    if let Some(tuning) = tuning {
        options.tuning = tuning;
    }
    match <[PathBuf; 2]>::try_from(paths) {
        Ok([path, out]) => Ok(Command::RigEval { path, out, options }),
        Err(_) => Err(CliError::RigEvalPaths),
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
        Command::RigEval { path, out, options } => {
            let bytes = std::fs::read(&path).map_err(|error| CliError::Read {
                path: path.clone(),
                error,
            })?;
            let recording = tracking_format::decode(&bytes).map_err(|error| CliError::Decode {
                path: path.clone(),
                error,
            })?;
            let clock = video_clock(&path);
            let (rows, summary) = rig_eval::evaluate(&recording, &options, clock.as_ref());
            let file = std::fs::File::create(&out).map_err(|error| CliError::Write {
                path: out.clone(),
                error,
            })?;
            let mut writer = std::io::BufWriter::new(file);
            rig_eval::write_csv(&rows, &mut writer)
                .and_then(|()| std::io::Write::flush(&mut writer))
                .map_err(|error| CliError::Write {
                    path: out.clone(),
                    error,
                })?;
            tracing::info!(
                rows = rows.len(),
                video = clock.is_some(),
                grounded_s = %format_args!("{:.1}", summary.grounded_s),
                steps = summary.steps,
                driving_s = %format_args!("{:.1}", summary.driving_s),
                turns = summary.turns,
                mean_yaw_error_deg = %format_args!("{:.1}", summary.mean_yaw_error_deg),
                mean_offset_m = %format_args!("{:.3}", summary.mean_offset_m),
                max_offset_m = %format_args!("{:.3}", summary.max_offset_m),
                recenters = summary.recenters,
                "rig-eval: wrote {}",
                out.display()
            );
            Ok(())
        }
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

/// The video sidecar beside a take's recording, when there is one.
fn video_clock(recording: &Path) -> Option<VideoClock> {
    let dir = recording.parent()?;
    let frames = std::fs::read_to_string(dir.join("frames.jsonl")).ok()?;
    let stream = std::fs::read_to_string(dir.join("stream.json")).ok()?;
    VideoClock::parse(&frames, &stream)
}

/// Where the synthetic scenarios look for their recordings: `testing/recordings/synthetic/` in the
/// repository this tool was built from.
fn default_out() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    // The tool's manifest is two levels below the repository's root.
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .join("testing/recordings/synthetic")
}
