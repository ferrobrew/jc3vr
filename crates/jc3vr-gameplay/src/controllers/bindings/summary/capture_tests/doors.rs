use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;
use jc3vr_body::controllers::{
    grab,
    grab::{
        Mark, MarkAt,
        target::TargetReach,
        world_reach::{DoorEligibility, DoorTarget, Reach, TargetKind},
    },
};
use jc3vr_core::published::Stamp;

use super::*;
use crate::controllers::config::ControllersConfig;

/// A door provider standing in for the physical hands: the reach targets it offers, and whether a
/// selected target still carries an offer it made. The physical crate's own tests cover when the
/// real provider makes and revokes an offer.
struct DoorProvider {
    epoch: u64,
    at: Stamp,
}

impl DoorProvider {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            at: Stamp::now(),
        }
    }

    /// The target `hand` reaches of `kind`, offered when it is a latched door.
    fn target(&self, hand: Hand, kind: TargetKind) -> GripTarget {
        let reach = Reach {
            object: NonZeroUsize::new(0x1000).unwrap(),
            body: 1,
            point: Vec3::ZERO,
            distance: 0.02,
            normal: Vec3::Y,
            kind,
        };
        let pickup = match kind {
            TargetKind::Door(_) => PickupKind::Door,
            _ => PickupKind::Prop,
        };
        let mark = Mark::ring(MarkAt::World(reach.point), Mark::world_size_m());
        let mut target = GripTarget::measured(
            GrabVolume::Pickup(pickup),
            hand,
            reach.distance,
            mark.on_surface(reach.normal),
        );
        target.observed = Some(reach);
        target.door_eligibility = matches!(kind, TargetKind::Door(door) if door.latched)
            .then(|| DoorEligibility::new(hand, self.epoch, self.at, reach));
        target
    }

    fn door(&self, hand: Hand, latched: bool) -> GripTarget {
        self.target(
            hand,
            TargetKind::Door(DoorTarget {
                vehicle: NonZeroUsize::new(0x2000).unwrap(),
                vehicle_body: Some(1),
                part_index: 2,
                slot: DoorSlot::FRONT_LEFT,
                latched,
                outward: Vec3::X,
            }),
        )
    }

    fn current(&self, hand: Hand, target: GripTarget) -> bool {
        target.door_eligibility.is_some_and(|offer| {
            offer.hand() == hand && offer.epoch() == self.epoch && offer.at() == self.at
        })
    }
}

fn row(hand: Hand) -> Row {
    Row {
        control: Control::Grip(hand),
        gesture: Gesture::Press,
        effect: Effect::Mod(ModAction::OpenVehicleDoor),
    }
}

fn capture_door(
    provider: &DoorProvider,
    tick: &grab::tick::Tick,
    released: [bool; 2],
    ready: [bool; 2],
    grabs: [Option<GrabVolume>; 2],
) -> Eligibility {
    capture_door_with_owner(
        provider,
        tick,
        released,
        ready,
        grabs,
        owner_state([None; 2]),
    )
}

fn capture_door_with_owner(
    provider: &DoorProvider,
    tick: &grab::tick::Tick,
    released: [bool; 2],
    ready: [bool; 2],
    grabs: [Option<GrabVolume>; 2],
    current: BeforeFinish,
) -> Eligibility {
    let config = ControllersConfig::default();
    let before = owner_state([None; 2]);
    let selected = Hand::BOTH
        .map(|hand| tick.selected_for_summary()[hand.index()].filter(|_| !released[hand.index()]));
    let doors = Hand::BOTH
        .map(|hand| selected[hand.index()].is_some_and(|target| provider.current(hand, target)));
    capture_current(
        Refresh {
            sequence: 7,
            belt_reload: None,
            settings: &Settings::of(&config),
            owners: [Owner {
                mode: Mode::OnFoot(OnFootVariant::Ground),
                gun: None,
            }; 2],
            ready,
            grapple_routes: Default::default(),
            trigger_blocked: [false; 2],
            chosen: Hand::BOTH
                .map(|hand| tick.chosen()[hand.index()].filter(|_| !released[hand.index()])),
            selected,
            grabs,
            before: &before,
            grapple: GrapplePermissions::default(),
            yank: None,
            flight: GestureAvailability::Unavailable,
            parachute: [None; 2],
        },
        current,
        [false; 2],
        ReloadPermissions::default(),
        doors,
    )
}

fn select(targets: Vec<GripTarget>) -> grab::tick::Tick {
    grab::tick::resolve(targets, [Vec::new(), Vec::new()], None)
}

fn has_door(snapshot: &Eligibility, hand: Hand) -> bool {
    let expected = row(hand);
    let report = eligibility::resolve(&[], snapshot);
    assert_eq!(
        snapshot.mechanics.contains(&expected),
        report.context.usable.contains(&expected)
    );
    report.context.usable.contains(&expected)
}

#[test]
fn actual_provider_selection_captures_only_the_chosen_unheld_latched_door() {
    for hand in Hand::BOTH {
        let provider = DoorProvider::new(1);
        let latched = provider.door(hand, true);
        let tick = select(vec![latched]);
        assert_eq!(tick.selected_for_summary()[hand.index()], Some(latched));
        let snapshot = capture_door(&provider, &tick, [false; 2], [true; 2], [None; 2]);
        assert!(has_door(&snapshot, hand));
        assert_eq!(snapshot.mechanics, vec![row(hand)]);
        assert!(!has_door(&snapshot, hand.other()));

        for other in 0..3 {
            let competitor = DoorProvider::new(2);
            let mut target = match other {
                0 => competitor.door(hand, false),
                1 => competitor.door(hand, true).held(hand),
                _ => competitor.target(hand, TargetKind::Prop { mass_kg: 1.0 }),
            };
            target.reach = TargetReach::Measured { distance: 0.0 };
            let tick = select(vec![latched, target]);
            assert_eq!(tick.resolved[hand.index()].chosen.unwrap().index, 1);
            assert!(!has_door(
                &capture_door(&provider, &tick, [false; 2], [true; 2], [None; 2]),
                hand,
            ));
        }
        assert!(!has_door(
            &capture_door(
                &provider,
                &select(Vec::new()),
                [false; 2],
                [true; 2],
                [None; 2]
            ),
            hand,
        ));
        let diagnostic_only = GripTarget {
            door_eligibility: None,
            ..latched
        };
        assert!(diagnostic_only.observed.is_some());
        assert!(!has_door(
            &capture_door(
                &provider,
                &select(vec![diagnostic_only]),
                [false; 2],
                [true; 2],
                [None; 2]
            ),
            hand,
        ));
    }
}

#[test]
fn final_capture_revokes_door_offers_after_release_consumption_and_unready_hands() {
    for hand in Hand::BOTH {
        let provider = DoorProvider::new(1);
        let tick = select(vec![provider.door(hand, true)]);
        let mut released = [false; 2];
        released[hand.index()] = true;
        let mut ready = [true; 2];
        ready[hand.index()] = false;
        let mut grabs = [None; 2];
        grabs[hand.index()] = Some(GrabVolume::Pickup(PickupKind::Door));
        for snapshot in [
            capture_door(&provider, &tick, released, [true; 2], [None; 2]),
            capture_door(&provider, &tick, [false; 2], ready, [None; 2]),
            capture_door(&provider, &tick, [false; 2], [true; 2], grabs),
        ] {
            assert!(!has_door(&snapshot, hand));
        }
        let changed = owner_state(Hand::BOTH.map(|h| {
            (h == hand).then_some(GunOwner {
                weapon: 3,
                role: GunHand::Grip,
            })
        }));
        assert!(!has_door(
            &capture_door_with_owner(&provider, &tick, [false; 2], [true; 2], [None; 2], changed),
            hand,
        ));
        let mut other_unready = [true; 2];
        other_unready[hand.other().index()] = false;
        assert!(has_door(
            &capture_door(&provider, &tick, [false; 2], other_unready, [None; 2]),
            hand,
        ));
    }
}
