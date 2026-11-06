//! The status rows for the physical grapple: where the grapple is, what its line holds, the
//! reel and the tethers the hand works, the lock, the seat grapple, the hang swing, the snatch, and
//! the grapple hand's vibration.

use smol_str::format_smolstr;

use crate::{
    controllers::{
        hands::anchor,
        menu::read_config,
        snatch::{self, SnatchStatus},
    },
    grapple::{
        physical::{
            self, GrapplePhase, GrappleStatus,
            lock::{self, LockStatus},
        },
        seat::{self, SeatStatus},
    },
    status::{Line, Row, Text, prose, text},
};

pub(crate) const STATE: Row = Row::new("GRAPPLE STATE", "Grapple state", state);
pub(crate) const TARGET: Row = Row::new("ATTACHED TARGET", "Attached target", target);
pub(crate) const REEL: Row = Row::new("REEL FROM HAND", "Reel from hand", reel);
pub(crate) const TETHERS: Row = Row::new("TETHERS", "Tethers", tethers);
pub(crate) const LOCK: Row = Row::new("LINE LOCK", "Line lock", lock);
pub(crate) const SEAT: Row = Row::new("SEAT GRAPPLE", "Seat grapple", seat_row);
pub(crate) const SWING: Row = Row::new("HANG SWING", "Hang swing", swing);
pub(crate) const SNATCH: Row = Row::new("SNATCH", "Snatch", snatch);
pub(crate) const HAPTICS: Row = Row::new("LINE HAPTICS", "Line haptics", haptics);

fn state() -> Line {
    state_line(&physical::status())
}

fn target() -> Line {
    target_line(&physical::status())
}

fn reel() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.pull_to_reel);
    reel_line(enabled, &physical::status())
}

fn tethers() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.pull_to_reel);
    tethers_line(enabled, &physical::status())
}

fn lock() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.lock.enabled);
    lock_line(enabled, &lock::status())
}

fn seat_row() -> Line {
    let enabled = read_config(|c| c.controllers.grapple.seat);
    seat_line(enabled, &seat::status())
}

fn swing() -> Line {
    let (hold, pump) = read_config(|c| {
        (
            c.controllers.hands.anchor.enabled,
            c.controllers.grapple.pump,
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

fn snatch() -> Line {
    let enabled = read_config(|c| c.controllers.weapons.snatch.enabled);
    snatch_line(enabled, &snatch::status())
}

fn haptics() -> Line {
    if !read_config(|c| c.controllers.haptics.enabled) {
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
    let target = match (status.swing, status.target, status.unoffered) {
        (Some(swing), _, _) => text!(
            format!("{:.0}", swing.mass_kg),
            " kg, ",
            held_length(swing.length_m),
            ", ",
            line_tension(swing.tension_n, swing.max_tension_n),
            if swing.braced {
                prose(", braced")
            } else {
                Text::default()
            }
        ),
        (None, Some(target), _) => match target.mass_kg {
            Some(mass) => text!(
                snatch_kind(target.kind),
                ", ",
                format!("{mass:.0}"),
                " kg, ",
                format!("{:.1}", target.distance_m),
                " m"
            ),
            None => text!(
                snatch_kind(target.kind),
                ", ",
                format!("{:.1}", target.distance_m),
                " m"
            ),
        },
        (None, None, Some(why)) => snatch_reason(why),
        (None, None, None) => prose("No target"),
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

fn lock_line(enabled: bool, status: &LockStatus) -> Line {
    if !enabled {
        return Line::off("");
    }
    let counts = text!(
        status.locks.to_string(),
        " locks, ",
        status.breaks.to_string(),
        " broke"
    );
    if !status.locked {
        let last = status
            .last_end
            .map_or_else(Text::default, |end| text!(", last ", prose(end.label())));
        return if status.locks == 0 {
            Line::waiting(counts)
        } else {
            Line::ok(text!(counts, last))
        };
    }
    let target = status
        .target
        .map_or_else(Text::default, |target| match target.mass_kg {
            Some(mass) => text!(prose(target.kind), " ", format!("{mass:.0}"), " kg, "),
            None => text!(prose(target.kind), ", "),
        });
    Line::ok(text!(
        if status.braking {
            prose("braking, ")
        } else {
            Text::default()
        },
        target,
        held_length(status.length_m),
        ", stretch ",
        format!("{:+.2}", status.stretch_m),
        " m, ",
        line_tension(status.tension_n, status.max_tension_n),
        ", hook ",
        format!("{:.1}", status.hook_speed_mps),
        " m/s, ",
        counts
    ))
}

fn seat_line(enabled: bool, status: &SeatStatus) -> Line {
    if !enabled {
        return Line::off("");
    }
    let counts = text!(
        status.fires.to_string(),
        " fired, ",
        status.tethers.to_string(),
        " tethered, ",
        status.misses.to_string(),
        " missed"
    );
    let refused = status.refused.map_or_else(Text::default, |why| {
        text!(", refused: ", prose(why.label()))
    });
    if status.flying {
        Line::ok(text!(prose("flying, "), counts))
    } else if status.fires == 0 {
        Line::waiting(text!(counts, refused))
    } else {
        Line::ok(text!(counts, refused))
    }
}

/// A held line's length, for the snatch's and the lock's rows.
fn held_length(length_m: f32) -> Text {
    text!("held ", format!("{length_m:.1}"), " m")
}

/// A held line's tension of its strength, for the snatch's and the lock's rows.
fn line_tension(tension_n: f32, max_tension_n: f32) -> Text {
    text!(
        format!("{tension_n:.0}"),
        " of ",
        format!("{max_tension_n:.0}"),
        " N"
    )
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
        "SWINGING" => "Swinging",
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
        "HEAVY OBJECT" => "heavy object",
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
    use glam::Vec3;

    use super::*;
    use crate::grapple::physical::TargetReadout;

    #[test]
    fn the_seat_row_reads_the_seat_grapple() {
        assert_eq!(seat_line(false, &SeatStatus::default()).text(), "OFF");
        let refused = SeatStatus {
            refused: Some(seat::Refusal::NoTarget),
            ..SeatStatus::default()
        };
        assert_eq!(
            seat_line(true, &refused).text(),
            "WAITING, 0 FIRED, 0 TETHERED, 0 MISSED, REFUSED: NOTHING IN RANGE"
        );
        let flying = SeatStatus {
            flying: true,
            fires: 1,
            ..SeatStatus::default()
        };
        assert_eq!(
            seat_line(true, &flying).text(),
            "OK, FLYING, 1 FIRED, 0 TETHERED, 0 MISSED"
        );
    }

    #[test]
    fn the_lock_row_reads_the_lock() {
        assert_eq!(lock_line(false, &LockStatus::default()).text(), "OFF");
        assert_eq!(
            lock_line(true, &LockStatus::default()).text(),
            "WAITING, 0 LOCKS, 0 BROKE"
        );
        let held = LockStatus {
            locked: true,
            length_m: 8.25,
            stretch_m: 0.12,
            tension_n: 900.0,
            max_tension_n: 3924.0,
            target: Some(lock::LockTarget {
                kind: "vehicle",
                mass_kg: Some(1500.0),
            }),
            hook_speed_mps: 12.0,
            locks: 1,
            ..LockStatus::default()
        };
        assert_eq!(
            lock_line(true, &held).text(),
            "OK, VEHICLE 1500 KG, HELD 8.2 M, STRETCH +0.12 M, 900 OF 3924 N, HOOK 12.0 M/S, 1 LOCKS, 0 BROKE"
        );
        let ended = LockStatus {
            locked: false,
            breaks: 1,
            last_end: Some(lock::LockEnd::Broke(lock::BreakCause::Stretched)),
            ..held
        };
        assert_eq!(
            lock_line(true, &ended).text(),
            "OK, 1 LOCKS, 1 BROKE, LAST BROKE: OVERSTRETCHED"
        );
    }

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
                mass_kg: None,
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
        let swinging = SnatchStatus {
            live: true,
            phase: "SWINGING",
            swing: Some(snatch::SwingReadout {
                length_m: 3.0,
                tension_n: 1200.0,
                max_tension_n: 5809.5,
                mass_kg: 150.0,
                braced: true,
                tethered: true,
                anchor: Vec3::ZERO,
                centre: Vec3::NEG_Z * 3.0,
            }),
            ..SnatchStatus::default()
        };
        assert_eq!(
            snatch_line(true, &swinging).text(),
            "OK, SWINGING, 150 KG, HELD 3.0 M, 1200 OF 5810 N, BRACED, 0 TAKEN OF 0"
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
