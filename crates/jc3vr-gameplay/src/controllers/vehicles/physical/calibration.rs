//! The cockpit controls' calibration from the player's hands.
//!
//! The cockpit table ([`native::cockpit_asset`]) holds each merged aircraft cockpit's controls,
//! measured from the meshes where they are modelled and placed where they are not, and a placed
//! control can sit out of reach. So the player can set a cockpit's controls from the hands: a
//! request starts a countdown ([`request`]), and when it runs out, seated at the controls, the
//! right hand's grip becomes the stick's (the cyclic, the flight stick, or the yoke's right horn)
//! and the left hand's the lever's (the collective or the throttle), in the cockpit part's own
//! space, for that part and seat. The countdown starts only once the fit sees the player seated at
//! the controls, since the debug panel the request comes from holds the controllers while it is
//! up; each second of it ticks on both hands, and at its end both hands buzz. The override applies from the next fit, and is
//! written at once to [`FILE_NAME`] in the configuration directory, which loads on start, so a
//! calibration both holds across sessions and can be read back into the table.

use std::{
    collections::BTreeMap,
    fmt,
    path::PathBuf,
    time::{Duration, Instant},
};

use glam::Vec3;
use jc3vr_core::config::engine::{paths, write};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::controllers::{
    haptics::{self, Event},
    vehicles::physical::{
        ControlKind,
        native::{CockpitAsset, YokeAsset},
    },
};

/// The calibration file's name, in the configuration directory.
pub const FILE_NAME: &str = "cockpit_calibration.toml";

/// The calibration's readout, for the debug UI.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibrationStatus {
    /// Whether a capture waits for the player to sit at a cockpit's controls.
    pub waiting: bool,
    /// The seconds left before the hands are captured, once the countdown runs.
    pub countdown_s: Option<f32>,
    /// The cockpit part and seat the last fit was in, as the file keys it.
    pub current: Option<String>,
    /// Whether the current cockpit has a calibration.
    pub current_calibrated: bool,
    /// How many cockpits the file holds.
    pub entries: usize,
    /// What the last capture or save did.
    pub last: Option<String>,
    /// The file's path.
    pub path: Option<PathBuf>,
}

/// The calibration's readout.
pub fn status() -> CalibrationStatus {
    let mut state = STATE.lock();
    let state = state.loaded();
    CalibrationStatus {
        waiting: state.pending.is_some_and(|p| p.armed.is_none()),
        countdown_s: state
            .pending
            .and_then(|p| p.armed)
            .map(|at| (COUNTDOWN.saturating_sub(at.elapsed())).as_secs_f32()),
        current: state.current.clone(),
        current_calibrated: state
            .current
            .as_ref()
            .is_some_and(|key| state.file.cockpits.contains_key(key)),
        entries: state.file.cockpits.len(),
        last: state.last.clone(),
        path: paths::file(FILE_NAME),
    }
}

/// Ask to capture the hands as a cockpit's controls: the countdown starts once the player is
/// seated at the controls with the controllers free.
pub fn request() {
    let mut state = STATE.lock();
    state.loaded().pending = Some(Pending {
        requested: Instant::now(),
        armed: None,
        ticks: 0,
    });
    tracing::info!(
        target: "controllers",
        "vehicles: cockpit calibration: requested; the countdown starts at the controls"
    );
}

/// Remove the current cockpit's calibration, back to the table's controls.
pub fn clear_current() {
    let mut guard = STATE.lock();
    let state = guard.loaded();
    let Some(key) = state.current.clone() else {
        return;
    };
    if state.file.cockpits.remove(&key).is_some() {
        let saved = state.save();
        state.last = Some(match saved {
            Ok(()) => format!("cleared {key}"),
            Err(error) => error.to_string(),
        });
        tracing::info!(target: "controllers", key, "vehicles: cockpit calibration: cleared");
    }
}

/// The fit's part of the calibration for the cockpit part `part` and the seat of `asset`: capture
/// the hands `hands_in_part` (the left, then the right, in the part's own space) when a countdown
/// has run out, then apply the cockpit's calibration to `asset`. Returns whether `asset` is
/// calibrated, so its controls need not lie inside the part's bounds.
pub(crate) fn fit(part: &str, asset: &mut CockpitAsset, hands_in_part: [Option<Vec3>; 2]) -> bool {
    let key = key(part, asset.seat);
    let mut guard = STATE.lock();
    let state = guard.loaded();
    state.current = Some(key.clone());
    if let Some(pending) = state.pending.as_mut() {
        let now = Instant::now();
        if pending.armed.is_none() {
            tracing::info!(
                target: "controllers",
                key,
                countdown_s = COUNTDOWN.as_secs_f32(),
                "vehicles: cockpit calibration: hold the left hand at the lever and the right at \
                 the stick"
            );
        }
        let armed = pending.arm(now);
        let whole = armed.elapsed().as_secs() as u32;
        if whole > pending.ticks {
            pending.ticks = whole;
            if armed.elapsed() < COUNTDOWN {
                haptics::play_hands(Hand::BOTH, Event::VolumeEnter);
            }
        }
    }
    if state
        .pending
        .and_then(|p| p.armed)
        .is_some_and(|at| at.elapsed() >= COUNTDOWN)
    {
        state.pending = None;
        state.last = Some(match hands_in_part {
            [Some(left), Some(right)] if left.is_finite() && right.is_finite() => {
                state.file.cockpits.insert(
                    key.clone(),
                    Entry {
                        stick: right.to_array(),
                        lever: left.to_array(),
                    },
                );
                haptics::play_hands(Hand::BOTH, Event::Take);
                tracing::info!(
                    target: "controllers",
                    key,
                    lever = ?left,
                    stick = ?right,
                    "vehicles: cockpit calibration: captured the hands"
                );
                match state.save() {
                    Ok(()) => format!("captured {key}"),
                    Err(error) => error.to_string(),
                }
            }
            _ => {
                tracing::warn!(
                    target: "controllers",
                    key,
                    "vehicles: cockpit calibration: a hand was not tracked; nothing captured"
                );
                "a hand was not tracked; nothing captured".to_owned()
            }
        });
    }
    let Some(entry) = state.file.cockpits.get(&key) else {
        return false;
    };
    apply(asset, entry);
    true
}

/// Drop a request that found no cockpit's controls to count down at. Called each fit tick.
pub(crate) fn expire() {
    let mut guard = STATE.lock();
    let state = guard.loaded();
    if state
        .pending
        .is_some_and(|p| p.armed.is_none() && p.requested.elapsed() >= EXPIRE_AFTER)
    {
        state.pending = None;
        state.last = Some("not seated at a cockpit's controls; nothing captured".to_owned());
        tracing::warn!(
            target: "controllers",
            "vehicles: cockpit calibration: not seated at a cockpit's controls; nothing captured"
        );
    }
}

/// Forget the current cockpit, once the fit has none; a countdown under way waits for the controls
/// again.
pub(crate) fn leave() {
    let mut guard = STATE.lock();
    guard.loaded().leave();
}

/// A capture asked for: when, and when its countdown began at the controls.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Pending {
    requested: Instant,
    armed: Option<Instant>,
    /// The whole seconds of the countdown ticked so far.
    ticks: u32,
}

impl Pending {
    fn arm(&mut self, now: Instant) -> Instant {
        *self.armed.get_or_insert(now)
    }
}

/// One cockpit seat's calibration, in the cockpit part's own space, metres.
#[derive(Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// Where the right hand grips the stick: the cyclic's or the flight stick's grip, or the
    /// yoke's right horn.
    stick: [f32; 3],
    /// Where the left hand grips the lever: the collective or the throttle.
    lever: [f32; 3],
}

/// Apply `entry` to `asset`: the stick moved to the right hand's grip (a yoke moved whole, by its
/// right horn), and the lever to the left hand's.
fn apply(asset: &mut CockpitAsset, entry: &Entry) {
    let stick = Vec3::from_array(entry.stick);
    let delta = stick - stick_grip(asset);
    asset.stick_grip += delta;
    if let Some(yoke) = asset.yoke.as_mut() {
        yoke.hub += delta;
    }
    let lever_kind = asset
        .lever
        .map(|(kind, _)| kind)
        .unwrap_or(match asset.stick_kind {
            ControlKind::Cyclic => ControlKind::Collective,
            _ => ControlKind::Throttle,
        });
    asset.lever = Some((lever_kind, Vec3::from_array(entry.lever)));
}

/// Where the right hand grips the asset's stick: the stick's grip, or the yoke's right horn.
fn stick_grip(asset: &CockpitAsset) -> Vec3 {
    asset.yoke.map_or(asset.stick_grip, right_horn)
}

/// The yoke's right horn: across the wheel to the right, and up it to the grips.
fn right_horn(yoke: YokeAsset) -> Vec3 {
    let up = yoke.axis.cross(yoke.right).normalize_or_zero();
    yoke.hub + yoke.right * yoke.half_span + up * yoke.grip_height
}

/// The file's key for a cockpit part and seat.
fn key(part: &str, seat: u8) -> String {
    format!("{part}#{seat}")
}

/// The calibration file: each cockpit seat's calibration by its key.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct CalibrationFile {
    #[serde(default)]
    cockpits: BTreeMap<String, Entry>,
}

/// Why the calibration file could not be read or written.
#[derive(Debug)]
enum CalibrationError {
    NoDirectory,
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Serialize(toml::ser::Error),
    Write(write::WriteError),
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDirectory => write!(
                f,
                "cockpit calibration: there is no configuration directory to save to"
            ),
            Self::Read(path, error) => write!(
                f,
                "cockpit calibration: could not read {}: {error}",
                path.display()
            ),
            Self::Parse(path, error) => write!(
                f,
                "cockpit calibration: could not parse {}: {error}",
                path.display()
            ),
            Self::Serialize(error) => write!(f, "cockpit calibration: {error}"),
            Self::Write(error) => write!(f, "cockpit calibration: {error}"),
        }
    }
}

impl std::error::Error for CalibrationError {}

struct State {
    file: CalibrationFile,
    loaded: bool,
    pending: Option<Pending>,
    current: Option<String>,
    last: Option<String>,
}

impl State {
    fn leave(&mut self) {
        self.current = None;
        if let Some(pending) = self.pending.as_mut() {
            pending.armed = None;
            pending.ticks = 0;
        }
    }

    /// The state with the file loaded, on first use.
    fn loaded(&mut self) -> &mut Self {
        if !self.loaded {
            self.loaded = true;
            match load() {
                Ok(file) => {
                    tracing::info!(
                        target: "controllers",
                        cockpits = file.cockpits.len(),
                        "vehicles: cockpit calibration: loaded"
                    );
                    self.file = file;
                }
                Err(error) => {
                    tracing::warn!(target: "controllers", "{error}");
                    self.last = Some(error.to_string());
                }
            }
        }
        self
    }

    fn save(&self) -> Result<(), CalibrationError> {
        let path = paths::file(FILE_NAME).ok_or(CalibrationError::NoDirectory)?;
        let text = toml::to_string_pretty(&self.file).map_err(CalibrationError::Serialize)?;
        write::replace(&path, text.as_bytes()).map_err(CalibrationError::Write)
    }
}

/// Read the calibration file; a missing file is an empty calibration.
fn load() -> Result<CalibrationFile, CalibrationError> {
    let path = paths::file(FILE_NAME).ok_or(CalibrationError::NoDirectory)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CalibrationFile::default());
        }
        Err(error) => return Err(CalibrationError::Read(path, error)),
    };
    toml::from_str(&text).map_err(|error| CalibrationError::Parse(path, error))
}

static STATE: Mutex<State> = Mutex::new(State {
    file: CalibrationFile {
        cockpits: BTreeMap::new(),
    },
    loaded: false,
    pending: None,
    current: None,
    last: None,
});

/// How long the player has to put the hands on the controls after a request.
const COUNTDOWN: Duration = Duration::from_secs(3);

/// How long a request waits for the player to sit at a cockpit's controls before it is dropped.
const EXPIRE_AFTER: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_calibration_starts_a_fresh_countdown() {
        let requested = Instant::now();
        let mut state = State {
            file: CalibrationFile::default(),
            loaded: true,
            pending: Some(Pending {
                requested,
                armed: Some(requested),
                ticks: 2,
            }),
            current: Some(key("utility_body", 0)),
            last: None,
        };
        state.leave();
        assert_eq!(state.current, None);
        let resumed = requested + COUNTDOWN + Duration::from_secs(1);
        let pending = state.pending.as_mut().unwrap();
        assert_eq!(pending.armed, None);
        assert_eq!(pending.ticks, 0);
        let armed = pending.arm(resumed);
        assert_eq!(resumed.duration_since(armed), Duration::ZERO);
        assert!(resumed.duration_since(armed) < COUNTDOWN);
        assert_eq!(pending.requested, requested);
    }

    fn cyclic() -> CockpitAsset {
        CockpitAsset {
            yoke: None,
            seat: 0,
            stick_kind: ControlKind::Cyclic,
            stick_grip: Vec3::new(-0.4, 1.5, -1.4),
            lever: None,
        }
    }

    #[test]
    fn a_calibration_moves_the_stick_and_places_the_missing_lever() {
        let mut asset = cyclic();
        apply(
            &mut asset,
            &Entry {
                stick: [-0.3, 1.4, -1.3],
                lever: [-0.6, 1.2, -1.1],
            },
        );
        assert_eq!(asset.stick_grip, Vec3::new(-0.3, 1.4, -1.3));
        assert_eq!(
            asset.lever,
            Some((ControlKind::Collective, Vec3::new(-0.6, 1.2, -1.1)))
        );
    }

    #[test]
    fn a_yoke_moves_whole_by_its_right_horn() {
        let hub = Vec3::new(0.3, 1.46, -4.5);
        let yoke = YokeAsset {
            hub,
            axis: Vec3::Z,
            right: Vec3::X,
            half_span: 0.14,
            grip_height: 0.07,
        };
        let mut asset = CockpitAsset {
            yoke: Some(yoke),
            seat: 1,
            stick_kind: ControlKind::Yoke,
            stick_grip: hub,
            lever: Some((ControlKind::Throttle, Vec3::ZERO)),
        };
        let horn = right_horn(yoke);
        assert!(horn.distance(hub + Vec3::new(0.14, 0.07, 0.0)) < 1e-6);
        let target = horn + Vec3::new(0.0, 0.1, 0.2);
        apply(
            &mut asset,
            &Entry {
                stick: target.to_array(),
                lever: [0.0, 1.3, -4.4],
            },
        );
        assert!(right_horn(asset.yoke.unwrap()).distance(target) < 1e-6);
        assert!(asset.stick_grip.distance(hub + Vec3::new(0.0, 0.1, 0.2)) < 1e-6);
        assert_eq!(asset.lever.unwrap().0, ControlKind::Throttle);
    }

    #[test]
    fn the_file_keys_each_cockpit_seat() {
        let mut file = CalibrationFile::default();
        file.cockpits.insert(
            key("utility_body", 0),
            Entry {
                stick: [1.0, 2.0, 3.0],
                lever: [4.0, 5.0, 6.0],
            },
        );
        let text = toml::to_string_pretty(&file).unwrap();
        assert!(text.contains("utility_body#0"));
        assert_eq!(toml::from_str::<CalibrationFile>(&text).unwrap(), file);
    }
}
