//! Bounded read-only tracking, native pre-IK, and movement-completion evidence.
//!
//! The raw packet is the VR runtime's live tracking publication ([`crate::vr::live_tracking`]).
//! Native and completion captures remain independent of it; host times never join simulation sides.
//! Exact completion tokens match frame movement and shift evidence, without historical basis sides.
//! Tracking-origin evidence covers only the acquired live packet through its exact frame receipt.
//! Missing per-side ownership, native history, support, and epochs prevent a checked observation.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::body::rig::{
        origin_provenance::{OriginEvidence, OriginToken, OriginUnavailable, TrackingReceipt},
        root::{
            self, CompletionSnapshot, CompletionSnapshotAvailability, CompletionToken,
            CompletionUnavailable, FrameSourceSnapshot, MovementCompletion,
        },
    },
    headpose::teleport,
    vr::{
        self,
        live_tracking::{
            self, DIAGNOSTIC_MAX_AGE, RawObservation, RawPacket, Unavailable, age_at, unit_rotation,
        },
    },
};

/// Values read together before the local character's procedural IK writes.
/// The previous anchor retains the preceding pre-override capture when its native tick is adjacent.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct NativeCapture {
    pub completion: Option<CompletionToken>,
    pub previous_root: Mat4,
    pub current_root: Mat4,
    pub previous_anchor: Vec3,
    pub current_anchor: Vec3,
    pub current_head_model: Vec3,
    pub current_neck_model: Vec3,
    pub current_eye_mid_model: Vec3,
}

/// Evidence still required before publishing a checked body observation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum MissingEvidence {
    CompletedLedgerAndShiftPair,
    CompositionBasisPair,
    /// Owners of both matched composition sides, beyond one consumed request's owner.
    OwnerAtCompletion,
    OriginEpoch,
    CalibrationAndScaleEpochs,
    AuthorityEpoch,
    PreviousNativePose,
    SupportIdentity,
}

/// Native capture availability, independent of its owned record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum NativeAvailability {
    Unavailable(Unavailable),
    Observed { age: Duration },
}

/// A fixed-size latest native capture, never a checked body observation.
/// Unavailable observations omit the capture rather than retaining a previous publication.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct NativeObservation {
    availability: NativeAvailability,
    capture: Option<NativeCapture>,
}

impl NativeObservation {
    fn unavailable(reason: Unavailable) -> Self {
        Self {
            availability: NativeAvailability::Unavailable(reason),
            capture: None,
        }
    }

    fn observed(capture: NativeCapture, age: Duration) -> Self {
        Self {
            availability: NativeAvailability::Observed { age },
            capture: Some(capture),
        }
    }

    pub(crate) fn availability(&self) -> NativeAvailability {
        self.availability
    }

    pub(crate) fn capture(&self) -> Option<NativeCapture> {
        self.capture
    }
}

/// Diagnostic rejection of a completion snapshot, separate from movement eligibility.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionRejection {
    Source(CompletionUnavailable),
    Diagnostic(Unavailable),
}

/// Diagnostic completion availability, independent of the owned completion record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum CompletionAvailability {
    Unavailable(CompletionRejection),
    Observed { age: Duration },
}

/// A fixed-size latest completion, never matched to tracking or a rendered composition.
/// Unavailable observations omit the record rather than retaining a previous completion.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct CompletionObservation {
    availability: CompletionAvailability,
    completion: Option<MovementCompletion>,
}

impl CompletionObservation {
    fn unavailable(reason: CompletionRejection) -> Self {
        Self {
            availability: CompletionAvailability::Unavailable(reason),
            completion: None,
        }
    }

    fn observed(completion: MovementCompletion, age: Duration) -> Self {
        Self {
            availability: CompletionAvailability::Observed { age },
            completion: Some(completion),
        }
    }

    pub(crate) fn availability(&self) -> CompletionAvailability {
        self.availability
    }

    pub(crate) fn completion(&self) -> Option<MovementCompletion> {
        self.completion
    }
}

/// Why the exact frame source cannot be matched to the present native completion.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum MatchUnavailable {
    NoFrame,
    FrameDiagnostic(Unavailable),
    Completion(CompletionRejection),
    Native(Unavailable),
    MissingNativeToken,
    DifferentCompletion,
    InvalidFrameSource,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum FrameMatchAvailability {
    Unavailable(MatchUnavailable),
    Matched,
}

/// Matched identity evidence only, not historical composition sides or a checked body pair.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MatchedFrameSource {
    pub source: FrameSourceSnapshot,
    pub native: NativeCapture,
}

/// Fixed-size match availability with no retained packet on rejection.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FrameMatchObservation {
    availability: FrameMatchAvailability,
    matched: Option<MatchedFrameSource>,
}

impl FrameMatchObservation {
    fn unavailable(reason: MatchUnavailable) -> Self {
        Self {
            availability: FrameMatchAvailability::Unavailable(reason),
            matched: None,
        }
    }

    fn matched(source: FrameSourceSnapshot, native: NativeCapture) -> Self {
        Self {
            availability: FrameMatchAvailability::Matched,
            matched: Some(MatchedFrameSource { source, native }),
        }
    }

    #[cfg(test)]
    pub(crate) fn availability(self) -> FrameMatchAvailability {
        self.availability
    }

    pub(crate) fn matched_source(self) -> Option<MatchedFrameSource> {
        self.matched
    }
}

/// Origin eligibility for live tracking through composition only. Native/root history is separate.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum TrackingOriginAvailability {
    Unavailable(OriginUnavailable),
    DifferentOrigins,
    DifferentTrackingPacket,
    SameOrigin(OriginToken),
}

/// Separate source availability and the missing checked-observation prerequisites.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ObservationStatus {
    pub raw: RawObservation,
    pub native: NativeObservation,
    pub completion: CompletionObservation,
    pub frame_match: FrameMatchObservation,
    pub tracking_origin: TrackingOriginAvailability,
    pub missing: &'static [MissingEvidence],
}

/// Replace the single owned capture. Host time is not a simulation-step identity.
pub(crate) fn publish_native(capture: NativeCapture) {
    SLOT.lock()
        .publish_native(capture, Instant::now(), OriginCounters::current());
}

/// Clear a previous capture when the local hook cannot read its animation controller.
pub(crate) fn native_unavailable() {
    SLOT.lock().clear_native();
}

/// Publish the exact source returned through the current rendered cockpit composition.
pub(crate) fn publish_composition(source: FrameSourceSnapshot) {
    SLOT.lock()
        .publish_composition(source, Instant::now(), OriginCounters::current());
}

/// Clear composition evidence when no live, unfrozen VR pose pair is published.
pub(crate) fn composition_unavailable() {
    SLOT.lock().clear_composition();
}

/// Read current diagnostic availability without reading engine pointers or composing a pose.
#[expect(
    dead_code,
    reason = "no checked body observation reads the joined evidence yet"
)]
pub(crate) fn status() -> ObservationStatus {
    let completion = root::completion_snapshot();
    let raw = live_tracking::publication();
    let now = Instant::now();
    SLOT.lock().read_completion_status(
        raw,
        now,
        DIAGNOSTIC_MAX_AGE,
        OriginCounters::current(),
        completion,
    )
}

const MISSING: &[MissingEvidence] = &[
    MissingEvidence::CompletedLedgerAndShiftPair,
    MissingEvidence::CompositionBasisPair,
    MissingEvidence::OwnerAtCompletion,
    MissingEvidence::OriginEpoch,
    MissingEvidence::CalibrationAndScaleEpochs,
    MissingEvidence::AuthorityEpoch,
    MissingEvidence::PreviousNativePose,
    MissingEvidence::SupportIdentity,
];

const MISSING_AFTER_MATCH: &[MissingEvidence] = &[
    MissingEvidence::CompositionBasisPair,
    MissingEvidence::OwnerAtCompletion,
    MissingEvidence::OriginEpoch,
    MissingEvidence::CalibrationAndScaleEpochs,
    MissingEvidence::AuthorityEpoch,
    MissingEvidence::PreviousNativePose,
    MissingEvidence::SupportIdentity,
];

/// These existing counters do not cover runtime resets, follow banking, or calibration changes.
/// Equality is only a diagnostic invalidation check, never a complete origin epoch.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct OriginCounters {
    teleports: u64,
    recenters: u32,
}

impl OriginCounters {
    fn current() -> Self {
        Self {
            teleports: teleport::epoch(),
            recenters: vr::recenter_count(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct NativePublication {
    capture: NativeCapture,
    observed_at: Instant,
    origin: OriginCounters,
}

#[derive(Debug, Copy, Clone)]
struct CompositionPublication {
    source: FrameSourceSnapshot,
    observed_at: Instant,
    origin: OriginCounters,
}

struct Publisher {
    native: Option<NativePublication>,
    composition: Option<CompositionPublication>,
}

impl Publisher {
    const fn new() -> Self {
        Self {
            native: None,
            composition: None,
        }
    }

    fn publish_native(
        &mut self,
        capture: NativeCapture,
        observed_at: Instant,
        origin: OriginCounters,
    ) {
        self.native = Some(NativePublication {
            capture,
            observed_at,
            origin,
        });
    }

    fn clear_native(&mut self) {
        self.native = None;
    }

    fn clear_composition(&mut self) {
        self.composition = None;
    }

    fn publish_composition(
        &mut self,
        source: FrameSourceSnapshot,
        observed_at: Instant,
        origin: OriginCounters,
    ) {
        self.composition = Some(CompositionPublication {
            source,
            observed_at,
            origin,
        });
    }

    #[cfg(test)]
    fn read(
        &self,
        raw: Result<RawPacket, Unavailable>,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
    ) -> ObservationStatus {
        self.read_completion_status(
            raw,
            now,
            max_age,
            origin,
            CompletionSnapshot::unavailable(CompletionUnavailable::NotCompleted),
        )
    }

    fn read_completion_status(
        &self,
        raw: Result<RawPacket, Unavailable>,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
        completion: CompletionSnapshot,
    ) -> ObservationStatus {
        let native = self.read_native(now, max_age, origin);
        let completion = read_completion(completion, now, max_age);
        let frame_match = self.read_frame_match(now, max_age, origin, completion, native);
        ObservationStatus {
            raw: live_tracking::observe(raw, now, max_age),
            native,
            completion,
            frame_match,
            tracking_origin: self.read_tracking_origin(raw, now, max_age),
            missing: if frame_match.matched_source().is_some() {
                MISSING_AFTER_MATCH
            } else {
                MISSING
            },
        }
    }

    fn read_tracking_origin(
        &self,
        raw: Result<RawPacket, Unavailable>,
        now: Instant,
        max_age: Duration,
    ) -> TrackingOriginAvailability {
        let absent = TrackingOriginAvailability::Unavailable(OriginUnavailable::SourceAbsent);
        let (Ok(packet), Some(composition)) = (raw, self.composition) else {
            return absent;
        };
        if age_at(packet.published_at, now, max_age, true).is_err()
            || age_at(composition.observed_at, now, max_age, true).is_err()
        {
            return TrackingOriginAvailability::Unavailable(OriginUnavailable::DiagnosticExpired);
        }
        let origin = match_tracking_origins(packet.origin, composition.source.tracking_origin());
        if matches!(origin, TrackingOriginAvailability::SameOrigin(_)) {
            let receipt = TrackingReceipt::from_receipt(
                packet.sequence,
                packet.display_time_ns,
                packet.published_at,
            );
            if composition.source.tracking_receipt() != Some(receipt) {
                return TrackingOriginAvailability::DifferentTrackingPacket;
            }
        }
        origin
    }

    fn read_frame_match(
        &self,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
        present: CompletionObservation,
        native: NativeObservation,
    ) -> FrameMatchObservation {
        let Some(publication) = self.composition else {
            return FrameMatchObservation::unavailable(MatchUnavailable::NoFrame);
        };
        if let Err(reason) = age_at(
            publication.observed_at,
            now,
            max_age,
            publication.origin == origin,
        ) {
            return FrameMatchObservation::unavailable(MatchUnavailable::FrameDiagnostic(reason));
        }
        let completion = read_completion(publication.source.completion(), now, max_age);
        match_frame_source(publication.source, completion, present, native)
    }

    fn read_native(
        &self,
        now: Instant,
        max_age: Duration,
        origin: OriginCounters,
    ) -> NativeObservation {
        let unavailable = NativeObservation::unavailable;
        let Some(publication) = self.native else {
            return unavailable(Unavailable::NotCaptured);
        };
        let age = match age_at(
            publication.observed_at,
            now,
            max_age,
            publication.origin == origin,
        ) {
            Ok(age) => age,
            Err(reason) => return unavailable(reason),
        };
        if !valid_capture(publication.capture) {
            return unavailable(Unavailable::InvalidCapture);
        }
        NativeObservation::observed(publication.capture, age)
    }
}

fn match_tracking_origins(
    raw: OriginEvidence,
    composition: OriginEvidence,
) -> TrackingOriginAvailability {
    match (raw, composition) {
        (OriginEvidence::Unavailable(reason), _) | (_, OriginEvidence::Unavailable(reason)) => {
            TrackingOriginAvailability::Unavailable(reason)
        }
        (OriginEvidence::Known(raw), OriginEvidence::Known(composition)) if raw == composition => {
            TrackingOriginAvailability::SameOrigin(raw)
        }
        _ => TrackingOriginAvailability::DifferentOrigins,
    }
}

fn match_frame_source(
    source: FrameSourceSnapshot,
    completion: CompletionObservation,
    present: CompletionObservation,
    native: NativeObservation,
) -> FrameMatchObservation {
    let reject = FrameMatchObservation::unavailable;
    for observed in [completion, present] {
        if let CompletionAvailability::Unavailable(reason) = observed.availability() {
            return reject(MatchUnavailable::Completion(reason));
        }
    }
    let (Some(completion), Some(present)) = (completion.completion(), present.completion()) else {
        return reject(MatchUnavailable::InvalidFrameSource);
    };
    if let NativeAvailability::Unavailable(reason) = native.availability() {
        return reject(MatchUnavailable::Native(reason));
    }
    let Some(capture) = native.capture() else {
        return reject(MatchUnavailable::Native(Unavailable::InvalidCapture));
    };
    let Some(token) = capture.completion else {
        return reject(MatchUnavailable::MissingNativeToken);
    };
    if token != completion.token() || token != present.token() {
        return reject(MatchUnavailable::DifferentCompletion);
    }
    let basis = source.current_basis();
    let shift = source.shift();
    if source.movement() != completion.ledger.movement
        || !unit_rotation(basis.body)
        || !basis.world_scale.is_finite()
        || basis.world_scale <= 0.0
        || !shift.previous.is_finite()
        || !shift.current.is_finite()
    {
        return reject(MatchUnavailable::InvalidFrameSource);
    }
    FrameMatchObservation::matched(source, capture)
}

fn read_completion(
    snapshot: CompletionSnapshot,
    now: Instant,
    max_age: Duration,
) -> CompletionObservation {
    if let CompletionSnapshotAvailability::Unavailable(reason) = snapshot.availability() {
        return CompletionObservation::unavailable(CompletionRejection::Source(reason));
    }
    let Some(completion) = snapshot.completion() else {
        return CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::InvalidCapture,
        ));
    };
    let age = match age_at(completion.completed_at, now, max_age, true) {
        Ok(age) => age,
        Err(reason) => {
            return CompletionObservation::unavailable(CompletionRejection::Diagnostic(reason));
        }
    };
    let request_valid = completion.ledger.request.is_none_or(|request| {
        request.movement.task.is_finite()
            && request.movement.drive.is_finite()
            && request.credited.is_finite()
    });
    if completion.sequence == 0
        || !valid_root(completion.previous_root)
        || !valid_root(completion.current_root)
        || !completion.ledger.movement.previous.is_finite()
        || !completion.ledger.movement.current.is_finite()
        || !request_valid
    {
        return CompletionObservation::unavailable(CompletionRejection::Diagnostic(
            Unavailable::InvalidCapture,
        ));
    }
    CompletionObservation::observed(completion, age)
}

fn valid_capture(capture: NativeCapture) -> bool {
    [capture.previous_root, capture.current_root]
        .into_iter()
        .all(valid_root)
        && [
            capture.previous_anchor,
            capture.current_anchor,
            capture.current_head_model,
            capture.current_neck_model,
            capture.current_eye_mid_model,
        ]
        .into_iter()
        .all(|point| point.is_finite())
}

fn valid_root(root: Mat4) -> bool {
    root.is_finite()
        && root.determinant().abs() > 1.0e-6
        && root.inverse().is_finite()
        && root.x_axis.w == 0.0
        && root.y_axis.w == 0.0
        && root.z_axis.w == 0.0
        && root.w_axis.w == 1.0
}

static SLOT: Mutex<Publisher> = Mutex::new(Publisher::new());

#[cfg(test)]
mod tests;
