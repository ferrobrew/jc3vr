//! What the grab view draws, pure: what the palms' reach recorded on a tick (its rays, the door
//! handles near the palms, the palms' state), the candidates near the hands, how a hand stands to
//! a grip target, and the colours and sizes each of those is drawn with.

use glam::Vec3;

use crate::controllers::{
    frame::Hand,
    overlay::primitive::Rgba,
    physical_hands::{PalmReadout, collider_view::model::Aabb, status::Idle},
};

/// One of a palm's reach rays as the tick cast it, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReachRay {
    pub from: Vec3,
    pub to: Vec3,
    /// Where it hit, if it did.
    pub hit: Option<Vec3>,
    pub outcome: RayOutcome,
}

impl ReachRay {
    /// A ray from `from` to `to` that hit nothing.
    pub const fn missed(from: Vec3, to: Vec3) -> Self {
        Self {
            from,
            to,
            hit: None,
            outcome: RayOutcome::Missed,
        }
    }
}

/// What a reach ray found.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RayOutcome {
    /// It hit nothing within the reach.
    Missed,
    /// It hit something a hand does not take: the static world, a body that is not dynamic, too
    /// heavy, a weapon that is not lying dropped, or what the player stands on.
    NotTaken,
    /// It hit something a hand takes.
    InReach,
}

impl RayOutcome {
    /// The outcome of a ray that `hit` something, which the reach `took` as a target.
    pub const fn of(hit: bool, took: bool) -> Self {
        match (hit, took) {
            (false, _) => RayOutcome::Missed,
            (true, false) => RayOutcome::NotTaken,
            (true, true) => RayOutcome::InReach,
        }
    }

    pub const fn colour(self) -> Rgba {
        match self {
            RayOutcome::Missed => [0.75, 0.75, 0.8, 0.3],
            RayOutcome::NotTaken => [1.0, 0.45, 0.2, 0.85],
            RayOutcome::InReach => [0.35, 1.0, 0.4, 0.95],
        }
    }
}

/// A vehicle door's handle near a palm, as the tick's door reach found it, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct HandleSeen {
    pub at: Vec3,
    pub outcome: HandleOutcome,
}

/// How a door's handle stood to the palm.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    /// Within the handle reach, with a body found at it: offered to the grip.
    InReach,
    /// Near the palm, but past the handle reach.
    OutOfReach,
    /// Within the handle reach, but no body was found at it, so it is not offered.
    NoBody,
}

impl HandleOutcome {
    pub const fn colour(self) -> Rgba {
        match self {
            HandleOutcome::InReach => [0.4, 1.0, 0.45, 0.95],
            HandleOutcome::OutOfReach => [0.4, 1.0, 0.45, 0.35],
            HandleOutcome::NoBody => [1.0, 0.25, 0.2, 0.9],
        }
    }
}

/// What the palms did for one hand on the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PalmState {
    /// The palms stood down for both hands, for this reason.
    Idle(Idle),
    /// The palms have not ticked lately.
    Stale,
    /// The hand is not tracked.
    Untracked,
    /// The hand holds something the palms leave alone: a gun, the foregrip, a grenade or a charge
    /// being thrown, or the animation owns it.
    NotFree,
    /// The hand holds what its grip took from the world.
    Holding,
    /// The hand's body drives it, and reaches from where the body is.
    Driven,
    /// The palm probes, pushes, and reaches.
    Acting,
}

impl PalmState {
    /// The state of the hand `readout` describes: why the palms stood down, or that they have not
    /// ticked within [`PALM_STALE_MS`], or what the hand was doing on the last tick.
    pub fn of(readout: &PalmReadout) -> Self {
        if let Some(idle) = readout.idle {
            return PalmState::Idle(idle);
        }
        if readout.age_ms.is_none_or(|age| age > PALM_STALE_MS) {
            return PalmState::Stale;
        }
        match (readout.free, readout.palm, readout.holding, readout.driven) {
            (false, ..) => PalmState::NotFree,
            (true, false, ..) => PalmState::Untracked,
            (true, true, true, _) => PalmState::Holding,
            (true, true, false, true) => PalmState::Driven,
            (true, true, false, false) => PalmState::Acting,
        }
    }

    pub const fn colour(self) -> Rgba {
        match self {
            PalmState::Idle(Idle::Disabled) => [0.5, 0.5, 0.5, 0.9],
            PalmState::Idle(Idle::Mode) => [0.3, 0.5, 1.0, 0.95],
            PalmState::Idle(Idle::NoHands) => [0.25, 0.25, 0.25, 0.9],
            PalmState::Idle(Idle::OffThread) => [1.0, 0.15, 0.15, 1.0],
            PalmState::Stale => [1.0, 1.0, 1.0, 0.5],
            PalmState::Untracked => [0.25, 0.25, 0.25, 0.9],
            PalmState::NotFree => [1.0, 0.85, 0.2, 0.95],
            PalmState::Holding => [0.2, 0.9, 1.0, 0.95],
            PalmState::Driven => [0.75, 0.45, 1.0, 0.95],
            PalmState::Acting => [0.35, 1.0, 0.4, 0.95],
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            PalmState::Idle(idle) => idle.label(),
            PalmState::Stale => "not ticking",
            PalmState::Untracked => "not tracked",
            PalmState::NotFree => "not free",
            PalmState::Holding => "holding",
            PalmState::Driven => "driven by its body",
            PalmState::Acting => "acting",
        }
    }
}

/// How old the palms' last tick may be and still describe the hands, milliseconds: they tick
/// every simulation tick while they act.
pub const PALM_STALE_MS: u128 = 200;

/// Something near the hands that a hand might take, as the last read of the world found it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Candidate {
    /// Its body's box, game space.
    pub aabb: Aabb,
    pub kind: CandidateKind,
    /// The hand nearest its box.
    pub hand: Hand,
    /// How far its box is from that hand, metres.
    pub distance_m: f32,
}

/// What a candidate is.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CandidateKind {
    /// A weapon lying dropped in the world.
    Weapon,
    /// A loose object light enough for a hand.
    Prop,
    /// A loose object too heavy for a hand.
    TooHeavy,
    /// A vehicle or a part loosened from one, whose doors are offered by their handles.
    Vehicle,
}

impl CandidateKind {
    pub const COUNT: usize = 4;

    pub const ALL: [CandidateKind; CandidateKind::COUNT] = [
        CandidateKind::Weapon,
        CandidateKind::Prop,
        CandidateKind::TooHeavy,
        CandidateKind::Vehicle,
    ];

    pub const fn index(self) -> usize {
        match self {
            CandidateKind::Weapon => 0,
            CandidateKind::Prop => 1,
            CandidateKind::TooHeavy => 2,
            CandidateKind::Vehicle => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            CandidateKind::Weapon => "weapon",
            CandidateKind::Prop => "object",
            CandidateKind::TooHeavy => "too heavy",
            CandidateKind::Vehicle => "vehicle",
        }
    }

    pub const fn colour(self) -> Rgba {
        match self {
            CandidateKind::Weapon => [1.0, 0.8, 0.25, 0.5],
            CandidateKind::Prop => [1.0, 1.0, 1.0, 0.4],
            CandidateKind::TooHeavy => [1.0, 0.35, 0.3, 0.35],
            CandidateKind::Vehicle => [0.4, 1.0, 0.45, 0.3],
        }
    }
}

/// How a hand stands to a grip target.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetState {
    /// Within the view's radius, but out of the hand's reach.
    OutOfReach,
    /// Within the hand's reach.
    InReach,
    /// The one the hand's squeeze would take.
    Chosen,
    /// The hand holds it.
    Held,
}

impl TargetState {
    /// The state of a target the hand `holds`, would take as `chosen`, or is `in_reach` of.
    pub const fn of(holds: bool, chosen: bool, in_reach: bool) -> Self {
        match (holds, chosen, in_reach) {
            (true, ..) => TargetState::Held,
            (false, true, _) => TargetState::Chosen,
            (false, false, true) => TargetState::InReach,
            (false, false, false) => TargetState::OutOfReach,
        }
    }

    pub const fn colour(self) -> Rgba {
        match self {
            TargetState::OutOfReach => [0.6, 0.65, 0.8, 0.3],
            TargetState::InReach => [1.0, 1.0, 0.55, 0.75],
            TargetState::Chosen => [1.0, 0.2, 0.9, 1.0],
            TargetState::Held => [0.2, 0.9, 1.0, 0.9],
        }
    }
}

/// How near a hand `distance` metres from a target has come to it, `0..=1`: one within the
/// target's `reach` metres, falling to zero at the view's `radius` metres.
pub fn approach(distance: f32, reach: f32, radius: f32) -> f32 {
    if distance.is_nan() {
        return 0.0;
    }
    let reach = if reach.is_finite() {
        reach.max(0.0)
    } else {
        0.0
    };
    if distance <= reach {
        return 1.0;
    }
    let span = radius - reach;
    if !(span.is_finite() && span > 0.0) {
        return 0.0;
    }
    (1.0 - (distance - reach) / span).clamp(0.0, 1.0)
}

/// A target's end marker's diameter at `approach`, metres.
pub fn target_mark_size(approach: f32) -> f32 {
    MIN_TARGET_MARK_M + (MAX_TARGET_MARK_M - MIN_TARGET_MARK_M) * approach.clamp(0.0, 1.0)
}

/// A target's line and end marker colour in `state` at `approach`: an out-of-reach target
/// brightens as the hand comes nearer.
pub fn target_colour(state: TargetState, approach: f32) -> Rgba {
    let [r, g, b, a] = state.colour();
    let a = match state {
        TargetState::OutOfReach => a + (0.75 - a) * approach.clamp(0.0, 1.0),
        _ => a,
    };
    [r, g, b, a]
}

/// How long a refused grab flashes, seconds.
pub const REFUSAL_FLASH_S: f32 = 0.5;

/// A refusal's flash `age_s` seconds after it, as its opacity and its size's share, or `None`
/// once it is spent: it starts full and small, and fades as it grows.
pub fn refusal_flash(age_s: f32) -> Option<(f32, f32)> {
    if !(0.0..REFUSAL_FLASH_S).contains(&age_s) {
        return None;
    }
    let t = age_s / REFUSAL_FLASH_S;
    Some((1.0 - t, 0.5 + 0.5 * t))
}

pub const REFUSAL_COLOUR: [f32; 3] = [1.0, 0.1, 0.1];

/// The smallest and largest a target's end marker is drawn, metres.
const MIN_TARGET_MARK_M: f32 = 0.015;
const MAX_TARGET_MARK_M: f32 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_is_classified_by_what_it_hit() {
        for (hit, took, expected) in [
            (false, false, RayOutcome::Missed),
            (false, true, RayOutcome::Missed),
            (true, false, RayOutcome::NotTaken),
            (true, true, RayOutcome::InReach),
        ] {
            assert_eq!(
                RayOutcome::of(hit, took),
                expected,
                "hit {hit}, took {took}"
            );
        }
    }

    #[test]
    fn the_ray_outcomes_have_distinct_colours() {
        let colours = [
            RayOutcome::Missed,
            RayOutcome::NotTaken,
            RayOutcome::InReach,
        ]
        .map(RayOutcome::colour);
        for (i, a) in colours.iter().enumerate() {
            for b in &colours[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    fn readout(free: bool, palm: bool, holding: bool, driven: bool) -> PalmReadout {
        PalmReadout {
            idle: None,
            age_ms: Some(10),
            free,
            driven,
            body_pose: driven,
            palm,
            reach: false,
            holding,
            casts: 0,
            last_error: None,
        }
    }

    #[test]
    fn a_palm_state_follows_the_hand() {
        for (free, palm, holding, driven, expected) in [
            (false, true, false, false, PalmState::NotFree),
            (true, false, false, false, PalmState::Untracked),
            (true, true, true, true, PalmState::Holding),
            (true, true, false, true, PalmState::Driven),
            (true, true, false, false, PalmState::Acting),
        ] {
            assert_eq!(
                PalmState::of(&readout(free, palm, holding, driven)),
                expected
            );
        }
    }

    #[test]
    fn an_idle_or_silent_tick_outranks_the_hand() {
        let idle = PalmReadout {
            idle: Some(Idle::Mode),
            ..readout(true, true, false, false)
        };
        assert_eq!(PalmState::of(&idle), PalmState::Idle(Idle::Mode));
        let never = PalmReadout {
            age_ms: None,
            ..readout(true, true, false, false)
        };
        assert_eq!(PalmState::of(&never), PalmState::Stale);
        let old = PalmReadout {
            age_ms: Some(PALM_STALE_MS + 1),
            ..readout(true, true, false, false)
        };
        assert_eq!(PalmState::of(&old), PalmState::Stale);
    }

    #[test]
    fn each_idle_reason_has_its_own_colour() {
        let idle = [Idle::Disabled, Idle::Mode, Idle::NoHands, Idle::OffThread]
            .map(|i| PalmState::Idle(i).colour());
        for (i, a) in idle.iter().enumerate() {
            for b in &idle[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_ne!(PalmState::Acting.colour(), PalmState::Driven.colour());
        assert_ne!(PalmState::NotFree.colour(), PalmState::Holding.colour());
    }

    #[test]
    fn a_target_state_ranks_held_over_chosen_over_reach() {
        for (holds, chosen, in_reach, expected) in [
            (true, true, true, TargetState::Held),
            (false, true, true, TargetState::Chosen),
            (false, false, true, TargetState::InReach),
            (false, false, false, TargetState::OutOfReach),
        ] {
            assert_eq!(TargetState::of(holds, chosen, in_reach), expected);
        }
    }

    #[test]
    fn the_target_states_have_distinct_colours() {
        let colours = [
            TargetState::OutOfReach,
            TargetState::InReach,
            TargetState::Chosen,
            TargetState::Held,
        ]
        .map(TargetState::colour);
        for (i, a) in colours.iter().enumerate() {
            for b in &colours[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn approach_is_full_within_reach_and_falls_to_the_radius() {
        assert_eq!(approach(0.05, 0.1, 2.0), 1.0);
        assert_eq!(approach(0.1, 0.1, 2.0), 1.0);
        assert_eq!(approach(2.0, 0.1, 2.0), 0.0);
        assert_eq!(approach(3.0, 0.1, 2.0), 0.0);
        let halfway = approach(1.05, 0.1, 2.0);
        assert!((halfway - 0.5).abs() < 1e-5, "{halfway}");
        assert!(approach(0.5, 0.1, 2.0) > approach(1.5, 0.1, 2.0));
    }

    #[test]
    fn approach_survives_degenerate_inputs() {
        assert_eq!(approach(f32::NAN, 0.1, 2.0), 0.0);
        assert_eq!(approach(1.0, 2.0, 2.0), 1.0);
        assert_eq!(approach(3.0, 2.0, 2.0), 0.0);
        assert_eq!(approach(1.0, f32::NAN, 2.0), 0.5);
        assert_eq!(approach(1.0, 0.0, f32::INFINITY), 0.0);
    }

    #[test]
    fn a_target_mark_grows_and_an_out_of_reach_line_brightens_as_the_hand_nears() {
        assert!(target_mark_size(1.0) > target_mark_size(0.0));
        assert_eq!(target_mark_size(2.0), target_mark_size(1.0));
        let far = target_colour(TargetState::OutOfReach, 0.0)[3];
        let near = target_colour(TargetState::OutOfReach, 0.9)[3];
        assert!(near > far);
        assert_eq!(
            target_colour(TargetState::Chosen, 0.0),
            TargetState::Chosen.colour()
        );
    }

    #[test]
    fn a_refusal_flashes_then_is_spent() {
        let (alpha, size) = refusal_flash(0.0).expect("fresh");
        assert_eq!((alpha, size), (1.0, 0.5));
        let (later_alpha, later_size) = refusal_flash(0.4).expect("still flashing");
        assert!(later_alpha < alpha && later_size > size);
        assert_eq!(refusal_flash(REFUSAL_FLASH_S), None);
        assert_eq!(refusal_flash(-0.1), None);
    }
}
