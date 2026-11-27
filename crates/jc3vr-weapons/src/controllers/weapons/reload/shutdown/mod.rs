//! The magazine executor's part in shutdown: its lifecycle participant, and the admission it opens
//! once the native serial callback's detour is live and closes once every hook is removed.
//!
//! Shutdown preparation stops magazine extraction and pending ejects, and drains the serial
//! executor's queue. It is ready only once no native magazine callback is running in payload code
//! ([`jc3vr_body::hooks::character::magazine::active_entries`]).

use std::sync::Arc;

use jc3vr_body::hooks::character::magazine;
use jc3vr_core::{
    lifecycle::{
        DeferReason, ParticipantId, Preparation, PrepareOutcome, PreparedParticipant,
        ShutdownParticipant,
    },
    thread_role::{self, ThreadRole},
};

use crate::controllers::weapons::reload::{cancel_extraction_on_standdown, serial};

/// The name the magazine executor's participant registers under.
pub const PARTICIPANT: ParticipantId = ParticipantId::new("the magazine executor");

/// The participant that keeps shutdown waiting while magazine work is pending or running.
pub fn participant() -> Arc<dyn ShutdownParticipant> {
    Arc::new(MagazineShutdown)
}

/// Open magazine admission after the native serial callback detour has been installed.
pub fn activate() {
    serial::set_installed(true);
}

/// Close magazine admission once hook removal has reverted every patch. Never refuses removal.
pub fn deactivate_after_unpatch() -> bool {
    serial::set_installed(false);
    true
}

struct MagazineShutdown;

impl ShutdownParticipant for MagazineShutdown {
    fn prepare(&self, preparation: Preparation) -> PrepareOutcome<Box<dyn PreparedParticipant>> {
        if thread_role::current() != ThreadRole::Game {
            return PrepareOutcome::Deferred(DeferReason::Busy {
                participant: PARTICIPANT,
                cause: "preparation runs only on the game thread",
            });
        }
        // Only the magazine work is canceled: preparation can be retried, and the rest of the
        // weapon handling must not stand down meanwhile.
        cancel_extraction_on_standdown();
        match magazine_preparation(serial::stop_and_drain(), magazine::active_entries() != 0) {
            PrepareOutcome::Ready(()) => {
                PrepareOutcome::Ready(Box::new(InactiveMagazine { preparation }))
            }
            PrepareOutcome::Deferred(reason) => PrepareOutcome::Deferred(reason),
            PrepareOutcome::Failed(error) => PrepareOutcome::Failed(error),
        }
    }
}

const MAGAZINE_BUSY: DeferReason = DeferReason::Busy {
    participant: PARTICIPANT,
    cause: "magazine work is pending or a native magazine callback is running",
};

fn magazine_preparation(drained: bool, entry_active: bool) -> PrepareOutcome<()> {
    if !drained || entry_active {
        PrepareOutcome::Deferred(MAGAZINE_BUSY)
    } else {
        PrepareOutcome::Ready(())
    }
}

struct InactiveMagazine {
    preparation: Preparation,
}

impl PreparedParticipant for InactiveMagazine {
    fn validate(&self, preparation: Preparation) -> bool {
        self.preparation == preparation
            && magazine::active_entries() == 0
            && serial::stop_and_drain()
    }

    fn commit(&mut self) {}
}

#[cfg(test)]
mod tests;
