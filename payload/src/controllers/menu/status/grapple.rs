//! The status page's rows for the physical grapple: where the grapple is, what its line holds, the
//! reel and the tethers the hand works, the hang swing, and the grapple hand's vibration.

use smol_str::format_smolstr;

use crate::{
    controllers::{
        hands::anchor,
        menu::{read_config, status::Line},
    },
    grapple::physical::{self, GrapplePhase, GrappleStatus},
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
    let detail = format_smolstr!("{} SWINGS, {} PUMPED", status.swings, status.pumps);
    if status.swings == 0 {
        Line::waiting(detail)
    } else {
        Line::ok(detail)
    }
}

pub(super) fn haptics() -> Line {
    if !read_config(|c| c.controllers.grapple.haptics.enabled) {
        return Line::off("");
    }
    let status = physical::status();
    Line::ok(format_smolstr!("{:.2}", status.haptic))
}

fn state_line(status: &GrappleStatus) -> Line {
    if !status.live {
        return Line::waiting("NO HAND INPUT");
    }
    Line::ok(status.phase.label())
}

fn target_line(status: &GrappleStatus) -> Line {
    match status.target {
        Some(target) => Line::ok(format_smolstr!(
            "{}, {:.1} M",
            if target.object { "OBJECT" } else { "SURFACE" },
            target.distance_m
        )),
        None if status.phase == GrapplePhase::InFlight => Line::waiting("IN FLIGHT"),
        None => Line::ok("NONE"),
    }
}

fn reel_line(enabled: bool, status: &GrappleStatus) -> Line {
    if !enabled {
        return Line::off("");
    }
    let counts = format_smolstr!("{} REELS, {} BOOSTS", status.reels, status.boosts);
    match status.reel_scale {
        Some(scale) => Line::ok(format_smolstr!(
            "{scale:.2}X{}, {counts}",
            if status.boosted { " BOOSTED" } else { "" }
        )),
        None if status.reels == 0 => Line::waiting(counts),
        None => Line::ok(counts),
    }
}

fn tethers_line(enabled: bool, status: &GrappleStatus) -> Line {
    let count = format_smolstr!("{} ATTACHED", status.tethers);
    if !enabled {
        return Line::off(count);
    }
    let pull = match status.retract {
        Some(value) => format_smolstr!(", PULL {value:.2}"),
        None => "".into(),
    };
    let squeeze = if status.squeezing {
        format_smolstr!(", TENSION {:.2}", status.tension)
    } else {
        "".into()
    };
    Line::ok(format_smolstr!(
        "{count}{pull}{squeeze}, {} PULLS",
        status.retracts
    ))
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
}
