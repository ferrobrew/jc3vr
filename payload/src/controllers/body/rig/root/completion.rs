//! Completion identities and the source copied at the frame's movement boundary.

use std::time::Instant;

use glam::Mat4;

use crate::controllers::body::rig::{
    collision::probe,
    origin_provenance::{OriginEvidence, OriginUnavailable, TrackingReceipt},
    root::{
        Sim,
        ledger::{CompletedMovement, LedgerCompletion},
        shift::{CockpitBasis, CockpitShift, ShiftPair},
    },
};

/// A publishable local pose-finalization identity, never a clock or simulation-step identity.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct CompletionToken(u64);

impl CompletionToken {
    #[cfg(test)]
    pub(crate) fn sequence(self) -> u64 {
        self.0
    }
}

/// One local pose-finalization completion; the owner belongs only to its consumed request.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MovementCompletion {
    pub sequence: u64,
    pub completed_at: Instant,
    pub previous_root: Mat4,
    pub current_root: Mat4,
    pub ledger: LedgerCompletion,
    token: CompletionToken,
}

impl MovementCompletion {
    pub(crate) fn token(self) -> CompletionToken {
        self.token
    }
}

/// Why no latest completion record is available for diagnostic inspection.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionUnavailable {
    NotCompleted,
    Cleared,
    Teleported,
    SourceUnavailable,
    SequenceExhausted,
}

/// Availability of the latest completion, independent of its owned record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionSnapshotAvailability {
    Unavailable(CompletionUnavailable),
    Completed,
}

/// A fixed-size latest completion, never a checked body pair.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CompletionSnapshot {
    availability: CompletionSnapshotAvailability,
    completion: Option<MovementCompletion>,
}

impl CompletionSnapshot {
    pub(crate) const fn unavailable(reason: CompletionUnavailable) -> Self {
        Self {
            availability: CompletionSnapshotAvailability::Unavailable(reason),
            completion: None,
        }
    }

    const fn completed(completion: MovementCompletion) -> Self {
        Self {
            availability: CompletionSnapshotAvailability::Completed,
            completion: Some(completion),
        }
    }

    pub(crate) fn availability(self) -> CompletionSnapshotAvailability {
        self.availability
    }

    pub(crate) fn completion(self) -> Option<MovementCompletion> {
        self.completion
    }
}

/// Movement and completion copied together while the simulation lock is held.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct FrameMovementSnapshot {
    movement: CompletedMovement,
    completion: CompletionSnapshot,
}

impl FrameMovementSnapshot {
    pub(super) fn movement(self) -> CompletedMovement {
        self.movement
    }

    pub(super) fn compose(
        self,
        current_basis: CockpitBasis,
        shift: &CockpitShift,
    ) -> FrameSourceSnapshot {
        FrameSourceSnapshot {
            completion: self.completion,
            movement: self.movement,
            current_basis,
            shift: shift.pair(),
            tracking_origin: OriginEvidence::Unavailable(OriginUnavailable::SourceAbsent),
            tracking_receipt: None,
        }
    }
}

/// The exact movement source and final shift used by one root frame composition.
/// `current_basis` reconstructs both shift sides; it is not a historical basis pair.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FrameSourceSnapshot {
    completion: CompletionSnapshot,
    movement: CompletedMovement,
    current_basis: CockpitBasis,
    shift: ShiftPair,
    tracking_origin: OriginEvidence,
    tracking_receipt: Option<TrackingReceipt>,
}

impl FrameSourceSnapshot {
    /// Origin of live tracking acquired for this composition, not of its root/native sources.
    pub(crate) fn tracking_origin(self) -> OriginEvidence {
        self.tracking_origin
    }

    pub(crate) fn tracking_receipt(self) -> Option<TrackingReceipt> {
        self.tracking_receipt
    }

    pub(crate) fn with_tracking_origin(
        mut self,
        origin: OriginEvidence,
        receipt: Option<TrackingReceipt>,
    ) -> Self {
        self.tracking_origin = origin;
        self.tracking_receipt = receipt;
        self
    }

    pub(crate) fn completion(self) -> CompletionSnapshot {
        self.completion
    }

    pub(crate) fn movement(self) -> CompletedMovement {
        self.movement
    }

    pub(crate) fn current_basis(self) -> CockpitBasis {
        self.current_basis
    }

    pub(crate) fn shift(self) -> ShiftPair {
        self.shift
    }
}

impl Sim {
    pub(super) fn completion_snapshot(&self, epoch: u64) -> CompletionSnapshot {
        if epoch != self.seen_epoch {
            CompletionSnapshot::unavailable(CompletionUnavailable::Teleported)
        } else {
            self.completion
        }
    }

    pub(super) fn frame_movement(&mut self, now: Instant, epoch: u64) -> FrameMovementSnapshot {
        if self
            .status
            .at
            .is_none_or(|at| now.saturating_duration_since(at) > probe::STALE_AFTER)
        {
            self.clear_movement(CompletionUnavailable::SourceUnavailable);
        }
        FrameMovementSnapshot {
            movement: self.movement.completed(),
            completion: self.completion_snapshot(epoch),
        }
    }

    pub(super) fn clear_movement(&mut self, reason: CompletionUnavailable) {
        self.movement.clear();
        self.completion = CompletionSnapshot::unavailable(reason);
    }

    pub(super) fn complete_movement(
        &mut self,
        previous_root: Mat4,
        current_root: Mat4,
        completed_at: Instant,
        epoch: u64,
    ) -> Option<CompletionToken> {
        let teleported = epoch != self.seen_epoch;
        if teleported {
            self.clear_movement(CompletionUnavailable::Teleported);
        }
        let ledger = self.movement.complete(
            previous_root.w_axis.truncate(),
            current_root.w_axis.truncate(),
        );
        let Some(sequence) = self.completion_sequence.checked_add(1) else {
            self.completion =
                CompletionSnapshot::unavailable(CompletionUnavailable::SequenceExhausted);
            return None;
        };
        self.completion_sequence = sequence;
        if teleported {
            return None;
        }
        let token = CompletionToken(sequence);
        self.completion = CompletionSnapshot::completed(MovementCompletion {
            sequence,
            completed_at,
            previous_root,
            current_root,
            ledger,
            token,
        });
        Some(token)
    }
}

#[cfg(test)]
pub(crate) fn test_source(sequence: u64, now: Instant) -> (CompletionToken, FrameSourceSnapshot) {
    test_source_with_root(sequence, now, Mat4::from_translation(glam::Vec3::X))
}

#[cfg(test)]
pub(crate) fn test_source_with_root(
    sequence: u64,
    now: Instant,
    current_root: Mat4,
) -> (CompletionToken, FrameSourceSnapshot) {
    use glam::{Quat, Vec2};

    use crate::controllers::body::rig::root::{ledger::Movement, owner::RootOwner};

    let mut sim = Sim::new();
    sim.completion_sequence = sequence - 1;
    sim.status.at = Some(now);
    sim.movement
        .submit_owned(Movement::new(Vec2::ZERO, Vec2::X, 1.0), RootOwner::Player);
    let token = sim
        .complete_movement(Mat4::IDENTITY, current_root, now, 0)
        .unwrap();
    let source = sim.frame_movement(now, 0);
    let basis = CockpitBasis {
        body: Quat::IDENTITY,
        world_scale: 1.0,
    };
    let mut shift = CockpitShift::new();
    shift.observe(&basis, source.movement());
    (token, source.compose(basis, &shift))
}
