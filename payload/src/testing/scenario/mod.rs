//! The scenario runner: replays a scenario's recording and checks its expectations against the
//! published signals ([`crate::testing::signals`]), the `VR: STATUS` page's rows, and the log
//! ([`crate::testing::log_tap`]) on every frame of the replay and for a settling time after it,
//! then writes PASS or FAIL for each expectation to the session log and a summary file in the
//! session folder.
//!
//! The runner is driven from the game thread once per frame ([`update`]), after the replay's own
//! update, outside the VR runtime lock. A frame counts once the replay plays: the load and the
//! teleport to the start are not observed, and their log lines are dropped. The file format is
//! [`file`]; see `docs/mod/testing/recorder.md`.

pub(crate) mod expect;
pub(crate) mod file;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Serialize;

use crate::{
    config::Config,
    testing::{
        log_tap::{self, LogLine},
        replay::{self, ReplayOutcome, RunId},
        scenario::{
            expect::{Tracker, Verdict},
            file::{Expectation, Probe, Scenario},
        },
        signals::{Value, status_row},
    },
};

/// One expectation's verdict, live or final.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ExpectationReport {
    pub(crate) name: String,
    pub(crate) verdict: Verdict,
}

/// A finished scenario's results.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Report {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    /// Why the replay did not run to its end, if it did not.
    pub(crate) aborted: Option<String>,
    pub(crate) frames: u32,
    pub(crate) results: Vec<ExpectationReport>,
    /// Log lines the tap's queue lost, which the log probes did not see.
    pub(crate) lost_log_lines: u64,
    /// The summary file, when it was written.
    pub(crate) summary: Option<PathBuf>,
}

impl Report {
    pub(crate) fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.verdict.passed).count()
    }
}

/// The runner's state for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScenarioStatus {
    /// The running scenario's name and its expectations' verdicts so far.
    pub(crate) running: Option<(String, Vec<ExpectationReport>)>,
    pub(crate) last: Option<Report>,
    /// Why the last scenario could not start, if it could not.
    pub(crate) error: Option<String>,
}

/// Ask for the scenario file at `path` to run, replacing any that runs. Any thread; serviced on
/// the next frame.
pub(crate) fn start(path: PathBuf) {
    let mut state = STATE.lock();
    state.last_path = Some(path.clone());
    state.pending = Some(Pending::Start(path));
}

/// Ask for the running scenario to stop. Any thread.
pub(crate) fn stop() {
    STATE.lock().pending = Some(Pending::Stop);
}

/// Whether a scenario runs.
pub(crate) fn is_running() -> bool {
    STATE.lock().running.is_some()
}

/// The scenario file most recently asked to run.
pub(crate) fn last_path() -> Option<PathBuf> {
    STATE.lock().last_path.clone()
}

/// The runner's state.
pub(crate) fn status() -> ScenarioStatus {
    let state = STATE.lock();
    ScenarioStatus {
        running: state.running.as_ref().map(|run| {
            (
                run.scenario.name.clone(),
                run.expectations.iter().map(Observed::report).collect(),
            )
        }),
        last: state.last.clone(),
        error: state.error.clone(),
    }
}

/// The folder the scenario files are in: `testing.scenario_dir`, or the `testing` folder in the
/// configuration directory.
pub(crate) fn folder() -> Option<PathBuf> {
    let configured = Config::lock_query(|c| c.testing.scenario_dir.trim().to_owned());
    if configured.is_empty() {
        crate::config::paths::file("testing")
    } else {
        Some(PathBuf::from(configured))
    }
}

/// The scenario files in [`folder`], by name.
pub(crate) fn list() -> Vec<PathBuf> {
    let Some(folder) = folder() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .collect();
    files.sort();
    files
}

/// Service the requests and observe the running scenario's frame. Game thread, once per frame,
/// after the replay's update.
pub(crate) fn update() {
    let mut state = STATE.lock();
    match state.pending.take() {
        Some(Pending::Start(path)) => {
            state.abort("another scenario replaced it");
            state.begin(&path);
        }
        Some(Pending::Stop) => state.abort("it was stopped"),
        None => {}
    }
    let Some(run) = state.running.as_mut() else {
        return;
    };
    let (lines, lost) = log_tap::drain();
    run.lost_log_lines += lost;
    let next = match replay::outcome(run.replay) {
        None if replay::is_driving() => {
            run.observe(&lines);
            Next::Continue
        }
        None if !replay::is_active() && run.requested.elapsed() > REQUEST_GRACE => Next::Abort(
            "the replay ended without an outcome; another replay replaced it".to_owned(),
        ),
        None => Next::Continue,
        Some(ReplayOutcome::Completed { .. }) => {
            run.observe(&lines);
            let until = *run
                .settle_until
                .get_or_insert_with(|| Instant::now() + run.scenario.settle);
            if Instant::now() >= until {
                Next::Finish
            } else {
                Next::Continue
            }
        }
        Some(ReplayOutcome::Stopped) => Next::Abort("the replay was stopped".to_owned()),
        Some(ReplayOutcome::Failed(error)) => Next::Abort(format!("the replay failed: {error}")),
    };
    match next {
        Next::Continue => {}
        Next::Finish => state.finish(None),
        Next::Abort(reason) => state.abort(&reason),
    }
}

/// How long after a scenario's request the replay may take to become active before its absence
/// means another replay replaced it.
const REQUEST_GRACE: Duration = Duration::from_secs(2);

enum Pending {
    Start(PathBuf),
    Stop,
}

/// What [`update`] does with the running scenario after this frame.
enum Next {
    Continue,
    Finish,
    Abort(String),
}

struct State {
    pending: Option<Pending>,
    running: Option<Run>,
    last: Option<Report>,
    error: Option<String>,
    last_path: Option<PathBuf>,
}

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    running: None,
    last: None,
    error: None,
    last_path: None,
});

struct Run {
    scenario: Scenario,
    replay: RunId,
    requested: Instant,
    started_at: String,
    expectations: Vec<Observed>,
    /// Frames observed.
    frames: u32,
    lost_log_lines: u64,
    /// When the settling after the replay ends.
    settle_until: Option<Instant>,
}

/// One expectation and its record.
struct Observed {
    expectation: Expectation,
    tracker: Tracker,
    /// A counter's value on the first frame, for a delta.
    baseline: Option<f64>,
    /// A log probe's matches so far.
    log_matches: u64,
}

impl Observed {
    fn report(&self) -> ExpectationReport {
        ExpectationReport {
            name: self.expectation.name.clone(),
            verdict: self.tracker.verdict(),
        }
    }

    /// Read this frame's value and fold it in. `lines` are the log lines since the last frame.
    fn observe(&mut self, lines: &[LogLine]) {
        let value = match &self.expectation.probe {
            Probe::Signal { signal, delta } => match (signal.read(), delta) {
                (Value::Number(n), true) => Value::Number(n - *self.baseline.get_or_insert(n)),
                (value, _) => value,
            },
            Probe::Status {
                label,
                health,
                detail,
            } => match status_row(label) {
                Some(line) => Value::Bool(
                    health.is_none_or(|h| line.health == h)
                        && detail.as_deref().is_none_or(|d| line.detail.contains(d)),
                ),
                None => Value::Missing,
            },
            Probe::Log { pattern, level } => {
                self.log_matches += lines
                    .iter()
                    .filter(|line| {
                        // `Level` orders the most verbose highest, so a line at a level or above
                        // is one that compares at or below it.
                        level.is_none_or(|level| line.level <= level)
                            && line.text.contains(pattern.as_str())
                    })
                    .count() as u64;
                Value::Number(self.log_matches as f64)
            }
        };
        self.tracker.observe(value);
    }
}

impl Run {
    fn observe(&mut self, lines: &[LogLine]) {
        self.frames += 1;
        for expectation in &mut self.expectations {
            expectation.observe(lines);
        }
    }
}

impl State {
    fn begin(&mut self, path: &Path) {
        self.error = None;
        if !Config::lock_query(|c| c.testing.enabled) {
            return self.refuse(
                "testing: scenario: the test tools are switched off (testing.enabled)".to_owned(),
            );
        }
        let scenario = match file::read(path) {
            Ok(scenario) => scenario,
            Err(error) => return self.refuse(error.to_string()),
        };
        tracing::info!(
            expectations = scenario.expectations.len(),
            "scenario: {}: replaying {}",
            scenario.name,
            scenario.recording.display()
        );
        if let Some(description) = &scenario.description {
            tracing::info!("scenario: {}: {description}", scenario.name);
        }
        let replay = replay::start(scenario.recording.clone(), scenario.options);
        log_tap::start();
        self.running = Some(Run {
            expectations: scenario
                .expectations
                .iter()
                .map(|expectation| Observed {
                    expectation: expectation.clone(),
                    tracker: Tracker::new(expectation.check, expectation.condition),
                    baseline: None,
                    log_matches: 0,
                })
                .collect(),
            scenario,
            replay,
            requested: Instant::now(),
            started_at: crate::session::stamp(),
            frames: 0,
            lost_log_lines: 0,
            settle_until: None,
        });
    }

    fn refuse(&mut self, error: String) {
        tracing::warn!("{error}");
        self.error = Some(error);
    }

    /// End the running scenario, if any, before its replay finished.
    fn abort(&mut self, reason: &str) {
        if self.running.is_some() {
            if replay::is_active() {
                replay::stop();
            }
            self.finish(Some(reason.to_owned()));
        }
    }

    /// Give the running scenario its verdicts, log them, and write the summary.
    fn finish(&mut self, aborted: Option<String>) {
        let Some(run) = self.running.take() else {
            return;
        };
        log_tap::stop();
        let name = &run.scenario.name;
        let results: Vec<ExpectationReport> = run
            .expectations
            .iter()
            .map(|observed| match &aborted {
                Some(reason) => ExpectationReport {
                    name: observed.expectation.name.clone(),
                    verdict: Verdict {
                        passed: false,
                        detail: format!("not checked: {reason}"),
                    },
                },
                None => observed.report(),
            })
            .collect();
        for result in &results {
            if result.verdict.passed {
                tracing::info!("scenario: {name}: {}: {}", result.verdict, result.name);
            } else {
                tracing::warn!("scenario: {name}: {}: {}", result.verdict, result.name);
            }
        }
        let mut report = Report {
            name: name.clone(),
            path: run.scenario.path.clone(),
            aborted,
            frames: run.frames,
            results,
            lost_log_lines: run.lost_log_lines,
            summary: None,
        };
        let passed = report.passed();
        let total = report.results.len();
        if let Some(reason) = &report.aborted {
            tracing::warn!("scenario: {name}: FAIL: {reason}; 0 of {total} expectations checked");
        } else if passed == total {
            tracing::info!(
                frames = report.frames,
                "scenario: {name}: PASS: {passed} of {total} expectations passed"
            );
        } else {
            tracing::warn!(
                frames = report.frames,
                "scenario: {name}: FAIL: {passed} of {total} expectations passed"
            );
        }
        if report.lost_log_lines > 0 {
            tracing::warn!(
                lost = report.lost_log_lines,
                "scenario: {name}: the log tap lost lines; the log expectations may undercount"
            );
        }
        match write_summary(&run, &report) {
            Ok(path) => {
                tracing::info!("scenario: {name}: the summary is at {}", path.display());
                report.summary = Some(path);
            }
            Err(error) => tracing::warn!("{error}"),
        }
        self.last = Some(report);
    }
}

/// Why the summary file could not be written.
#[derive(Debug)]
enum SummaryError {
    NoSessionFolder,
    Folder(std::io::Error),
    Serialize(toml::ser::Error),
    Write {
        path: PathBuf,
        error: std::io::Error,
    },
}

impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "scenario: summary: ")?;
        match self {
            SummaryError::NoSessionFolder => write!(f, "the session folder is unavailable"),
            SummaryError::Folder(e) => {
                write!(f, "could not create the session's testing folder: {e}")
            }
            SummaryError::Serialize(e) => write!(f, "could not serialize it: {e}"),
            SummaryError::Write { path, error } => {
                write!(f, "could not write {}: {error}", path.display())
            }
        }
    }
}

/// The summary file's contents.
#[derive(Serialize)]
struct Summary<'a> {
    scenario: &'a str,
    file: String,
    recording: String,
    started_at: &'a str,
    build: &'a str,
    passed: bool,
    passed_count: usize,
    total: usize,
    frames: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    aborted: Option<&'a str>,
    lost_log_lines: u64,
    expectation: Vec<SummaryExpectation<'a>>,
}

#[derive(Serialize)]
struct SummaryExpectation<'a> {
    name: &'a str,
    passed: bool,
    detail: &'a str,
}

/// Write the summary to the session's `testing` folder. A small file once per scenario, so it is
/// written on the game thread.
fn write_summary(run: &Run, report: &Report) -> Result<PathBuf, SummaryError> {
    let folder = crate::session::subdir("testing")
        .ok_or(SummaryError::NoSessionFolder)?
        .map_err(SummaryError::Folder)?;
    let stem = run.scenario.path.file_stem().map_or_else(
        || "scenario".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    );
    let path = folder.join(format!("{}_{stem}.toml", run.started_at));
    let summary = Summary {
        scenario: &report.name,
        file: report.path.display().to_string(),
        recording: run.scenario.recording.display().to_string(),
        started_at: &run.started_at,
        build: crate::BUILD_STAMP,
        passed: report.aborted.is_none() && report.passed() == report.results.len(),
        passed_count: report.passed(),
        total: report.results.len(),
        frames: report.frames,
        aborted: report.aborted.as_deref(),
        lost_log_lines: report.lost_log_lines,
        expectation: report
            .results
            .iter()
            .map(|r| SummaryExpectation {
                name: &r.name,
                passed: r.verdict.passed,
                detail: &r.verdict.detail,
            })
            .collect(),
    };
    let text = toml::to_string(&summary).map_err(SummaryError::Serialize)?;
    std::fs::write(&path, text).map_err(|error| SummaryError::Write {
        path: path.clone(),
        error,
    })?;
    Ok(path)
}
