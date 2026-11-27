//! The status rows for what the hands feel and hear: the haptic cues sent and how the mixer
//! folded them together, and the impact sounds the hands made.

use jc3vr_body::controllers::haptics;
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_physical::controllers::physical_hands::sounds;
use smol_str::format_smolstr;

use crate::status::{Line, Row};

pub(crate) const HAPTIC_CUES: Row = Row::new("Haptic cues", haptic_cues);
pub(crate) const IMPACT_SOUNDS: Row = Row::new("Impact sounds", impact_sounds);

fn haptic_cues() -> Line {
    if !CONTROLLERS_CONFIG.query(|c| c.haptics.enabled) {
        return Line::off("");
    }
    let status = haptics::status();
    let detail = format_smolstr!(
        "{} sent, {} masked, {} limited",
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

fn impact_sounds() -> Line {
    let status = sounds::status();
    let detail = format_smolstr!(
        "{} played, {} held back, {} silent",
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
