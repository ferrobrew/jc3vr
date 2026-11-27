//! Staging a scene for the visual harness: move the player to a distant location, spawn objects
//! into the view, and let the world settle with the clock running, all before a visual
//! configuration freezes it.
//!
//! A `visual_stage` request file ([`crate::testing::request`]) carries a [`StageRequest`]. A stage
//! with an environment sets its hour and pins its weather first, without holding the day cycle, so
//! the rain or snow has fallen for the whole settle. The stage then runs from the game thread's
//! update ([`update`]) in three phases: the teleport (through
//! the director's teleport, which streams the destination in when it is far), the spawns (through
//! the director's spawner, once the player has stood at the destination for the tuning's
//! [`arrived_frames`](crate::tuning::StageTuning::arrived_frames)),
//! and the settle (wall-clock seconds with the game running, so streaming, physics, and the
//! spawned entities come to rest). It then writes `visual/<id>.json` with `state = "staged"` and
//! the player's landed position, or `state = "failed"` and why.
//!
//! A stage that sets `reuse_within_m` is acknowledged at once, with its environment set but no
//! teleport, spawns, or settle, when the player already stands that close to its location: the
//! requester staged the same scene last, so the world there has streamed in and settled.
//!
//! A stage is refused while a visual configuration is active: it needs the clock running, and a
//! configuration holds it. Spawned objects persist in the world, so a spawn is submitted once per
//! session for each distinct tag set and position; staging the same scene again reports it as
//! already present rather than stacking a second copy on the first.

use std::{
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use glam::Vec3;
use jc3gi::types::math::Matrix4;
use jc3vr_capture::testing::visual::{
    self, EnvironmentRequest, EnvironmentTable, RequestId, environment,
};
use jc3vr_weapons::controllers::{spawn, spawn::Submission};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    director::teleport::{self, TeleportMode},
    tuning::TOOLING,
};

/// A checked stage request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StageRequest {
    id: RequestId,
    location: Option<Placement>,
    environment: Option<EnvironmentRequest>,
    settle: Duration,
    spawns: Vec<SpawnRequest>,
    /// Skip the teleport, the spawns, and the settle when the player already stands within this
    /// many metres of the location: the requester staged this scene last and nothing has moved the
    /// player since.
    reuse_within_m: Option<f32>,
}

/// The `[stage]` table of a request file, before validation.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StageTable {
    location: Option<PlacementTable>,
    environment: Option<EnvironmentTable>,
    settle_seconds: f32,
    #[serde(default)]
    spawn: Vec<SpawnTable>,
    reuse_within_m: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlacementTable {
    position: [f32; 3],
    #[serde(default)]
    yaw_deg: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpawnTable {
    tags: String,
    position: [f32; 3],
    #[serde(default)]
    yaw_deg: f32,
}

/// Why a stage request is invalid.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StageError {
    NonFinite(&'static str),
    SettleOutOfRange(f32),
    TooManySpawns(usize),
    EmptyTags(usize),
    NothingToStage,
    ReuseOutOfRange(f32),
    Environment(visual::EnvironmentError),
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite(field) => write!(f, "visual stage: {field} is not finite"),
            Self::SettleOutOfRange(seconds) => write!(
                f,
                "visual stage: settle_seconds {seconds} is outside [0, {MAX_SETTLE_SECONDS}]"
            ),
            Self::TooManySpawns(count) => write!(
                f,
                "visual stage: {count} spawns exceed the limit of {MAX_SPAWNS}"
            ),
            Self::EmptyTags(index) => write!(f, "visual stage: spawn {index} has no tags"),
            Self::NothingToStage => {
                write!(
                    f,
                    "visual stage: the stage names neither a location nor a spawn"
                )
            }
            Self::ReuseOutOfRange(metres) => write!(
                f,
                "visual stage: reuse_within_m {metres} is outside (0, {MAX_REUSE_METRES}] or names \
                 no location"
            ),
            Self::Environment(error) => write!(f, "visual stage: {error}"),
        }
    }
}

impl StageRequest {
    pub(crate) fn new(id: RequestId, table: StageTable) -> Result<Self, StageError> {
        let settle = table.settle_seconds;
        if !settle.is_finite() {
            return Err(StageError::NonFinite("settle_seconds"));
        }
        if !(0.0..=MAX_SETTLE_SECONDS).contains(&settle) {
            return Err(StageError::SettleOutOfRange(settle));
        }
        if table.spawn.len() > MAX_SPAWNS {
            return Err(StageError::TooManySpawns(table.spawn.len()));
        }
        if table.location.is_none() && table.spawn.is_empty() {
            return Err(StageError::NothingToStage);
        }
        if let Some(metres) = table.reuse_within_m
            && (!(metres > 0.0 && metres <= MAX_REUSE_METRES) || table.location.is_none())
        {
            return Err(StageError::ReuseOutOfRange(metres));
        }
        let environment = table
            .environment
            .map(EnvironmentRequest::try_from)
            .transpose()
            .map_err(StageError::Environment)?;
        let location = table
            .location
            .map(|l| Placement::new(l.position, l.yaw_deg, "location"))
            .transpose()?;
        let spawns = table
            .spawn
            .into_iter()
            .enumerate()
            .map(|(index, s)| {
                let tags = s.tags.trim().to_owned();
                if tags.is_empty() {
                    return Err(StageError::EmptyTags(index));
                }
                Ok(SpawnRequest {
                    tags,
                    placement: Placement::new(s.position, s.yaw_deg, "spawn")?,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            id,
            location,
            environment,
            settle: Duration::from_secs_f32(settle),
            spawns,
            reuse_within_m: table.reuse_within_m,
        })
    }
}

/// Queue a stage; it starts on the next game-thread update. Any thread.
pub(crate) fn enqueue(request: StageRequest) {
    let mut state = STATE.lock();
    if state.running.is_some() || state.pending.is_some() {
        publish(&StageResult::failed(
            &request.id,
            "visual stage: another stage is in progress",
        ));
        return;
    }
    state.pending = Some(request);
}

/// Advance the stage. Game thread, once per frame, after the director's pass (which services the
/// teleport and the spawns it queues).
pub(crate) fn update() {
    let mut state = STATE.lock();
    if let Some(request) = state.pending.take() {
        if visual::is_active() {
            publish(&StageResult::failed(
                &request.id,
                "visual stage: a visual configuration is active; end it before staging",
            ));
            return;
        }
        if let Some(request_environment) = &request.environment
            && let Err(error) = environment::preview(request_environment)
        {
            publish(&StageResult::failed(
                &request.id,
                &format!("visual stage: {error}"),
            ));
            return;
        }
        if let Some(result) = reused(&request) {
            tracing::info!(id = ?request.id, distance_m = ?result.distance_m, "visual stage: reused");
            publish(&result);
            return;
        }
        let phase = match request.location {
            Some(location) => {
                teleport::request_transform(location.transform().data, TeleportMode::Auto);
                Phase::Teleporting { arrived: 0 }
            }
            None => Phase::Spawning,
        };
        state.running = Some(Running {
            request,
            started: Instant::now(),
            phase,
        });
    }
    let State {
        running, spawned, ..
    } = &mut *state;
    let Some(run) = running.as_mut() else {
        return;
    };
    if run.started.elapsed() > TIMEOUT {
        let result = StageResult::failed(
            &run.request.id,
            &format!(
                "visual stage: did not finish within {} s (phase {})",
                TIMEOUT.as_secs(),
                run.phase.label()
            ),
        );
        *running = None;
        publish(&result);
        return;
    }
    match run.phase {
        Phase::Teleporting { arrived } => {
            let settled = !teleport::in_flight() && jc3vr_core::hooks::in_gameplay();
            let arrived = if settled { arrived + 1 } else { 0 };
            run.phase = if arrived >= TOOLING.get().stage.arrived_frames {
                Phase::Spawning
            } else {
                Phase::Teleporting { arrived }
            };
        }
        Phase::Spawning => {
            let mut submitted = 0;
            let mut present = 0;
            for request in &run.request.spawns {
                let key = request.key();
                if spawned.contains(&key) {
                    present += 1;
                    continue;
                }
                match spawn::submit(&request.tags, &request.placement.transform()) {
                    Submission::Submitted => {
                        spawned.insert(key);
                        submitted += 1;
                    }
                    other => {
                        let result = StageResult::failed(
                            &run.request.id,
                            &format!("visual stage: spawn \"{}\": {other:?}", request.tags),
                        );
                        *running = None;
                        publish(&result);
                        return;
                    }
                }
            }
            run.phase = Phase::Settling {
                since: Instant::now(),
                submitted,
                present,
            };
        }
        Phase::Settling {
            since,
            submitted,
            present,
        } => {
            if since.elapsed() < run.request.settle {
                return;
            }
            let player = teleport::player_position();
            let distance_m = run.request.location.zip(player).map(|(location, player)| {
                (Vec3::from(player) - Vec3::from(location.position)).length()
            });
            let result = StageResult {
                id: run.request.id.clone(),
                state: ResultState::Staged,
                error: None,
                player_position: player,
                distance_m,
                spawns_submitted: submitted,
                spawns_present: present,
                reused: false,
            };
            tracing::info!(
                id = ?run.request.id,
                ?player,
                ?distance_m,
                submitted,
                present,
                "visual stage: staged"
            );
            *running = None;
            publish(&result);
        }
    }
}

/// The staged result for a request that may reuse the scene the player already stands in, or
/// `None` when it must be staged afresh. The environment is set again either way.
fn reused(request: &StageRequest) -> Option<StageResult> {
    let within = request.reuse_within_m?;
    let location = request.location?;
    if teleport::in_flight() || !jc3vr_core::hooks::in_gameplay() {
        return None;
    }
    let player = teleport::player_position()?;
    let distance = (Vec3::from(player) - Vec3::from(location.position)).length();
    (distance <= within).then(|| StageResult {
        id: request.id.clone(),
        state: ResultState::Staged,
        error: None,
        player_position: Some(player),
        distance_m: Some(distance),
        spawns_submitted: 0,
        spawns_present: request.spawns.len(),
        reused: true,
    })
}

/// The longest settle a request may ask for, seconds.
const MAX_SETTLE_SECONDS: f32 = 300.0;
/// The farthest a reused stage may find the player from its location, metres.
const MAX_REUSE_METRES: f32 = 50.0;
/// The most spawns one stage may submit.
const MAX_SPAWNS: usize = 16;
/// How long a stage may take in all, including its settle.
const TIMEOUT: Duration = Duration::from_secs(420);

static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    running: None,
    spawned: BTreeSet::new(),
});

/// A position and a heading, in degrees about world up from `+Z` toward `+X`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    position: [f32; 3],
    yaw_deg: f32,
}

impl Placement {
    fn new(position: [f32; 3], yaw_deg: f32, field: &'static str) -> Result<Self, StageError> {
        if !position.iter().all(|v| v.is_finite()) || !yaw_deg.is_finite() {
            return Err(StageError::NonFinite(field));
        }
        Ok(Self { position, yaw_deg })
    }

    /// The row-major world matrix: rows right, up, forward, and translation, with the object's
    /// forward (`+Z`) along the heading and its up along world up.
    fn transform(&self) -> Matrix4 {
        let yaw = self.yaw_deg.to_radians();
        let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let up = Vec3::Y;
        let right = up.cross(forward).normalize();
        let mut m = Matrix4::IDENTITY;
        m.data[0..3].copy_from_slice(&right.to_array());
        m.data[4..7].copy_from_slice(&up.to_array());
        m.data[8..11].copy_from_slice(&forward.to_array());
        m.data[12..15].copy_from_slice(&self.position);
        m
    }
}

#[derive(Debug, Clone, PartialEq)]
struct SpawnRequest {
    tags: String,
    placement: Placement,
}

impl SpawnRequest {
    /// The identity a spawn is submitted once per session under: its tags and its position to the
    /// centimetre.
    fn key(&self) -> SpawnKey {
        let [x, y, z] = self.placement.position.map(|v| (v * 100.0).round() as i64);
        (self.tags.clone(), [x, y, z])
    }
}

type SpawnKey = (String, [i64; 3]);

struct State {
    pending: Option<StageRequest>,
    running: Option<Running>,
    /// The spawns submitted this session.
    spawned: BTreeSet<SpawnKey>,
}

struct Running {
    request: StageRequest,
    started: Instant,
    phase: Phase,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Teleporting {
        arrived: u32,
    },
    Spawning,
    Settling {
        since: Instant,
        submitted: usize,
        present: usize,
    },
}

impl Phase {
    fn label(&self) -> &'static str {
        match self {
            Self::Teleporting { .. } => "teleporting",
            Self::Spawning => "spawning",
            Self::Settling { .. } => "settling",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResultState {
    Staged,
    Failed,
}

#[derive(Debug, Serialize)]
struct StageResult {
    id: RequestId,
    state: ResultState,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    player_position: Option<[f32; 3]>,
    distance_m: Option<f32>,
    spawns_submitted: usize,
    spawns_present: usize,
    /// Whether the player already stood at the location, so the teleport and settle were skipped.
    reused: bool,
}

impl StageResult {
    fn failed(id: &RequestId, error: &str) -> Self {
        tracing::warn!("{error}");
        Self {
            id: id.clone(),
            state: ResultState::Failed,
            error: Some(error.to_owned()),
            player_position: None,
            distance_m: None,
            spawns_submitted: 0,
            spawns_present: 0,
            reused: false,
        }
    }
}

fn publish(result: &StageResult) {
    let Some(dir) = jc3vr_core::config::engine::paths::dir() else {
        tracing::error!("visual stage: no configuration directory for the result");
        return;
    };
    let path = dir
        .join("visual")
        .join(format!("{}.json", result.id.as_str()));
    if let Err(error) = write_result(&path, result) {
        tracing::error!(path = %path.display(), "visual stage: {error:#}");
    }
}

fn write_result(path: &Path, result: &StageResult) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("visual stage: the result has no parent directory")?;
    std::fs::create_dir_all(parent).context("visual stage: create the result directory")?;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(result).context("visual stage: encode the result")?;
    std::fs::write(&temporary, bytes).context("visual stage: write the temporary result")?;
    std::fs::rename(&temporary, path).context("visual stage: publish the result")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn table(text: &str) -> Result<StageRequest, String> {
        let table: StageTable = toml::from_str(text).map_err(|e| e.to_string())?;
        StageRequest::new(RequestId::parse("stage_1").unwrap(), table).map_err(|e| e.to_string())
    }

    #[test]
    fn a_location_and_spawns_parse() {
        let request = table(
            "settle_seconds = 20.0\n\
             [location]\nposition = [1.0, 2.0, 3.0]\nyaw_deg = 90.0\n\
             [environment]\nhour = 22.5\nweather = 'snow'\n\
             [[spawn]]\ntags = ' vehicle,car '\nposition = [4.0, 5.0, 6.0]\n",
        )
        .unwrap();
        assert!(request.environment.is_some());
        assert_eq!(request.settle, Duration::from_secs(20));
        assert_eq!(
            request.location,
            Some(Placement {
                position: [1.0, 2.0, 3.0],
                yaw_deg: 90.0
            })
        );
        assert_eq!(request.spawns.len(), 1);
        assert_eq!(request.spawns[0].tags, "vehicle,car");
    }

    #[test]
    fn a_reuse_distance_parses() {
        let request = table(
            "settle_seconds = 20.0\nreuse_within_m = 3.0\n[location]\nposition = [1.0, 2.0, 3.0]",
        )
        .unwrap();
        assert_eq!(request.reuse_within_m, Some(3.0));
    }

    #[test]
    fn invalid_stages_are_rejected() {
        for text in [
            "settle_seconds = 20.0",
            "settle_seconds = -1.0\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 301.0\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = nan\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\n[location]\nposition = [inf, 0.0, 0.0]",
            "settle_seconds = 1.0\n[[spawn]]\ntags = '  '\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\nweather = 'rain'\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\n[environment]\nhour = 25.0\nweather = 'rain'\n\
             [location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\nreuse_within_m = 0.0\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\nreuse_within_m = 51.0\n[location]\nposition = [0.0, 0.0, 0.0]",
            "settle_seconds = 1.0\nreuse_within_m = 3.0\n[[spawn]]\ntags = 'car'\n\
             position = [0.0, 0.0, 0.0]",
        ] {
            assert!(table(text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_heading_turns_forward_from_z_toward_x() {
        let east = Placement {
            position: [10.0, 20.0, 30.0],
            yaw_deg: 90.0,
        }
        .transform();
        let forward = Vec3::from_slice(&east.data[8..11]);
        let right = Vec3::from_slice(&east.data[0..3]);
        assert!((forward - Vec3::X).length() < 1e-6, "{forward}");
        assert!((right - Vec3::NEG_Z).length() < 1e-6, "{right}");
        assert_eq!(&east.data[12..15], &[10.0, 20.0, 30.0]);
    }

    #[test]
    fn a_spawn_key_rounds_to_the_centimetre() {
        let spawn = |x| SpawnRequest {
            tags: "prop".to_owned(),
            placement: Placement {
                position: [x, 0.0, 0.0],
                yaw_deg: 0.0,
            },
        };
        assert_eq!(spawn(1.0001).key(), spawn(1.0).key());
        assert_ne!(spawn(1.02).key(), spawn(1.0).key());
    }
}
