//! Select socket interaction poses once for the candidate adapter and the copied mirror sample.

use glam::Mat4;

use crate::controllers::{
    body::rig::frames::Frames,
    grab::{Candidate, GrabVolume},
    hands::ActiveHands,
    loadout::Socket,
    weapons::{
        config::HolstersConfig,
        docks::{self, SocketUse, attachments::Sample},
        volumes::BodyFrame,
    },
};

/// Adapt this tick's native observations and publish the exact selected target sample.
pub(super) fn socket_candidates(
    native: Option<&Sample>,
    active: &ActiveHands,
    character: Option<Mat4>,
    body: Option<&BodyFrame>,
    holsters: &HolstersConfig,
) -> (Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let frames = character.and_then(|character| docks::cockpit_frames(active, character));
    let Some((native, frames)) = native.zip(frames).filter(|_| holsters.enabled) else {
        docks::publish_targets(None);
        return (Vec::new(), Vec::new());
    };
    let (targets, draws, others) = provide(native, &frames, body, holsters);
    docks::publish_targets(Some(targets));
    (draws, others)
}

/// Build the canonical sample and the actual candidate lists from the same selected poses.
pub(crate) fn provide(
    native: &Sample,
    frames: &Frames,
    body: Option<&BodyFrame>,
    holsters: &HolstersConfig,
) -> (Sample, Vec<Candidate>, Vec<(Candidate, SocketUse)>) {
    let targets = native.targets(frames, body, holsters);
    let (draws, others) = candidates_from_sample(&targets, frames, holsters);
    (targets, draws, others)
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
