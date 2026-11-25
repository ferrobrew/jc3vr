//! Source-owned identity for live cockpit tracking acquisition and its rendered composition.
//!
//! Tokens cover baseline, LOCAL reference-space, runtime, and detected teleport discontinuities.
//! They do not identify native/root history, calibration, scale, authority, or support.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use openxr as xr;

use crate::controllers::frame::ControllerFrame;

/// A non-reused source identity. Only the source can construct a token.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct OriginToken(u64);

/// Why acquisition or composition has no supported origin identity.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum OriginUnavailable {
    SourceAbsent,
    DiagnosticExpired,
    MutationInProgress,
    GenerationExhausted,
    ReferenceSpacePending,
    ReferenceSpaceUnknown,
    ChangedDuringAcquisition,
}

/// Evidence attached to the acquired packet, never inferred when reading diagnostics.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum OriginEvidence {
    Known(OriginToken),
    Unavailable(OriginUnavailable),
}

impl OriginEvidence {
    /// Check the held identity at the end of acquisition or composition without relabelling it.
    pub(crate) fn finish(self) -> Self {
        self.finish_with(&SOURCE)
    }

    fn finish_with(self, source: &Source) -> Self {
        match self {
            Self::Unavailable(_) => self,
            Self::Known(token) => match source.capture() {
                Self::Known(current) if current == token => self,
                Self::Known(_) => Self::Unavailable(OriginUnavailable::ChangedDuringAcquisition),
                unavailable => unavailable,
            },
        }
    }
}

/// The actual controller receipt associated with the acquired live head and frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct TrackingReceipt {
    sequence: u64,
    display_time_ns: i64,
    published_at: Instant,
}

impl TrackingReceipt {
    pub(crate) fn from_frame(frame: &ControllerFrame) -> Self {
        Self::from_receipt(frame.sequence, frame.display_time_ns, frame.published_at)
    }

    pub(crate) fn from_receipt(sequence: u64, display_time_ns: i64, published_at: Instant) -> Self {
        Self {
            sequence,
            display_time_ns,
            published_at,
        }
    }
}

/// Capture once before locating the live head and grips.
/// Reference-space readiness belongs to the locked runtime that supplies those poses.
pub(crate) fn begin(readiness: Result<(), OriginUnavailable>) -> OriginEvidence {
    match readiness {
        Ok(()) => SOURCE.capture(),
        Err(reason) => OriginEvidence::Unavailable(reason),
    }
}

/// An atomic-only invalidation held across an actual source mutation and its dependent rebases.
/// This never takes a runtime, configuration, body, or publication lock.
pub(crate) fn changing() -> OriginMutation<'static> {
    SOURCE.changing()
}

pub(crate) struct OriginMutation<'a> {
    source: &'a Source,
}

impl Drop for OriginMutation<'_> {
    fn drop(&mut self) {
        let _ = self
            .source
            .state
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                if state == 0 || state & ACTIVE_MASK == 0 {
                    None
                } else {
                    Some(state - 1)
                }
            });
    }
}

/// Pending LOCAL-space changes for one runtime session, under the existing runtime lock.
/// The event helper ignores unrelated sessions and space types.
pub(crate) struct ReferenceSpaceChanges {
    pending: [Option<i64>; PENDING_CAPACITY],
    unknown: bool,
    last_display_time: Option<i64>,
}

impl ReferenceSpaceChanges {
    pub(crate) const fn new() -> Self {
        Self {
            pending: [None; PENDING_CAPACITY],
            unknown: false,
            last_display_time: None,
        }
    }

    pub(crate) fn note_event(
        &mut self,
        matches_session: bool,
        space: xr::ReferenceSpaceType,
        change_time_ns: i64,
    ) {
        self.note_event_with(matches_session, space, change_time_ns, &SOURCE);
    }

    fn note_event_with(
        &mut self,
        matches_session: bool,
        space: xr::ReferenceSpaceType,
        change_time_ns: i64,
        source: &Source,
    ) {
        if matches_session && space == xr::ReferenceSpaceType::LOCAL {
            self.note_pending_with(change_time_ns, source);
        }
    }

    fn note_pending_with(&mut self, change_time_ns: i64, source: &Source) {
        let _mutation = source.changing();
        if let Some(slot) = self.pending.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(change_time_ns);
        } else {
            self.unknown = true;
        }
    }

    pub(crate) fn events_unknown(&mut self) {
        self.events_unknown_with(&SOURCE);
    }

    fn events_unknown_with(&mut self, source: &Source) {
        let _mutation = source.changing();
        self.unknown = true;
    }

    /// Resolve only recorded change times. No previous transform is reconstructed.
    pub(crate) fn prepare(&mut self, display_time_ns: i64) -> Result<(), OriginUnavailable> {
        self.prepare_with(display_time_ns, &SOURCE)
    }

    fn prepare_with(
        &mut self,
        display_time_ns: i64,
        source: &Source,
    ) -> Result<(), OriginUnavailable> {
        if self
            .last_display_time
            .is_some_and(|previous| display_time_ns < previous)
        {
            let _mutation = source.changing();
            self.unknown = true;
        }
        self.last_display_time = Some(display_time_ns);
        if self.unknown {
            return Err(OriginUnavailable::ReferenceSpaceUnknown);
        }
        for pending in &mut self.pending {
            if pending.is_some_and(|at| display_time_ns >= at) {
                let _mutation = source.changing();
                *pending = None;
            }
        }
        if self.pending.iter().any(Option::is_some) {
            Err(OriginUnavailable::ReferenceSpacePending)
        } else {
            Ok(())
        }
    }
}

// The active count and generation share one atomic so overlapping writers cannot publish a
// stable intermediate state. Exhaustion is terminal and runtime resets never reset this source.
const ACTIVE_BITS: u32 = 16;
const GENERATION_STEP: u64 = 1 << ACTIVE_BITS;
const ACTIVE_MASK: u64 = GENERATION_STEP - 1;
const PENDING_CAPACITY: usize = 4;

struct Source {
    state: AtomicU64,
}

impl Source {
    const fn new() -> Self {
        Self {
            state: AtomicU64::new(GENERATION_STEP),
        }
    }

    fn capture(&self) -> OriginEvidence {
        let state = self.state.load(Ordering::SeqCst);
        if state == 0 {
            OriginEvidence::Unavailable(OriginUnavailable::GenerationExhausted)
        } else if state & ACTIVE_MASK != 0 {
            OriginEvidence::Unavailable(OriginUnavailable::MutationInProgress)
        } else {
            OriginEvidence::Known(OriginToken(state >> ACTIVE_BITS))
        }
    }

    fn changing(&self) -> OriginMutation<'_> {
        let _ = self
            .state
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                Some(if state == 0 || state & ACTIVE_MASK == ACTIVE_MASK {
                    0
                } else {
                    state.checked_add(GENERATION_STEP + 1).unwrap_or(0)
                })
            });
        OriginMutation { source: self }
    }
}

static SOURCE: Source = Source::new();

#[cfg(test)]
pub(crate) fn test_origins() -> [OriginEvidence; 2] {
    let source = Source::new();
    let before = source.capture();
    drop(source.changing());
    [before, source.capture()]
}

#[cfg(test)]
#[path = "origin_provenance_tests.rs"]
mod tests;
