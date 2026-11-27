//! The status rows for the physical grapple: where the grapple is, what its line holds, the
//! reel and the tethers the hand works, the lock, the seat grapple, the hang swing, the snatch, and
//! the grapple hand's vibration.

use jc3vr_body::controllers::hands::anchor;
use jc3vr_gameplay::controllers::config::CONTROLLERS_CONFIG;
use jc3vr_grapple::{
    controllers::{snatch, snatch::SnatchStatus},
    grapple::{
        physical,
        physical::{GrapplePhase, GrappleStatus, lock, lock::LockStatus},
        seat,
        seat::SeatStatus,
    },
};
use smol_str::{SmolStr, format_smolstr};

use crate::status::{Line, Row};

pub(crate) const STATE: Row = Row::new("Grapple state", state);
pub(crate) const TARGET: Row = Row::new("Attached target", target);
pub(crate) const REEL: Row = Row::new("Reel from hand", reel);
pub(crate) const TETHERS: Row = Row::new("Tethers", tethers);
pub(crate) const LOCK: Row = Row::new("Line lock", lock);
pub(crate) const SEAT: Row = Row::new("Seat grapple", seat_row);
pub(crate) const SWING: Row = Row::new("Hang swing", swing);
pub(crate) const SNATCH: Row = Row::new("Snatch", snatch);
pub(crate) const HAPTICS: Row = Row::new("Line haptics", haptics);

fn state() -> Line {
    state_line(&physical::status())
}

fn target() -> Line {
    target_line(&physical::status())
}

fn reel() -> Line {
    reel_line(&physical::status())
}

fn tethers() -> Line {
    tethers_line(&physical::status())
}

fn lock() -> Line {
    lock_line(&lock::status())
}

fn seat_row() -> Line {
    seat_line(&seat::status())
}

fn swing() -> Line {
    let status = anchor::status();
    let detail = format_smolstr!("{} swings, {} pumped", status.swings, status.pumps);
    if status.swings == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}

fn snatch() -> Line {
    snatch_line(&snatch::status())
}

fn haptics() -> Line {
    if !CONTROLLERS_CONFIG.query(|c| c.haptics.enabled) {
        return Line::off("");
    }
    let status = physical::status();
    Line::ok(format_smolstr!("{:.2}", status.haptic))
}

fn snatch_line(status: &SnatchStatus) -> Line {
    if !status.live {
        return Line::waiting("No hand input");
    }
    let target = match (status.swing, status.target, status.unoffered) {
        (Some(swing), _, _) => format_smolstr!(
            "{:.0} kg, {}, {}{}",
            swing.mass_kg,
            held_length(swing.length_m),
            line_tension(swing.tension_n, swing.max_tension_n),
            if swing.braced {
                SmolStr::new_static(", braced")
            } else {
                SmolStr::default()
            }
        ),
        (None, Some(target), _) => match target.mass_kg {
            Some(mass) => {
                format_smolstr!("{}, {mass:.0} kg, {:.1} m", target.kind, target.distance_m)
            }
            None => format_smolstr!("{}, {:.1} m", target.kind, target.distance_m),
        },
        (None, None, Some(why)) => why.into(),
        (None, None, None) => SmolStr::new_static("No target"),
    };
    let line = Line::ok(format_smolstr!(
        "{}, {}, {} taken of {}",
        status.phase,
        target,
        status.taken,
        status.fired
    ));
    let mut note = format!("{} failed, {} tossed", status.failed, status.tosses);
    if let Some(mass) = status.aimed_mass_kg {
        note += &format!(", {mass:.0} kg under the aim");
    }
    if let Some(refusal) = &status.last_refusal {
        note += &format!(", last refusal: {refusal}");
    }
    line.with_note(note)
}

fn state_line(status: &GrappleStatus) -> Line {
    if !status.live {
        return Line::waiting("No hand input");
    }
    Line::ok(status.phase.label())
}

fn target_line(status: &GrappleStatus) -> Line {
    match status.target {
        Some(target) => Line::ok(format_smolstr!(
            "{}, {:.1} m",
            if target.object {
                SmolStr::new_static("Object")
            } else {
                SmolStr::new_static("Surface")
            },
            target.distance_m
        )),
        None if status.phase == GrapplePhase::InFlight => Line::waiting("In flight"),
        None => Line::ok("None"),
    }
}

fn reel_line(status: &GrappleStatus) -> Line {
    let counts = format_smolstr!(
        "{} reels, {} boosts",
        status.counts.reels,
        status.counts.boosts
    );
    match status.reel_scale {
        Some(scale) => Line::ok(format_smolstr!(
            "{scale:.2}x{}, {}",
            if status.boosted {
                SmolStr::new_static(" boosted")
            } else {
                SmolStr::default()
            },
            counts
        )),
        None if status.counts.reels == 0 => Line::waiting(counts),
        None => Line::ok(counts),
    }
}

fn lock_line(status: &LockStatus) -> Line {
    let counts = format_smolstr!("{} locks, {} broke", status.locks, status.breaks);
    if !status.locked {
        let last = status.last_end.map_or_else(SmolStr::default, |end| {
            format_smolstr!(", last {}", end.label())
        });
        return if status.locks == 0 {
            Line::waiting(counts)
        } else {
            Line::ok(format_smolstr!("{}{}", counts, last))
        };
    }
    let target = status
        .target
        .map_or_else(SmolStr::default, |target| match target.mass_kg {
            Some(mass) => format_smolstr!("{} {mass:.0} kg, ", target.kind),
            None => format_smolstr!("{}, ", target.kind),
        });
    Line::ok(format_smolstr!(
        "{}{}{}{}, stretch {:+.2} m, {}, hook {:.1} m/s, {}",
        if status.hanging {
            SmolStr::new_static("hanging, ")
        } else {
            SmolStr::default()
        },
        if status.braking {
            SmolStr::new_static("coasting in, ")
        } else {
            SmolStr::default()
        },
        target,
        held_length(status.length_m),
        status.stretch_m,
        line_tension(status.tension_n, status.max_tension_n),
        status.hook_speed_mps,
        counts
    ))
}

fn seat_line(status: &SeatStatus) -> Line {
    let counts = format_smolstr!(
        "{} fired, {} tethered, {} missed",
        status.fires,
        status.tethers,
        status.misses
    );
    let refused = status.refused.map_or_else(SmolStr::default, |why| {
        format_smolstr!(", refused: {}", why.label())
    });
    if status.flying {
        Line::ok(format_smolstr!("flying, {}", counts))
    } else if status.fires == 0 {
        Line::waiting(format_smolstr!("{}{}", counts, refused))
    } else {
        Line::ok(format_smolstr!("{}{}", counts, refused))
    }
}

/// A held line's length, for the snatch's and the lock's rows.
fn held_length(length_m: f32) -> SmolStr {
    format_smolstr!("held {length_m:.1} m")
}

/// A held line's tension of its strength, for the snatch's and the lock's rows.
fn line_tension(tension_n: f32, max_tension_n: f32) -> SmolStr {
    format_smolstr!("{tension_n:.0} of {max_tension_n:.0} N")
}

fn tethers_line(status: &GrappleStatus) -> Line {
    let count = format_smolstr!("{} attached", status.tethers);
    let pull = status.retract.map_or_else(SmolStr::default, |value| {
        format_smolstr!(", pull {value:.2}")
    });
    let squeeze = if status.squeezing {
        format_smolstr!(", tension {:.2}", status.tension)
    } else {
        SmolStr::default()
    };
    Line::ok(format_smolstr!(
        "{}{}{}, {} pulls",
        count,
        pull,
        squeeze,
        status.counts.retracts
    ))
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use jc3vr_grapple::grapple::physical::{GrappleCounts, TargetReadout};

    use super::*;

    #[test]
    fn the_seat_row_reads_the_seat_grapple() {
        let refused = SeatStatus {
            refused: Some(seat::Refusal::NoTarget),
            ..SeatStatus::default()
        };
        assert_eq!(
            seat_line(&refused).text(),
            "Waiting, 0 fired, 0 tethered, 0 missed, refused: nothing in range"
        );
        let flying = SeatStatus {
            flying: true,
            fires: 1,
            ..SeatStatus::default()
        };
        assert_eq!(
            seat_line(&flying).text(),
            "Ok, flying, 1 fired, 0 tethered, 0 missed"
        );
    }

    #[test]
    fn the_lock_row_reads_the_lock() {
        assert_eq!(
            lock_line(&LockStatus::default()).text(),
            "Waiting, 0 locks, 0 broke"
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
            lock_line(&held).text(),
            "Ok, vehicle 1500 kg, held 8.2 m, stretch +0.12 m, 900 of 3924 N, hook 12.0 m/s, 1 locks, 0 broke"
        );
        let hanging = LockStatus {
            hanging: true,
            target: Some(lock::LockTarget {
                kind: "static",
                mass_kg: None,
            }),
            ..held
        };
        assert!(
            lock_line(&hanging)
                .text()
                .starts_with("Ok, hanging, static, held 8.2 m")
        );
        let ended = LockStatus {
            locked: false,
            breaks: 1,
            last_end: Some(lock::LockEnd::Broke(lock::BreakCause::Stretched)),
            ..held
        };
        assert_eq!(
            lock_line(&ended).text(),
            "Ok, 1 locks, 1 broke, last broke: overstretched"
        );
    }

    #[test]
    fn the_rows_read_the_readout() {
        let idle = GrappleStatus::default();
        assert_eq!(state_line(&idle).text(), "Waiting, No hand input");
        let live = GrappleStatus {
            live: true,
            phase: GrapplePhase::ReelingIn,
            target: Some(TargetReadout {
                object: true,
                distance_m: 12.34,
            }),
            reel_scale: Some(1.25),
            boosted: true,
            counts: GrappleCounts {
                reels: 2,
                boosts: 1,
                retracts: 3,
            },
            tethers: 2,
            retract: Some(0.5),
            squeezing: true,
            tension: 0.75,
            ..GrappleStatus::default()
        };
        assert_eq!(state_line(&live).text(), "Ok, Reeling in");
        assert_eq!(target_line(&live).text(), "Ok, Object, 12.3 m");
        assert_eq!(
            reel_line(&live).text(),
            "Ok, 1.25x boosted, 2 reels, 1 boosts"
        );
        assert_eq!(reel_line(&idle).text(), "Waiting, 0 reels, 0 boosts");
        assert_eq!(
            tethers_line(&live).text(),
            "Ok, 2 attached, pull 0.50, tension 0.75, 3 pulls"
        );
    }

    #[test]
    fn the_snatch_row_reads_the_readout() {
        let idle = SnatchStatus::default();
        assert_eq!(snatch_line(&idle).text(), "Waiting, No hand input");
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
        let line = snatch_line(&aimed);
        assert_eq!(
            line.text(),
            "Ok, ARMED, WEAPON ON THE GROUND, 10.0 m, 2 taken of 3"
        );
        assert_eq!(
            line.note.as_deref(),
            Some("0 failed, 0 tossed, last refusal: snatch: the weapon snagged")
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
                line_held: false,
                tethered: true,
                anchor: Vec3::ZERO,
                centre: Vec3::NEG_Z * 3.0,
            }),
            ..SnatchStatus::default()
        };
        assert_eq!(
            snatch_line(&swinging).text(),
            "Ok, SWINGING, 150 kg, held 3.0 m, 1200 of 5810 N, braced, 0 taken of 0"
        );
        let busy = SnatchStatus {
            live: true,
            phase: "IDLE",
            unoffered: Some("HAND NOT EMPTY"),
            ..SnatchStatus::default()
        };
        assert_eq!(
            snatch_line(&busy).text(),
            "Ok, IDLE, HAND NOT EMPTY, 0 taken of 0"
        );
    }
}
