//! The status rows for the grip model: what each hand's squeeze would take or holds.

use jc3vr_body::controllers::grab::{latch::GripLatch, tick, tick::GrabStatus};
use jc3vr_input::frame::Hand;

use crate::status::{Health, Line, Row, prose, text};

pub(crate) const LEFT_GRIP: Row = Row::new("LEFT GRIP", "Left grip", left_grip);
pub(crate) const RIGHT_GRIP: Row = Row::new("RIGHT GRIP", "Right grip", right_grip);

fn left_grip() -> Line {
    grip_line(&tick::status(), Hand::Left)
}

fn right_grip() -> Line {
    grip_line(&tick::status(), Hand::Right)
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

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::grab::{GrabVolume, tick::HandStatus};

    use super::*;

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
}
