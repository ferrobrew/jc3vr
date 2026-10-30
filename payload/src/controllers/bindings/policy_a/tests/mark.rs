//! The mark chord: the left stick click and then the right A, the panel chord's sibling.

use super::*;

const A: [bool; CONTROLS] = [false, false, false, true];
const LA: [bool; CONTROLS] = [true, false, false, true];
const LBA: [bool; CONTROLS] = [true, false, true, true];
const LRA: [bool; CONTROLS] = [true, true, false, true];

#[test]
fn generated_mark_order_and_exclusive_join_boundaries() {
    for gap in [1, 99, 100, 349, 350, 351, 1000] {
        let mut history = History::new();
        history.step(1, LS);
        let joined = history.step(1 + gap, LA);
        assert_eq!(
            command(joined),
            (gap < 350).then_some(SystemCommand::Mark),
            "gap={gap}, joined={joined:?}"
        );
        assert!(!joined.sources[Control::RightA.index()].single_down);
        assert_eq!(joined.singles[Control::RightA.index()], None);
        assert_eq!(command(history.step(2000, LA)), None);
    }
}

#[test]
fn a_alone_commits_on_its_press_while_the_left_click_is_neutral() {
    let mut history = History::new();
    let pressed = history.step(1, A);
    assert!(matches!(
        pressed.singles[Control::RightA.index()],
        Some(SingleEvent::Pressed { accepted_at, .. }) if accepted_at == ms(1)
    ));
    assert!(pressed.sources[Control::RightA.index()].single_down);
    assert!(!pressed.withheld[Control::RightA.index()]);
    assert!(matches!(
        history.step(2, UP).singles[Control::RightA.index()],
        Some(SingleEvent::Released { .. })
    ));
}

#[test]
fn immediate_a_first_commits_and_cannot_promote() {
    let mut history = History::new();
    history.step(1, A);
    let joined = history.step(2, LA);
    assert_eq!(joined.request, None);
    assert!(joined.sources[Control::RightA.index()].single_down);
    assert_eq!(joined.singles[Control::LeftStick.index()], None);
    assert_eq!(joined.refused[0], Some(RefusalReason::CommittedSingle));
    assert_eq!(command(history.step(5000, LA)), None);
    assert!(matches!(
        history.step(5001, UP).singles[Control::RightA.index()],
        Some(SingleEvent::Released { .. })
    ));
    assert!(matches!(
        history.step(5002, A).singles[Control::RightA.index()],
        Some(SingleEvent::Pressed { .. })
    ));
}

#[test]
fn the_mark_fires_once_and_its_members_stay_consumed_until_common_neutral() {
    for release_left_first in [false, true] {
        let mut history = History::new();
        let mut outputs = vec![history.step(1, LS), history.step(100, LA)];
        assert_eq!(command(outputs[1]), Some(SystemCommand::Mark));
        let partial = if release_left_first { A } else { LS };
        outputs.extend([
            history.step(200, LA),
            history.step(300, partial),
            history.step(400, LA),
            history.step(500, partial),
            history.step(600, UP),
        ]);
        for output in &outputs {
            no_singles(output);
        }
        assert_eq!(
            outputs.iter().filter(|out| out.request.is_some()).count(),
            1
        );
        let neutral = outputs.last().unwrap();
        assert!(neutral.withheld[0] && neutral.withheld[3]);
        // A fresh chord marks again after the common neutral.
        history.step(700, LS);
        assert_eq!(command(history.step(710, LA)), Some(SystemCommand::Mark));
    }
}

#[test]
fn the_left_click_wait_is_unchanged_by_the_mark_chord() {
    let mut history = History::new();
    no_singles(&history.step(1, LS));
    no_singles(&history.step(350, LS));
    let committed = history.step(351, LS);
    assert!(matches!(
        committed.singles[Control::LeftStick.index()],
        Some(SingleEvent::Pressed { accepted_at, .. }) if accepted_at == ms(351)
    ));
    // An A after the left click committed is refused rather than marking or jumping.
    let late = history.step(352, LA);
    assert_eq!(late.request, None);
    assert_eq!(late.refused[3], Some(RefusalReason::CommittedSingle));
    assert_eq!(late.singles[Control::RightA.index()], None);
}

#[test]
fn ls_keeps_waiting_for_the_mark_after_recenter_expiry() {
    let mut history = History::new();
    history.step(1, LS);
    no_singles(&history.step(101, LS));
    let mark = history.step(300, LA);
    assert_eq!(mark.singles, [None; CONTROLS]);
    assert_eq!(command(mark), Some(SystemCommand::Mark));
}

#[test]
fn ambiguous_ls_a_group_is_consumed_without_guessing_order() {
    let mut history = History::new();
    let ambiguous = history.step(1, LA);
    no_singles(&ambiguous);
    assert_eq!(ambiguous.request, None);
    assert_eq!(ambiguous.refused[0], Some(RefusalReason::AmbiguousOrder));
    assert_eq!(ambiguous.refused[3], Some(RefusalReason::AmbiguousOrder));
    no_singles(&history.step(5000, LA));
    no_singles(&history.step(5001, A));
    no_singles(&history.step(5002, UP));
    assert!(history.step(5003, A).sources[3].single_down);
}

#[test]
fn the_face_button_after_the_left_click_picks_the_panel_or_the_mark() {
    for (partner, expected) in [(LB, SystemCommand::TogglePanel), (LA, SystemCommand::Mark)] {
        let mut history = History::new();
        history.step(1, LS);
        let joined = history.step(50, partner);
        no_singles(&joined);
        assert_eq!(command(joined), Some(expected));
    }
}

#[test]
fn b_and_a_joining_together_form_the_panel_and_refuse_the_mark() {
    let mut history = History::new();
    history.step(1, LS);
    let joined = history.step(50, LBA);
    no_singles(&joined);
    assert_eq!(command(joined), Some(SystemCommand::TogglePanel));
    assert_eq!(joined.refused[3], Some(RefusalReason::CompetingChord));
    no_singles(&history.step(60, LA));
    no_singles(&history.step(70, UP));
    assert!(history.step(80, A).sources[3].single_down);
}

#[test]
fn an_a_after_the_panel_formed_is_consumed_not_a_second_command() {
    let mut history = History::new();
    history.step(1, LS);
    assert_eq!(
        command(history.step(50, LB)),
        Some(SystemCommand::TogglePanel)
    );
    let late = history.step(100, LBA);
    no_singles(&late);
    assert_eq!(late.request, None);
    assert_eq!(late.refused[3], Some(RefusalReason::ClaimedPress));
}

#[test]
fn recenter_keeps_priority_over_a_mark_that_joins_with_it() {
    let mut history = History::new();
    history.step(1, LS);
    let formed = history.step(50, LRA);
    no_singles(&formed);
    assert_eq!(formed.request, None);
    assert_eq!(formed.refused[3], Some(RefusalReason::CompetingChord));
    let fired = history.step(1050, LRA);
    no_singles(&fired);
    assert_eq!(command(fired), Some(SystemCommand::Recenter));
}

#[test]
fn generated_short_histories_with_a_never_emit_claimed_constituents() {
    let levels = [UP, LS, RS, B, A, LB, LA, LBA, LRA];
    for &first in &levels {
        for &second in &levels {
            for &third in &levels {
                let mut history = History::new();
                let inputs = [(1, first), (2, second), (3, third), (4, LA), (1004, LA)];
                let outputs = inputs.map(|(at, down)| history.step(at, down));
                assert_source_history_contract(&inputs, &outputs);
            }
        }
    }
}
