//! The status page's rows for what the hands feel: the haptic cues sent and how the mixer folded
//! them together.

use smol_str::format_smolstr;

use crate::controllers::{
    haptics,
    menu::{read_config, status::Line},
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
