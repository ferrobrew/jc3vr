//! The status rows for what the hands feel and hear: the haptic cues sent and how the mixer
//! folded them together, and the impact sounds the hands made.

use smol_str::format_smolstr;

use crate::{
    controllers::{haptics, menu::read_config, physical_hands::sounds},
    status::Line,
};

pub(super) fn haptic_cues() -> Line {
    if !read_config(|c| c.controllers.haptics.enabled) {
        return Line::off("");
    }
    let status = haptics::status();
    let detail = format_smolstr!(
        "{} SENT, {} MASKED, {} LIMITED",
        status.total_sent(),
        status.mix.masked,
        status.mix.limited
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
    let detail = format_smolstr!(
        "{} PLAYED, {} HELD BACK, {} SILENT",
        status.played,
        status.limited,
        status.silent
    );
    if status.played == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}
