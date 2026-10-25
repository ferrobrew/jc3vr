//! Bounded policy-A arbitration for the physical left stick, right stick, and right B.
//!
//! Each observation is one coherent, unmasked timestamp group. Changes within a group have
//! unknown internal order. All changes at an equal timestamp belong in one group; splitting
//! them cannot establish LS-before-B. Callers retain multiple ordered groups from one sync
//! when distinct observation times are available. A snapshot cannot recover missed events.
//!
//! Source decisions authorize only contributions from these controls. They never assert a
//! semantic action. An adapter must retain sampled semantic values, quarantine inseparable
//! merged routes through their own neutral observation, and leave independent commands alone.
//! OpenXR binding enumeration does not identify contributors to a merged action value.

mod attribution;
mod recording;
pub(crate) mod runtime;

use std::{error::Error, fmt, time::Duration};

/// The only physical controls in this arbiter's overlap domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    LeftStick,
    RightStick,
    RightB,
}

impl Control {
    pub(crate) const ALL: [Self; 3] = [Self::LeftStick, Self::RightStick, Self::RightB];

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::LeftStick => 0,
            Self::RightStick => 1,
            Self::RightB => 2,
        }
    }
}

/// The validated physical-source interpretation, including device and binding changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BindingGeneration(pub(crate) u64);

/// The gameplay interpretation of a single, independent of system-command availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GameplayContext(pub(crate) u64);

/// An explicitly replaced input-clock history; ordinary resets retain this identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockEpoch(pub(crate) u64);

/// Logical input time and observed order, not consumer time or rendering time.
/// All equal-time changes share one group; successive groups strictly increase `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservationStamp {
    pub(crate) epoch: ClockEpoch,
    pub(crate) at: Duration,
    pub(crate) sync: u64,
    /// Strictly increasing between groups; it never resolves internal timestamp-group order.
    pub(crate) sequence: u64,
}

/// An observed down-to-up episode under one physical-source interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PressIdentity {
    pub(crate) control: Control,
    pub(crate) generation: BindingGeneration,
    pub(crate) onset: ObservationStamp,
}

/// Explicit timings; no default or saved-settings migration is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyAConfig {
    pub(crate) recenter_join: Duration,
    pub(crate) panel_join: Duration,
    pub(crate) recenter_hold: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigError {
    ZeroRecenterJoin,
    ZeroPanelJoin,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self {
            Self::ZeroRecenterJoin => "the recenter join window must be positive",
            Self::ZeroPanelJoin => "the panel join window must be positive",
        };
        write!(f, "policy-A arbitration: {cause}")
    }
}

impl Error for ConfigError {}

/// A discontinuity cancels pending work and neutralizes only owned single output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResetReason {
    #[cfg(test)]
    FocusLost,
    #[cfg(test)]
    TrackingLost,
    MissingOrStaleInput,
    #[cfg(test)]
    ControllerDisabled,
    #[cfg(test)]
    SchemeChanged,
    BindingUnresolved,
    BindingChanged,
    MaskedObservation,
    InvalidHistory,
    ClockDiscontinuity,
}

/// Verification applies to the physical labels and the coherent source interpretation.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputEvidence {
    VerifiedUnmasked,
    Unavailable(ResetReason),
}

/// One valid group may contain at most one observed level change per control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Observation {
    pub(crate) stamp: ObservationStamp,
    pub(crate) generation: BindingGeneration,
    pub(crate) context: GameplayContext,
    pub(crate) singles_enabled: bool,
    /// Real, unmasked levels, indexed by `Control::index`.
    pub(crate) down: [bool; 3],
    #[cfg(test)]
    pub(crate) evidence: InputEvidence,
}

/// Releases use the last accepted clock watermark, never an unvalidated reset timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResetRequest {
    pub(crate) reason: ResetReason,
}

/// Ends the old history before accepting a new, strictly newer epoch identity.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockEpochRequest {
    pub(crate) next: ClockEpoch,
}

/// Singles begin at acceptance, with no credit for the reservation's physical dwell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleEvent {
    Pressed {
        press: PressIdentity,
        accepted_at: Duration,
    },
    Released {
        press: PressIdentity,
        at: Duration,
    },
    /// One logical press/release with zero accepted hold duration.
    Tap {
        press: PressIdentity,
        accepted_at: Duration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    Idle,
    Pending,
    Single,
    Consumed,
    AwaitingNeutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SourceDecision {
    pub(crate) press: Option<PressIdentity>,
    pub(crate) disposition: Disposition,
    /// The arbiter's owned single contribution, never a semantic action value.
    pub(crate) single_down: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SystemCommand {
    Recenter,
    TogglePanel,
}

/// Member identities also identify the request; holding cannot generate duplicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SystemRequest {
    pub(crate) command: SystemCommand,
    pub(crate) members: [Option<PressIdentity>; 3],
    pub(crate) formed_at: Duration,
    pub(crate) accepted_at: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusalReason {
    CommittedSingle,
    ClaimedPress,
    JoinExpired,
    AmbiguousPanelOrder,
    CompetingChord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationStatus {
    Accepted,
    AwaitingNeutral,
    Recovered,
    Reset(ResetReason),
}

/// Fixed-size output for one group, suitable for raw, semantic, and pointer adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Arbitration {
    pub(crate) status: ObservationStatus,
    pub(crate) singles: [Option<SingleEvent>; 3],
    pub(crate) sources: [SourceDecision; 3],
    /// Suppress raw and attributable constituent edges for this entire group, including neutral.
    /// `singles` is the only accepted edge stream; a true flag never cancels an earlier commit.
    pub(crate) withheld: [bool; 3],
    pub(crate) request: Option<SystemRequest>,
    pub(crate) refused: [Option<RefusalReason>; 3],
}

/// Three source episodes, one exclusive chord, and one common-neutral consumption set.
#[derive(Debug, Clone)]
pub(crate) struct PolicyA {
    config: PolicyAConfig,
    epoch: ClockEpoch,
    last: Option<ObservationStamp>,
    interpretation: Option<(BindingGeneration, GameplayContext, bool)>,
    down: [bool; 3],
    presses: [Option<Press>; 3],
    locked: Members,
    chord: Option<Chord>,
    awaiting_neutral: bool,
}

impl PolicyA {
    /// Positive join windows are required; a zero completion hold is explicitly supported.
    pub(crate) fn new(config: PolicyAConfig, epoch: ClockEpoch) -> Result<Self, ConfigError> {
        if config.recenter_join.is_zero() {
            return Err(ConfigError::ZeroRecenterJoin);
        }
        if config.panel_join.is_zero() {
            return Err(ConfigError::ZeroPanelJoin);
        }
        Ok(Self {
            config,
            epoch,
            last: None,
            interpretation: None,
            down: [false; 3],
            presses: [None; 3],
            locked: Members::EMPTY,
            chord: None,
            awaiting_neutral: true,
        })
    }

    /// Reset retains the temporal watermark and never treats masked up levels as neutral.
    /// Even `ClockDiscontinuity` is only a reason here; it does not replace the clock epoch.
    pub(crate) fn reset(&mut self, request: ResetRequest) -> Arbitration {
        let mut out = Arbitration::empty(ObservationStatus::Reset(request.reason));
        self.release_owned(self.last.map_or(Duration::ZERO, |last| last.at), &mut out);
        self.interpretation = None;
        self.down = [false; 3];
        self.presses = [None; 3];
        self.locked = Members::EMPTY;
        self.chord = None;
        self.awaiting_neutral = true;
        self.describe(&mut out);
        out
    }

    /// Neutralize old output before replacing the clock; a lower new time is valid only here.
    /// Every observation carries its epoch, so queued observations from the old history stay stale.
    #[cfg(test)]
    pub(crate) fn begin_clock_epoch(&mut self, request: ClockEpochRequest) -> Arbitration {
        if request.next.0 <= self.epoch.0 {
            return self.reset(ResetRequest {
                reason: ResetReason::InvalidHistory,
            });
        }
        let out = self.reset(ResetRequest {
            reason: ResetReason::ClockDiscontinuity,
        });
        self.epoch = request.next;
        self.last = None;
        out
    }

    /// Feed every observed group in order, even when several groups precede one game tick.
    /// Gameplay context changes invalidate singles but retain system-chord ownership.
    pub(crate) fn observe(&mut self, input: Observation) -> Arbitration {
        if input.stamp.epoch != self.epoch
            || self.last.is_some_and(|last| {
                input.stamp.at <= last.at
                    || input.stamp.sequence <= last.sequence
                    || input.stamp.sync < last.sync
            })
        {
            return self.reset(ResetRequest {
                reason: ResetReason::InvalidHistory,
            });
        }
        // Only a validated stamp can advance time. Unavailable evidence still cannot arm a press.
        self.last = Some(input.stamp);
        #[cfg(test)]
        if let InputEvidence::Unavailable(reason) = input.evidence {
            return self.reset(ResetRequest { reason });
        }
        if self
            .interpretation
            .is_some_and(|(generation, _, _)| generation != input.generation)
        {
            let out = self.reset(ResetRequest {
                reason: ResetReason::BindingChanged,
            });
            self.interpretation = Some((input.generation, input.context, input.singles_enabled));
            return out;
        }
        let mut out = Arbitration::empty(ObservationStatus::Accepted);
        if self.interpretation.is_some_and(|(_, context, enabled)| {
            context != input.context || enabled != input.singles_enabled
        }) {
            self.invalidate_singles(input.stamp.at, &mut out);
        }
        self.interpretation = Some((input.generation, input.context, input.singles_enabled));
        if self.awaiting_neutral {
            if input.down == [false; 3] {
                self.awaiting_neutral = false;
                out.status = ObservationStatus::Recovered;
            } else {
                out.status = ObservationStatus::AwaitingNeutral;
            }
            self.describe(&mut out);
            return out;
        }
        let previous = self.down;
        self.down = input.down;
        for control in Control::ALL {
            let i = control.index();
            if input.down[i] && !previous[i] {
                self.presses[i] = Some(Press {
                    identity: PressIdentity {
                        control,
                        generation: input.generation,
                        onset: input.stamp,
                    },
                    kind: if self.locked.contains(control) {
                        PressKind::Consumed
                    } else {
                        PressKind::Pending
                    },
                    single_eligible: input.singles_enabled,
                });
            }
            if !input.down[i] && previous[i] {
                self.release_single(control, input.stamp.at, &mut out);
            }
        }
        self.cancel_interrupted_chord();
        self.refuse_committed_overlaps(previous, &mut out);
        self.resolve_chords(input.stamp, &mut out);
        self.resolve_singles(input.stamp.at, &mut out);
        self.advance_chord(input.stamp.at, &mut out);
        self.describe(&mut out);
        self.finish_neutral();
        out
    }
}

impl Arbitration {
    fn empty(status: ObservationStatus) -> Self {
        Self {
            status,
            singles: [None; 3],
            sources: [SourceDecision {
                press: None,
                disposition: Disposition::Idle,
                single_down: false,
            }; 3],
            withheld: [true; 3],
            request: None,
            refused: [None; 3],
        }
    }
}

impl PolicyA {
    fn release_owned(&mut self, at: Duration, out: &mut Arbitration) {
        for control in Control::ALL {
            self.release_single(control, at, out);
        }
    }

    fn release_single(&mut self, control: Control, at: Duration, out: &mut Arbitration) {
        if let Some(press) = self.presses[control.index()].as_mut()
            && press.kind == (PressKind::Single { live: true })
        {
            out.singles[control.index()] = Some(SingleEvent::Released {
                press: press.identity,
                at,
            });
            press.kind = PressKind::Single { live: false };
        }
    }

    fn describe(&self, out: &mut Arbitration) {
        for control in Control::ALL {
            let i = control.index();
            out.sources[i] = if self.awaiting_neutral {
                SourceDecision {
                    press: None,
                    disposition: Disposition::AwaitingNeutral,
                    single_down: false,
                }
            } else if let Some(press) = self.presses[i] {
                SourceDecision {
                    press: Some(press.identity),
                    disposition: match press.kind {
                        PressKind::Pending => Disposition::Pending,
                        PressKind::Single { .. } => Disposition::Single,
                        PressKind::Consumed => Disposition::Consumed,
                    },
                    single_down: press.kind == (PressKind::Single { live: true }),
                }
            } else {
                SourceDecision {
                    press: None,
                    disposition: if self.locked.contains(control) {
                        Disposition::Consumed
                    } else {
                        Disposition::Idle
                    },
                    single_down: false,
                }
            };
            out.withheld[i] = self.awaiting_neutral
                || self.locked.contains(control)
                || self.presses[i].is_some_and(|press| {
                    matches!(press.kind, PressKind::Pending | PressKind::Consumed)
                });
        }
    }

    fn invalidate_singles(&mut self, at: Duration, out: &mut Arbitration) {
        self.release_owned(at, out);
        for press in self.presses.iter_mut().flatten() {
            press.single_eligible = false;
        }
    }

    fn cancel_interrupted_chord(&mut self) {
        if let Some(chord) = self.chord.as_mut()
            && !chord.members.all_down(self.down)
        {
            chord.cancelled = true;
        }
    }

    fn refuse_committed_overlaps(&mut self, previous: [bool; 3], out: &mut Arbitration) {
        for pair in [Members::RECENTER, Members::PANEL] {
            if !pair.all_down(self.down)
                || !Control::ALL
                    .into_iter()
                    .any(|c| pair.contains(c) && !previous[c.index()])
            {
                continue;
            }
            let committed = Control::ALL.into_iter().any(|c| {
                pair.contains(c)
                    && self.presses[c.index()]
                        .is_some_and(|p| matches!(p.kind, PressKind::Single { .. }))
            });
            let claimed = Control::ALL.into_iter().any(|c| {
                pair.contains(c)
                    && (self.locked.contains(c)
                        || self.presses[c.index()].is_some_and(|p| p.kind == PressKind::Consumed))
            });
            if committed || claimed {
                self.consume(
                    pair,
                    if committed {
                        RefusalReason::CommittedSingle
                    } else {
                        RefusalReason::ClaimedPress
                    },
                    out,
                );
            }
        }
    }

    fn resolve_chords(&mut self, stamp: ObservationStamp, out: &mut Arbitration) {
        let recenter = self.pending_pair(Members::RECENTER);
        let panel = self.pending_pair(Members::PANEL);
        let recenter_eligible = recenter
            .is_some_and(|[left, right]| onset_gap(left, right) < self.config.recenter_join);
        let panel_eligible = panel.is_some_and(|[left, b]| {
            left.onset.sequence < b.onset.sequence
                && b.onset.at - left.onset.at < self.config.panel_join
        });
        if recenter_eligible || panel_eligible {
            let (command, members) = if recenter_eligible {
                (SystemCommand::Recenter, Members::RECENTER)
            } else {
                (SystemCommand::TogglePanel, Members::PANEL)
            };
            let identities = Control::ALL.map(|c| {
                if members.contains(c) {
                    self.presses[c.index()].map(|press| press.identity)
                } else {
                    None
                }
            });
            self.claim(members);
            self.chord = Some(Chord {
                command,
                members,
                identities,
                formed_at: stamp.at,
                fired: false,
                cancelled: false,
            });
            for pair in [Members::RECENTER, Members::PANEL] {
                if pair != members && pair.all_down(self.down) {
                    self.consume(pair, RefusalReason::CompetingChord, out);
                }
            }
            return;
        }
        if panel.is_some_and(|[left, b]| left.onset.sequence == b.onset.sequence) {
            self.consume(Members::PANEL, RefusalReason::AmbiguousPanelOrder, out);
        }
        // Expired older singles commit before a late partner is refused. A newly joining B
        // cannot commit here and escape consumption through its zero single-button delay.
        for control in Control::ALL {
            let i = control.index();
            if self.presses[i].is_some_and(|p| {
                p.kind == PressKind::Pending
                    && p.identity.onset.sequence < stamp.sequence
                    && self.down[i]
                    && stamp.at - p.identity.onset.at >= self.reservation(control)
            }) {
                self.commit_single(control, stamp.at, out);
            }
        }
        for pair in [Members::RECENTER, Members::PANEL] {
            if pair.all_down(self.down)
                && Control::ALL.into_iter().any(|c| {
                    pair.contains(c)
                        && self.presses[c.index()].is_some_and(|p| p.kind == PressKind::Pending)
                })
            {
                let committed = Control::ALL.into_iter().any(|c| {
                    pair.contains(c)
                        && self.presses[c.index()]
                            .is_some_and(|p| matches!(p.kind, PressKind::Single { .. }))
                });
                self.consume(
                    pair,
                    if committed {
                        RefusalReason::CommittedSingle
                    } else {
                        RefusalReason::JoinExpired
                    },
                    out,
                );
            }
        }
    }

    fn pending_pair(&self, members: Members) -> Option<[PressIdentity; 2]> {
        if !members.all_down(self.down) {
            return None;
        }
        let partner = if members == Members::RECENTER {
            Control::RightStick
        } else {
            Control::RightB
        };
        let left = self.presses[Control::LeftStick.index()]?;
        let other = self.presses[partner.index()]?;
        (left.kind == PressKind::Pending && other.kind == PressKind::Pending)
            .then_some([left.identity, other.identity])
    }

    fn claim(&mut self, members: Members) {
        self.locked = self.locked.union(members);
        for control in Control::ALL {
            if members.contains(control)
                && let Some(press) = self.presses[control.index()].as_mut()
                && press.kind == PressKind::Pending
            {
                press.kind = PressKind::Consumed;
            }
        }
    }

    fn consume(&mut self, members: Members, reason: RefusalReason, out: &mut Arbitration) {
        self.claim(members);
        for control in Control::ALL {
            if members.contains(control) {
                out.refused[control.index()] = Some(reason);
            }
        }
    }

    fn reservation(&self, control: Control) -> Duration {
        match control {
            Control::LeftStick => self.config.recenter_join.max(self.config.panel_join),
            Control::RightStick => self.config.recenter_join,
            Control::RightB => Duration::ZERO,
        }
    }

    fn commit_single(&mut self, control: Control, at: Duration, out: &mut Arbitration) {
        let i = control.index();
        let Some(press) = self.presses[i].as_mut() else {
            return;
        };
        if !press.single_eligible {
            press.kind = PressKind::Consumed;
            self.locked = self.locked.union(Members::of(control));
            return;
        }
        if self.down[i] {
            press.kind = PressKind::Single { live: true };
            out.singles[i] = Some(SingleEvent::Pressed {
                press: press.identity,
                accepted_at: at,
            });
        } else {
            out.singles[i] = Some(SingleEvent::Tap {
                press: press.identity,
                accepted_at: at,
            });
            self.presses[i] = None;
        }
    }

    fn resolve_singles(&mut self, at: Duration, out: &mut Arbitration) {
        for control in Control::ALL {
            let i = control.index();
            if self.presses[i].is_some_and(|p| {
                p.kind == PressKind::Pending
                    && (!self.down[i] || at - p.identity.onset.at >= self.reservation(control))
            }) {
                self.commit_single(control, at, out);
            }
        }
    }

    fn advance_chord(&mut self, at: Duration, out: &mut Arbitration) {
        let Some(chord) = self.chord.as_mut() else {
            return;
        };
        if !chord.cancelled
            && !chord.fired
            && (chord.command == SystemCommand::TogglePanel
                || at - chord.formed_at >= self.config.recenter_hold)
        {
            chord.fired = true;
            out.request = Some(SystemRequest {
                command: chord.command,
                members: chord.identities,
                formed_at: chord.formed_at,
                accepted_at: at,
            });
        }
    }

    fn finish_neutral(&mut self) {
        if self.locked.all_up(self.down) {
            for control in Control::ALL {
                if self.locked.contains(control) {
                    self.presses[control.index()] = None;
                }
            }
            self.locked = Members::EMPTY;
            self.chord = None;
        }
        for control in Control::ALL {
            if !self.down[control.index()] && !self.locked.contains(control) {
                self.presses[control.index()] = None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Press {
    identity: PressIdentity,
    kind: PressKind,
    single_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PressKind {
    Pending,
    Single { live: bool },
    Consumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Members(u8);

impl Members {
    const EMPTY: Self = Self(0);
    const RECENTER: Self = Self(0b011);
    const PANEL: Self = Self(0b101);

    const fn of(control: Control) -> Self {
        Self(1 << control.index())
    }

    const fn contains(self, control: Control) -> bool {
        self.0 & Self::of(control).0 != 0
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn all_down(self, down: [bool; 3]) -> bool {
        Control::ALL
            .into_iter()
            .all(|c| !self.contains(c) || down[c.index()])
    }

    fn all_up(self, down: [bool; 3]) -> bool {
        Control::ALL
            .into_iter()
            .all(|c| !self.contains(c) || !down[c.index()])
    }
}

#[derive(Debug, Clone, Copy)]
struct Chord {
    command: SystemCommand,
    members: Members,
    identities: [Option<PressIdentity>; 3],
    formed_at: Duration,
    fired: bool,
    cancelled: bool,
}

fn onset_gap(left: PressIdentity, right: PressIdentity) -> Duration {
    left.onset.at.abs_diff(right.onset.at)
}

#[cfg(test)]
mod tests;
