//! Select socket interaction poses once for the candidate adapter and the copied mirror sample.

use glam::{Mat4, Vec3};

use crate::controllers::{
    body::rig::frames::Frames,
    frame::Hand,
    grab::{Candidate, GrabVolume},
    hands::ActiveHands,
    loadout::Socket,
    weapons::{
        config::HolstersConfig,
        docks::{self, SocketUse, attachments::Sample},
        volumes::{self, BodyFrame},
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
    body: Option<&BodyFrame>,
    holsters: &HolstersConfig,
    draw: DrawContext,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let frames = character.and_then(|character| docks::cockpit_frames(active, character));
    provide_and_publish(
        native,
        frames.as_ref(),
        body,
        holsters,
        draw,
        docks::publish_targets,
    )
}

/// Build the canonical sample and the actual candidate lists from the same selected offers.
pub(crate) fn provide(
    native: &Sample,
    frames: &Frames,
    body: Option<&BodyFrame>,
    holsters: &HolstersConfig,
    draw: DrawContext,
) -> (Sample, Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let mut targets = native.targets(frames, body, holsters);
    select_draw_offers(&mut targets, holsters, draw);
    let (draws, others) = candidates_from_sample(&targets, frames, holsters);
    (targets, draws, others)
}

fn provide_and_publish(
    native: Option<&Sample>,
    frames: Option<&Frames>,
    body: Option<&BodyFrame>,
    holsters: &HolstersConfig,
    draw: DrawContext,
    publish: impl FnOnce(Option<Sample>),
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let Some((native, frames)) = native.zip(frames).filter(|_| holsters.enabled) else {
        publish(None);
        return (Vec::new(), Vec::new());
    };
    let (targets, draws, others) = provide(native, frames, body, holsters, draw);
    publish(Some(targets));
    (draws, others)
}

fn select_draw_offers(sample: &mut Sample, holsters: &HolstersConfig, draw: DrawContext) {
    if !holsters.long_guns_dominant || draw.occupied != [false; 2] {
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
    holsters: &HolstersConfig,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let mut draws = Vec::new();
    let mut others = Vec::new();
    for point in sample.points() {
        let candidate = Candidate {
            volume: GrabVolume::Socket(point.socket),
            centre: frames.world_to_cockpit(point.point),
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

fn socket_radius(socket: Socket, holsters: &HolstersConfig) -> f32 {
    match socket {
        Socket::Hip(_) => holsters.hip.radius_m,
        Socket::Shoulder(_) | Socket::BackOuter => holsters.shoulder.radius_m,
        Socket::Chest => holsters.chest.radius_m,
    }
}

#[cfg(test)]
mod tests;
