//! Effective settings and the final protected decision for one observed input group.

use crate::input::InputStamp;

pub const POLICY_A_VERSION: u16 = 3;
pub const SOURCE_MASK: u32 = 0xffff;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PolicyATiming {
    pub recenter_join_ns: u64,
    pub panel_join_ns: u64,
    pub recenter_hold_ns: u64,
}

impl PolicyATiming {
    pub fn is_valid(self) -> bool {
        self.recenter_join_ns > 0
            && self.recenter_join_ns <= 1_000_000_000
            && self.panel_join_ns > 0
            && self.panel_join_ns <= 1_000_000_000
            && self.recenter_hold_ns > 0
            && self.recenter_hold_ns <= 60_000_000_000
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AcceptedSingle {
    None,
    Pressed,
    Released,
    Tap,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AcceptedCommand {
    None,
    Recenter,
    TogglePanel,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PolicyOutcome {
    Evaluated,
    Cancelled,
    UnavailablePrefix,
    NeutralBoundary,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum DirectAcceptance {
    #[default]
    Absent,
    Unavailable,
    Neutral,
    Withheld,
    Accepted,
    Held,
    Conflict,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct DirectDecision {
    pub sources: u32,
    pub acceptance: DirectAcceptance,
}

/// Direct routes are recenter-left, recenter-right, panel-left, and panel-right. Raw flags are
/// down, pressed, and released. Command bits are recenter and panel; disjoint sources may coexist.
/// Playback compares the resolved decision before exposing any command or accepted transition.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PolicyEvaluation {
    pub version: u16,
    pub stamp: InputStamp,
    pub action_schema: u32,
    pub timing: Option<PolicyATiming>,
    pub context: u64,
    pub gameplay: bool,
    pub outcome: PolicyOutcome,
    pub neutral: bool,
    pub singles: [AcceptedSingle; 3],
    pub single_down: [bool; 3],
    pub withheld: [bool; 3],
    pub command: AcceptedCommand,
    pub raw: [u8; 3],
    pub direct: [DirectDecision; 4],
    pub claims: u32,
    pub commands: u8,
}

impl PolicyEvaluation {
    pub fn is_valid(&self) -> bool {
        self.version == POLICY_A_VERSION
            && (self.action_schema > 0
                || matches!(
                    self.outcome,
                    PolicyOutcome::Cancelled | PolicyOutcome::UnavailablePrefix
                ))
            && self.timing.is_none_or(PolicyATiming::is_valid)
            && (!matches!(
                self.outcome,
                PolicyOutcome::Evaluated | PolicyOutcome::NeutralBoundary
            ) || self.timing.is_some())
            && self.raw.iter().all(|flags| flags & !7 == 0)
            && self.commands & !3 == 0
            && self.claims & !SOURCE_MASK == 0
            && self
                .direct
                .iter()
                .all(|direct| direct.sources & !SOURCE_MASK == 0)
            && (!matches!(self.outcome, PolicyOutcome::NeutralBoundary) || self.neutral)
            && (matches!(self.outcome, PolicyOutcome::Evaluated)
                || (self.commands == 0
                    && self.raw == [0; 3]
                    && self.claims == 0
                    && self.command == AcceptedCommand::None
                    && self.singles == [AcceptedSingle::None; 3]
                    && self.single_down == [false; 3]
                    && self.direct == [DirectDecision::default(); 4]))
    }
}
