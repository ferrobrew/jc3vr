//! The status rows for the grip model: what each hand's squeeze would take or holds, and
//! the markers.

use smol_str::{SmolStr, format_smolstr};

use crate::{
    controllers::{
        frame::Hand,
        grab::{
            latch::GripLatch,
            tick::{self, GrabStatus},
        },
        menu::read_config,
    },
    status::{Health, Line},
};

pub(super) fn left_grip() -> Line {
    grip_line(&tick::status(), Hand::Left)
}

pub(super) fn right_grip() -> Line {
    grip_line(&tick::status(), Hand::Right)
}

pub(super) fn markers() -> Line {
    let on = read_config(|c| c.controllers.overlay.enabled && c.controllers.overlay.markers);
    markers_line(on, &tick::status())
}

/// What `hand`'s grip is doing, and what a squeeze would take.
fn grip_line(status: &GrabStatus, hand: Hand) -> Line {
    if !status.live {
        return Line::waiting("NOT ACTING");
    }
    let hand = status.hands[hand.index()];
    let chosen = hand.chosen.map(|(volume, distance)| {
        format_smolstr!(
            "{} AT {:.0} MM",
            volume.label().to_uppercase(),
            distance * 1000.0
        )
    });
    let detail: SmolStr = match (hand.latch, chosen) {
        (GripLatch::Grab(volume), _) => format_smolstr!("HOLDS {}", volume.label().to_uppercase()),
        (GripLatch::Default, _) => "SQUEEZED FOR A GAME ACTION".into(),
        (GripLatch::Fist { .. }, Some(chosen)) => {
            format_smolstr!("FIST, SLOW DOWN TO TAKE {chosen}")
        }
        (GripLatch::Fist { .. }, None) => "FIST, NOTHING IN REACH".into(),
        (GripLatch::Open, Some(chosen)) => format_smolstr!("SQUEEZE TAKES {chosen}"),
        (GripLatch::Open, None) => format_smolstr!("NOTHING IN REACH OF {}", status.targets),
    };
    Line::new(Health::Ok, detail).with_note(format!("{} targets in reach", hand.in_reach))
}

fn markers_line(on: bool, status: &GrabStatus) -> Line {
    if !on {
        return Line::off("SWITCH ON IN VR: CONTROLS (GRIP MARKERS)");
    }
    if !status.live {
        return Line::waiting("NOT ACTING");
    }
    Line::ok(format_smolstr!("{} TARGETS", status.targets))
        .with_note(format!("{} grabs taken", status.grabs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::grab::{GrabVolume, tick::HandStatus};

    fn live(hands: [HandStatus; 2]) -> GrabStatus {
        GrabStatus {
            live: true,
            targets: 7,
            hands,
            ..GrabStatus::default()
        }
    }

    #[test]
    fn a_grip_row_says_what_a_squeeze_takes_or_what_it_holds() {
        let idle = HandStatus {
            chosen: Some((GrabVolume::GrenadePouch, 0.042)),
            in_reach: 2,
            ..HandStatus::default()
        };
        let line = grip_line(&live([idle, HandStatus::default()]), Hand::Left);
        assert_eq!(line.text(), "OK, SQUEEZE TAKES GRENADE POUCH AT 42 MM");
        let line = grip_line(&live([idle, HandStatus::default()]), Hand::Right);
        assert_eq!(line.text(), "OK, NOTHING IN REACH OF 7");
        let holding = HandStatus {
            latch: GripLatch::Grab(GrabVolume::Foregrip),
            ..idle
        };
        let line = grip_line(&live([holding, HandStatus::default()]), Hand::Left);
        assert_eq!(line.text(), "OK, HOLDS FOREGRIP");
        assert_eq!(
            grip_line(&GrabStatus::default(), Hand::Left).health,
            Health::Waiting
        );
    }

    #[test]
    fn the_markers_row_follows_its_switch() {
        assert_eq!(
            markers_line(false, &live(Default::default())).health,
            Health::Off
        );
        assert_eq!(
            markers_line(true, &live(Default::default())).text(),
            "OK, 7 TARGETS"
        );
    }
}
