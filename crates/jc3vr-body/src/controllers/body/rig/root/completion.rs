//! The movement completed at the pose-finalization boundary and copied at the frame's.

use glam::Mat4;
use jc3vr_core::published::Stamp;

use crate::controllers::body::rig::{
    collision::probe,
    root::{
        Sim,
        ledger::{CompletedMovement, LedgerCompletion},
    },
};

impl Sim {
    /// The running totals the frame folds into the cockpit shift, settled first when the
    /// simulation half has not run within the staleness window.
    pub(super) fn frame_movement(&mut self, now: Stamp) -> CompletedMovement {
        if !probe::stale_after().holds(self.status.at, now) {
            self.movement.clear();
        }
        self.movement.completed()
    }

    /// Complete the tick's request against the root's displacement. A teleport since the
    /// simulation half last ran drops the pending request first, so it is never credited.
    pub(super) fn complete_movement(
        &mut self,
        previous_root: Mat4,
        current_root: Mat4,
        epoch: u64,
    ) -> LedgerCompletion {
        if epoch != self.seen_epoch {
            self.movement.clear();
        }
        self.movement.complete(
            previous_root.w_axis.truncate(),
            current_root.w_axis.truncate(),
        )
    }
}
