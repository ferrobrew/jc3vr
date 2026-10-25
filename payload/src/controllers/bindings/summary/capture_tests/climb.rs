use glam::Vec3;

use super::*;
use crate::controllers::{
    grab::{self, Mark, MarkAt},
    summary::{
        current::{self, CellKind, CurrentSummary, Source},
        labels::Text,
    },
};

const MODE: Mode = Mode::OnFoot(OnFootVariant::Ground);

struct Capture {
    ready: [bool; 2],
    released: [bool; 2],
    chosen: [Option<GrabVolume>; 2],
    selected: [Option<GripTarget>; 2],
    grabs: [Option<GrabVolume>; 2],
    guns: [Option<GunOwner>; 2],
}

impl Capture {
    fn new(targets: Vec<GripTarget>) -> Self {
        let grips = [Some(Vec3::ZERO); 2];
        let tick = grab::tick::Tick {
            resolved: grab::resolve::resolve(&targets, grips),
            targets,
            grips,
            ..Default::default()
        };
        Self {
            ready: [true; 2],
            released: [false; 2],
            chosen: tick.chosen(),
            selected: tick.selected_for_summary(),
            grabs: [None; 2],
            guns: [None; 2],
        }
    }

    fn snapshot(&self) -> Eligibility {
        let config = ControllersConfig::default();
        let before = owner_state([None; 2]);
        capture_current(
            Refresh {
                sequence: 7,
                belt_reload: None,
                config: &config,
                owners: [Owner {
                    mode: MODE,
                    gun: None,
                }; 2],
                ready: self.ready,
                trigger_blocked: [true; 2],
                chosen: Hand::BOTH
                    .map(|hand| self.chosen[hand.index()].filter(|_| !self.released[hand.index()])),
                selected: Hand::BOTH.map(|hand| {
                    self.selected[hand.index()].filter(|_| !self.released[hand.index()])
                }),
                grabs: self.grabs,
                before: &before,
                grapple: GrapplePermissions::default(),
                yank: None,
                flight: GestureAvailability::Unavailable,
                parachute: [None; 2],
            },
            owner_state(self.guns),
            [false; 2],
            ReloadPermissions::default(),
            [false; 2],
        )
    }
}

fn target(hand: Hand) -> GripTarget {
    GripTarget::measured(
        GrabVolume::Climb,
        hand,
        0.0,
        Mark::ring(MarkAt::World(Vec3::ZERO), 0.1),
    )
}

fn row(hand: Hand) -> Row {
    Row {
        control: Control::Grip(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::Climb),
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
        &|_| panic!("a climb grip has no semantic source"),
        &|_| panic!("a climb offer has no native description"),
    )
}

#[test]
fn selected_unheld_climb_projects_only_the_ready_physical_grip() {
    for hand in Hand::BOTH {
        let mut capture = Capture::new(vec![target(hand)]);
        capture.ready[hand.other().index()] = false;
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.mechanics, vec![row(hand)]);
        assert_eq!(snapshot.routing.triggers, [TriggerUse::Reserved; 2]);
        assert_eq!(
            eligibility::resolve(&[], &snapshot).context.usable,
            vec![row(hand)]
        );
        let current = project(&snapshot);
        let cell = match hand {
            Hand::Left => CellKind::Left,
            Hand::Right => CellKind::Right,
        };
        let hints = &current.cells[cell.index()];
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].text, "Grip to climb");
        assert_eq!(
            hints[0].sources,
            vec![Source {
                control: Control::Grip(hand),
                name: None,
            }]
        );
        assert!(hints[0].prefix.is_none());
        assert_eq!(current.cells.iter().map(Vec::len).sum::<usize>(), 1);
    }
    let both = Capture::new(Hand::BOTH.map(target).to_vec()).snapshot();
    assert_eq!(both.mechanics, Hand::BOTH.map(row).to_vec());
    assert_eq!(project(&both).cells.iter().map(Vec::len).sum::<usize>(), 2);
}

#[derive(Debug, Copy, Clone)]
enum InvalidOffer {
    NoChoice,
    OtherChoice,
    NoSelection,
    OtherVolume,
    ForeignHand,
    EitherHand,
    Held,
    ForeignHolder,
    Released,
    Unready,
    Consumed,
    Replaced,
    OwnerChanged,
}

#[test]
fn climb_capture_and_projection_reject_incomplete_or_invalidated_copied_offers() {
    for hand in Hand::BOTH {
        for invalid in [
            InvalidOffer::NoChoice,
            InvalidOffer::OtherChoice,
            InvalidOffer::NoSelection,
            InvalidOffer::OtherVolume,
            InvalidOffer::ForeignHand,
            InvalidOffer::EitherHand,
            InvalidOffer::Held,
            InvalidOffer::ForeignHolder,
            InvalidOffer::Released,
            InvalidOffer::Unready,
            InvalidOffer::Consumed,
            InvalidOffer::Replaced,
            InvalidOffer::OwnerChanged,
        ] {
            let mut capture = Capture::new(vec![target(hand)]);
            let i = hand.index();
            match invalid {
                InvalidOffer::NoChoice => capture.chosen[i] = None,
                InvalidOffer::OtherChoice => capture.chosen[i] = Some(GrabVolume::StuntHold),
                InvalidOffer::NoSelection => capture.selected[i] = None,
                InvalidOffer::OtherVolume => {
                    capture.selected[i].as_mut().unwrap().volume = GrabVolume::StuntHold;
                }
                InvalidOffer::ForeignHand => {
                    capture.selected[i].as_mut().unwrap().hand = Some(hand.other());
                }
                InvalidOffer::EitherHand => capture.selected[i].as_mut().unwrap().hand = None,
                InvalidOffer::Held => capture.selected[i].as_mut().unwrap().held_by = Some(hand),
                InvalidOffer::ForeignHolder => {
                    capture.selected[i].as_mut().unwrap().held_by = Some(hand.other());
                }
                InvalidOffer::Released => capture.released[i] = true,
                InvalidOffer::Unready => capture.ready[i] = false,
                InvalidOffer::Consumed => capture.grabs[i] = Some(GrabVolume::Climb),
                InvalidOffer::Replaced => capture.grabs[i] = Some(GrabVolume::StuntHold),
                InvalidOffer::OwnerChanged => {
                    capture.guns[hand.other().index()] = Some(GunOwner {
                        weapon: 3,
                        role: GunHand::Grip,
                    });
                }
            }
            let snapshot = capture.snapshot();
            assert!(snapshot.mechanics.is_empty(), "{hand:?}: {invalid:?}");
            assert!(
                eligibility::resolve(&[], &snapshot)
                    .context
                    .usable
                    .is_empty(),
                "{hand:?}: {invalid:?}"
            );
            assert!(
                project(&snapshot).cells.iter().all(Vec::is_empty),
                "{hand:?}: {invalid:?}"
            );
        }
    }
}

#[test]
fn climb_requires_the_resolver_selection_and_does_not_persist_a_previous_offer() {
    for hand in Hand::BOTH {
        let climb = target(hand);
        let competitor = GripTarget::measured(
            GrabVolume::StuntHold,
            hand,
            0.0,
            Mark::ring(MarkAt::World(Vec3::ZERO), 0.1),
        );
        let capture = Capture::new(vec![competitor, climb]);
        assert_eq!(capture.chosen[hand.index()], Some(GrabVolume::StuntHold));
        assert_eq!(capture.selected[hand.index()], Some(competitor));
        assert!(capture.snapshot().mechanics.is_empty());
        assert!(project(&capture.snapshot()).cells.iter().all(Vec::is_empty));

        let mut capture = Capture::new(vec![climb]);
        let previous = capture.snapshot();
        assert_eq!(previous.mechanics, vec![row(hand)]);
        let absent = Capture::new(Vec::new()).snapshot();
        assert!(absent.mechanics.is_empty());
        assert!(project(&absent).cells.iter().all(Vec::is_empty));
        capture.selected[hand.index()] = None;
        let chosen_only = capture.snapshot();
        assert_eq!(chosen_only.chosen[hand.index()], Some(GrabVolume::Climb));
        assert!(chosen_only.mechanics.is_empty());
        assert!(project(&chosen_only).cells.iter().all(Vec::is_empty));
    }
}
