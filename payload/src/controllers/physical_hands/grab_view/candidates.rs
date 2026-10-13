//! The grab view's candidates: the weapons lying in the world, the loose objects, and the vehicles
//! whose boxes come near a hand, whether or not the reach's rays find them. The world is read
//! through the collider view's read ([`collider_view::scan_moving`]), and each body near a hand is
//! told apart by its owner the way the reach tells what it hits apart ([`bodies::classify`]).

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    physical_hands::{
        PhysicalHandsConfig, bodies,
        collider_view::{self, ScanError, model::BodyKind},
        doors::engine::vehicle_of,
        grab_view::model::{Candidate, CandidateKind},
        grasp::{self, TargetKind},
    },
};

/// The candidates whose boxes come within the candidate radius of a hand at `hands` (game space,
/// by [`Hand::index`]), nearest first, at most the configured number, one per object. The body
/// `standing_on` the player stands on is not one.
///
/// # Safety
///
/// The main thread, outside the physics step.
pub(super) unsafe fn scan(
    hands: [Option<Vec3>; 2],
    standing_on: Option<u32>,
    config: &PhysicalHandsConfig,
) -> Result<Vec<Candidate>, ScanError> {
    let view = &config.grab_view;
    let tracked: Vec<(Hand, Vec3)> = Hand::BOTH
        .into_iter()
        .filter_map(|hand| hands[hand.index()].map(|p| (hand, p)))
        .filter(|(_, p)| p.is_finite())
        .collect();
    if tracked.is_empty() {
        return Ok(Vec::new());
    }
    let centre = tracked.iter().map(|(_, p)| *p).sum::<Vec3>() / tracked.len() as f32;
    let spread = tracked
        .iter()
        .map(|(_, p)| p.distance(centre))
        .fold(0.0, f32::max);
    let radius = view.candidate_radius_m.max(0.0);
    // SAFETY: forwarded from the caller.
    let snapshot = unsafe { collider_view::scan_moving(centre, radius + spread, SCAN_MAX_BODIES) }?;

    let mut found: Vec<(NonZeroUsize, Candidate)> = Vec::new();
    for body in &snapshot.bodies {
        if !matches!(body.kind, BodyKind::Dynamic | BodyKind::Vehicle) {
            continue;
        }
        let Some((hand, distance_m)) = tracked
            .iter()
            .map(|&(hand, p)| (hand, body.aabb.distance(p)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
        else {
            continue;
        };
        if distance_m > radius {
            continue;
        }
        // SAFETY: forwarded from the caller; the body was read a moment ago on this thread.
        let Some(object) = (unsafe { bodies::owner_of(body.id) }) else {
            continue;
        };
        // SAFETY: as above; the owner is the body's live object.
        let Some(kind) = (unsafe { kind_of(object, body.id, standing_on, config) }) else {
            continue;
        };
        let candidate = Candidate {
            aabb: body.aabb,
            kind,
            hand,
            distance_m,
        };
        match found.iter_mut().find(|(o, _)| *o == object) {
            Some((_, kept)) if kept.distance_m <= distance_m => {}
            Some((_, kept)) => *kept = candidate,
            None => found.push((object, candidate)),
        }
    }
    let mut out: Vec<Candidate> = found.into_iter().map(|(_, c)| c).collect();
    out.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    out.truncate(view.max_candidates as usize);
    Ok(out)
}

/// What the object `object`, owning the body `body`, is as a candidate, or `None` when it is no
/// candidate: a weapon that is not lying dropped, a body without a mass, or what the player
/// stands on.
///
/// # Safety
///
/// The main thread, outside the physics step; `object` must be the live owner of `body`.
unsafe fn kind_of(
    object: NonZeroUsize,
    body: u32,
    standing_on: Option<u32>,
    config: &PhysicalHandsConfig,
) -> Option<CandidateKind> {
    if unsafe { vehicle_of(object) }.is_some() {
        return Some(CandidateKind::Vehicle);
    }
    if standing_on == Some(body) {
        return None;
    }
    match unsafe { bodies::classify(object, body) } {
        Ok(TargetKind::Weapon) => Some(CandidateKind::Weapon),
        Ok(TargetKind::Prop { mass_kg }) if grasp::can_hold(mass_kg, &config.pickup) => {
            Some(CandidateKind::Prop)
        }
        Ok(TargetKind::Prop { .. }) => Some(CandidateKind::TooHeavy),
        Ok(_) => Some(CandidateKind::Prop),
        Err(_) => None,
    }
}

/// The most bodies a read keeps, the nearest the hands first, before they are told apart.
const SCAN_MAX_BODIES: u32 = 64;
