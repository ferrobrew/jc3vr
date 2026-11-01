//! The sockets' attraction: a weapon let go of near a free socket that takes it, though outside the
//! socket's own reach, travels into the socket rather than falling. Pure.
//!
//! A socket's reach ([`crate::controllers::weapons::volumes::SOCKETS`]) is tight, so that a grab
//! between two sockets takes the nearer one. Putting a weapon back needs less precision than that:
//! the back's sockets are behind the head, out of sight, and a seated player's chair stands
//! between the hand and the shoulder blade. Each free socket that takes the weapon a hand holds
//! therefore has a wider attraction around its centre ([`ATTRACTION`]), which only a release
//! consults, after the socket's own reach has found nothing.
//!
//! The attraction is limited on two sides, so that a weapon dropped on purpose still falls:
//!
//! - a socket on the back or a hip attracts only a hand that is not out in front of the body (a
//!   weapon let go of in front of the chest or the belly is being dropped);
//! - a hand moving fast at the release is throwing the weapon, which the attraction never catches.
//!
//! The weapon then travels from the hand into the socket over a time set by the distance
//! ([`travel_s`]), along the dock's own slide.
//!
//! A hand holding a weapon that comes into a socket's attraction feels one tick ([`ZoneWatch`]),
//! which says that letting go there docks it: the back's sockets cannot be seen, so the hand finds
//! them by feel. The tick does not repeat while the hand stays, and comes again only once the hand
//! has left the attraction by a margin, so a hand resting on its edge does not buzz.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    loadout::Socket,
    weapons::{docks::SocketUse, volumes::BodyFrame},
};

/// The attraction's fixed tuning.
pub(crate) const ATTRACTION: AttractionTuning = AttractionTuning {
    shoulder_m: 0.45,
    hip_m: 0.25,
    chest_m: 0.18,
    back_front_m: 0.08,
    hip_front_m: 0.1,
    max_speed_mps: 2.5,
    speed_window_s: 0.1,
    travel_mps: 1.5,
    max_travel_s: 0.35,
    exit_margin_m: 0.03,
};

/// How far each kind of socket attracts a released weapon, where the hand must be, and how fast it
/// may move.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct AttractionTuning {
    /// The attraction of the shoulders' and the outer back's sockets from their centres, metres: a
    /// hand let open over the shoulder, beside the head on that side, or behind it, which is about
    /// a forearm's length from the point behind the shoulder blade a long gun's grip sits at. The
    /// sockets are out of sight, so the hand finds them by the body alone.
    pub shoulder_m: f32,
    /// The attraction of the hips' sockets, metres: about twice their reach, so a hand let open
    /// anywhere beside the thigh docks the pistol.
    pub hip_m: f32,
    /// The attraction of the chest's socket, metres: twice its reach. The chest socket is in sight
    /// and between the chest pouches, so it attracts little beyond them.
    pub chest_m: f32,
    /// How far in front of the line through the shoulder joints a hand may be for a back socket to
    /// attract it, metres: about as far forward as the ear.
    pub back_front_m: f32,
    /// How far in front of the line through the hip joints a hand may be for a hip socket to
    /// attract it, metres: the front of the thigh, short of a hand held out over the belly.
    pub hip_front_m: f32,
    /// The fastest the hand may move at the release for the attraction to take the weapon, metres
    /// per second at the player's scale: a hand placing a weapon slows to well under it, and a
    /// throw is faster.
    pub max_speed_mps: f32,
    /// How far back from the release the hand's speed is read, seconds: the fastest sample of
    /// that span counts, as a throw slows before the fingers open.
    pub speed_window_s: f32,
    /// How fast an attracted weapon travels into its socket on average, metres per second.
    pub travel_mps: f32,
    /// The longest an attracted weapon's travel takes, seconds.
    pub max_travel_s: f32,
    /// How far past a socket's attraction, or past its front limit, a hand must go to leave it,
    /// metres, before coming back into it ticks again.
    pub exit_margin_m: f32,
}

/// Where a hand must not be for a socket to attract it: further than `max_m` along `forward` from
/// `origin`, all in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct FrontLimit {
    pub origin: Vec3,
    /// The body's forward, unit length.
    pub forward: Vec3,
    pub max_m: f32,
}

impl FrontLimit {
    fn allows(&self, point: Vec3) -> bool {
        self.allows_within(point, 0.0)
    }

    fn allows_within(&self, point: Vec3, margin: f32) -> bool {
        (point - self.origin).dot(self.forward) <= self.max_m + margin
    }
}

/// One free socket's attraction for one hand, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Attractor {
    pub socket: Socket,
    pub hand: Hand,
    /// Where the weapon's grip goes on the socket: the centre of the socket's own volume.
    pub centre: Vec3,
    /// The centre in the world.
    pub world: Vec3,
    /// How far from the centre the socket attracts, metres.
    pub radius: f32,
    pub front: Option<FrontLimit>,
}

/// The socket a release is attracted to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Attraction {
    pub socket: Socket,
    /// How far the hand's grip is from where the weapon's grip goes, metres.
    pub distance: f32,
    /// Where the weapon's grip goes, in the world.
    pub toward: Vec3,
}

/// The attractions of this tick's free sockets, from the sockets' candidates (`others`: the free
/// sockets for the hand holding a weapon that fits, and the ones that do not fit, which attract
/// nothing), with the body they ride, `body`, for the front limits, and `to_world` carrying a
/// cockpit point into the world. Without a body, no socket on the back or a hip attracts, since
/// nothing says where the front of the body is.
pub(crate) fn attractors(
    others: &[(Candidate, SocketUse)],
    body: Option<&BodyFrame>,
    to_world: impl Fn(Vec3) -> Vec3,
    tuning: &AttractionTuning,
) -> Vec<Attractor> {
    others
        .iter()
        .filter(|(_, usage)| *usage == SocketUse::Dock)
        .filter_map(|(candidate, _)| {
            let GrabVolume::Socket(socket) = candidate.volume else {
                return None;
            };
            let hand = candidate.hand?;
            let front = match socket {
                Socket::Chest => None,
                Socket::Shoulder(_) | Socket::BackOuter => {
                    let body = body?;
                    Some(FrontLimit {
                        origin: body.skeleton.chest(),
                        forward: body.chest * Vec3::NEG_Z,
                        max_m: tuning.back_front_m,
                    })
                }
                Socket::Hip(_) => {
                    let body = body?;
                    Some(FrontLimit {
                        origin: body.skeleton.hip_centre(),
                        forward: body.pelvis * Vec3::NEG_Z,
                        max_m: tuning.hip_front_m,
                    })
                }
            };
            let radius = radius_of(socket, tuning).max(candidate.radius);
            let world = to_world(candidate.centre);
            (candidate.centre.is_finite() && world.is_finite() && radius.is_finite()).then_some(
                Attractor {
                    socket,
                    hand,
                    centre: candidate.centre,
                    world,
                    radius,
                    front,
                },
            )
        })
        .collect()
}

/// The socket a release of `hand`'s weapon with its grip at `grip`, moving at up to `speed` metres
/// per second over the speed window, is attracted to: of the attractors for `hand` that reach the
/// grip and whose front limit allows it, the one the grip is deepest inside, as a share of its
/// radius. `None` for a throw, a grip that is not finite, or no attractor.
pub(crate) fn attract(
    attractors: &[Attractor],
    hand: Hand,
    grip: Vec3,
    speed: f32,
    tuning: &AttractionTuning,
) -> Option<Attraction> {
    // A speed that is not a number is no evidence of a placing hand.
    let placing = speed.is_finite() && speed <= tuning.max_speed_mps;
    if !grip.is_finite() || !placing {
        return None;
    }
    attractors
        .iter()
        .filter(|a| a.hand == hand && a.radius > 0.0)
        .filter(|a| a.front.is_none_or(|front| front.allows(grip)))
        .filter_map(|a| {
            let distance = grip.distance(a.centre);
            (distance <= a.radius).then_some((
                distance / a.radius,
                Attraction {
                    socket: a.socket,
                    distance,
                    toward: a.world,
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, attraction)| attraction)
}

/// How long a weapon attracted from `distance` metres away travels into its socket, seconds: at
/// the travel speed on average, never quicker than a dock within the socket's reach (`snap_s`),
/// and never longer than the longest travel.
pub(crate) fn travel_s(distance: f32, snap_s: f32, tuning: &AttractionTuning) -> f32 {
    let travel = distance / tuning.travel_mps.max(f32::EPSILON);
    if travel.is_finite() {
        travel.clamp(snap_s, tuning.max_travel_s.max(snap_s))
    } else {
        snap_s
    }
}

/// Which socket's attraction each hand is in, for the tick on coming into one.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) struct ZoneWatch {
    inside: [Option<Socket>; 2],
}

impl ZoneWatch {
    pub(crate) const fn new() -> Self {
        Self { inside: [None; 2] }
    }

    /// Follow `hand`, with its grip at `grip` (`None` while it has nothing to dock, or its gun is
    /// braced, or the attraction is off), among `attractors`: whether it came into a socket's
    /// attraction this tick, which plays the tick. The hand stays in the socket it is in until it
    /// is past the attraction's radius or its front limit by the exit margin; coming into the
    /// attraction of a socket it is not already in ticks.
    pub(crate) fn step(
        &mut self,
        hand: Hand,
        attractors: &[Attractor],
        grip: Option<Vec3>,
        tuning: &AttractionTuning,
    ) -> bool {
        let slot = &mut self.inside[hand.index()];
        let Some(grip) = grip.filter(|g| g.is_finite()) else {
            *slot = None;
            return false;
        };
        let mine = || {
            attractors
                .iter()
                .filter(|a| a.hand == hand && a.radius > 0.0)
        };
        let margin = tuning.exit_margin_m.max(0.0);
        let stays = slot.is_some_and(|socket| {
            mine().filter(|a| a.socket == socket).any(|a| {
                grip.distance(a.centre) <= a.radius + margin
                    && a.front
                        .is_none_or(|front| front.allows_within(grip, margin))
            })
        });
        if stays {
            return false;
        }
        let entered = mine()
            .filter(|a| a.front.is_none_or(|front| front.allows(grip)))
            .filter_map(|a| {
                let distance = grip.distance(a.centre);
                (distance <= a.radius).then_some((distance / a.radius, a.socket))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, socket)| socket);
        *slot = entered;
        entered.is_some()
    }
}

fn radius_of(socket: Socket, tuning: &AttractionTuning) -> f32 {
    match socket {
        Socket::Shoulder(_) | Socket::BackOuter => tuning.shoulder_m,
        Socket::Hip(_) => tuning.hip_m,
        Socket::Chest => tuning.chest_m,
    }
}

#[cfg(test)]
mod tests;
