//! `tracking-synth`: writes the built-in synthetic tracking recordings.
//!
//! ```text
//! tracking-synth [--out <dir>] [--rate <hz>] [<script>...]
//! tracking-synth --list
//! ```
//!
//! With no script named, every built-in script is written. The recordings go to `<dir>/<script>.jc3trk`,
//! by default `testing/recordings/synthetic/` in the repository, where the synthetic scenarios look
//! for them. `--list` prints each built-in script's name and summary, tab-separated, one per line.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::ExitCode,
};

use tracing_subscriber::EnvFilter;
use tracking_synth::{
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

const USAGE: &str = "usage: tracking-synth [--out <dir>] [--rate <hz>] [<script>...] | --list";

/// What the command line asks for.
#[derive(Debug, Clone, PartialEq)]
enum Command {
    Help,
    List,
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
    /// A script that is not built in.
    UnknownScript(String),
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
            CliError::UnknownScript(name) => write!(
                f,
                "there is no built-in script `{name}`; `--list` lists them"
            ),
            CliError::Synth { script, error } => write!(f, "{script}: {error}"),
            CliError::Write { path, error } => {
                write!(f, "could not write {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for CliError {}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, CliError> {
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
