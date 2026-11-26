use glam::Vec3;

use super::*;
use crate::{
    controllers::{
        bindings::tables::{declare::TableCtx, describe::DescribeCtx},
        grab::{self, Mark, MarkAt},
        summary::{
            current::{self, CellKind, CurrentSummary, Source},
            labels::{ModText, Text},
        },
        vehicles::parachute::{self, HandleState, StepInputs},
    },
    vr::input::semantic::bound::SourceTable,
};

const MODE: Mode = Mode::Airborne(AirborneVariant::Parachute);

struct Capture {
    config: ControllersConfig,
    before: HandleState,
    after: HandleState,
    ready: [bool; 2],
    released: [bool; 2],
    grabs: [Option<GrabVolume>; 2],
    mode: Mode,
    guns: [Option<GunOwner>; 2],
}

impl Capture {
    fn held(hand: Hand) -> Self {
        let mut this = Self {
            config: ControllersConfig::default(),
            before: HandleState::new(),
            after: HandleState::new(),
            ready: [true; 2],
            released: [false; 2],
            grabs: [None; 2],
            mode: MODE,
            guns: [None; 2],
        };
        this.step(hand, true, true, Some(Vec3::ZERO));
        this.before = this.after;
        this
    }

    fn step(&mut self, hand: Hand, held: bool, grab: bool, grip: Option<Vec3>) {
        let i = hand.index();
        parachute::step(
            &mut self.after,
            &self.config.vehicles.parachute_handles,
            &StepInputs {
                grips: Hand::BOTH.map(|h| if h == hand { grip } else { None }),
                held: Hand::BOTH.map(|h| h == hand && held),
                grabs: Hand::BOTH.map(|h| h == hand && grab),
            },
            &mut Vec::new(),
        );
        assert_eq!(self.after.holds[hand.other().index()], None);
        assert_eq!(self.after.holds[i].is_some(), held && grip.is_some());
    }

    fn targets(&self, hand: Hand) -> Vec<GripTarget> {
        let candidate = parachute::candidates(
            &self.config.vehicles.parachute_handles,
            &self.before,
            [Vec3::ZERO; 2],
            [Some(Vec3::ZERO); 2],
        )[hand.index()];
        let target = candidate.target(Mark::ring(MarkAt::Cockpit(candidate.centre), 0.1));
        vec![if self.before.holds[hand.index()].is_some() {
            target.held(hand)
        } else {
            target
        }]
    }

    fn capture(&self, targets: Vec<GripTarget>) -> Eligibility {
        let grips = Hand::BOTH.map(|hand| self.ready[hand.index()].then_some(Vec3::ZERO));
        let tick = grab::tick::Tick {
            resolved: grab::resolve::resolve(&targets, grips),
            targets,
            grips,
            ..Default::default()
        };
        let mut before = owner_state([None; 2]);
        before.parachute = self.before.holds;
        let mut after = owner_state(self.guns);
        after.parachute = self.after.holds;
        capture_current(
            Refresh {
                sequence: 7,
                belt_reload: None,
                config: &self.config,
                owners: [Owner {
                    mode: self.mode,
                    gun: None,
                }; 2],
                ready: self.ready,
                grapple_routes: Default::default(),
                trigger_blocked: [true; 2],
                chosen: Hand::BOTH.map(|hand| {
                    tick.chosen()[hand.index()].filter(|_| !self.released[hand.index()])
                }),
                selected: Hand::BOTH.map(|hand| {
                    tick.selected_for_summary()[hand.index()]
                        .filter(|_| !self.released[hand.index()])
                }),
                grabs: self.grabs,
                before: &before,
                grapple: GrapplePermissions::default(),
                yank: None,
                flight: GestureAvailability::Unavailable,
                parachute: self.after.holds,
            },
            after,
            [false; 2],
            ReloadPermissions::default(),
            [false; 2],
        )
    }
}

fn pull(hand: Hand) -> Row {
    Row {
        control: Control::Motion(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::PullParachuteHandle),
    }
}

fn take(hand: Hand) -> Row {
    Row {
        control: Control::Grip(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::ParachuteHandles),
    }
}

fn project(snapshot: &Eligibility) -> CurrentSummary {
    let report = eligibility::resolve(&[], snapshot);
    current::project(
        &[],
        MODE,
        &report.context,
        &[],
        &|text| match text {
            Text::Mod(text) => text.english().to_owned(),
            Text::Game(key) => key.to_owned(),
        },
        &|_| panic!("a handle motion has no semantic source"),
        &|_| None,
    )
}

#[test]
fn held_handle_candidates_capture_and_project_only_their_physical_motion_hand() {
    for hand in Hand::BOTH {
        let capture = Capture::held(hand);
        let snapshot = capture.capture(capture.targets(hand));
        assert_eq!(snapshot.mechanics, vec![pull(hand)]);
        assert_eq!(snapshot.routing.triggers, [TriggerUse::Reserved; 2]);
        assert_eq!(
            eligibility::resolve(&[], &snapshot).context.usable,
            vec![pull(hand)]
        );
        let current = project(&snapshot);
        let cell = if hand == Hand::Left {
            CellKind::Left
        } else {
            CellKind::Right
        };
        let hints = &current.cells[cell.index()];
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].text, ModText::PullParachuteHandle.english());
        assert_eq!(
            hints[0].sources,
            vec![Source {
                control: Control::Motion(hand),
                name: None
            }]
        );
        assert!(hints[0].prefix.is_none());
        assert_eq!(current.cells.iter().map(Vec::len).sum::<usize>(), 1);
        assert!(!snapshot.mechanics.contains(&take(hand)));
    }
}

#[test]
fn free_handle_requires_the_selected_side_and_projects_a_grip_not_held_motion() {
    for hand in Hand::BOTH {
        let mut capture = Capture::held(hand);
        capture.before = HandleState::new();
        capture.after = HandleState::new();
        let targets = capture.targets(hand);
        let snapshot = capture.capture(targets.clone());
        assert_eq!(snapshot.mechanics, vec![take(hand)]);
        assert_eq!(
            eligibility::resolve(&[], &snapshot).context.usable,
            vec![take(hand)]
        );
        let mut wrong = targets;
        wrong[0].volume = GrabVolume::ParachuteHandle(hand.other());
        assert!(capture.capture(wrong).mechanics.is_empty());
        let chosen_only = Eligibility {
            routing: CurrentRouting {
                ready: [true; 2],
                ..Default::default()
            },
            chosen: Hand::BOTH.map(|h| (h == hand).then_some(GrabVolume::ParachuteHandle(hand))),
            ..Default::default()
        };
        assert!(
            eligibility::resolve(&[], &chosen_only)
                .context
                .usable
                .is_empty()
        );
        capture.step(hand, true, true, Some(Vec3::ZERO));
        assert!(capture.capture(capture.targets(hand)).mechanics.is_empty());
    }
}

#[test]
fn held_handle_capture_omits_release_consumption_late_owner_and_source_changes() {
    for hand in Hand::BOTH {
        for change in 0..14 {
            let mut capture = Capture::held(hand);
            let mut targets = capture.targets(hand);
            let i = hand.index();
            match change {
                0 => capture.released[i] = true,
                1 => capture.ready[i] = false,
                2 => capture.grabs[i] = Some(GrabVolume::ParachuteHandle(hand)),
                3 => capture.step(hand, false, false, Some(Vec3::ZERO)),
                4 => capture.step(hand, true, false, None),
                5 => {
                    capture.step(hand, false, false, Some(Vec3::ZERO));
                    capture.step(hand, true, true, Some(Vec3::X));
                }
                6 => targets[0].volume = GrabVolume::ParachuteHandle(hand.other()),
                7 => targets[0].hand = Some(hand.other()),
                8 => targets[0].held_by = Some(hand.other()),
                9 => targets[0].held_by = None,
                10 => capture.config.vehicles.parachute_handles.enabled = false,
                11 => capture.mode = Mode::Airborne(AirborneVariant::Falling),
                12 => capture.after.holds[hand.other().index()] = capture.after.holds[i],
                _ => {
                    capture.guns[i] = Some(GunOwner {
                        weapon: 3,
                        role: GunHand::Grip,
                    })
                }
            }
            let snapshot = capture.capture(targets);
            assert!(snapshot.mechanics.is_empty(), "change {change}");
            assert!(
                project(&snapshot).cells.iter().all(Vec::is_empty),
                "change {change}"
            );
        }
        let mut capture = Capture::held(hand);
        capture.ready[hand.other().index()] = false;
        assert_eq!(
            capture.capture(capture.targets(hand)).mechanics,
            vec![pull(hand)]
        );
    }
}

#[test]
fn bound_handle_description_does_not_claim_a_current_held_motion() {
    let capture = Capture::held(Hand::Left);
    let ctx = DescribeCtx {
        mode: MODE,
        dominant: capture.config.acting_hand(),
        table: TableCtx::default(),
        features: tables::describe::Features {
            parachute_handles: true,
            ..Default::default()
        },
    };
    let bound = tables::describe::describe_bound(&ctx, &SourceTable::default());
    assert!(
        bound
            .iter()
            .any(|row| row.effect == Effect::Mod(ModAction::ParachuteHandles))
    );
    assert!(
        !bound
            .iter()
            .any(|row| row.effect == Effect::Mod(ModAction::PullParachuteHandle))
    );
    let absent = capture.capture(Vec::new());
    assert!(
        eligibility::resolve(&bound, &absent)
            .context
            .usable
            .iter()
            .all(|row| {
                !matches!(
                    row.effect,
                    Effect::Mod(ModAction::ParachuteHandles | ModAction::PullParachuteHandle)
                )
            })
    );
}
