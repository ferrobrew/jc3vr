//! The independent, bounded, uncompressed input-record wire.

use crate::{
    input::{
        CapturedInput, ClockDomain, DiscontinuityReason, GroupOrder, INPUT_VERSION, InputGroup,
        InputRecord, InputRoute, InputStamp, MAX_ROUTES, MAX_SOURCE_PATH, MAX_SOURCES, RouteSample,
        RuntimeChange, SourceQuery, ValueBits,
    },
    wire::{DecodeError, DecodeErrorKind, Decoder, Encoder},
};

pub(crate) fn encode(out: &mut Encoder, captured: &CapturedInput) {
    out.u16(INPUT_VERSION);
    out.u64(captured.capture_time_ns);
    out.bool(captured.frame_sequence.is_some());
    if let Some(sequence) = captured.frame_sequence {
        out.u64(sequence);
    }
    match &captured.record {
        InputRecord::Discontinuity { reason, discarded } => {
            out.u8(1);
            out.u8(reason_tag(*reason));
            out.u64(*discarded);
        }
        InputRecord::Group(group) => {
            out.u8(0);
            let stamp = group.stamp;
            out.u64(stamp.epoch);
            out.u8(match stamp.clock {
                ClockDomain::HostMonotonic => 0,
                ClockDomain::Recorded => 1,
                ClockDomain::Synthetic => 2,
            });
            out.u64(stamp.time_ns);
            out.u64(stamp.sync);
            out.u64(stamp.sequence);
            out.u8(match group.order {
                GroupOrder::UnorderedSync => 0,
                GroupOrder::ObservedGroup => 1,
            });
            out.u64(group.source_generation);
            out.u64(group.binding_generation);
            out.u32(group.action_schema);
            out.u64(group.context);
            out.bool(group.focused);
            out.u16(group.samples.len() as u16);
            for sample in &group.samples {
                match sample.route {
                    InputRoute::Raw(control) => {
                        out.u8(0);
                        out.u8(control);
                    }
                    InputRoute::Semantic { set, action, hand } => {
                        out.u8(1);
                        out.u8(set);
                        out.u16(action);
                        out.u8(hand);
                    }
                }
                match sample.value {
                    ValueBits::Bool(value) => {
                        out.u8(0);
                        out.bool(value);
                    }
                    ValueBits::Float(bits) => {
                        out.u8(1);
                        out.u32(bits);
                    }
                    ValueBits::Vec2(bits) => {
                        out.u8(2);
                        out.u32(bits[0]);
                        out.u32(bits[1]);
                    }
                }
                // Only these three flag bits are defined; the decoder rejects every reserved bit.
                out.u8(u8::from(sample.active)
                    | (u8::from(sample.down) << 1)
                    | (u8::from(sample.change.is_some()) << 2));
                if let Some(change) = sample.change {
                    out.bool(change.changed_since_sync);
                    out.i64(change.last_change_time_ns);
                }
                match &sample.sources {
                    SourceQuery::NotEnumerated => out.u8(0),
                    SourceQuery::Failed => out.u8(1),
                    SourceQuery::Paths(paths) => {
                        out.u8(2);
                        out.u8(paths.len() as u8);
                        for path in paths.iter() {
                            out.str(path);
                        }
                    }
                    SourceQuery::LimitExceeded => out.u8(3),
                }
            }
        }
    }
}

pub(crate) fn decode(input: &mut Decoder) -> Result<CapturedInput, DecodeError> {
    let version = input.u16()?;
    if version != INPUT_VERSION {
        return Err(invalid(input, "input schema", u64::from(version)));
    }
    let capture_time_ns = input.u64()?;
    let frame_sequence = if input.bool("input frame sequence present")? {
        Some(input.u64()?)
    } else {
        None
    };
    let record = read_record(input)?;
    Ok(CapturedInput {
        capture_time_ns,
        frame_sequence,
        record,
    })
}

fn read_record(input: &mut Decoder) -> Result<InputRecord, DecodeError> {
    match input.u8()? {
        0 => read_group(input).map(InputRecord::Group),
        1 => {
            let tag = input.u8()?;
            let reason = match tag {
                0 => DiscontinuityReason::Overflow,
                1 => DiscontinuityReason::Unavailable,
                2 => DiscontinuityReason::GenerationChanged,
                3 => DiscontinuityReason::ClockChanged,
                4 => DiscontinuityReason::InvalidOrder,
                5 => DiscontinuityReason::LegacyOrderAbsent,
                6 => DiscontinuityReason::BoundsExceeded,
                _ => return Err(invalid(input, "input discontinuity", u64::from(tag))),
            };
            Ok(InputRecord::Discontinuity {
                reason,
                discarded: input.u64()?,
            })
        }
        value => Err(invalid(input, "input record kind", u64::from(value))),
    }
}

fn read_group(input: &mut Decoder) -> Result<InputGroup, DecodeError> {
    let epoch = input.u64()?;
    let clock = match input.u8()? {
        0 => ClockDomain::HostMonotonic,
        1 => ClockDomain::Recorded,
        2 => ClockDomain::Synthetic,
        value => return Err(invalid(input, "input clock", u64::from(value))),
    };
    let stamp = InputStamp {
        epoch,
        clock,
        time_ns: input.u64()?,
        sync: input.u64()?,
        sequence: input.u64()?,
    };
    let order = match input.u8()? {
        0 => GroupOrder::UnorderedSync,
        1 => GroupOrder::ObservedGroup,
        value => return Err(invalid(input, "input order", u64::from(value))),
    };
    let source_generation = input.u64()?;
    let binding_generation = input.u64()?;
    let action_schema = input.u32()?;
    let context = input.u64()?;
    let focused = input.bool("input focus")?;
    let count = usize::from(input.u16()?);
    if count > MAX_ROUTES {
        return Err(invalid(input, "input route count", count as u64));
    }
    let mut samples = Vec::with_capacity(count);
    for _ in 0..count {
        let route = match input.u8()? {
            0 => InputRoute::Raw(input.u8()?),
            1 => InputRoute::Semantic {
                set: input.u8()?,
                action: input.u16()?,
                hand: input.u8()?,
            },
            value => return Err(invalid(input, "input route", u64::from(value))),
        };
        let value = match input.u8()? {
            0 => ValueBits::Bool(input.bool("input boolean")?),
            1 => ValueBits::Float(input.u32()?),
            2 => ValueBits::Vec2([input.u32()?, input.u32()?]),
            value => return Err(invalid(input, "input value kind", u64::from(value))),
        };
        let flags = input.u8()?;
        if flags & !7 != 0 {
            return Err(invalid(input, "input flags", u64::from(flags)));
        }
        let change = if flags & 4 != 0 {
            Some(RuntimeChange {
                changed_since_sync: input.bool("input changed")?,
                last_change_time_ns: input.i64()?,
            })
        } else {
            None
        };
        let sources = match input.u8()? {
            0 => SourceQuery::NotEnumerated,
            1 => SourceQuery::Failed,
            2 => {
                let count = usize::from(input.u8()?);
                if count > MAX_SOURCES {
                    return Err(invalid(input, "input source count", count as u64));
                }
                let mut paths = Vec::with_capacity(count);
                for _ in 0..count {
                    let len = usize::from(input.u16()?);
                    if len > MAX_SOURCE_PATH {
                        return Err(invalid(input, "input source path length", len as u64));
                    }
                    let path = std::str::from_utf8(input.take(len)?).map_err(|_| {
                        input.error(DecodeErrorKind::InvalidUtf8 {
                            field: "input source path",
                        })
                    })?;
                    paths.push(path.to_owned());
                }
                SourceQuery::Paths(paths.into())
            }
            3 => SourceQuery::LimitExceeded,
            value => return Err(invalid(input, "input source query", u64::from(value))),
        };
        samples.push(RouteSample {
            route,
            value,
            active: flags & 1 != 0,
            down: flags & 2 != 0,
            change,
            sources,
        });
    }
    let group = InputGroup {
        stamp,
        order,
        source_generation,
        binding_generation,
        action_schema,
        context,
        focused,
        samples,
    };
    if !group.is_bounded() {
        return Err(invalid(input, "input group", count as u64));
    }
    Ok(group)
}

fn invalid(input: &Decoder, field: &'static str, value: u64) -> DecodeError {
    input.error(DecodeErrorKind::InvalidValue { field, value })
}

fn reason_tag(reason: DiscontinuityReason) -> u8 {
    match reason {
        DiscontinuityReason::Overflow => 0,
        DiscontinuityReason::Unavailable => 1,
        DiscontinuityReason::GenerationChanged => 2,
        DiscontinuityReason::ClockChanged => 3,
        DiscontinuityReason::InvalidOrder => 4,
        DiscontinuityReason::LegacyOrderAbsent => 5,
        DiscontinuityReason::BoundsExceeded => 6,
    }
}
