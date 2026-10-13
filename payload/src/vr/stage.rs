//! The STAGE reference space: the runtime's floor-level play area, located alongside the LOCAL space
//! the rest of the runtime works in. It supplies what the posture needs and LOCAL cannot give: the
//! head's true height above the floor. The play area's bounds are read alongside it for the
//! readouts and the recordings.
//!
//! The eye views, the controllers, and the compositor layer all stay in LOCAL. The STAGE space is
//! only located against it once per frame (see [`locate_head`]), so a runtime without a STAGE space
//! loses nothing but the floor height.
//!
//! The floor is not trusted blindly across a reference space change. When the runtime announces
//! one, the head's height above the floor is watched for [`FLOOR_WATCH`]: a step between two
//! frames larger than a head moves in one ([`MAX_HEAD_STEP_M`]) is the floor moving under the
//! player, not the player moving, and is taken back out ([`FloorWatch`]), so the published height
//! stays continuous and the view and the body keep their standing height. Each such jump is counted
//! ([`StageInfo::floor_jumps`]), which the body takes as the prompt to measure the player's standing
//! height afresh.

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use glam::{Quat, Vec2, Vec3};
use openxr as xr;
use parking_lot::Mutex;

/// What the runtime reports about its STAGE space, published once per frame.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StageInfo {
    /// Whether the session has a STAGE space at all.
    pub available: bool,
    /// The play area's extent (width along X, depth along Z), metres, or `None` when the runtime
    /// reports no bounds (a standing-only or stationary setup, or no STAGE space).
    pub bounds: Option<Vec2>,
    /// The head's height above the STAGE floor, metres, from the latest located frame, or `None`
    /// when the STAGE space could not be located against LOCAL. Corrected by
    /// [`floor_shift_m`](Self::floor_shift_m).
    pub head_height: Option<f32>,
    /// How far the floor has jumped across reference space changes this session, metres, up
    /// positive: taken out of [`head_height`](Self::head_height).
    pub floor_shift_m: f32,
    /// How many floor jumps have been taken out this session.
    pub floor_jumps: u32,
}

/// The latest STAGE information.
pub fn stage() -> StageInfo {
    *STAGE.lock()
}

/// Publish a replay's recorded STAGE information in place of the located one. Called by the frame
/// loop after it located the head, while a replay plays.
pub(super) fn publish_replayed(info: StageInfo) {
    *STAGE.lock() = info;
}

/// Create the session's STAGE space, or `None` (logged) when the runtime does not offer one.
pub(super) fn create_space(session: &xr::Session<xr::D3D11>) -> Option<xr::Space> {
    let offered = match session.enumerate_reference_spaces() {
        Ok(spaces) => spaces.contains(&xr::ReferenceSpaceType::STAGE),
        Err(e) => {
            tracing::warn!(target: "vr", "stage: enumerating the reference spaces failed: {e}");
            false
        }
    };
    let space = offered
        .then(|| {
            session
                .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
                .map_err(|e| {
                    tracing::warn!(target: "vr", "stage: creating the STAGE reference space failed: {e}");
                })
                .ok()
        })
        .flatten();
    if space.is_none() {
        tracing::info!(target: "vr", "stage: no STAGE reference space; the floor height is unavailable");
    }
    BOUNDS_DIRTY.store(true, Ordering::Relaxed);
    *STAGE.lock() = StageInfo {
        available: space.is_some(),
        ..StageInfo::default()
    };
    space
}

/// Mark the bounds for re-reading and watch the floor, after the runtime reports a reference space
/// change (the user redrew the play area or recentered the stage).
pub(super) fn mark_bounds_dirty() {
    BOUNDS_DIRTY.store(true, Ordering::Relaxed);
    WATCH.lock().changed(Instant::now());
}

/// Forget the published information, when the session goes away.
pub(super) fn clear() {
    *STAGE.lock() = StageInfo::default();
    *WATCH.lock() = FloorWatch::new();
}

/// Update the published information for one frame: re-read the bounds if they are stale, and
/// locate the head (the mid-eye pose in LOCAL) above the STAGE floor. Called by the frame loop under
/// the runtime lock.
pub(super) fn locate_head(
    session: &xr::Session<xr::D3D11>,
    local: &xr::Space,
    stage: Option<&xr::Space>,
    time: xr::Time,
    head_local: Option<xr::Posef>,
) {
    let Some(stage) = stage else {
        return;
    };
    let mut info = STAGE.lock();
    if BOUNDS_DIRTY.swap(false, Ordering::Relaxed) {
        let bounds = match session.reference_space_bounds_rect(xr::ReferenceSpaceType::STAGE) {
            Ok(extent) => extent.map(|e| Vec2::new(e.width, e.height)),
            Err(e) => {
                tracing::warn!(target: "vr", "stage: reading the play area bounds failed: {e}");
                None
            }
        };
        // A runtime may announce a burst of reference space changes, each re-reading the bounds;
        // only a change of the bounds, or the first reading, is worth a line.
        let first = !BOUNDS_LOGGED.swap(true, Ordering::Relaxed);
        if first || bounds != info.bounds {
            tracing::info!(target: "vr", ?bounds, "stage: play area bounds");
        } else {
            tracing::debug!(target: "vr", ?bounds, "stage: play area bounds re-read, unchanged");
        }
        info.bounds = bounds;
    }
    let Some(head) = head_local else {
        return;
    };
    let raw = match local.locate(stage, time) {
        Ok(location)
            if location.location_flags.contains(
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
            ) =>
        {
            Some(height_above_floor(location.pose, head))
        }
        _ => None,
    };
    let mut watch = WATCH.lock();
    let jumped = raw.and_then(|raw| watch.observe(raw, Instant::now()));
    if let Some(jump) = jumped {
        tracing::info!(
            target: "vr",
            jump_m = jump,
            floor_shift_m = watch.shift_m,
            "stage: the floor jumped across a reference space change; the jump is taken out of the \
             head height"
        );
    }
    info.head_height = raw.map(|raw| raw - watch.shift_m);
    info.floor_shift_m = watch.shift_m;
    info.floor_jumps = watch.jumps;
}

/// Watches the head's height above the floor across a reference space change, and takes a floor
/// jump back out of it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FloorWatch {
    /// The uncorrected head height last frame.
    last_raw: Option<f32>,
    /// Until when a step in the height is taken for the floor's.
    watch_until: Option<Instant>,
    /// The floor jumps taken out so far, metres.
    pub shift_m: f32,
    /// How many floor jumps were taken out.
    pub jumps: u32,
}

impl FloorWatch {
    pub const fn new() -> Self {
        Self {
            last_raw: None,
            watch_until: None,
            shift_m: 0.0,
            jumps: 0,
        }
    }

    /// The runtime announced a reference space change at `now`.
    pub fn changed(&mut self, now: Instant) {
        self.watch_until = Some(now + FLOOR_WATCH);
    }

    /// Take the uncorrected head height `raw` located at `now`. Returns the jump taken out, metres,
    /// when the step from last frame's is the floor's.
    pub fn observe(&mut self, raw: f32, now: Instant) -> Option<f32> {
        if !raw.is_finite() {
            return None;
        }
        let last = self.last_raw.replace(raw)?;
        let watching = self.watch_until.is_some_and(|until| now <= until);
        if !watching {
            self.watch_until = None;
            return None;
        }
        let step = raw - last;
        if step.abs() <= MAX_HEAD_STEP_M {
            return None;
        }
        self.shift_m += step;
        self.jumps = self.jumps.wrapping_add(1);
        Some(step)
    }
}

impl Default for FloorWatch {
    fn default() -> Self {
        Self::new()
    }
}

/// How long after a reference space change a step in the head's height may be the floor's.
pub const FLOOR_WATCH: Duration = Duration::from_secs(5);

/// The largest step in the head's height between two frames that is the head's own, metres: a
/// head dropping as fast as a player can duck covers a few centimetres in a frame.
pub const MAX_HEAD_STEP_M: f32 = 0.08;

static WATCH: Mutex<FloorWatch> = Mutex::new(FloorWatch::new());

/// Whether the bounds have been logged once.
static BOUNDS_LOGGED: AtomicBool = AtomicBool::new(false);

/// The published information.
static STAGE: Mutex<StageInfo> = Mutex::new(StageInfo {
    available: false,
    bounds: None,
    head_height: None,
    floor_shift_m: 0.0,
    floor_jumps: 0,
});

/// Whether the bounds must be re-read on the next frame.
static BOUNDS_DIRTY: AtomicBool = AtomicBool::new(true);

/// The height above the STAGE floor of a LOCAL-space head position, given LOCAL's pose in STAGE.
fn height_above_floor(local_in_stage: xr::Posef, head_local: xr::Posef) -> f32 {
    let rotation = Quat::from_xyzw(
        local_in_stage.orientation.x,
        local_in_stage.orientation.y,
        local_in_stage.orientation.z,
        local_in_stage.orientation.w,
    );
    let origin = Vec3::new(
        local_in_stage.position.x,
        local_in_stage.position.y,
        local_in_stage.position.z,
    );
    let head = Vec3::new(
        head_local.position.x,
        head_local.position.y,
        head_local.position.z,
    );
    (origin + rotation * head).y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose(position: Vec3, orientation: Quat) -> xr::Posef {
        xr::Posef {
            orientation: xr::Quaternionf {
                x: orientation.x,
                y: orientation.y,
                z: orientation.z,
                w: orientation.w,
            },
            position: xr::Vector3f {
                x: position.x,
                y: position.y,
                z: position.z,
            },
        }
    }

    #[test]
    fn a_floor_jump_after_a_reference_space_change_is_taken_out() {
        let start = Instant::now();
        let mut watch = FloorWatch::new();
        // Without a change announced, any step is the head's.
        assert_eq!(watch.observe(1.58, start), None);
        assert_eq!(watch.observe(1.93, start), None);
        assert_eq!(watch.shift_m, 0.0);
        // The runtime announces a change, and the floor drops by 0.35 m a frame later.
        watch.changed(start);
        let frame = Duration::from_millis(11);
        assert_eq!(watch.observe(1.93, start + frame), None);
        let jump = watch.observe(2.28, start + frame * 2).unwrap();
        assert!((jump - 0.35).abs() < 1e-5);
        assert_eq!(watch.jumps, 1);
        // The corrected height continues where it was, and the head's own motion passes.
        assert!((2.28 - watch.shift_m - 1.93).abs() < 1e-5);
        assert_eq!(watch.observe(2.25, start + frame * 3), None);
        // Past the watch, a large step is the head's again.
        let late = start + FLOOR_WATCH + Duration::from_secs(1);
        assert_eq!(watch.observe(1.9, late), None);
        assert_eq!(watch.jumps, 1);
        assert_eq!(watch.observe(f32::NAN, late), None);
    }

    #[test]
    fn the_head_height_adds_the_local_origin_height() {
        // LOCAL's origin sits at the eyes of a standing player, 1.7 m above the floor, turned.
        let local = pose(Vec3::new(0.3, 1.7, -0.2), Quat::from_rotation_y(1.0));
        let head = pose(Vec3::new(0.1, -0.4, 0.5), Quat::IDENTITY);
        assert!((height_above_floor(local, head) - 1.3).abs() < 1e-5);
    }
}
