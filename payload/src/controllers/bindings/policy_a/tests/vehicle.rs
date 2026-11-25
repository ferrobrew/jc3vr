//! The developer chords in a vehicle: withheld under the suppression setting, so the left stick
//! click waits only for the recenter join window, and formed as on foot without it.

use super::*;

const A: [bool; CONTROLS] = [false, false, false, true];
const LA: [bool; CONTROLS] = [true, false, false, true];

fn in_vehicle(suppress: bool) -> History {
    let mut history = History::with_config(PolicyAConfig {
        recenter_join: ms(100),
        panel_join: ms(350),
        recenter_hold: ms(1000),
        suppress_developer_chords_in_vehicles: suppress,
    });
    history.vehicle = true;
    history
}

fn left_tap_committed_at(history: &mut History) -> Option<u64> {
    history.step(1, LS);
    (2..=400).find(|&at| {
        matches!(
            history.step(at, LS).singles[Control::LeftStick.index()],
            Some(SingleEvent::Pressed { .. })
        )
    })
}

#[test]
fn suppressed_left_click_waits_only_for_the_recenter_window() {
    let mut history = in_vehicle(true);
    assert_eq!(left_tap_committed_at(&mut history), Some(101));
}

#[test]
fn unsuppressed_left_click_waits_for_the_panel_window() {
    let mut history = in_vehicle(false);
    assert_eq!(left_tap_committed_at(&mut history), Some(351));
}

#[test]
fn on_foot_left_click_waits_for_the_panel_window_under_suppression() {
    let mut history = History::new();
    assert_eq!(left_tap_committed_at(&mut history), Some(351));
}

#[test]
fn a_released_suppressed_left_click_commits_as_a_tap_at_once() {
    let mut history = in_vehicle(true);
    history.step(1, LS);
    assert!(matches!(
        history.step(20, UP).singles[Control::LeftStick.index()],
        Some(SingleEvent::Tap { .. })
    ));
}

#[test]
fn suppressed_panel_and_mark_presses_act_as_singles() {
    for (partner, joined) in [(Control::RightB, LB), (Control::RightA, LA)] {
        let mut history = in_vehicle(true);
        history.step(1, LS);
        let out = history.step(50, joined);
        assert_eq!(out.request, None, "{partner:?}: {out:?}");
        assert_eq!(out.refused, [None; CONTROLS], "{partner:?}");
        assert!(matches!(
            out.singles[partner.index()],
            Some(SingleEvent::Pressed { .. })
        ));
        assert!(matches!(
            history.step(101, joined).singles[Control::LeftStick.index()],
            Some(SingleEvent::Pressed { .. })
        ));
        assert_eq!(command(history.step(400, joined)), None);
    }
}

#[test]
fn unsuppressed_panel_and_mark_form_in_a_vehicle() {
    for (joined, expected) in [(LB, SystemCommand::TogglePanel), (LA, SystemCommand::Mark)] {
        let mut history = in_vehicle(false);
        history.step(1, LS);
        let out = history.step(50, joined);
        assert_eq!(command(out), Some(expected));
        no_singles(&out);
    }
}

#[test]
fn suppressed_unordered_sync_is_not_refused_as_ambiguous() {
    let mut history = in_vehicle(true);
    let out = history.step(1, LB);
    assert_eq!(out.refused, [None; CONTROLS]);
    assert!(matches!(
        out.singles[Control::RightB.index()],
        Some(SingleEvent::Pressed { .. })
    ));
}

#[test]
fn recenter_still_forms_under_suppression() {
    let mut history = in_vehicle(true);
    history.step(1, LS);
    history.step(50, LR);
    assert_eq!(
        command(history.step(1050, LR)),
        Some(SystemCommand::Recenter)
    );
}

#[test]
fn a_lone_face_button_still_commits_at_once_under_suppression() {
    let mut history = in_vehicle(true);
    assert!(matches!(
        history.step(1, A).singles[Control::RightA.index()],
        Some(SingleEvent::Pressed { .. })
    ));
}
