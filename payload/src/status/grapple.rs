//! The status rows for the physical grapple: where the grapple is, what its line holds, the
//! reel and the tethers the hand works, the hang swing, the snatch, and the grapple hand's
//! vibration.

use smol_str::format_smolstr;

use crate::{
    controllers::{
        hands::anchor,
        menu::read_config,
        snatch::{self, SnatchStatus},
    },
    grapple::physical::{self, GrapplePhase, GrappleStatus},
    status::{Line, Text, prose, text},
};

pub(super) fn state() -> Line {
    state_line(&physical::status())
}

pub(super) fn target() -> Line {
    target_line(&physical::status())
}

pub(super) fn reel() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.reel.enabled);
    reel_line(enabled, &physical::status())
}

pub(super) fn tethers() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.tether.enabled);
    tethers_line(enabled, &physical::status())
}

pub(super) fn swing() -> Line {
    let (hold, pump) = read_config(|c| {
        (
            c.controllers.hands.anchor.hang_swing,
            c.controllers.grapple.swing.pump,
        )
    });
    let status = anchor::status();
    if !(hold || pump) {
        return Line::off("");
    }
    let detail = text!(
        status.swings.to_string(),
        " swings, ",
        status.pumps.to_string(),
        " pumped"
    );
    if status.swings == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}

pub(super) fn snatch() -> Line {
    let enabled = read_config(|c| c.controllers.weapons.snatch.enabled);
    snatch_line(enabled, &snatch::status())
}

pub(super) fn haptics() -> Line {
    if !read_config(|c| c.controllers.haptics.categories.traversal) {
        return Line::off("");
    }
    let status = physical::status();
    Line::ok(format_smolstr!("{:.2}", status.haptic))
}

fn snatch_line(enabled: bool, status: &SnatchStatus) -> Line {
    if !enabled {
        return Line::off("");
    }
    if !status.live {
        return Line::waiting(prose("No hand input"));
    }
    let target = match (status.target, status.unoffered) {
        (Some(target), _) => text!(
            snatch_kind(target.kind),
            ", ",
            format!("{:.1}", target.distance_m),
            " m"
        ),
        (None, Some(why)) => snatch_reason(why),
        (None, None) => prose("No target"),
    };
    let line = Line::ok(text!(
        snatch_phase(status.phase),
        ", ",
        target,
        ", ",
        status.taken.to_string(),
        " taken of ",
        status.fired.to_string()
    ));
    match &status.last_refusal {
        Some(refusal) => line.with_note(format!("last: {refusal}")),
        None => line,
    }
}

fn state_line(status: &GrappleStatus) -> Line {
    if !status.live {
        return Line::waiting(prose("No hand input"));
    }
    let display = match status.phase {
        GrapplePhase::Inactive => "Inactive",
        GrapplePhase::Firing => "Firing",
        GrapplePhase::InFlight => "In flight",
        GrapplePhase::Attached => "Attached",
        GrapplePhase::ReelingIn => "Reeling in",
        GrapplePhase::Hanging => "Hanging",
        GrapplePhase::ReeledIn => "Reeled in",
        GrapplePhase::ScriptedWire => "Scripted wire",
    };
    Line::ok(Text::pair(status.phase.label(), display))
}

fn target_line(status: &GrappleStatus) -> Line {
    match status.target {
        Some(target) => Line::ok(text!(
            if target.object {
                prose("Object")
            } else {
                prose("Surface")
            },
            ", ",
            format!("{:.1}", target.distance_m),
            " m"
        )),
        None if status.phase == GrapplePhase::InFlight => Line::waiting(prose("In flight")),
        None => Line::ok(prose("None")),
    }
}

fn reel_line(enabled: bool, status: &GrappleStatus) -> Line {
    if !enabled {
        return Line::off("");
    }
    let counts = text!(
        status.reels.to_string(),
        " reels, ",
        status.boosts.to_string(),
        " boosts"
    );
    match status.reel_scale {
        Some(scale) => Line::ok(text!(
            format!("{scale:.2}"),
            "x",
            if status.boosted {
                prose(" boosted")
            } else {
                Text::default()
            },
            ", ",
            counts
        )),
        None if status.reels == 0 => Line::waiting(counts),
        None => Line::ok(counts),
    }
}

fn tethers_line(enabled: bool, status: &GrappleStatus) -> Line {
    let count = text!(status.tethers.to_string(), " attached");
    if !enabled {
        return Line::off(count);
    }
    let pull = status.retract.map_or_else(Text::default, |value| {
        text!(", pull ", format!("{value:.2}"))
    });
    let squeeze = if status.squeezing {
        text!(", tension ", format!("{:.2}", status.tension))
    } else {
        Text::default()
    };
    Line::ok(text!(
        count,
        pull,
        squeeze,
        ", ",
        status.retracts.to_string(),
        " pulls"
    ))
}

fn snatch_phase(phase: &str) -> Text {
    let display = match phase {
        "IDLE" => "Idle",
        "ARMED" => "Armed",
        "FREEING" => "Freeing",
        "HOOK OUT" => "Hook out",
        "PULLING" => "Pulling",
        other => other,
    };
    Text::pair(phase, display)
}

fn snatch_kind(kind: &str) -> Text {
    let display = match kind {
        "WEAPON ON THE GROUND" => "weapon on the ground",
        "WEAPON IN HANDS" => "weapon in hands",
        "HOLSTERED WEAPON" => "holstered weapon",
        "OBJECT" => "object",
        "CAR DOOR" => "car door",
        other => other,
    };
    Text::pair(kind, display)
}

fn snatch_reason(reason: &str) -> Text {
    let display = match reason {
        "NOT ON FOOT" => "not on foot",
        "HAND NOT EMPTY" => "hand not empty",
        "GRAPPLE BUTTON HELD" => "grapple button held",
        "NO HOOK" => "no hook",
        "LINE OUT" => "line out",
        "HOOK BUSY" => "hook busy",
        other => other,
    };
    Text::pair(reason, display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grapple::physical::TargetReadout;

    #[test]
    fn the_rows_read_the_readout() {
        let idle = GrappleStatus::default();
        assert_eq!(state_line(&idle).text(), "WAITING, NO HAND INPUT");
        let live = GrappleStatus {
            live: true,
            phase: GrapplePhase::ReelingIn,
            target: Some(TargetReadout {
                object: true,
                distance_m: 12.34,
            }),
            reel_scale: Some(1.25),
            boosted: true,
            reels: 2,
            boosts: 1,
            tethers: 2,
            retract: Some(0.5),
            retracts: 3,
            squeezing: true,
            tension: 0.75,
            ..GrappleStatus::default()
        };
        assert_eq!(state_line(&live).text(), "OK, REELING IN");
        assert_eq!(target_line(&live).text(), "OK, OBJECT, 12.3 M");
        assert_eq!(
            reel_line(true, &live).text(),
            "OK, 1.25X BOOSTED, 2 REELS, 1 BOOSTS"
        );
        assert_eq!(reel_line(false, &live).text(), "OFF");
        assert_eq!(reel_line(true, &idle).text(), "WAITING, 0 REELS, 0 BOOSTS");
        assert_eq!(
            tethers_line(true, &live).text(),
            "OK, 2 ATTACHED, PULL 0.50, TENSION 0.75, 3 PULLS"
        );
        assert_eq!(tethers_line(false, &live).text(), "OFF, 2 ATTACHED");
    }

    #[test]
    fn the_snatch_row_reads_the_readout() {
        let idle = SnatchStatus::default();
        assert_eq!(snatch_line(false, &idle).text(), "OFF");
        assert_eq!(snatch_line(true, &idle).text(), "WAITING, NO HAND INPUT");
        let aimed = SnatchStatus {
            live: true,
            phase: "ARMED",
            target: Some(snatch::TargetReadout {
                kind: "WEAPON ON THE GROUND",
                distance_m: 9.96,
            }),
            fired: 3,
            taken: 2,
            last_refusal: Some("snatch: the weapon snagged".to_owned()),
            ..SnatchStatus::default()
        };
        let line = snatch_line(true, &aimed);
        assert_eq!(
            line.text(),
            "OK, ARMED, WEAPON ON THE GROUND, 10.0 M, 2 TAKEN OF 3"
        );
        assert_eq!(
            line.note.as_deref(),
            Some("last: snatch: the weapon snagged")
        );
        let busy = SnatchStatus {
            live: true,
            phase: "IDLE",
            unoffered: Some("HAND NOT EMPTY"),
            ..SnatchStatus::default()
        };
        assert_eq!(
            snatch_line(true, &busy).text(),
            "OK, IDLE, HAND NOT EMPTY, 0 TAKEN OF 0"
        );
    }
}
