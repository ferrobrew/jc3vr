//! The in-headset sight tuning tool (`controllers.aim.sight_tuning`, off by default).
//!
//! Lining up places a marker in the world, a fixed distance straight ahead of the head, which the
//! overlay draws. The player holds the gun so that its sights sit on the marker and captures, from
//! the Controllers tab or with the capture chord (both grips held and the dominant A pressed, on
//! foot, while the marker is placed). The capture solves the sight offset that makes the held
//! weapon's sight line, at its current angles, pass through the marker, and stores it as the
//! weapon's entry in the config's sight table, which takes precedence over the embedded table.
//!
//! The solve keeps the angles, so an offset captured with a wrong pitch is right at the marker's
//! distance only; the offset is the point on the line directly above or below the muzzle, the
//! embedded table's convention. The config has no load path, so a tuned entry lasts until the
//! payload unloads; [`json_line`] renders it in the table's line format for pasting back into
//! `tools/sights/sights.json`.
//!
//! Requests from the UI and the chord are serviced on the next aim update ([`service`]), which has
//! the gun's fire point and the head in the world; a request that waits longer than a few ticks
//! (the aim is idle) is dropped rather than acting later on a pose the player no longer holds.

use std::{
    fmt,
    time::{Duration, Instant},
};

use glam::{Mat4, Vec2, Vec3};
use parking_lot::Mutex;

use crate::{
    config::CONFIG,
    controllers::{
        aim::{gun::HeldWeapon, ray, sights},
        config::{SightOverride, SightTuningConfig},
        frame::{Hand, Pose},
    },
};

/// The tuning tool's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TuningStatus {
    /// The line-up marker in the world, while lining up.
    pub marker: Option<Vec3>,
    /// The last capture's result.
    pub last: Option<Result<SightOverride, LineUpError>>,
    /// Captures stored since the payload loaded.
    pub captures: u64,
}

/// Why a capture stored nothing.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum LineUpError {
    /// No marker is placed.
    NoMarker,
    /// No weapon is wielded in the hands.
    NoWeapon,
    /// The hand holds no gun of the wielded weapon.
    NoGun(Hand),
    /// The marker is not ahead of the muzzle along the sight line.
    MarkerBehindMuzzle,
    /// The fire point's frame is singular, or the sight line runs across the barrel.
    Degenerate,
    /// The solved origin is this far from the fire point, metres, more than any sight sits: the
    /// sights were not on the marker.
    Implausible { offset_m: f32 },
    /// The config's sight table has no room for another weapon.
    TableFull,
}

impl fmt::Display for LineUpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sight tuning: ")?;
        match self {
            LineUpError::NoMarker => write!(f, "no marker is placed"),
            LineUpError::NoWeapon => write!(f, "no weapon is wielded in the hands"),
            LineUpError::NoGun(hand) => write!(f, "the {} hand holds no gun", hand.label()),
            LineUpError::MarkerBehindMuzzle => write!(f, "the marker is not ahead of the muzzle"),
            LineUpError::Degenerate => {
                write!(f, "the gun's fire point gives no usable sight line")
            }
            LineUpError::Implausible { offset_m } => write!(
                f,
                "the solved sight is {offset_m:.2} m from the muzzle; the sights were not on the \
                 marker"
            ),
            LineUpError::TableFull => write!(f, "the config's sight table is full"),
        }
    }
}

impl std::error::Error for LineUpError {}

/// Place the marker ahead of the head on the next aim update, replacing any.
pub fn place_marker() {
    STATE.lock().place = Some(Instant::now());
}

/// Stop lining up: remove the marker and drop any waiting request.
pub fn clear_marker() {
    let mut state = STATE.lock();
    state.place = None;
    state.capture = None;
    state.status.marker = None;
}

/// Capture the held weapon's offset on the next aim update.
pub fn request_capture() {
    STATE.lock().capture = Some(Instant::now());
}

/// Whether a marker is placed, which arms the capture chord.
pub fn lining_up() -> bool {
    STATE.lock().status.marker.is_some()
}

/// The latest readout.
pub fn status() -> TuningStatus {
    STATE.lock().status
}

/// The weapon's entry in the sight table's line format (`tools/sights/sights.json`), graded
/// `tuned`, named from the embedded table.
pub fn json_line(sight: &SightOverride) -> String {
    let hash = sight.weapon_hash;
    let name = sights::table_entry(hash)
        .map_or_else(|| format!("unknown_{hash:08x}"), |e| e.weapon.clone());
    let name = serde_json::Value::String(name);
    let [x, y, z] = sight.offset_m.to_array().map(|v| round(v, 4));
    let [pitch, yaw] = sight.angles_deg.to_array().map(|v| round(v, 3));
    format!(
        "{{\"weapon\": {name}, \"weapon_hash\": {hash}, \"offset_m\": [{x:?}, {y:?}, {z:?}], \
         \"angles_deg\": [{pitch:?}, {yaw:?}], \"grade\": \"tuned\"}}"
    )
}

/// Service the waiting requests from an aim update: place the marker from the head's world pose,
/// and capture from the held weapon's gun in the dominant hand. With the tool off, forget
/// everything.
pub(crate) fn service(
    config: &SightTuningConfig,
    head: Pose,
    held: Option<&HeldWeapon>,
    dominant: Hand,
) {
    let (place, capture, marker) = {
        let mut state = STATE.lock();
        if !config.enabled {
            *state = State::EMPTY;
            return;
        }
        let fresh = |at: Option<Instant>| at.is_some_and(|at| at.elapsed() < REQUEST_TTL);
        let place = fresh(state.place.take());
        let capture = fresh(state.capture.take());
        if place {
            state.status.marker = Some(marker_position(head, config.marker_distance_m));
        }
        (place, capture, state.status.marker)
    };
    if place {
        tracing::info!(target: "controllers", ?marker, "sight tuning: marker placed");
    }
    if !capture {
        return;
    }
    // The config is locked with the tuning state released: the Controllers tab reads the state
    // while it holds the config.
    let result = solve_capture(marker, held, dominant).and_then(|sight| {
        CONFIG
            .lock()
            .controllers
            .aim
            .sights
            .set(sight)
            .then_some(sight)
            .ok_or(LineUpError::TableFull)
    });
    match &result {
        Ok(sight) => {
            tracing::info!(target: "controllers", "sight tuning: captured {}", json_line(sight))
        }
        Err(e) => tracing::warn!(target: "controllers", "{e}"),
    }
    let mut state = STATE.lock();
    state.status.captures += u64::from(result.is_ok());
    state.status.last = Some(result);
}

/// The marker's position: `distance_m` along the head's forward (-Z).
pub fn marker_position(head: Pose, distance_m: f32) -> Vec3 {
    head.position + head.orientation * Vec3::NEG_Z * distance_m.max(MIN_MARKER_DISTANCE_M)
}

/// The sight origin, in the fire point's frame, that makes a sight line at `angles_deg` from the
/// fire point whose world frame is `fire_frame` pass through `marker`: the point on that line
/// directly above or below the muzzle (on the fire point's XY plane).
pub fn line_up(fire_frame: Mat4, angles_deg: Vec2, marker: Vec3) -> Result<Vec3, LineUpError> {
    let determinant = fire_frame.determinant();
    if !determinant.is_finite() || determinant.abs() < 1e-6 {
        return Err(LineUpError::Degenerate);
    }
    let local = fire_frame.inverse().transform_point3(marker);
    let direction = ray::sight_direction(angles_deg);
    if direction.z > -MIN_FORWARD {
        return Err(LineUpError::Degenerate);
    }
    if local.dot(direction) <= 0.0 {
        return Err(LineUpError::MarkerBehindMuzzle);
    }
    let offset = local - direction * (local.z / direction.z);
    if !offset.is_finite() {
        return Err(LineUpError::Degenerate);
    }
    let distance = offset.length();
    if distance > MAX_OFFSET_M {
        return Err(LineUpError::Implausible { offset_m: distance });
    }
    Ok(offset)
}

/// The capture: the held weapon's offset from its gun in the `dominant` hand, keeping the angles
/// it aims with now.
fn solve_capture(
    marker: Option<Vec3>,
    held: Option<&HeldWeapon>,
    dominant: Hand,
) -> Result<SightOverride, LineUpError> {
    let marker = marker.ok_or(LineUpError::NoMarker)?;
    let held = held.ok_or(LineUpError::NoWeapon)?;
    let frame = held.fire_frames[dominant.index()].ok_or(LineUpError::NoGun(dominant))?;
    let angles_deg = held.sight.angles_deg();
    Ok(SightOverride {
        weapon_hash: held.hash,
        offset_m: line_up(frame, angles_deg, marker)?,
        angles_deg,
    })
}

fn round(value: f32, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    let rounded = (f64::from(value) * scale).round() / scale;
    // No negative zero in the table.
    rounded + 0.0
}

struct State {
    place: Option<Instant>,
    capture: Option<Instant>,
    status: TuningStatus,
}

impl State {
    const EMPTY: State = State {
        place: None,
        capture: None,
        status: TuningStatus {
            marker: None,
            last: None,
            captures: 0,
        },
    };
}

static STATE: Mutex<State> = Mutex::new(State::EMPTY);

/// How long a request waits for an aim update before it is dropped.
const REQUEST_TTL: Duration = Duration::from_millis(500);
/// The nearest the marker is placed, metres: nearer, the eye cannot focus on the sights and the
/// marker together, and the solve is dominated by where the eye sits.
const MIN_MARKER_DISTANCE_M: f32 = 2.0;
/// The farthest a sight's origin may be from the fire point, metres. The embedded table's highest
/// sight is under a fifth of this.
const MAX_OFFSET_M: f32 = 0.5;
/// The least the sight line may run along the barrel, as the -Z component of its direction.
const MIN_FORWARD: f32 = 0.2;

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::aim::sights::table;

    fn frame() -> Mat4 {
        Mat4::from_rotation_translation(
            Quat::from_euler(glam::EulerRot::YXZ, 0.7, -0.2, 0.3),
            Vec3::new(12.0, 3.0, -40.0),
        )
    }

    #[test]
    fn the_solved_line_passes_through_the_marker() {
        let offset = Vec3::new(0.004, 0.083, 0.0);
        let angles = Vec2::new(0.6, -0.2);
        let sight = SightOverride {
            weapon_hash: 1,
            offset_m: offset,
            angles_deg: angles,
        };
        let line = ray::sight_ray(frame(), Mat4::IDENTITY, Some(&sight)).unwrap();
        let marker = line.at(10.0);
        let solved = line_up(frame(), angles, marker).unwrap();
        assert!(solved.distance(offset) < 1e-4, "{solved} != {offset}");
    }

    #[test]
    fn the_solve_keeps_the_origin_above_the_muzzle() {
        let marker = frame().transform_point3(Vec3::new(0.0, 0.1, -10.0));
        let solved = line_up(frame(), Vec2::ZERO, marker).unwrap();
        assert!(solved.z.abs() < 1e-4);
        assert!((solved.y - 0.1).abs() < 1e-4);
    }

    #[test]
    fn a_marker_behind_the_muzzle_is_refused() {
        let marker = frame().transform_point3(Vec3::new(0.0, 0.05, 5.0));
        assert_eq!(
            line_up(frame(), Vec2::ZERO, marker),
            Err(LineUpError::MarkerBehindMuzzle)
        );
    }

    #[test]
    fn a_gun_far_off_the_marker_is_implausible() {
        let marker = frame().transform_point3(Vec3::new(2.0, 0.0, -10.0));
        assert!(matches!(
            line_up(frame(), Vec2::ZERO, marker),
            Err(LineUpError::Implausible { .. })
        ));
    }

    #[test]
    fn a_singular_frame_is_degenerate() {
        assert_eq!(
            line_up(Mat4::ZERO, Vec2::ZERO, Vec3::NEG_Z),
            Err(LineUpError::Degenerate)
        );
    }

    #[test]
    fn the_marker_is_ahead_of_the_head() {
        let head = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        };
        let marker = marker_position(head, 10.0);
        assert!(marker.distance(Vec3::new(-9.0, 2.0, 3.0)) < 1e-4);
    }

    #[test]
    fn the_json_line_reads_back_as_a_tuned_entry() {
        let sight = SightOverride {
            weapon_hash: 4_000_000_000,
            offset_m: Vec3::new(-0.00001, 0.08694, 0.0),
            angles_deg: Vec2::new(1.2345, 0.0),
        };
        let line = json_line(&sight);
        let entries = table::parse(&format!("[{line}]")).unwrap();
        let entry = &entries[0];
        assert_eq!(entry.weapon, "unknown_ee6b2800");
        assert_eq!(entry.weapon_hash, sight.weapon_hash);
        assert_eq!(entry.grade, table::SightGrade::Tuned);
        assert_eq!(entry.offset_m, Vec3::new(0.0, 0.0869, 0.0));
        assert_eq!(entry.angles_deg, Vec2::new(1.235, 0.0));
        assert!(line.contains("\"offset_m\": [0.0, 0.0869, 0.0]"), "{line}");
    }

    #[test]
    fn the_json_line_names_a_weapon_from_the_embedded_table() {
        let entry = &table::entries()[0];
        let line = json_line(&entry.sight());
        assert!(line.starts_with(&format!("{{\"weapon\": \"{}\"", entry.weapon)));
    }
}
