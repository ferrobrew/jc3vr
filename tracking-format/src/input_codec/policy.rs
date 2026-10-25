//! Fixed-size final policy decision wire, independent of tracking-frame compression.

use crate::{
    input::{
        ClockDomain, InputStamp,
        policy::{
            AcceptedCommand, AcceptedSingle, DirectAcceptance, DirectDecision, PolicyATiming,
            PolicyEvaluation, PolicyOutcome,
        },
    },
    input_codec::invalid,
    wire::{DecodeError, Decoder, Encoder},
};

pub(super) fn encode(out: &mut Encoder, policy: &PolicyEvaluation) {
    out.u16(policy.version);
    out.u64(policy.stamp.epoch);
    out.u8(match policy.stamp.clock {
        ClockDomain::HostMonotonic => 0,
        ClockDomain::Recorded => 1,
        ClockDomain::Synthetic => 2,
    });
    out.u64(policy.stamp.time_ns);
    out.u64(policy.stamp.sync);
    out.u64(policy.stamp.sequence);
    out.u32(policy.action_schema);
    out.bool(policy.timing.is_some());
    if let Some(timing) = policy.timing {
        out.u64(timing.recenter_join_ns);
        out.u64(timing.panel_join_ns);
        out.u64(timing.recenter_hold_ns);
    }
    out.u64(policy.context);
    out.bool(policy.gameplay);
    for i in 0..3 {
        out.u8(match policy.singles[i] {
            AcceptedSingle::None => 0,
            AcceptedSingle::Pressed => 1,
            AcceptedSingle::Released => 2,
            AcceptedSingle::Tap => 3,
        });
        out.bool(policy.single_down[i]);
        out.bool(policy.withheld[i]);
    }
    out.u8(match policy.command {
        AcceptedCommand::None => 0,
        AcceptedCommand::Recenter => 1,
        AcceptedCommand::TogglePanel => 2,
    });
    out.u8(match policy.outcome {
        PolicyOutcome::Evaluated => 0,
        PolicyOutcome::Cancelled => 1,
        PolicyOutcome::UnavailablePrefix => 2,
        PolicyOutcome::NeutralBoundary => 3,
    });
    out.bool(policy.neutral);
    for flags in policy.raw {
        out.u8(flags);
    }
    for direct in policy.direct {
        out.u32(direct.sources);
        out.u8(match direct.acceptance {
            DirectAcceptance::Absent => 0,
            DirectAcceptance::Unavailable => 1,
            DirectAcceptance::Neutral => 2,
            DirectAcceptance::Withheld => 3,
            DirectAcceptance::Accepted => 4,
            DirectAcceptance::Held => 5,
            DirectAcceptance::Conflict => 6,
        });
    }
    out.u32(policy.claims);
    out.u8(policy.commands);
}

pub(super) fn decode(input: &mut Decoder) -> Result<PolicyEvaluation, DecodeError> {
    let version = input.u16()?;
    let epoch = input.u64()?;
    let clock = match input.u8()? {
        0 => ClockDomain::HostMonotonic,
        1 => ClockDomain::Recorded,
        2 => ClockDomain::Synthetic,
        value => return Err(invalid(input, "policy clock", u64::from(value))),
    };
    let stamp = InputStamp {
        epoch,
        clock,
        time_ns: input.u64()?,
        sync: input.u64()?,
        sequence: input.u64()?,
    };
    let action_schema = input.u32()?;
    let timing = if input.bool("policy timing present")? {
        Some(PolicyATiming {
            recenter_join_ns: input.u64()?,
            panel_join_ns: input.u64()?,
            recenter_hold_ns: input.u64()?,
        })
    } else {
        None
    };
    let context = input.u64()?;
    let gameplay = input.bool("policy gameplay")?;
    let mut singles = [AcceptedSingle::None; 3];
    let mut single_down = [false; 3];
    let mut withheld = [false; 3];
    for i in 0..3 {
        singles[i] = match input.u8()? {
            0 => AcceptedSingle::None,
            1 => AcceptedSingle::Pressed,
            2 => AcceptedSingle::Released,
            3 => AcceptedSingle::Tap,
            value => return Err(invalid(input, "policy single", u64::from(value))),
        };
        single_down[i] = input.bool("policy single down")?;
        withheld[i] = input.bool("policy withheld")?;
    }
    let command = match input.u8()? {
        0 => AcceptedCommand::None,
        1 => AcceptedCommand::Recenter,
        2 => AcceptedCommand::TogglePanel,
        value => return Err(invalid(input, "policy command", u64::from(value))),
    };
    let outcome = match input.u8()? {
        0 => PolicyOutcome::Evaluated,
        1 => PolicyOutcome::Cancelled,
        2 => PolicyOutcome::UnavailablePrefix,
        3 => PolicyOutcome::NeutralBoundary,
        value => return Err(invalid(input, "policy outcome", u64::from(value))),
    };
    let neutral = input.bool("policy neutral")?;
    let raw = [input.u8()?, input.u8()?, input.u8()?];
    let mut direct = [DirectDecision::default(); 4];
    for entry in &mut direct {
        entry.sources = input.u32()?;
        entry.acceptance = match input.u8()? {
            0 => DirectAcceptance::Absent,
            1 => DirectAcceptance::Unavailable,
            2 => DirectAcceptance::Neutral,
            3 => DirectAcceptance::Withheld,
            4 => DirectAcceptance::Accepted,
            5 => DirectAcceptance::Held,
            6 => DirectAcceptance::Conflict,
            value => return Err(invalid(input, "policy direct acceptance", u64::from(value))),
        };
    }
    let claims = input.u32()?;
    let commands = input.u8()?;
    let policy = PolicyEvaluation {
        version,
        stamp,
        action_schema,
        timing,
        context,
        gameplay,
        outcome,
        neutral,
        singles,
        single_down,
        withheld,
        command,
        raw,
        direct,
        claims,
        commands,
    };
    if !policy.is_valid() {
        return Err(invalid(input, "policy decision", u64::from(version)));
    }
    Ok(policy)
}

#[cfg(test)]
mod tests;
