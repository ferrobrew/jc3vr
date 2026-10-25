//! Conversion of resolved protected decisions to the versioned recording wire.

use std::time::Duration;

use tracking_format::input::{
    InputGroup,
    policy::{
        AcceptedCommand, AcceptedSingle, POLICY_A_VERSION, PolicyATiming, PolicyEvaluation,
        PolicyOutcome,
    },
};

use crate::controllers::bindings::policy_a::{
    Arbitration, PolicyAConfig, SingleEvent, SystemCommand,
};

pub(super) fn timing(config: Option<PolicyAConfig>) -> Option<PolicyATiming> {
    config.map(|config| PolicyATiming {
        recenter_join_ns: config.recenter_join.as_nanos() as u64,
        panel_join_ns: config.panel_join.as_nanos() as u64,
        recenter_hold_ns: config.recenter_hold.as_nanos() as u64,
    })
}

pub(super) fn config(timing: Option<PolicyATiming>) -> Option<PolicyAConfig> {
    timing
        .filter(|timing| timing.is_valid())
        .map(|timing| PolicyAConfig {
            recenter_join: Duration::from_nanos(timing.recenter_join_ns),
            panel_join: Duration::from_nanos(timing.panel_join_ns),
            recenter_hold: Duration::from_nanos(timing.recenter_hold_ns),
        })
}

pub(super) fn cancelled(
    group: &InputGroup,
    settings: Option<PolicyAConfig>,
    context: u64,
    gameplay: bool,
) -> PolicyEvaluation {
    PolicyEvaluation {
        version: POLICY_A_VERSION,
        stamp: group.stamp,
        action_schema: group.action_schema,
        timing: timing(settings),
        context,
        gameplay,
        outcome: PolicyOutcome::Cancelled,
        neutral: false,
        singles: [AcceptedSingle::None; 3],
        single_down: [false; 3],
        withheld: [true; 3],
        command: AcceptedCommand::None,
        raw: [0; 3],
        direct: Default::default(),
        claims: 0,
        commands: 0,
    }
}

pub(super) fn evaluation(
    group: &InputGroup,
    settings: Option<PolicyAConfig>,
    context: u64,
    gameplay: bool,
    decision: &Arbitration,
) -> PolicyEvaluation {
    let mut out = cancelled(group, settings, context, gameplay);
    out.outcome = PolicyOutcome::Evaluated;
    out.singles = decision.singles.map(|event| match event {
        None => AcceptedSingle::None,
        Some(SingleEvent::Pressed { .. }) => AcceptedSingle::Pressed,
        Some(SingleEvent::Released { .. }) => AcceptedSingle::Released,
        Some(SingleEvent::Tap { .. }) => AcceptedSingle::Tap,
    });
    out.single_down = decision.sources.map(|source| source.single_down);
    out.withheld = decision.withheld;
    out.command = match decision.request.map(|request| request.command) {
        None => AcceptedCommand::None,
        Some(SystemCommand::Recenter) => AcceptedCommand::Recenter,
        Some(SystemCommand::TogglePanel) => AcceptedCommand::TogglePanel,
    };
    out
}
