//! Copied native resting-handle samples and their input-session policy.
//!
//! Rig-model joint translations compose through the sampled parachute-object transform, then the
//! current cockpit frame. The character root is provenance only, never a substitute for that
//! transform. The live adapter retains the owner's weak reference outside this copied model.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};

use crate::controllers::{body::rig::frames::Frames, vehicles::wheel::CockpitTransform};

/// Why a free handle has no normal or wider-reach target.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RestUnavailable {
    Inactive,
    MissingSample,
    SessionChanged,
    OwnerChanged,
    Stale,
    FutureSample,
    InvalidCharacterRoot,
    TransformUnavailable,
    InvalidFrames,
    MissingJoint,
    InvalidPoint,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct OwnerIdentity {
    pub character: usize,
    pub control_block: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Session {
    pub owner: OwnerIdentity,
    pub generation: u64,
}

/// Opaque comparison tokens only. The adapter never reconstructs pointers from these values.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct RigIdentity {
    pub object: usize,
    pub control_block: usize,
    pub model: usize,
    pub controller: usize,
}

/// Copied values only. The adapter retains the owner's weak control block while the session acts,
/// so character-address reuse cannot also reuse the allocation identity.
#[derive(Debug, Copy, Clone)]
pub(super) struct NativeRestSnapshot {
    pub session: Session,
    pub sequence: u64,
    pub sampled_at: Instant,
    pub rig: RigIdentity,
    pub character_world: Mat4,
    pub rig_world: Option<Mat4>,
    pub rest_model: [Option<Vec3>; 2],
}

impl NativeRestSnapshot {
    pub(super) fn centres(
        self,
        session: Session,
        cockpit: &CockpitTransform,
        now: Instant,
    ) -> [Result<Vec3, RestUnavailable>; 2] {
        let frames = self.frames(session, cockpit, now);
        self.rest_model.map(|point| {
            let frames = frames?;
            let point = point.ok_or(RestUnavailable::MissingJoint)?;
            if !point.is_finite() {
                return Err(RestUnavailable::InvalidPoint);
            }
            let point = frames.model_to_cockpit(point);
            point
                .is_finite()
                .then_some(point)
                .ok_or(RestUnavailable::InvalidPoint)
        })
    }

    fn frames(
        self,
        session: Session,
        cockpit: &CockpitTransform,
        now: Instant,
    ) -> Result<Frames, RestUnavailable> {
        if self.session.owner != session.owner {
            return Err(RestUnavailable::OwnerChanged);
        }
        if self.session.generation != session.generation {
            return Err(RestUnavailable::SessionChanged);
        }
        let age = now
            .checked_duration_since(self.sampled_at)
            .ok_or(RestUnavailable::FutureSample)?;
        if age > STALE_AFTER {
            return Err(RestUnavailable::Stale);
        }
        if !usable_matrix(self.character_world) {
            return Err(RestUnavailable::InvalidCharacterRoot);
        }
        let frames = Frames {
            head_world: cockpit.head_world,
            head_cockpit: cockpit.head_cockpit,
            view: cockpit.body,
            world_scale: cockpit.world_scale,
            character: self
                .rig_world
                .ok_or(RestUnavailable::TransformUnavailable)?,
        };
        (frames.is_usable() && usable_matrix(frames.character))
            .then_some(frames)
            .ok_or(RestUnavailable::InvalidFrames)
    }
}

pub(super) struct State {
    pub generation: u64,
    pub active: Option<(Session, Instant)>,
    pub holds: Option<([bool; 2], Instant)>,
    pub snapshot: Option<NativeRestSnapshot>,
    rig: Option<RigIdentity>,
    last_sequence: u64,
}

impl State {
    pub(super) const fn new() -> Self {
        Self {
            generation: 0,
            active: None,
            holds: None,
            snapshot: None,
            rig: None,
            last_sequence: 0,
        }
    }

    pub(super) fn configure(&mut self, owner: Option<OwnerIdentity>, now: Instant) {
        let continuing = self.active.is_some_and(|(session, at)| {
            Some(session.owner) == owner
                && now
                    .checked_duration_since(at)
                    .is_some_and(|age| age <= STALE_AFTER)
        });
        if !continuing && (self.active.is_some() || owner.is_some()) {
            self.clear_session();
        }
        self.active = owner.map(|owner| {
            (
                Session {
                    owner,
                    generation: self.generation,
                },
                now,
            )
        });
    }

    pub(super) fn request(
        &self,
        owner: Option<OwnerIdentity>,
        now: Instant,
    ) -> Option<(Session, [bool; 2])> {
        let (session, at) = self.active?;
        if Some(session.owner) != owner
            || now
                .checked_duration_since(at)
                .is_none_or(|age| age > STALE_AFTER)
        {
            return None;
        }
        Some((session, unplaced(self.holds, now)))
    }

    /// Record one whole pre-sync pair. A replacement rig retires the old pair and held state; its
    /// next callback must sample under the new generation before it can offer a free handle.
    pub(super) fn record(&mut self, snapshot: NativeRestSnapshot) -> bool {
        if self
            .active
            .is_none_or(|(session, _)| session != snapshot.session)
            || snapshot.sequence == 0
            || self.last_sequence >= snapshot.sequence
        {
            return false;
        }
        self.last_sequence = snapshot.sequence;
        if self.rig.is_some_and(|rig| rig != snapshot.rig) {
            self.generation = self.generation.wrapping_add(1);
            if let Some((session, _)) = self.active.as_mut() {
                session.generation = self.generation;
            }
            self.holds = None;
            self.snapshot = None;
            self.rig = Some(snapshot.rig);
            return true;
        }
        self.rig = Some(snapshot.rig);
        self.snapshot = Some(snapshot);
        true
    }

    pub(super) fn centres(
        &self,
        cockpit: &CockpitTransform,
        now: Instant,
    ) -> [Result<Vec3, RestUnavailable>; 2] {
        let Some((session, at)) = self.active else {
            return [Err(RestUnavailable::Inactive); 2];
        };
        let Some(age) = now.checked_duration_since(at) else {
            return [Err(RestUnavailable::FutureSample); 2];
        };
        if age > STALE_AFTER {
            return [Err(RestUnavailable::Stale); 2];
        }
        self.snapshot
            .map_or([Err(RestUnavailable::MissingSample); 2], |sample| {
                sample.centres(session, cockpit, now)
            })
    }

    fn clear_session(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.holds = None;
        self.snapshot = None;
        self.rig = None;
        self.last_sequence = 0;
    }
}

pub(super) fn usable_matrix(matrix: Mat4) -> bool {
    let determinant = matrix.determinant();
    matrix.is_finite()
        && determinant.is_finite()
        && determinant.abs() > 1.0e-6
        && matrix.x_axis.w.abs() <= 1.0e-5
        && matrix.y_axis.w.abs() <= 1.0e-5
        && matrix.z_axis.w.abs() <= 1.0e-5
        && (matrix.w_axis.w - 1.0).abs() <= 1.0e-5
}

pub(super) fn unplaced(holds: Option<([bool; 2], Instant)>, now: Instant) -> [bool; 2] {
    match holds {
        Some((held, at))
            if now
                .checked_duration_since(at)
                .is_some_and(|age| age <= STALE_AFTER) =>
        {
            held.map(|h| !h)
        }
        _ => [false; 2],
    }
}

pub(super) const STALE_AFTER: Duration = Duration::from_millis(250);

#[cfg(test)]
#[path = "parachute_rig_tests.rs"]
mod tests;
