//! The physical hands' grip targets ([`crate::controllers::grab`]): what each empty hand's reach
//! found in the world (an object, a weapon, a vehicle's door, or a character), the object a hand
//! holds, the surface a hand could climb, and the helicopter the player hangs from.
//!
//! The reach runs on the simulation tick and is read here on the input tick, measured: the reach's
//! own casts found each target, so its distance is the reach's. The markers lie on what was found,
//! in the world, on its surface where the reach knows the surface's normal.

use std::time::Instant;

use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        grab::{GrabVolume, GripTarget, Mark, MarkAt, PickupKind, Provided, Shown},
        physical_hands::{
            STATE, climb,
            grasp::{Reach, TargetKind},
            pickup::diagnostics::scan::Scan,
            stunt,
        },
    },
};

/// The physical hands' targets for this input tick, with the reach's notes for each hand, at the VR
/// world scale `world_scale`.
pub(crate) fn targets(world_scale: f32, scans: &mut [Option<Scan>; 2]) -> Provided {
    *scans = [None; 2];
    let config = Config::lock_query(|c| c.controllers.physical_hands);
    let scale = world_scale.max(1e-3);
    let mut out = Provided::default();
    if !config.enabled {
        return out;
    }
    let palms = {
        let state = STATE.lock();
        let fresh = state.fresh(Instant::now());
        for hand in Hand::BOTH {
            let i = hand.index();
            if !fresh {
                continue;
            }
            out.notes[i].clone_from(&state.reach_notes[i]);
            if !config.pickup.enabled {
                continue;
            }
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
            if let Some(reach) = status
                .reach
                .filter(|r| config.pickup.weapons || r.kind != TargetKind::Weapon)
            {
                out.targets.push(reach_target(hand, &reach, scale));
            }
        }
        state.status.hands.map(|h| h.palm.map(|p| p.centre))
    };
    if climb::enabled(&config) {
        for hand in Hand::BOTH {
            let Some(hold) = climb::reach_hold(hand) else {
                continue;
            };
            let distance = palms[hand.index()].map_or(0.0, |p| p.distance(hold.point) / scale);
            out.targets.push(GripTarget::measured(
                GrabVolume::Climb,
                hand,
                distance,
                Mark::ring(MarkAt::World(hold.point), WORLD_MARK_M).on_surface(hold.normal),
            ));
        }
    }
    if stunt::hanging() {
        for hand in Hand::BOTH {
            out.targets.push(GripTarget::measured(
                GrabVolume::StuntHold,
                hand,
                0.0,
                Mark::ring(MarkAt::World(glam::Vec3::ZERO), WORLD_MARK_M).showing(Shown::Never),
            ));
        }
    }
    out
}

/// What `reach` found, as a target `hand` is at the reach's distance from, at the player's scale
/// (`scale` world metres to one).
fn reach_target(hand: Hand, reach: &Reach, scale: f32) -> GripTarget {
    let mark = Mark::ring(MarkAt::World(reach.point), WORLD_MARK_M);
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

/// The marker on something in the world, metres.
const WORLD_MARK_M: f32 = 0.06;
/// The marker on the grab point of what a hand holds, metres.
const HELD_MARK_M: f32 = 0.03;
