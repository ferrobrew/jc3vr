//! Select socket interaction poses once for the candidate adapter and the copied mirror sample.

use std::time::Instant;

use glam::{Mat4, Vec3};

use crate::controllers::{
    body::rig::frames::Frames,
    frame::Hand,
    grab::{Candidate, GrabVolume, GripTarget, target::TargetReach},
    hands::ActiveHands,
    loadout::Socket,
    roomscale::{self, posture::EffectivePosture},
    weapons::{
        docks::{self, SocketUse, attachments::Sample},
        volumes::{self, SocketTuning},
    },
};

/// The acting hand and weapon occupancy used by the existing long-gun draw restriction.
#[derive(Debug, Copy, Clone)]
pub(crate) struct DrawContext {
    pub dominant: Hand,
    pub occupied: [bool; 2],
}

/// Adapt this tick's native observations and publish the exact selected target sample.
pub(super) fn socket_candidates(
    native: Option<&Sample>,
    active: &ActiveHands,
    character: Option<Mat4>,
    holsters: &SocketTuning,
    draw: DrawContext,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let frames = character.and_then(|character| docks::cockpit_frames(active, character));
    provide_and_publish(
        native,
        frames.as_ref(),
        holsters,
        draw,
        docks::publish_targets,
    )
}

pub(super) fn reach_seated(targets: &mut [GripTarget], controllers: [Option<Vec3>; 2]) {
    let seated = roomscale::frame_shared().is_some_and(|frame| {
        frame.fresh(Instant::now()) && frame.posture == EffectivePosture::Seated
    });
    if !seated {
        return;
    }
    reach_raw(targets, controllers);
}

fn reach_raw(targets: &mut [GripTarget], controllers: [Option<Vec3>; 2]) {
    for target in targets {
        if !matches!(target.volume, GrabVolume::Socket(_)) {
            continue;
        }
        let (Some(hand), TargetReach::Sphere { centre, radius }) = (target.hand, target.reach)
        else {
            continue;
        };
        let Some(controller) = controllers[hand.index()].filter(|point| point.is_finite()) else {
            continue;
        };
        let distance = controller.distance(centre);
        if radius.is_finite() && radius > 0.0 && distance.is_finite() && distance <= radius {
            target.reach = TargetReach::Measured { distance };
        }
    }
}

/// Build the canonical sample and the actual candidate lists from the same selected offers.
pub(crate) fn provide(
    native: &Sample,
    frames: &Frames,
    holsters: &SocketTuning,
    draw: DrawContext,
) -> (Sample, Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let mut targets = native.clone();
    select_draw_offers(&mut targets, draw);
    let (draws, others) = candidates_from_sample(&targets, frames, holsters);
    (targets, draws, others)
}

fn provide_and_publish(
    native: Option<&Sample>,
    frames: Option<&Frames>,
    holsters: &SocketTuning,
    draw: DrawContext,
    publish: impl FnOnce(Option<Sample>),
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let Some((native, frames)) = native.zip(frames) else {
        publish(None);
        return (Vec::new(), Vec::new());
    };
    let (targets, draws, others) = provide(native, frames, holsters, draw);
    publish(Some(targets));
    (draws, others)
}

fn select_draw_offers(sample: &mut Sample, draw: DrawContext) {
    if draw.occupied != [false; 2] {
        return;
    }
    // The restriction depends on the socket and hand, not on pose availability.
    let mut offers = sample
        .attachments
        .iter()
        .filter(|attachment| attachment.usage == Some(SocketUse::Draw))
        .filter_map(|attachment| {
            Some(Candidate {
                volume: GrabVolume::Socket(attachment.socket),
                centre: Vec3::ZERO,
                radius: 0.0,
                hand: Some(attachment.hand?),
            })
        })
        .collect();
    volumes::reserve_long_guns(&mut offers, draw.dominant, true);
    sample.attachments.retain(|attachment| {
        attachment.hand.is_none()
            || attachment.usage != Some(SocketUse::Draw)
            || offers.iter().any(|offer| {
                offer.volume == GrabVolume::Socket(attachment.socket)
                    && offer.hand == attachment.hand
            })
    });
}

fn candidates_from_sample(
    sample: &Sample,
    frames: &Frames,
    holsters: &SocketTuning,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let mut draws = Vec::new();
    let mut others = Vec::new();
    for point in sample.points() {
        let candidate = Candidate {
            volume: GrabVolume::Socket(point.socket),
            centre: frames.cockpit.point_to_cockpit(point.point),
            radius: socket_radius(point.socket, holsters).max(0.0),
            hand: Some(point.hand),
        };
        match point.usage {
            SocketUse::Draw => draws.push(candidate),
            usage @ (SocketUse::Dock | SocketUse::NoFit) => others.push((candidate, usage)),
        }
    }
    (draws, others)
}

fn socket_radius(socket: Socket, holsters: &SocketTuning) -> f32 {
    match socket {
        Socket::Hip(_) => holsters.hip_radius_m,
        Socket::Shoulder(_) | Socket::BackOuter => holsters.shoulder_radius_m,
        Socket::Chest => holsters.chest_radius_m,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod seated_reach_tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::{
        cockpit::CockpitTransform,
        grab::{Mark, MarkAt},
    };

    #[test]
    fn raw_reach_uses_the_calibrated_socket_without_moving_its_marker_or_radius() {
        let cockpit = CockpitTransform {
            head_world: Vec3::new(10.0, 1.7, -4.0),
            head_cockpit: Vec3::new(0.2, 1.1, -0.3),
            body: Quat::from_rotation_y(0.7),
            world_scale: 1.3,
        };
        let centre = cockpit.point_to_cockpit(Vec3::new(10.2, 0.9, -3.8));
        let target = Candidate {
            volume: GrabVolume::Socket(Socket::Hip(Hand::Right)),
            centre,
            radius: 0.1,
            hand: Some(Hand::Right),
        }
        .target(Mark::ring(MarkAt::Cockpit(centre), 0.05));
        let drawn = centre + Vec3::Y * 0.4;
        assert!(
            crate::controllers::grab::resolve::reaches(Hand::Right, &[target], Some(drawn))
                .is_empty()
        );
        for offset in [0.05, 0.15] {
            let mut targets = [target];
            reach_raw(&mut targets, [None, Some(centre + Vec3::X * offset)]);
            assert_eq!(targets[0].mark, target.mark);
            if offset < 0.1 {
                assert!(
                    matches!(targets[0].reach, TargetReach::Measured { distance } if (distance - offset).abs() < 1e-5)
                );
                assert_eq!(
                    crate::controllers::grab::resolve::reaches(Hand::Right, &targets, Some(drawn))
                        .len(),
                    1
                );
            } else {
                assert_eq!(targets[0], target);
            }
        }
        let mut targets = [target];
        reach_raw(&mut targets, [Some(centre), None]);
        assert_eq!(targets[0], target);
    }
}
