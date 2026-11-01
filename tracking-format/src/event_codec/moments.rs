//! The encoding of the event kinds a version 7 container adds: the body's animation state, throws,
//! grip releases, and the wingsuit gesture and snatch phases. Each is a moment the mod notes when it
//! happens, not a per-frame sample.

use crate::{
    codec::{hand_code, read_hand},
    event::{BodyState, GripRelease, Snatch, Throw, WingsuitGesture},
    event_codec::{
        label, opt, opt_f32, opt_hand, opt_label, opt_u32, opt_u64, read_label, read_opt,
        read_opt_f32, read_opt_hand, read_opt_label, read_opt_u32, read_opt_u64, read_world_motion,
        world_motion,
    },
    wire::{DecodeError, Decoder, Encoder},
};

pub(super) fn body_state(out: &mut Encoder, state: &BodyState) {
    opt_u32(out, state.rule_state);
    opt_label(out, &state.rule_state_name);
    label(out, &state.case);
    out.bool(state.repeat);
}

pub(super) fn read_body_state(input: &mut Decoder) -> Result<BodyState, DecodeError> {
    Ok(BodyState {
        rule_state: read_opt_u32(input, "the rule state flag")?,
        rule_state_name: read_opt_label(input, "the rule state name")?,
        case: read_label(input, "the hand-over case")?,
        repeat: input.bool("the body state repeat flag")?,
    })
}

pub(super) fn throw(out: &mut Encoder, throw: &Throw) {
    out.u8(hand_code(throw.hand));
    label(out, &throw.item);
    world_motion(out, &throw.motion);
    out.f32(throw.time_scale);
    opt_label(out, &throw.source);
    opt(out, throw.armed.as_ref(), |out, armed| out.bool(*armed));
}

pub(super) fn read_throw(input: &mut Decoder) -> Result<Throw, DecodeError> {
    Ok(Throw {
        hand: read_hand(input, "the throwing hand")?,
        item: read_label(input, "the thrown item")?,
        motion: read_world_motion(input)?,
        time_scale: input.f32()?,
        source: read_opt_label(input, "the throw's sample source")?,
        armed: read_opt(input, "the armed flag", |input| {
            input.bool("the armed state")
        })?,
    })
}

pub(super) fn grip_release(out: &mut Encoder, release: &GripRelease) {
    out.u8(hand_code(release.hand));
    label(out, &release.held);
    opt_u64(out, release.object);
    opt_u32(out, release.body);
    label(out, &release.cause);
    out.bool(release.thrown);
}

pub(super) fn read_grip_release(input: &mut Decoder) -> Result<GripRelease, DecodeError> {
    Ok(GripRelease {
        hand: read_hand(input, "the releasing hand")?,
        held: read_label(input, "what the grip held")?,
        object: read_opt_u64(input, "the released object flag")?,
        body: read_opt_u32(input, "the released body flag")?,
        cause: read_label(input, "the release cause")?,
        thrown: input.bool("the thrown flag")?,
    })
}

pub(super) fn wingsuit_gesture(out: &mut Encoder, gesture: &WingsuitGesture) {
    opt_label(out, &gesture.phase);
    opt_label(out, &gesture.why);
}

pub(super) fn read_wingsuit_gesture(input: &mut Decoder) -> Result<WingsuitGesture, DecodeError> {
    Ok(WingsuitGesture {
        phase: read_opt_label(input, "the gesture phase")?,
        why: read_opt_label(input, "the gesture block")?,
    })
}

pub(super) fn snatch(out: &mut Encoder, snatch: &Snatch) {
    opt_hand(out, snatch.hand);
    label(out, &snatch.from);
    label(out, &snatch.to);
    opt_label(out, &snatch.outcome);
    opt_label(out, &snatch.reason);
    opt_label(out, &snatch.target);
    opt_f32(out, snatch.distance_m);
    opt_f32(out, snatch.yank_m);
    opt_f32(out, snatch.yank_mps);
}

pub(super) fn read_snatch(input: &mut Decoder) -> Result<Snatch, DecodeError> {
    let hand = read_opt_hand(input, "the snatch hand")?;
    let from = read_label(input, "the snatch's previous phase")?;
    let to = read_label(input, "the snatch's phase")?;
    let outcome = read_opt_label(input, "the snatch outcome")?;
    let reason = read_opt_label(input, "the snatch reason")?;
    let target = read_opt_label(input, "the snatch target")?;
    let distance_m = read_opt_f32(input, "the snatch distance flag")?;
    let yank_m = read_opt_f32(input, "the yank distance flag")?;
    let yank_mps = read_opt_f32(input, "the yank speed flag")?;
    Ok(Snatch {
        hand,
        from,
        to,
        outcome,
        reason,
        target,
        distance_m,
        yank_m,
        yank_mps,
    })
}
