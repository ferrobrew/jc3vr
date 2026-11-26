//! The status rows for what the hands feel and hear: the haptic cues sent and how the mixer
//! folded them together, and the impact sounds the hands made.

use jc3vr_gameplay::controllers::{config::CONTROLLERS_CONFIG, haptics, physical_hands::sounds};

use crate::{
    controllers::menu::read,
    status::{Line, Row, text},
};

pub(crate) const HAPTIC_CUES: Row = Row::new("HAPTIC CUES", "Haptic cues", haptic_cues);
pub(crate) const IMPACT_SOUNDS: Row = Row::new("IMPACT SOUNDS", "Impact sounds", impact_sounds);

fn haptic_cues() -> Line {
    if !read(&CONTROLLERS_CONFIG, |c| c.haptics.enabled) {
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

fn impact_sounds() -> Line {
    if !read(&CONTROLLERS_CONFIG, |c| c.physical_hands.sounds.enabled) {
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
