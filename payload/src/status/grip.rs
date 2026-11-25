//! The status rows for the grip model: what each hand's squeeze would take or holds, and
//! the markers.

use crate::{
    controllers::{
        frame::Hand,
        grab::{
            latch::GripLatch,
            tick::{self, GrabStatus},
        },
        menu::read_config,
    },
    status::{Health, Line, Row, prose, text},
};

pub(crate) const LEFT_GRIP: Row = Row::new("LEFT GRIP", "Left grip", left_grip);
pub(crate) const RIGHT_GRIP: Row = Row::new("RIGHT GRIP", "Right grip", right_grip);
pub(crate) const MARKERS: Row = Row::new("GRIP MARKERS", "Grip markers", markers);

fn left_grip() -> Line {
    grip_line(&tick::status(), Hand::Left)
}

fn right_grip() -> Line {
    grip_line(&tick::status(), Hand::Right)
}

fn markers() -> Line {
    let on = read_config(|c| c.controllers.overlay.enabled && c.controllers.overlay.markers);
    markers_line(on, &tick::status())
}

/// What `hand`'s grip is doing, and what a squeeze would take.
fn grip_line(status: &GrabStatus, hand: Hand) -> Line {
    if !status.live {
        return Line::waiting(prose("Not acting"));
    }
    let hand = status.hands[hand.index()];
    let chosen = hand.chosen.map(|(volume, distance)| {
        text!(
            prose(volume.label()),
            " at ",
            format!("{:.0}", distance * 1000.0),
            " mm"
        )
    });
    let detail = match (hand.latch, chosen) {
        (GripLatch::Grab(volume), _) => text!("Holds ", prose(volume.label())),
        (GripLatch::Default, _) => prose("Squeezed for a game action"),
        (GripLatch::Fist { .. }, Some(chosen)) => text!("Fist, slow down to take ", chosen),
        (GripLatch::Fist { .. }, None) => prose("Fist, nothing in reach"),
        (GripLatch::Open, Some(chosen)) => text!("Squeeze takes ", chosen),
        (GripLatch::Open, None) => text!("Nothing in reach of ", status.targets.to_string()),
    };
    Line::new(Health::Ok, detail).with_note(format!("{} targets in reach", hand.in_reach))
}

fn markers_line(on: bool, status: &GrabStatus) -> Line {
    if !on {
        return Line::off(prose("Switch on in VR: controls (grip markers)"));
    }
    if !status.live {
        return Line::waiting(prose("Not acting"));
    }
    Line::ok(text!(status.targets.to_string(), " targets"))
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
