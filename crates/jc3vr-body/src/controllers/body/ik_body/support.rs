//! The feet's supports: which body a foot stands on, read where the mod reads the physics world,
//! and the frame it has when the solve gate asks.
//!
//! A ground probe that lands on the static world names [`SupportId::WORLD`], whose frame is the
//! identity. One that lands on a body that moves (a vehicle, a keyframed platform, a loose crate)
//! names that body: its id in the Havok world and the lifetime stamp the climb keeps for it
//! ([`crate::controllers::facts::climb::body_identity`]), so a freed and reused id is a different
//! support. The gate runs inside the pose-finalization pass, where the mod does not read the physics
//! world, so each support's transform and motion are snapshotted where the probes cast (the player
//! aim control's post-simulation update, outside the physics step), and the gate carries the
//! snapshot forward to its own moment by the body's velocities ([`SupportFrame::predict`]).

use body_rig::contact::{SupportFrame, SupportId, SupportMotion};
use glam::Quat;
use jc3vr_core::published::Stamp;
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::ik_body::tuning::IK_BODY,
        facts::climb::{body_frame, body_identity},
    },
    tuning::FRESHNESS,
};

/// The support the body `body` stands for: the static world for a static body, and the body itself
/// otherwise, recorded so its frame is read from now on.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn identify(body: u32, now: Stamp) -> SupportId {
    let Some(frame) = (unsafe { body_frame(body) }) else {
        return SupportId::WORLD;
    };
    if frame.is_static {
        return SupportId::WORLD;
    }
    let Some(identity) = (unsafe { body_identity(body) }) else {
        return SupportId::WORLD;
    };
    let id = support_id(body, identity.lifetime);
    let mut supports = SUPPORTS.lock();
    if !supports.iter().any(|s| s.id == id) {
        if supports.len() >= MAX_SUPPORTS {
            supports.sort_by_key(|s| std::cmp::Reverse(s.asked));
            supports.truncate(MAX_SUPPORTS - 1);
        }
        let mut tracked = Tracked {
            id,
            body,
            owner: identity.owner,
            lifetime: identity.lifetime,
            asked: now,
            snapshot: None,
        };
        tracked.snapshot = unsafe { snapshot(&tracked, now) };
        supports.push(tracked);
        tracing::debug!(
            target: "body",
            body,
            owner = format_args!("{:#x}", identity.owner),
            "ik body: a foot's support moves"
        );
    }
    id
}

/// Read every support the gate has asked for recently, and forget the rest.
///
/// # Safety
///
/// The game thread, outside the physics step.
pub(crate) unsafe fn tick(now: Stamp) {
    let mut supports = SUPPORTS.lock();
    supports.retain(|s| {
        FRESHNESS
            .get()
            .body
            .ik_support_kept_ms
            .window()
            .holds(s.asked, now)
    });
    for support in supports.iter_mut() {
        support.snapshot = unsafe { snapshot(support, now) };
    }
}

/// The frame of `support` at `now`, or `None` once it is gone.
pub(crate) fn frame(support: SupportId, now: Stamp) -> Option<SupportFrame> {
    if support == SupportId::WORLD {
        return Some(SupportFrame::IDENTITY);
    }
    let mut supports = SUPPORTS.lock();
    let tracked = supports.iter_mut().find(|s| s.id == support)?;
    tracked.asked = now;
    let snapshot = tracked.snapshot?;
    let age = now.saturating_duration_since(snapshot.at);
    (age <= FRESHNESS.get().body.ik_support_ms.duration()).then(|| {
        snapshot.frame.predict(
            &snapshot.motion,
            age.as_secs_f32().min(IK_BODY.get().support.max_predict_s),
        )
    })
}

/// How many moving supports are tracked, for the readout.
pub(crate) fn tracked() -> usize {
    SUPPORTS.lock().len()
}

/// Forget every support: a teleport.
pub(crate) fn forget() {
    SUPPORTS.lock().clear();
}

/// A moving support's identity: the body's id, and the low bits of its lifetime stamp above it.
/// Never [`SupportId::WORLD`], since the id is offset by one.
fn support_id(body: u32, lifetime: u64) -> SupportId {
    SupportId(((lifetime & u64::from(u32::MAX)) << 32) | (u64::from(body) + 1))
}

/// The frame and motion of `support`'s body at `now`, checked to be the body it was recorded from.
///
/// # Safety
///
/// The game thread, outside the physics step.
unsafe fn snapshot(support: &Tracked, now: Stamp) -> Option<Snapshot> {
    let identity = unsafe { body_identity(support.body) }?;
    if identity.owner != support.owner || identity.lifetime != support.lifetime {
        return None;
    }
    let body = unsafe { body_frame(support.body) }?;
    let rotation = Quat::from_mat3a(&body.transform.matrix3);
    let frame = SupportFrame {
        translation: body.transform.translation.into(),
        rotation: if rotation.is_finite() {
            rotation.normalize()
        } else {
            return None;
        },
    };
    let motion = SupportMotion {
        centre: body.centre,
        linear: body.linear,
        angular: body.angular,
    };
    Some(Snapshot {
        frame,
        motion,
        at: now,
    })
}

/// The most moving supports tracked at once: two feet, and the supports they just left.
const MAX_SUPPORTS: usize = 8;

#[derive(Debug, Copy, Clone, PartialEq)]
struct Snapshot {
    frame: SupportFrame,
    motion: SupportMotion,
    at: Stamp,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct Tracked {
    id: SupportId,
    body: u32,
    owner: usize,
    lifetime: u64,
    /// When the gate last asked for its frame, or the probe found it.
    asked: Stamp,
    snapshot: Option<Snapshot>,
}

static SUPPORTS: Mutex<Vec<Tracked>> = Mutex::new(Vec::new());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moving_support_is_never_the_world_and_a_reused_id_is_another_support() {
        assert_ne!(support_id(0, 0), SupportId::WORLD);
        assert_ne!(support_id(41, 3), support_id(41, 4));
        assert_ne!(support_id(41, 3), support_id(42, 3));
    }

    #[test]
    fn the_world_needs_no_snapshot_and_an_unknown_support_is_gone() {
        let now = Stamp::now();
        assert_eq!(frame(SupportId::WORLD, now), Some(SupportFrame::IDENTITY));
        assert_eq!(frame(support_id(9_999, 77), now), None);
    }
}
