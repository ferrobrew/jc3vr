//! The encoding of the version 8 notes: injected actions, diagnostics, haptic cues, the settings,
//! and the frame-time breakdown. The settings' TOML is a long text with a `u32` length.

use crate::{
    codec::{hand_code, read_hand},
    event::{Diagnostic, FrameTiming, Haptic, Injected, InjectedWrite, Settings, SlotTiming},
    event_codec::{
        label, list, opt_f32, opt_hand, opt_label, read_label, read_list, read_opt_f32,
        read_opt_hand, read_opt_label,
    },
    wire::{DecodeError, Decoder, Encoder},
};

pub(super) fn injected(out: &mut Encoder, injected: &Injected) {
    list(out, &injected.writes, |out, write| {
        label(out, &write.map);
        label(out, &write.action);
        out.f32(write.value);
        label(out, &write.phase);
        opt_f32(out, write.held_s);
        label(out, &write.outcome);
    });
}

pub(super) fn read_injected(input: &mut Decoder) -> Result<Injected, DecodeError> {
    Ok(Injected {
        writes: read_list(input, "the injected write count", |input| {
            Ok(InjectedWrite {
                map: read_label(input, "the action map")?,
                action: read_label(input, "the injected action")?,
                value: input.f32()?,
                phase: read_label(input, "the write phase")?,
                held_s: read_opt_f32(input, "the held time flag")?,
                outcome: read_label(input, "the write outcome")?,
            })
        })?,
    })
}

pub(super) fn diagnostic(out: &mut Encoder, diagnostic: &Diagnostic) {
    label(out, &diagnostic.source);
    opt_hand(out, diagnostic.hand);
    label(out, &diagnostic.what);
    opt_label(out, &diagnostic.detail);
}

pub(super) fn read_diagnostic(input: &mut Decoder) -> Result<Diagnostic, DecodeError> {
    Ok(Diagnostic {
        source: read_label(input, "the diagnostic source")?,
        hand: read_opt_hand(input, "the diagnostic hand")?,
        what: read_label(input, "the diagnostic")?,
        detail: read_opt_label(input, "the diagnostic detail")?,
    })
}

pub(super) fn haptic(out: &mut Encoder, haptic: &Haptic) {
    out.u8(hand_code(haptic.hand));
    label(out, &haptic.event);
    label(out, &haptic.primitive);
    out.f32(haptic.amplitude);
    out.f32(haptic.duration_s);
    opt_f32(out, haptic.frequency_hz);
}

pub(super) fn read_haptic(input: &mut Decoder) -> Result<Haptic, DecodeError> {
    Ok(Haptic {
        hand: read_hand(input, "the haptic hand")?,
        event: read_label(input, "the haptic event")?,
        primitive: read_label(input, "the haptic primitive")?,
        amplitude: input.f32()?,
        duration_s: input.f32()?,
        frequency_hz: read_opt_f32(input, "the haptic frequency flag")?,
    })
}

pub(super) fn settings(out: &mut Encoder, settings: &Settings) {
    out.long_str(&settings.toml);
}

pub(super) fn read_settings(input: &mut Decoder) -> Result<Settings, DecodeError> {
    Ok(Settings {
        toml: input.long_str("the settings")?.into(),
    })
}

pub(super) fn frame_timing(out: &mut Encoder, timing: &FrameTiming) {
    out.f32(timing.window_s);
    out.u32(timing.frames);
    out.u32(timing.sim_ticks);
    out.f32(timing.frame_ms);
    out.f32(timing.frame_max_ms);
    list(out, &timing.slots, |out, slot| {
        label(out, &slot.name);
        out.f32(slot.self_ms);
        out.f32(slot.max_ms);
        out.u32(slot.calls);
    });
}

pub(super) fn read_frame_timing(input: &mut Decoder) -> Result<FrameTiming, DecodeError> {
    Ok(FrameTiming {
        window_s: input.f32()?,
        frames: input.u32()?,
        sim_ticks: input.u32()?,
        frame_ms: input.f32()?,
        frame_max_ms: input.f32()?,
        slots: read_list(input, "the timing slot count", |input| {
            Ok(SlotTiming {
                name: read_label(input, "the timing slot")?,
                self_ms: input.f32()?,
                max_ms: input.f32()?,
                calls: input.u32()?,
            })
        })?,
    })
}
