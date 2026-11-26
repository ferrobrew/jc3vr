//! The physical hands' grip targets ([`jc3vr_body::controllers::grab`]): what each empty hand's reach
//! found in the world (an object, a weapon, a vehicle's door, or a character), the object a hand
//! holds, the surface a hand could climb, and the helicopter the player hangs from.
//!
//! The reach runs on the simulation tick and is read here on the input tick, measured: the reach's
//! own casts found each target, so its distance is the reach's. The markers lie on what was found,
//! in the world, on its surface where the reach knows the surface's normal.

use std::time::Instant;

use jc3vr_body::controllers::grab::{
    GrabVolume, GripTarget, Mark, MarkAt, PickupKind, Provided, Shown,
    world_reach::{DoorEligibility, DoorTarget, Reach, TargetKind},
};
use jc3vr_input::frame::Hand;

use crate::controllers::physical_hands::{
    STATE, climb, pickup::diagnostics::scan::Scan, state::State, stunt,
};

/// Whether `offer` still stands for `hand`: the same fresh sample of the same source, and the door
/// it offers still unclaimed and in reach.
pub fn door_offer_current(offer: DoorEligibility, hand: Hand) -> bool {
    let state = STATE.lock();
    door_offer_current_in(offer, hand, &state, Instant::now())
}

/// The physical hands' targets for this input tick, with the reach's notes for each hand, at the VR
/// world scale `world_scale`.
pub fn targets(world_scale: f32, scans: &mut [Option<Scan>; 2]) -> Provided {
    *scans = [None; 2];
    let scale = world_scale.max(1e-3);
    let mut out = Provided::default();
    let palms = {
        let state = STATE.lock();
        let now = Instant::now();
        let fresh = state.fresh(now);
        for hand in Hand::BOTH {
            let i = hand.index();
            if !fresh {
                continue;
            }
            out.notes[i].clone_from(&state.reach_notes[i]);
            scans[i] = Some(state.pickup.scans[i]);
            let status = &state.status.hands[i];
            if let (Some(held), Some(kind)) = (status.held, state.pickup.held_kind(hand)) {
                out.targets.push(
                    GripTarget::measured(
                        GrabVolume::Pickup(kind),
                        hand,
                        0.0,
                        Mark::ring(MarkAt::World(held.anchor), HELD_MARK_M),
                    )
                    .held(hand),
                );
                continue;
            }
            if let Some(reach) = status.reach {
                out.targets
                    .push(provided_reach(hand, &reach, scale, &state, now));
            }
        }
        state.status.hands.map(|h| h.palm.map(|p| p.centre))
    };
    for hand in Hand::BOTH {
        let Some(hold) = climb::reach_hold(hand) else {
            continue;
        };
        let distance = palms[hand.index()].map_or(0.0, |p| p.distance(hold.point) / scale);
        out.targets.push(GripTarget::measured(
            GrabVolume::Climb,
            hand,
            distance,
            Mark::ring(MarkAt::World(hold.point), Mark::world_size_m()).on_surface(hold.normal),
        ));
    }
    if stunt::hanging() {
        for hand in Hand::BOTH {
            out.targets.push(GripTarget::measured(
                GrabVolume::StuntHold,
                hand,
                0.0,
                Mark::ring(MarkAt::World(glam::Vec3::ZERO), Mark::world_size_m())
                    .showing(Shown::Never),
            ));
        }
    }
    out
}

fn provided_reach(
    hand: Hand,
    reach: &Reach,
    scale: f32,
    state: &State,
    now: Instant,
) -> GripTarget {
    let mut target = reach_target(hand, reach, scale);
    target.door_eligibility = door_offer(hand, reach, state, now);
    target
}

fn door_offer_current_in(offer: DoorEligibility, hand: Hand, state: &State, now: Instant) -> bool {
    offer.hand() == hand
        && state.epoch == offer.epoch()
        && state.at == Some(offer.at())
        && door_offer(hand, &offer.reach(), state, now) == Some(offer)
}

fn door_offer(hand: Hand, reach: &Reach, state: &State, now: Instant) -> Option<DoorEligibility> {
    let i = hand.index();
    let TargetKind::Door(door) = reach.kind else {
        return None;
    };
    (state.fresh(now)
        && door.latched
        && state.status.hands[i].held.is_none()
        && !state.status.hands[i].latched
        && !state.pickup.holding(hand)
        && !state.pickup.latched(hand)
        && state.pickup.seizes[i].is_none()
        && state.pickup.regrips[i].is_none()
        && state.pickup.requests[i].is_none()
        && !door_claimed(door, reach, state)
        && state.status.hands[i].reach == Some(*reach))
    .then_some(DoorEligibility::new(hand, state.epoch, state.at?, *reach))
}

fn door_claimed(door: DoorTarget, reach: &Reach, state: &State) -> bool {
    state
        .pickup
        .held
        .iter()
        .flatten()
        .any(|held| held.object == reach.object && held.body == reach.body)
        || state
            .pickup
            .latches
            .iter()
            .flatten()
            .any(|latch| same_door(latch.door, door))
        || state.pickup.requests.iter().flatten().any(|request| {
            request.grab().is_some_and(|(claimed, _)| {
            matches!(claimed.kind, TargetKind::Door(other) if same_door(other, door))
        })
        })
}

fn same_door(a: DoorTarget, b: DoorTarget) -> bool {
    a.vehicle == b.vehicle && a.part_index == b.part_index && a.slot == b.slot
}

/// What `reach` found, as a target `hand` is at the reach's distance from, at the player's scale
/// (`scale` world metres to one).
fn reach_target(hand: Hand, reach: &Reach, scale: f32) -> GripTarget {
    let mark = Mark::ring(MarkAt::World(reach.point), Mark::world_size_m());
    let mark = match reach.normal.try_normalize() {
        Some(normal) => mark.on_surface(normal),
        None => mark,
    };
    let mut target = GripTarget::measured(
        GrabVolume::Pickup(kind_of(&reach.kind)),
        hand,
        reach.distance / scale,
        mark,
    );
    target.observed = Some(*reach);
    target
}

/// The kind of target a reach's object is.
pub(super) fn kind_of(kind: &TargetKind) -> PickupKind {
    match kind {
        TargetKind::Prop { .. } => PickupKind::Prop,
        TargetKind::Articulated { .. } => PickupKind::Hinged,
        TargetKind::Weapon => PickupKind::Weapon,
        TargetKind::Door(_) => PickupKind::Door,
        TargetKind::Character(_) => PickupKind::Character,
    }
}

/// The marker on the grab point of what a hand holds, metres.
const HELD_MARK_M: f32 = 0.03;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;
