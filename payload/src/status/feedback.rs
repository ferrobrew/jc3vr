//! The status rows for what the hands feel and hear: the haptic cues sent and how the mixer
//! folded them together, and the impact sounds the hands made.

use crate::{
    controllers::{haptics, menu::read_config, physical_hands::sounds},
    status::{Line, text},
};

pub(super) fn haptic_cues() -> Line {
    if !read_config(|c| c.controllers.haptics.enabled) {
        return Line::off("");
    }
    let status = haptics::status();
    let detail = text!(
        status.total_sent().to_string(),
        " sent, ",
        status.mix.masked.to_string(),
        " masked, ",
        status.mix.limited.to_string(),
        " limited"
    );
    if status.total_sent() == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}

pub(super) fn impact_sounds() -> Line {
    if !read_config(|c| c.controllers.physical_hands.sounds.enabled) {
        return Line::off("");
    }
    let status = sounds::status();
    let detail = text!(
        status.played.to_string(),
        " played, ",
        status.limited.to_string(),
        " held back, ",
        status.silent.to_string(),
        " silent"
    );
    if status.played == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}
