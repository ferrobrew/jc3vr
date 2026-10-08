//! The STAGE reference space: the runtime's floor-level play area, located alongside the LOCAL space
//! the rest of the runtime works in. It supplies the two things the posture needs and LOCAL cannot
//! give: the head's true height above the floor, and the play area's bounds, which separate a
//! room-scale setup from a standing-only one.
//!
//! The eye views, the controllers, and the compositor layer all stay in LOCAL. The STAGE space is
//! only located against it once per frame (see [`locate_head`]), so a runtime without a STAGE space
//! loses nothing but the floor height.

use std::sync::atomic::{AtomicBool, Ordering};

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
    /// when the STAGE space could not be located against LOCAL.
    pub head_height: Option<f32>,
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

/// Mark the bounds for re-reading, after the runtime reports a reference space change (the user
/// redrew the play area or recentered the stage).
pub(super) fn mark_bounds_dirty() {
    BOUNDS_DIRTY.store(true, Ordering::Relaxed);
}

/// Forget the published information, when the session goes away.
pub(super) fn clear() {
    *STAGE.lock() = StageInfo::default();
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
        info.bounds = match session.reference_space_bounds_rect(xr::ReferenceSpaceType::STAGE) {
            Ok(extent) => extent.map(|e| Vec2::new(e.width, e.height)),
            Err(e) => {
                tracing::warn!(target: "vr", "stage: reading the play area bounds failed: {e}");
                None
            }
        };
        tracing::info!(target: "vr", bounds = ?info.bounds, "stage: play area bounds");
    }
    let Some(head) = head_local else {
        return;
    };
    info.head_height = match local.locate(stage, time) {
        Ok(location)
            if location.location_flags.contains(
                xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID,
            ) =>
        {
            Some(height_above_floor(location.pose, head))
        }
        _ => None,
    };
}

/// The published information.
static STAGE: Mutex<StageInfo> = Mutex::new(StageInfo {
    available: false,
    bounds: None,
    head_height: None,
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
    fn the_head_height_adds_the_local_origin_height() {
        // LOCAL's origin sits at the eyes of a standing player, 1.7 m above the floor, turned.
        let local = pose(Vec3::new(0.3, 1.7, -0.2), Quat::from_rotation_y(1.0));
        let head = pose(Vec3::new(0.1, -0.4, 0.5), Quat::IDENTITY);
        assert!((height_above_floor(local, head) - 1.3).abs() < 1e-5);
    }
}
