use super::*;

mod recovery;

const UP: [bool; 3] = [false; 3];
const LS: [bool; 3] = [true, false, false];
const RS: [bool; 3] = [false, true, false];
const B: [bool; 3] = [false, false, true];
const LR: [bool; 3] = [true, true, false];
const LB: [bool; 3] = [true, false, true];
const ALL: [bool; 3] = [true; 3];

struct History {
    arbiter: PolicyA,
    epoch: ClockEpoch,
    sequence: u64,
    generation: BindingGeneration,
    context: GameplayContext,
    enabled: bool,
}

impl History {
    fn new() -> Self {
        Self::with_config(PolicyAConfig {
            recenter_join: ms(100),
            panel_join: ms(350),
            recenter_hold: ms(1000),
        })
    }

    fn with_config(config: PolicyAConfig) -> Self {
        let mut history = Self {
            arbiter: PolicyA::new(config, ClockEpoch(1)).unwrap(),
            epoch: ClockEpoch(1),
            sequence: 0,
            generation: BindingGeneration(1),
            context: GameplayContext(1),
            enabled: true,
        };
        assert_eq!(history.step(0, UP).status, ObservationStatus::Recovered);
        history
    }

    fn step(&mut self, at: u64, down: [bool; 3]) -> Arbitration {
        self.evidence(at, down, InputEvidence::VerifiedUnmasked)
    }

    fn evidence(&mut self, at: u64, down: [bool; 3], evidence: InputEvidence) -> Arbitration {
        self.sequence += 1;
        self.stamped(
            ObservationStamp {
                epoch: self.epoch,
                at: ms(at),
                sync: self.sequence,
                sequence: self.sequence,
            },
            down,
            evidence,
        )
    }

    fn stamped(
        &mut self,
        stamp: ObservationStamp,
        down: [bool; 3],
        evidence: InputEvidence,
    ) -> Arbitration {
        self.arbiter.observe(Observation {
            stamp,
            generation: self.generation,
            context: self.context,
            singles_enabled: self.enabled,
            down,
            evidence,
        })
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn no_singles(out: &Arbitration) {
    assert_eq!(out.singles, [None; 3], "{out:?}");
    assert!(out.sources.iter().all(|source| !source.single_down));
}

fn command(out: Arbitration) -> Option<SystemCommand> {
    out.request.map(|request| request.command)
}

#[test]
fn policy_a_no_constituent_output_through_neutral() {
    for first in [LS, RS, LR] {
        for release_left_first in [false, true] {
            let mut history = History::new();
            let formed_at = if first == LR { 1 } else { 50 };
            let mut outputs = vec![history.step(1, first), history.step(50, LR)];
            outputs.push(history.step(formed_at + 999, LR));
            assert_eq!(outputs.last().unwrap().request, None);
            outputs.push(history.step(formed_at + 1000, LR));
            let partial = if release_left_first { RS } else { LS };
            outputs.extend([
                history.step(1051, LR),
                history.step(1052, partial),
                history.step(1053, LR),
                history.step(2053, LR),
                history.step(2054, partial),
                history.step(2055, UP),
            ]);
            for output in &outputs {
                no_singles(output);
            }
            assert_eq!(
                outputs.iter().filter(|out| out.request.is_some()).count(),
                1
            );
            let neutral = outputs.last().unwrap();
            assert!(neutral.withheld[0] && neutral.withheld[1]);
            assert_eq!(neutral.sources[0].disposition, Disposition::Consumed);
            assert_eq!(neutral.sources[1].disposition, Disposition::Consumed);
            assert_eq!(
                command(history.step(2056, LR)),
                None,
                "only a fresh chord can re-arm after common neutral"
            );
            assert_eq!(
                command(history.step(3056, LR)),
                Some(SystemCommand::Recenter)
            );
        }
    }
}

#[test]
fn generated_recenter_orders_and_exclusive_join_boundaries() {
    for first in [LS, RS] {
        for gap in [0, 1, 25, 99, 100, 101, 350, 500] {
            let mut history = History::new();
            if gap != 0 {
                no_singles(&history.step(1, first));
            }
            let joined = history.step(1 + gap, LR);
            let completed = history.step(1001 + gap, LR);
            assert_eq!(
                command(completed),
                (gap < 100).then_some(SystemCommand::Recenter),
                "first={first:?}, gap={gap}, joined={joined:?}"
            );
            if gap < 100 {
                no_singles(&joined);
                no_singles(&completed);
            } else {
                assert!(joined.refused.iter().any(Option::is_some));
            }
        }
    }
}

#[test]
fn generated_panel_order_and_exclusive_join_boundaries() {
    for gap in [1, 99, 100, 349, 350, 351, 1000] {
        let mut history = History::new();
        history.step(1, LS);
        let joined = history.step(1 + gap, LB);
        assert_eq!(
            joined.request.map(|request| request.command),
            (gap < 350).then_some(SystemCommand::TogglePanel),
            "gap={gap}, joined={joined:?}"
        );
        assert!(!joined.sources[Control::RightB.index()].single_down);
        assert_eq!(joined.singles[Control::RightB.index()], None);
        assert_eq!(command(history.step(2000, LB)), None);
    }
}

#[test]
fn immediate_b_first_commits_and_cannot_promote() {
    let mut history = History::new();
    let first = history.step(1, B);
    assert!(matches!(
        first.singles[Control::RightB.index()],
        Some(SingleEvent::Pressed { accepted_at, .. }) if accepted_at == ms(1)
    ));
    let joined = history.step(2, LB);
    assert_eq!(joined.request, None);
    assert!(joined.sources[Control::RightB.index()].single_down);
    assert_eq!(joined.singles[Control::LeftStick.index()], None);
    assert_eq!(joined.refused[0], Some(RefusalReason::CommittedSingle));
    assert_eq!(command(history.step(5000, LB)), None);
    assert!(matches!(
        history.step(5001, LS).singles[Control::RightB.index()],
        Some(SingleEvent::Released { .. })
    ));
    no_singles(&history.step(5002, LB));
    no_singles(&history.step(5003, LS));
    no_singles(&history.step(5004, UP));
    assert!(matches!(
        history.step(5005, B).singles[Control::RightB.index()],
        Some(SingleEvent::Pressed { .. })
    ));
}

#[test]
fn committed_stick_singles_cannot_promote_or_restart_the_late_partner() {
    for first in [LS, RS] {
        let mut history = History::new();
        let index = if first == LS { 0 } else { 1 };
        let expiry = if first == LS { 351 } else { 101 };
        history.step(1, first);
        let committed = history.step(expiry, first);
        let Some(SingleEvent::Pressed { press, accepted_at }) = committed.singles[index] else {
            panic!("the single must commit: {committed:?}");
        };
        assert_eq!(press.onset.at, ms(1));
        assert_eq!(accepted_at, ms(expiry));
        let joined = history.step(expiry + 1, LR);
        assert_eq!(joined.request, None);
        assert_eq!(joined.singles, [None; 3]);
        assert!(joined.sources[index].single_down);
        assert_eq!(joined.sources[index].press, Some(press));
        assert_eq!(command(history.step(expiry + 1001, LR)), None);
        let partial = history.step(expiry + 1002, first);
        assert_eq!(partial.singles, [None; 3]);
        assert!(partial.sources[index].single_down);
        let retry = history.step(expiry + 1003, LR);
        assert_eq!(retry.singles, [None; 3]);
        assert_eq!(retry.request, None);
        assert!(!retry.sources[1 - index].single_down);
        assert!(matches!(
            history.step(expiry + 1004, UP).singles[index],
            Some(SingleEvent::Released { press: released, .. }) if released == press
        ));
    }
}

#[test]
fn ls_reservation_is_the_maximum_eligible_window() {
    for (recenter, panel) in [(100, 350), (350, 100), (100, 100)] {
        for control in [Control::LeftStick, Control::RightStick] {
            let mut history = History::with_config(PolicyAConfig {
                recenter_join: ms(recenter),
                panel_join: ms(panel),
                recenter_hold: ms(1000),
            });
            let down = if control == Control::LeftStick {
                LS
            } else {
                RS
            };
            let window = if control == Control::LeftStick {
                recenter.max(panel)
            } else {
                recenter
            };
            no_singles(&history.step(1, down));
            no_singles(&history.step(window, down));
            let committed = history.step(window + 1, down);
            assert!(matches!(
                committed.singles[control.index()],
                Some(SingleEvent::Pressed { accepted_at, .. }) if accepted_at == ms(window + 1)
            ));
            assert_eq!(history.step(window + 2, down).singles, [None; 3]);
        }
    }
}

#[test]
fn ls_keeps_waiting_for_panel_after_recenter_expiry() {
    let mut history = History::new();
    history.step(1, LS);
    no_singles(&history.step(101, LS));
    let panel = history.step(300, LB);
    assert_eq!(panel.singles, [None; 3]);
    assert_eq!(command(panel), Some(SystemCommand::TogglePanel));
}

#[test]
fn ambiguous_ls_b_group_is_consumed_without_guessing_order() {
    let mut history = History::new();
    let ambiguous = history.step(1, LB);
    no_singles(&ambiguous);
    assert_eq!(ambiguous.request, None);
    assert_eq!(
        ambiguous.refused[0],
        Some(RefusalReason::AmbiguousPanelOrder)
    );
    assert_eq!(
        ambiguous.refused[2],
        Some(RefusalReason::AmbiguousPanelOrder)
    );
    no_singles(&history.step(5000, LB));
    no_singles(&history.step(5001, B));
    no_singles(&history.step(5002, LB));
    no_singles(&history.step(5003, UP));
    assert!(history.step(5004, B).sources[2].single_down);
}

#[test]
fn recenter_wins_indistinguishable_triple_and_consumes_b() {
    for first in [UP, LS, RS] {
        let mut history = History::new();
        history.step(1, first);
        let formed = history.step(2, ALL);
        no_singles(&formed);
        assert_eq!(formed.request, None);
        let fired = history.step(1002, ALL);
        no_singles(&fired);
        assert_eq!(command(fired), Some(SystemCommand::Recenter));
        no_singles(&history.step(1003, B));
        no_singles(&history.step(1004, LB));
        no_singles(&history.step(1005, ALL));
        no_singles(&history.step(2005, ALL));
        no_singles(&history.step(2006, UP));
        assert!(history.step(2007, B).sources[2].single_down);
    }
}

#[test]
fn first_complete_chord_owns_shared_ls() {
    for panel_first in [true, false] {
        let mut history = History::new();
        history.step(1, LS);
        let first = history.step(2, if panel_first { LB } else { LR });
        assert_eq!(
            command(first),
            panel_first.then_some(SystemCommand::TogglePanel)
        );
        no_singles(&history.step(3, ALL));
        let last = history.step(1002, ALL);
        no_singles(&last);
        assert_eq!(
            command(last),
            (!panel_first).then_some(SystemCommand::Recenter)
        );
    }
}

#[test]
fn recenter_hold_starts_at_formation_and_release_at_threshold_cancels() {
    for release in [1000, 1001] {
        let mut history = History::new();
        history.step(1, LS);
        history.step(2, LR);
        assert_eq!(command(history.step(release, RS)), None);
        no_singles(&history.step(2000, LR));
        assert_eq!(command(history.step(3000, LR)), None);
    }
    let mut history = History::new();
    history.step(1, LS);
    history.step(50, LR);
    assert_eq!(command(history.step(1049, LR)), None);
    assert_eq!(command(history.step(1050, RS)), None);
    assert_eq!(command(history.step(3000, LR)), None);
}

#[test]
fn pending_released_singles_are_single_zero_hold_taps() {
    for (down, index) in [(LS, 0), (RS, 1)] {
        let mut history = History::new();
        history.step(1, down);
        let released = history.step(2, UP);
        let Some(SingleEvent::Tap { press, accepted_at }) = released.singles[index] else {
            panic!("expected one tap: {released:?}");
        };
        assert_eq!(press.onset.at, ms(1));
        assert_eq!(accepted_at, ms(2));
        assert!(!released.sources[index].single_down);
        assert_eq!(history.step(3, UP).singles, [None; 3]);
    }
}

#[test]
fn a_release_and_new_partner_in_one_group_do_not_prove_overlap() {
    let mut history = History::new();
    history.step(1, RS);
    let changed = history.step(2, LS);
    assert!(matches!(changed.singles[1], Some(SingleEvent::Tap { .. })));
    assert_eq!(changed.request, None);
    assert_eq!(command(history.step(1002, LS)), None);
}

#[test]
fn release_order_without_common_neutral_never_rearms() {
    let mut history = History::new();
    history.step(1, LR);
    no_singles(&history.step(2, LS));
    no_singles(&history.step(3, RS));
    no_singles(&history.step(4, LR));
    let held = history.step(1004, LR);
    no_singles(&held);
    assert_eq!(held.request, None);
    no_singles(&history.step(1005, UP));
    history.step(1006, LR);
    assert_eq!(
        command(history.step(2006, LR)),
        Some(SystemCommand::Recenter)
    );
}

#[test]
fn context_changes_drop_delayed_singles_and_never_create_new_held_actions() {
    for first in [LS, RS, B] {
        let mut history = History::new();
        history.step(1, first);
        history.context = GameplayContext(2);
        let changed = history.step(2, first);
        assert!(changed.singles.iter().all(|event| !matches!(
            event,
            Some(SingleEvent::Pressed { .. } | SingleEvent::Tap { .. })
        )));
        no_singles(&history.step(1000, first));
        no_singles(&history.step(1001, UP));
        history.step(1002, first);
        let accepted = history.step(2002, first);
        assert!(accepted.sources.iter().any(|source| source.single_down));
    }
}

#[test]
fn system_ownership_survives_panel_gameplay_stand_down() {
    let mut history = History::new();
    history.step(1, LS);
    assert_eq!(
        command(history.step(2, LB)),
        Some(SystemCommand::TogglePanel)
    );
    history.enabled = false;
    history.context = GameplayContext(2);
    no_singles(&history.step(3, LB));
    no_singles(&history.step(4, B));
    no_singles(&history.step(5, UP));
    no_singles(&history.step(6, LS));
    let closing = history.step(7, LB);
    no_singles(&closing);
    assert_eq!(command(closing), Some(SystemCommand::TogglePanel));
    history.enabled = true;
    history.context = GameplayContext(3);
    no_singles(&history.step(8, LB));
    no_singles(&history.step(9, B));
    no_singles(&history.step(10, UP));
    assert!(history.step(11, B).sources[2].single_down);
}

#[test]
fn context_change_does_not_cancel_an_eligible_system_reservation() {
    let mut history = History::new();
    history.step(1, LS);
    history.context = GameplayContext(2);
    history.enabled = false;
    no_singles(&history.step(2, LS));
    assert_eq!(
        command(history.step(3, LB)),
        Some(SystemCommand::TogglePanel)
    );
}

#[test]
fn reset_causes_require_fresh_unmasked_common_neutral() {
    for reason in [
        ResetReason::FocusLost,
        ResetReason::TrackingLost,
        ResetReason::MissingOrStaleInput,
        ResetReason::ControllerDisabled,
        ResetReason::SchemeChanged,
        ResetReason::BindingUnresolved,
        ResetReason::MaskedObservation,
        ResetReason::ClockDiscontinuity,
    ] {
        let mut history = History::new();
        history.step(1, LR);
        let reset = history.evidence(2, UP, InputEvidence::Unavailable(reason));
        assert_eq!(reset.status, ObservationStatus::Reset(reason));
        no_singles(&reset);
        let still_held = history.step(5000, LR);
        assert_eq!(still_held.status, ObservationStatus::AwaitingNeutral);
        no_singles(&still_held);
        assert_eq!(still_held.request, None);
        assert_eq!(
            history.step(5001, LS).status,
            ObservationStatus::AwaitingNeutral
        );
        assert_eq!(
            history.step(5002, RS).status,
            ObservationStatus::AwaitingNeutral
        );
        assert_eq!(history.step(5003, UP).status, ObservationStatus::Recovered);
        history.step(5004, LR);
        assert_eq!(
            command(history.step(6004, LR)),
            Some(SystemCommand::Recenter)
        );
    }
}

#[test]
fn reset_releases_only_owned_singles_once_and_drops_pending_taps() {
    let mut history = History::new();
    let accepted = history.step(1, B);
    let Some(SingleEvent::Pressed { press, .. }) = accepted.singles[2] else {
        panic!("B must commit immediately");
    };
    let reset = history.arbiter.reset(ResetRequest {
        reason: ResetReason::FocusLost,
    });
    assert_eq!(
        reset.singles[2],
        Some(SingleEvent::Released { press, at: ms(1) })
    );
    no_singles(&history.arbiter.reset(ResetRequest {
        reason: ResetReason::FocusLost,
    }));
    assert_eq!(history.step(4, UP).status, ObservationStatus::Recovered);
    history.step(5, RS);
    no_singles(&history.arbiter.reset(ResetRequest {
        reason: ResetReason::SchemeChanged,
    }));
    no_singles(&history.step(7, UP));
}

#[test]
fn generation_change_neutralizes_owned_output_without_reinterpreting_held_sources() {
    let mut history = History::new();
    history.step(1, B);
    history.generation = BindingGeneration(2);
    let changed = history.step(2, B);
    assert_eq!(
        changed.status,
        ObservationStatus::Reset(ResetReason::BindingChanged)
    );
    assert!(matches!(
        changed.singles[2],
        Some(SingleEvent::Released { .. })
    ));
    no_singles(&history.step(3, B));
    assert_eq!(history.step(4, UP).status, ObservationStatus::Recovered);
    let fresh = history.step(5, B);
    assert!(matches!(
        fresh.singles[2],
        Some(SingleEvent::Pressed { press, .. }) if press.generation == BindingGeneration(2)
    ));
}

#[test]
fn invalid_order_time_or_sync_resets_without_hold_credit() {
    for stamp in [
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(1),
            sync: 2,
            sequence: 4,
        },
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(3),
            sync: 3,
            sequence: 3,
        },
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(3),
            sync: 1,
            sequence: 4,
        },
    ] {
        let mut history = History::new();
        history.step(1, LS);
        history.step(2, LR);
        let invalid = history.arbiter.observe(Observation {
            stamp,
            generation: history.generation,
            context: history.context,
            singles_enabled: true,
            down: LR,
            evidence: InputEvidence::VerifiedUnmasked,
        });
        assert_eq!(
            invalid.status,
            ObservationStatus::Reset(ResetReason::InvalidHistory)
        );
        no_singles(&invalid);
        assert_eq!(invalid.request, None);
    }
}

#[test]
fn starts_disarmed_and_does_not_synthesize_a_held_startup_press() {
    let mut arbiter = PolicyA::new(
        PolicyAConfig {
            recenter_join: ms(100),
            panel_join: ms(350),
            recenter_hold: ms(1000),
        },
        ClockEpoch(1),
    )
    .unwrap();
    for sequence in 1..=3 {
        let out = arbiter.observe(Observation {
            stamp: ObservationStamp {
                epoch: ClockEpoch(1),
                at: ms(sequence),
                sync: sequence,
                sequence,
            },
            generation: BindingGeneration(1),
            context: GameplayContext(1),
            singles_enabled: true,
            down: ALL,
            evidence: InputEvidence::VerifiedUnmasked,
        });
        assert_eq!(out.status, ObservationStatus::AwaitingNeutral);
        no_singles(&out);
        assert_eq!(out.request, None);
    }
}

#[test]
fn splitting_equal_time_groups_cannot_invent_panel_order() {
    let mut history = History::new();
    history.step(1, LS);
    let split = history.step(1, LB);
    assert_eq!(
        split.status,
        ObservationStatus::Reset(ResetReason::InvalidHistory)
    );
    no_singles(&split);
    assert_eq!(split.request, None);
    assert_eq!(
        history.step(2, LB).status,
        ObservationStatus::AwaitingNeutral
    );
}

#[test]
fn separate_ordered_groups_from_one_sync_retain_press_order() {
    let mut history = History::new();
    for (at, down) in [(1, LS), (2, LB)] {
        history.sequence += 1;
        let out = history.arbiter.observe(Observation {
            stamp: ObservationStamp {
                epoch: history.epoch,
                at: ms(at),
                sync: 2,
                sequence: history.sequence,
            },
            generation: history.generation,
            context: history.context,
            singles_enabled: true,
            down,
            evidence: InputEvidence::VerifiedUnmasked,
        });
        if at == 2 {
            assert_eq!(command(out), Some(SystemCommand::TogglePanel));
        } else {
            no_singles(&out);
        }
    }
}

#[test]
fn config_rejects_zero_join_windows_without_replacement_constants() {
    let valid = PolicyAConfig {
        recenter_join: ms(1),
        panel_join: ms(1),
        recenter_hold: Duration::ZERO,
    };
    assert_eq!(
        PolicyA::new(
            PolicyAConfig {
                recenter_join: Duration::ZERO,
                ..valid
            },
            ClockEpoch(1)
        )
        .unwrap_err(),
        ConfigError::ZeroRecenterJoin
    );
    assert_eq!(
        PolicyA::new(
            PolicyAConfig {
                panel_join: Duration::ZERO,
                ..valid
            },
            ClockEpoch(1)
        )
        .unwrap_err(),
        ConfigError::ZeroPanelJoin
    );
    let mut history = History::with_config(valid);
    assert_eq!(command(history.step(1, LR)), Some(SystemCommand::Recenter));
}

#[test]
fn invalid_owned_history_releases_at_watermark_and_cannot_recover_from_stale_neutral() {
    for rejected in [
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(50),
            sync: 1,
            sequence: 1,
        },
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(100),
            sync: 2,
            sequence: 2,
        },
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(200),
            sync: 3,
            sequence: 2,
        },
        ObservationStamp {
            epoch: ClockEpoch(1),
            at: ms(200),
            sync: 1,
            sequence: 3,
        },
    ] {
        for evidence in [
            InputEvidence::VerifiedUnmasked,
            InputEvidence::Unavailable(ResetReason::FocusLost),
        ] {
            let mut history = History::new();
            let accepted = history.step(100, B);
            let Some(SingleEvent::Pressed { press, accepted_at }) = accepted.singles[2] else {
                panic!("B must be owned: {accepted:?}");
            };
            assert_eq!(accepted_at, ms(100));
            let invalid = history.stamped(rejected, B, evidence);
            assert_eq!(
                invalid.status,
                ObservationStatus::Reset(ResetReason::InvalidHistory)
            );
            assert_eq!(
                invalid.singles[2],
                Some(SingleEvent::Released { press, at: ms(100) })
            );
            assert_eq!(invalid.request, None);
            assert!(invalid.withheld.into_iter().all(|withheld| withheld));
            for (at, sequence, down) in [(60, 2, UP), (70, 3, B), (101, 2, UP), (100, 4, UP)] {
                let stale = history.stamped(
                    ObservationStamp {
                        epoch: history.epoch,
                        at: ms(at),
                        sync: sequence,
                        sequence,
                    },
                    down,
                    InputEvidence::VerifiedUnmasked,
                );
                assert_eq!(
                    stale.status,
                    ObservationStatus::Reset(ResetReason::InvalidHistory)
                );
                no_singles(&stale);
                assert_eq!(stale.request, None);
            }
            let newer = ObservationStamp {
                epoch: history.epoch,
                at: ms(101),
                sync: 3,
                sequence: 3,
            };
            let recovered = history.stamped(newer, UP, InputEvidence::VerifiedUnmasked);
            assert_eq!(recovered.status, ObservationStatus::Recovered);
            no_singles(&recovered);
            let fresh = history.stamped(
                ObservationStamp {
                    epoch: history.epoch,
                    at: ms(102),
                    sync: 4,
                    sequence: 4,
                },
                B,
                InputEvidence::VerifiedUnmasked,
            );
            assert!(
                matches!(fresh.singles[2], Some(SingleEvent::Pressed { accepted_at, .. }) if accepted_at == ms(102))
            );
        }
    }
}

#[test]
fn common_neutral_allows_distinct_panel_and_recenter_requests() {
    for neutral in [UP, RS] {
        let mut history = History::new();
        let inputs = [(1, LS), (2, LB), (3, neutral), (4, ALL), (1004, ALL)];
        let outputs = inputs.map(|(at, down)| history.step(at, down));
        assert_eq!(command(outputs[1]), Some(SystemCommand::TogglePanel));
        assert_eq!(command(outputs[4]), Some(SystemCommand::Recenter));
        let panel_left = outputs[1].request.unwrap().members[0];
        let recenter_left = outputs[4].request.unwrap().members[0];
        assert_ne!(panel_left, recenter_left);
        assert!(outputs[2].withheld[0] && outputs[2].withheld[2]);
        for output in &outputs {
            no_singles(output);
        }
        assert_source_history_contract(&inputs, &outputs);
    }
}

#[test]
fn generated_short_histories_never_emit_claimed_constituents() {
    let levels = [UP, LS, RS, LR, B, LB, ALL];
    for &first in &levels {
        for &second in &levels {
            for &third in &levels {
                let mut history = History::new();
                let inputs = [(1, first), (2, second), (3, third), (4, ALL), (1004, ALL)];
                let outputs = inputs.map(|(at, down)| history.step(at, down));
                assert_source_history_contract(&inputs, &outputs);
            }
        }
    }
}

fn assert_source_history_contract(inputs: &[(u64, [bool; 3])], outputs: &[Arbitration]) {
    assert_eq!(inputs.len(), outputs.len());
    let mut claimed = Vec::new();
    let mut requested = Vec::new();
    let mut enrolled = [false; 3];
    let mut requests_since_neutral = 0;
    for ((_, down), output) in inputs.iter().zip(outputs) {
        for control in Control::ALL {
            let i = control.index();
            if output.sources[i].disposition == Disposition::Consumed {
                enrolled[i] = true;
                if let Some(press) = output.sources[i].press {
                    claimed.push(press);
                }
                assert!(
                    output.withheld[i],
                    "inputs={inputs:?}, outputs={outputs:#?}"
                );
            }
            if output.refused[i].is_some() {
                enrolled[i] = true;
            }
        }
        if let Some(request) = output.request {
            requests_since_neutral += 1;
            assert!(
                requests_since_neutral <= 1,
                "inputs={inputs:?}, outputs={outputs:#?}"
            );
            for control in Control::ALL {
                let i = control.index();
                if let Some(press) = request.members[i] {
                    enrolled[i] = true;
                    claimed.push(press);
                    assert!(
                        !requested.contains(&press),
                        "a claimed press requests twice: inputs={inputs:?}, outputs={outputs:#?}"
                    );
                    requested.push(press);
                }
            }
        }
        // The neutral group's output remains protected before a distinct attempt can begin.
        if Control::ALL
            .into_iter()
            .all(|c| !enrolled[c.index()] || !down[c.index()])
        {
            enrolled = [false; 3];
            requests_since_neutral = 0;
        }
    }
    // Check the whole history, including reservation before a command's completion hold.
    for output in outputs {
        for control in Control::ALL {
            let i = control.index();
            if let Some(event) = output.singles[i] {
                let press = match event {
                    SingleEvent::Pressed { press, .. }
                    | SingleEvent::Released { press, .. }
                    | SingleEvent::Tap { press, .. } => press,
                };
                assert!(
                    !claimed.contains(&press),
                    "a claimed press emits a single: inputs={inputs:?}, outputs={outputs:#?}"
                );
            }
            if output.sources[i].single_down {
                assert!(
                    !output.sources[i]
                        .press
                        .is_some_and(|press| claimed.contains(&press)),
                    "a claimed press holds a single: inputs={inputs:?}, outputs={outputs:#?}"
                );
            }
        }
    }
}
