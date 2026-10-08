//! Replay: drive the game with a tracking recording ([`crate::recorder::tracking`]) in place of
//! the live OpenXR runtime's poses and inputs.
//!
//! A replay loads its recording on a thread of its own, teleports the player to the recording's
//! start marker through the director and waits for the teleport to settle, then plays: on each
//! rendered frame, at the top of the render update, [`begin_frame`] works out which recorded
//! frames have come due ([`cursor`]) and stages them for the seams that would otherwise publish
//! the runtime's answers:
//!
//! - The eye views: with a session running, the frame loop replaces the located views' cockpit
//!   poses with the recorded ones ([`head`]), keeping the live poses for the compositor, so the
//!   headset (on or off a head) shows what the recording saw. With no session, the render update
//!   publishes the recorded head through the same path a VR frame takes, less the per-eye render
//!   parameters, so the desktop renders the recorded view.
//! - The controllers and the semantic actions: the input update publishes the recorded frames
//!   ([`take_frames`]) instead of sampling the controllers, or the render update does with no
//!   session.
//! - The STAGE floor height ([`stage`]) and the recenters the recording saw.
//!
//! Every consumer reads the published state, so every consumer sees the replay. The headpose
//! source is the VR source while a replay plays, with or without a session. Live recenters are
//! held off meanwhile; the recorded ones are applied as they come due.
//!
//! The state is driven from the game thread: [`update`] from the top of the game's update, and
//! [`begin_frame`] from the top of its render update, before the frame takes the VR runtime lock.
//! The seams read the staged step under that lock through a leaf slot. See
//! `docs/mod/testing/recorder.md`.

pub(crate) mod cursor;
mod load;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(crate) use cursor::Pacing;
use glam::Vec3;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    controllers::frame as controller_frame,
    director::teleport::{self, TeleportMode},
    recorder::tracking::format::{
        ControllerSample, DecodeError, HeadSample, RecenterEvent, SemanticSample,
    },
    testing::replay::{
        cursor::Cursor,
        load::{Expected, Loaded},
    },
    vr::{input::semantic::frame as semantic_frame, stage::StageInfo},
};

/// Where a replay starts.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartMode {
    /// At the recording's start marker: the player's transform when it started.
    #[default]
    Marker,
    /// At the director bookmark the start marker names, as the bookmark stands now, or at the
    /// marker when it names none or the bookmark is gone.
    Bookmark,
    /// Where the player is.
    InPlace,
}

impl StartMode {
    pub(crate) const ALL: [StartMode; 3] =
        [StartMode::Marker, StartMode::Bookmark, StartMode::InPlace];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            StartMode::Marker => "at the start marker",
            StartMode::Bookmark => "at the bookmark",
            StartMode::InPlace => "in place",
        }
    }
}

/// How a replay runs.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct ReplayOptions {
    pub(crate) start: StartMode,
    pub(crate) pacing: Pacing,
}

/// A replay request's identity, to match it with its outcome.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct RunId(u32);

/// How a replay ended.
#[derive(Debug, Clone)]
pub(crate) enum ReplayOutcome {
    /// Every recorded frame was published.
    Completed {
        frames: usize,
        duration: Duration,
    },
    /// It was stopped, or another replay replaced it.
    Stopped,
    Failed(ReplayError),
}

/// Why a replay could not run.
#[derive(Debug, Clone)]
pub(crate) enum ReplayError {
    /// The test tools are switched off.
    Disabled,
    /// A tracking recording is running.
    Recording,
    Read {
        path: PathBuf,
        error: Arc<std::io::Error>,
    },
    Decode {
        path: PathBuf,
        error: DecodeError,
    },
    /// The recording holds no frames.
    Empty {
        path: PathBuf,
    },
    LoaderThread(Arc<std::io::Error>),
    /// The loader thread panicked.
    LoaderPanicked {
        path: PathBuf,
    },
    /// The teleport to the start did not settle in time.
    TeleportTimeout {
        seconds: f32,
        /// How far the player was from the start when it gave up.
        distance_m: Option<f32>,
    },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay: ")?;
        match self {
            ReplayError::Disabled => write!(f, "the test tools are switched off (testing.enabled)"),
            ReplayError::Recording => write!(f, "a tracking recording is running"),
            ReplayError::Read { path, error } => {
                write!(f, "could not read {}: {error}", path.display())
            }
            ReplayError::Decode { path, error } => write!(f, "{}: {error}", path.display()),
            ReplayError::Empty { path } => write!(f, "{} holds no frames", path.display()),
            ReplayError::LoaderThread(error) => {
                write!(f, "could not spawn the loader thread: {error}")
            }
            ReplayError::LoaderPanicked { path } => {
                write!(f, "the loader panicked reading {}", path.display())
            }
            ReplayError::TeleportTimeout {
                seconds,
                distance_m,
            } => {
                write!(
                    f,
                    "the teleport to the start did not settle within {seconds:.0} s"
                )?;
                match distance_m {
                    Some(d) => write!(f, " (the player is {d:.1} m from it)"),
                    None => write!(f, " (there is no player character)"),
                }
            }
        }
    }
}

impl std::error::Error for ReplayError {}

/// Where a replay is.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Loading,
    Teleporting,
    Playing,
}

/// A replay's state for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct ReplayStatus {
    pub(crate) phase: Phase,
    pub(crate) path: Option<PathBuf>,
    /// The next recorded frame to publish, and how many there are.
    pub(crate) frame: usize,
    pub(crate) frames: usize,
    pub(crate) elapsed: Duration,
    pub(crate) warnings: Vec<String>,
    pub(crate) options: ReplayOptions,
    /// How the last replay ended.
    pub(crate) last: Option<ReplayOutcome>,
}

/// One recorded frame for the input seam to publish.
#[derive(Debug, Clone)]
pub(crate) struct ReplayedFrame {
    /// The display time to stamp the controller frame with: the recording's own clock, so the
    /// hands' motion history spaces the frames as they were recorded.
    pub(crate) display_time_ns: i64,
    pub(crate) controllers: Option<ControllerSample>,
    pub(crate) semantic: Option<SemanticSample>,
}

/// Ask for the recording at `path` to be replayed, replacing any replay that runs. Any thread;
/// serviced at the top of the next frame.
pub(crate) fn start(path: PathBuf, options: ReplayOptions) -> RunId {
    let mut state = STATE.lock();
    state.next_run = state.next_run.wrapping_add(1);
    let run = RunId(state.next_run);
    state.pending = Some(Request { run, path, options });
    run
}

/// Ask for the running replay to stop. Any thread.
pub(crate) fn stop() {
    STATE.lock().stop = true;
}

/// Whether a replay is loading, teleporting, or playing.
pub(crate) fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Whether a replay is playing, so its frames stand in for the runtime's.
pub(crate) fn is_driving() -> bool {
    DRIVING.load(Ordering::Relaxed)
}

/// The replay's state.
pub(crate) fn status() -> ReplayStatus {
    let state = STATE.lock();
    let mut status = ReplayStatus {
        last: state.last.as_ref().map(|(_, outcome)| outcome.clone()),
        ..ReplayStatus::default()
    };
    match &state.phase {
        PhaseState::Idle => {}
        PhaseState::Loading { request, .. } => {
            status.phase = Phase::Loading;
            status.path = Some(request.path.clone());
            status.options = request.options;
        }
        PhaseState::Teleporting { run, loaded, .. } | PhaseState::Playing { run, loaded, .. } => {
            status.phase = match &state.phase {
                PhaseState::Teleporting { .. } => Phase::Teleporting,
                _ => Phase::Playing,
            };
            status.path = Some(loaded.path.clone());
            status.frames = loaded.recording.frames.len();
            status.warnings = loaded.warnings.clone();
            status.options = run.options;
            if let PhaseState::Playing {
                cursor, started, ..
            } = &state.phase
            {
                status.frame = cursor.position();
                status.elapsed = started.map_or(Duration::ZERO, |s| s.elapsed());
            }
        }
    }
    status
}

/// How the replay `run` ended, once it has.
pub(crate) fn outcome(run: RunId) -> Option<ReplayOutcome> {
    STATE
        .lock()
        .last
        .as_ref()
        .filter(|(last, _)| *last == run)
        .map(|(_, outcome)| outcome.clone())
}

/// Service the requests and move the replay through its phases. Game thread, once per frame from
/// the top of the game's update, after the director's pass.
pub(crate) fn update() {
    let mut state = STATE.lock();
    if std::mem::take(&mut state.stop) {
        state.finish(ReplayOutcome::Stopped);
    }
    if let Some(request) = state.pending.take() {
        state.finish(ReplayOutcome::Stopped);
        state.begin(request);
    }
    state.advance();
    let (active, driving) = match state.phase {
        PhaseState::Idle => (false, false),
        PhaseState::Playing { .. } => (true, true),
        _ => (true, false),
    };
    ACTIVE.store(active, Ordering::Relaxed);
    DRIVING.store(driving, Ordering::Relaxed);
}

/// Stage this rendered frame's recorded frames for the seams, and apply the recenters among them.
/// Game thread, at the top of the render update before the frame takes the VR runtime lock.
/// Returns whether the replay drives this frame.
pub(crate) fn begin_frame() -> bool {
    let mut state = STATE.lock();
    let PhaseState::Playing {
        run,
        loaded,
        started,
        cursor,
        head,
    } = &mut state.phase
    else {
        *STEP.lock() = Step::default();
        return false;
    };
    let now = Instant::now();
    let started = *started.get_or_insert_with(|| {
        // The first frame after the live state: publishing on top of it would fold the live
        // buttons into the recording's first edges.
        controller_frame::clear();
        semantic_frame::clear();
        now
    });
    let elapsed =
        u64::try_from(now.saturating_duration_since(started).as_nanos()).unwrap_or(u64::MAX);
    let range = cursor.advance(&loaded.times, elapsed, run.options.pacing);
    let due = &loaded.recording.frames[range];
    let mut recenters: Vec<RecenterEvent> = Vec::new();
    let mut frames = Vec::with_capacity(due.len());
    for frame in due {
        if let Some(sample) = frame.head {
            *head = Some(sample);
        }
        recenters.extend_from_slice(&frame.recenters);
        frames.push(ReplayedFrame {
            display_time_ns: DISPLAY_TIME_ORIGIN_NS
                .saturating_add(i64::try_from(frame.time_ns).unwrap_or(i64::MAX)),
            controllers: frame.controllers.clone(),
            semantic: frame.semantic.clone().filter(|_| loaded.semantic),
        });
    }
    {
        let mut step = STEP.lock();
        step.head = *head;
        if let Some(last) = due.last() {
            step.stage = Some(last.stage);
        }
        step.frames = (!frames.is_empty()).then_some(frames);
    }
    drop(state);
    for event in recenters {
        crate::vr::apply_replayed_recenter(event.kind, event.cause, event.turn);
    }
    true
}

/// The recorded head for this frame: the latest recorded frame with located views, at or before
/// this one. `None` while no replay plays or before its first located views.
pub(crate) fn head() -> Option<HeadSample> {
    STEP.lock().head
}

/// The recorded STAGE information for this frame, `None` while no replay plays.
pub(crate) fn stage() -> Option<StageInfo> {
    STEP.lock().stage
}

/// The recorded frames that came due this frame, for the input seam to publish in order. Taken
/// once: a second call in the same frame gets `None`, as does a frame on which none came due.
pub(crate) fn take_frames() -> Option<Vec<ReplayedFrame>> {
    STEP.lock().frames.take()
}

/// Stop the replay and wait for a loader thread, for eject. Returns whether no thread of the
/// replay's is left running; `false` means the payload must stay mapped.
#[must_use = "a loader that did not finish means the payload must stay mapped"]
pub(crate) fn shutdown() -> bool {
    let mut state = STATE.lock();
    state.pending = None;
    let mut handles = std::mem::take(&mut state.orphans);
    if let PhaseState::Loading { handle, .. } =
        std::mem::replace(&mut state.phase, PhaseState::Idle)
    {
        handles.push(handle);
    }
    drop(state);
    *STEP.lock() = Step::default();
    ACTIVE.store(false, Ordering::Relaxed);
    DRIVING.store(false, Ordering::Relaxed);
    let deadline = Instant::now() + SHUTDOWN_BUDGET;
    for handle in handles {
        while !handle.is_finished() {
            if Instant::now() >= deadline {
                tracing::error!(
                    "replay: a loader did not finish within {} s; leaving the payload mapped",
                    SHUTDOWN_BUDGET.as_secs()
                );
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = handle.join();
    }
    true
}

/// The display time of a replay's first frame. Any positive value works: the hands' motion
/// history is cleared when a replay starts, and a live frame after it has a later time.
const DISPLAY_TIME_ORIGIN_NS: i64 = 1_000_000_000;

/// How close to the start the player must stand for the teleport to count as arrived.
const ARRIVED_RADIUS_M: f32 = 3.0;

/// How many consecutive frames the player must stand arrived, in gameplay, with no teleport in
/// flight, before the replay plays: long enough for the landing to settle.
const SETTLE_FRAMES: u32 = 60;

/// How long eject waits for a loader thread.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

static ACTIVE: AtomicBool = AtomicBool::new(false);
static DRIVING: AtomicBool = AtomicBool::new(false);

struct Request {
    run: RunId,
    path: PathBuf,
    options: ReplayOptions,
}

/// The identity and options of a replay past its request.
#[derive(Copy, Clone)]
struct Run {
    id: RunId,
    options: ReplayOptions,
}

enum PhaseState {
    Idle,
    Loading {
        request: Request,
        handle: LoaderHandle,
    },
    Teleporting {
        run: Run,
        loaded: Arc<Loaded>,
        target: Vec3,
        since: Instant,
        settled: u32,
    },
    Playing {
        run: Run,
        loaded: Arc<Loaded>,
        /// When the first frame was published, set by the first [`begin_frame`].
        started: Option<Instant>,
        cursor: Cursor,
        /// The latest recorded head published.
        head: Option<HeadSample>,
    },
}

struct State {
    pending: Option<Request>,
    stop: bool,
    next_run: u32,
    phase: PhaseState,
    last: Option<(RunId, ReplayOutcome)>,
    /// The loaders of replays replaced while they loaded.
    orphans: Vec<LoaderHandle>,
}

type LoaderHandle = JoinHandle<Result<Loaded, ReplayError>>;

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    stop: false,
    next_run: 0,
    phase: PhaseState::Idle,
    last: None,
    orphans: Vec::new(),
});

/// What [`begin_frame`] stages for the seams.
#[derive(Default)]
struct Step {
    head: Option<HeadSample>,
    stage: Option<StageInfo>,
    frames: Option<Vec<ReplayedFrame>>,
}

static STEP: Mutex<Step> = Mutex::new(Step {
    head: None,
    stage: None,
    frames: None,
});

impl State {
    fn run_id(&self) -> Option<RunId> {
        match &self.phase {
            PhaseState::Idle => None,
            PhaseState::Loading { request, .. } => Some(request.run),
            PhaseState::Teleporting { run, .. } | PhaseState::Playing { run, .. } => Some(run.id),
        }
    }

    /// End the current replay, if any, with `outcome`.
    fn finish(&mut self, outcome: ReplayOutcome) {
        let Some(run) = self.run_id() else {
            return;
        };
        let was_playing = matches!(self.phase, PhaseState::Playing { .. });
        // A loader still reading finishes on its own, and its result has nowhere to go; its
        // handle is kept for eject to wait on.
        if let PhaseState::Loading { handle, .. } =
            std::mem::replace(&mut self.phase, PhaseState::Idle)
        {
            self.orphans.push(handle);
        }
        match &outcome {
            ReplayOutcome::Completed { frames, duration } => {
                tracing::info!(frames, seconds = duration.as_secs_f32(), "replay: finished")
            }
            ReplayOutcome::Stopped => tracing::info!("replay: stopped"),
            ReplayOutcome::Failed(error) => tracing::warn!("{error}"),
        }
        self.last = Some((run, outcome));
        *STEP.lock() = Step::default();
        if was_playing {
            // The last recorded hands would otherwise stand for the controllers until the frame
            // aged out, holding whatever they held.
            controller_frame::clear();
            semantic_frame::clear();
        }
    }

    fn fail(&mut self, run: RunId, error: ReplayError) {
        tracing::warn!("{error}");
        self.phase = PhaseState::Idle;
        self.last = Some((run, ReplayOutcome::Failed(error)));
    }

    fn begin(&mut self, request: Request) {
        let (enabled, expected) = Config::lock_query(|c| {
            (
                c.testing.enabled,
                Expected {
                    action_schema: crate::vr::input::semantic::SCHEMA_VERSION,
                    dominant_hand: c.controllers.dominant_hand,
                    world_scale: c.vr.world_scale,
                },
            )
        });
        if !enabled {
            return self.fail(request.run, ReplayError::Disabled);
        }
        if crate::recorder::tracking::is_recording() {
            return self.fail(request.run, ReplayError::Recording);
        }
        match load::spawn(request.path.clone(), expected) {
            Ok(handle) => {
                tracing::info!("replay: loading {}", request.path.display());
                self.phase = PhaseState::Loading { request, handle };
            }
            Err(error) => self.fail(request.run, error),
        }
    }

    fn advance(&mut self) {
        self.orphans.retain(|handle| !handle.is_finished());
        if let PhaseState::Playing {
            loaded,
            started,
            cursor,
            ..
        } = &self.phase
        {
            if cursor.finished(loaded.recording.frames.len()) {
                let outcome = ReplayOutcome::Completed {
                    frames: loaded.recording.frames.len(),
                    duration: started.map_or(Duration::ZERO, |s| s.elapsed()),
                };
                self.finish(outcome);
            }
            return;
        }
        match std::mem::replace(&mut self.phase, PhaseState::Idle) {
            PhaseState::Idle => {}
            PhaseState::Loading { request, handle } => {
                if !handle.is_finished() {
                    self.phase = PhaseState::Loading { request, handle };
                    return;
                }
                let loaded = match handle.join() {
                    Ok(Ok(loaded)) => Arc::new(loaded),
                    Ok(Err(error)) => return self.fail(request.run, error),
                    Err(_) => {
                        return self.fail(
                            request.run,
                            ReplayError::LoaderPanicked { path: request.path },
                        );
                    }
                };
                self.loaded(
                    Run {
                        id: request.run,
                        options: request.options,
                    },
                    loaded,
                );
            }
            PhaseState::Teleporting {
                run,
                loaded,
                target,
                since,
                settled,
            } => self.settle(run, loaded, target, since, settled),
            playing @ PhaseState::Playing { .. } => self.phase = playing,
        }
    }

    /// Start a loaded replay: teleport to its start, or play in place.
    fn loaded(&mut self, run: Run, loaded: Arc<Loaded>) {
        let header = &loaded.recording.header;
        tracing::info!(
            frames = loaded.recording.frames.len(),
            build = %header.build,
            recorded_at = %header.recorded_at,
            "replay: loaded {}",
            loaded.path.display()
        );
        for warning in &loaded.warnings {
            tracing::warn!("replay: {warning}");
        }
        let transform = match run.options.start {
            StartMode::InPlace => None,
            StartMode::Marker => Some(header.start.transform),
            StartMode::Bookmark => Some(bookmark_transform(header.start.bookmark.as_deref()).unwrap_or_else(|| {
                tracing::warn!(
                    bookmark = ?header.start.bookmark,
                    "replay: the start marker names no bookmark that exists; starting at the marker"
                );
                header.start.transform
            })),
        };
        match transform {
            Some(transform) => {
                teleport::request_transform(transform, TeleportMode::Auto);
                self.phase = PhaseState::Teleporting {
                    run,
                    loaded,
                    target: Vec3::new(transform[12], transform[13], transform[14]),
                    since: Instant::now(),
                    settled: 0,
                };
            }
            None => self.play(run, loaded),
        }
    }

    /// Wait for the teleport to the start to settle, then play.
    fn settle(
        &mut self,
        run: Run,
        loaded: Arc<Loaded>,
        target: Vec3,
        since: Instant,
        settled: u32,
    ) {
        let distance = teleport::player_position().map(|p| (Vec3::from(p) - target).length());
        let arrived = !teleport::in_flight()
            && crate::hooks::in_gameplay()
            && distance.is_some_and(|d| d <= ARRIVED_RADIUS_M);
        let settled = if arrived { settled + 1 } else { 0 };
        if settled >= SETTLE_FRAMES {
            return self.play(run, loaded);
        }
        let timeout = Config::lock_query(|c| c.testing.teleport_timeout_s);
        let timeout = if timeout.is_finite() {
            timeout.max(1.0)
        } else {
            60.0
        };
        if since.elapsed().as_secs_f32() > timeout {
            return self.fail(
                run.id,
                ReplayError::TeleportTimeout {
                    seconds: timeout,
                    distance_m: distance,
                },
            );
        }
        self.phase = PhaseState::Teleporting {
            run,
            loaded,
            target,
            since,
            settled,
        };
    }

    fn play(&mut self, run: Run, loaded: Arc<Loaded>) {
        tracing::info!(
            pacing = run.options.pacing.label(),
            semantic = loaded.semantic,
            "replay: playing {} frames",
            loaded.recording.frames.len()
        );
        self.phase = PhaseState::Playing {
            run,
            loaded,
            started: None,
            cursor: Cursor::default(),
            head: None,
        };
    }
}

/// The transform of the director bookmark named `name`, as it stands now.
fn bookmark_transform(name: Option<&str>) -> Option<[f32; 16]> {
    let name = name?;
    teleport::bookmarks()
        .into_iter()
        .find(|b| b.name == name)
        .map(|b| b.transform)
}
