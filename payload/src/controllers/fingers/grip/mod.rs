//! Grip poses: the fingers wrap what the hand holds. The design is in
//! `docs/mod/input/grip-poses.md`.
//!
//! - [`catalogue`]: the named poses, each a shape for every finger;
//! - [`blend`]: the blend from the measured curls to a pose, by how firmly the grip holds;
//! - [`contact`]: the fingers closed onto a held body's surface by short casts.
//!
//! Every system that puts something in a hand declares it on each of its ticks while the hand
//! holds it, with [`declare`] and a [`Grip`] built from its [`GripSource`]:
//!
//! ```ignore
//! grip::declare(hand, Grip::new(GripSource::Wheel));
//! ```
//!
//! The source names the default pose and how firmly it holds; a declaration may name another pose
//! ([`Grip::pose`]), scale the pose's share as its hold blends in ([`Grip::share`]), or carry the
//! contact fit's curls ([`Grip::fitted`]). The finger curls read the declarations once per sim
//! tick. A declaration lasts a few of their ticks, so a source that stops declaring (the hold
//! ended, or the source stood down) lets the hand go without a release call of its own. When two
//! sources declare for one hand, the first in [`GripSource::ALL`] wins: what the hand holds in
//! its fingers over what it holds on to.
//!
//! The declarations are a leaf lock: a source declares from its own tick, on whatever thread it
//! runs, holding its own state's lock or not, and nothing is taken under the slot.

pub mod blend;
pub mod catalogue;
pub mod contact;

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::controllers::{
    fingers::grip::catalogue::GripPose, frame::Hand, vehicles::physical::ControlKind,
};

/// What put something in a hand, in the order the declarations win.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GripSource {
    /// A magazine or a round taken for the reload.
    Magazine,
    Grenade,
    C4,
    /// A gun in the hand, by its grip.
    Gun,
    /// The off-hand on the held gun's foregrip.
    Foregrip,
    /// A character taken hold of, or the ragdoll of one.
    Character,
    /// A vehicle's door: its handle area while latched, or the door itself once open.
    Door,
    /// An object hinged to the world or another body.
    Articulated,
    /// A loose object picked up.
    Pickup,
    /// A stunt handhold: a helicopter's skid, gripped to hang from it, or held in its hang.
    Stunt,
    /// The world, or a moving body, gripped to climb.
    Climb,
    Wheel,
    Handlebars,
    /// A helicopter's cyclic or a plane's flight stick.
    Stick,
    /// A helicopter's collective or a plane's throttle.
    Lever,
    ParachuteHandle,
    /// A mounted gun's handles.
    MountedGun,
    /// The grapple line, while the grapple carries the player.
    GrappleLine,
    /// An empty palm pressed on a surface: a wall, a crate, or a ledge pushed down on.
    Surface,
}

impl GripSource {
    pub const ALL: [GripSource; 19] = [
        GripSource::Magazine,
        GripSource::Grenade,
        GripSource::C4,
        GripSource::Gun,
        GripSource::Foregrip,
        GripSource::Character,
        GripSource::Door,
        GripSource::Articulated,
        GripSource::Pickup,
        GripSource::Stunt,
        GripSource::Climb,
        GripSource::Wheel,
        GripSource::Handlebars,
        GripSource::Stick,
        GripSource::Lever,
        GripSource::ParachuteHandle,
        GripSource::MountedGun,
        GripSource::GrappleLine,
        GripSource::Surface,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn label(self) -> &'static str {
        match self {
            GripSource::Magazine => "magazine",
            GripSource::Grenade => "grenade",
            GripSource::C4 => "c4",
            GripSource::Gun => "gun",
            GripSource::Foregrip => "foregrip",
            GripSource::Character => "character",
            GripSource::Door => "door",
            GripSource::Articulated => "hinged object",
            GripSource::Pickup => "loose object",
            GripSource::Stunt => "stunt handhold",
            GripSource::Climb => "climb",
            GripSource::Wheel => "wheel",
            GripSource::Handlebars => "handlebars",
            GripSource::Stick => "stick",
            GripSource::Lever => "lever",
            GripSource::ParachuteHandle => "parachute handle",
            GripSource::MountedGun => "mounted gun",
            GripSource::GrappleLine => "grapple line",
            GripSource::Surface => "surface",
        }
    }

    /// The source of a hand on the physical vehicle control `kind`.
    pub const fn of_control(kind: ControlKind) -> GripSource {
        match kind {
            ControlKind::Wheel => GripSource::Wheel,
            ControlKind::Handlebars => GripSource::Handlebars,
            ControlKind::Cyclic | ControlKind::FlightStick => GripSource::Stick,
            ControlKind::Collective | ControlKind::Throttle => GripSource::Lever,
        }
    }

    /// The pose a hand takes on what this source puts in it.
    pub const fn pose(self) -> GripPose {
        match self {
            GripSource::Magazine => GripPose::MagazinePinch,
            GripSource::Grenade => GripPose::GrenadePalm,
            GripSource::C4 => GripPose::ChargePinch,
            GripSource::Gun => GripPose::PistolGrip,
            GripSource::Foregrip => GripPose::Foregrip,
            GripSource::Character => GripPose::CollarGrab,
            GripSource::Door => GripPose::DoorHandle,
            GripSource::Articulated => GripPose::Cylinder { diameter_m: 0.04 },
            GripSource::Pickup => GripPose::Wrap,
            GripSource::Stunt => GripPose::Cylinder { diameter_m: 0.05 },
            GripSource::Climb => GripPose::LedgeHook,
            GripSource::Wheel => GripPose::Cylinder { diameter_m: 0.035 },
            GripSource::Handlebars => GripPose::Cylinder { diameter_m: 0.032 },
            GripSource::Stick => GripPose::Cylinder { diameter_m: 0.038 },
            GripSource::Lever => GripPose::Cylinder { diameter_m: 0.04 },
            GripSource::ParachuteHandle => GripPose::Cylinder { diameter_m: 0.025 },
            GripSource::MountedGun => GripPose::Cylinder { diameter_m: 0.035 },
            GripSource::GrappleLine => GripPose::Cylinder { diameter_m: 0.015 },
            GripSource::Surface => GripPose::FlatPalm,
        }
    }

    /// How firmly the source's hold holds: [`Firmness::Held`] for what stays in the hand after the
    /// grip lets go (the weapons, the vehicles' controls, and the grapple line, whose holds are
    /// sticky or are ended by the source itself) and for a palm pressed on a surface, which no grip
    /// holds, and [`Firmness::Grip`] for what the grip has to keep holding.
    pub const fn firmness(self) -> Firmness {
        match self {
            GripSource::Character
            | GripSource::Door
            | GripSource::Articulated
            | GripSource::Pickup
            | GripSource::Stunt
            | GripSource::Climb => Firmness::Grip,
            _ => Firmness::Held,
        }
    }

    /// Whether the source is a gun in the hand or its foregrip, whose fingers the game's grip
    /// animation wraps when the grip poses leave weapons alone.
    pub const fn is_weapon(self) -> bool {
        matches!(self, GripSource::Gun | GripSource::Foregrip)
    }
}

/// How firmly a hold holds, which sets the pose's share
/// ([`blend::firmness_share`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Firmness {
    /// The pose follows the grip's squeeze.
    Grip,
    /// The whole pose, whatever the grip does.
    Held,
}

/// What a hand holds, as its source declares it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Grip {
    pub source: GripSource,
    pub pose: GripPose,
    pub firmness: Firmness,
    /// The share of the pose the source's hold gives, `0..=1`: below one while the hold blends in
    /// or out.
    pub share: f32,
    /// The contact fit's curl for each finger that found the held body.
    pub fit: [Option<f32>; 5],
}

impl Grip {
    /// The source's default pose and firmness, at the whole share, with no fit.
    pub const fn new(source: GripSource) -> Self {
        Self {
            source,
            pose: source.pose(),
            firmness: source.firmness(),
            share: 1.0,
            fit: [None; 5],
        }
    }

    /// The grip with `pose` instead of the source's default.
    pub const fn pose(self, pose: GripPose) -> Self {
        Self { pose, ..self }
    }

    /// The grip with its share scaled by `share`, `0..=1`.
    pub fn share(self, share: f32) -> Self {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            share: self.share * share,
            ..self
        }
    }

    /// The grip with the contact fit's curls, when there are any.
    pub const fn fitted(self, fit: Option<[Option<f32>; 5]>) -> Self {
        match fit {
            Some(fit) => Self { fit, ..self },
            None => self,
        }
    }
}

/// Declare what `hand` holds on this tick. Call it on every tick of the source while the hand
/// holds the thing; a hand whose declarations stop is let go.
pub fn declare(hand: Hand, grip: Grip) {
    DECLARED.lock()[hand.index()][grip.source.index()] = Some((grip, Instant::now()));
}

/// The grip `hand` holds now: the winning declaration made within `max_age`.
pub(crate) fn current(hand: Hand, now: Instant, max_age: Duration) -> Option<Grip> {
    let declared = DECLARED.lock()[hand.index()];
    winner(&declared, now, max_age)
}

/// How long a declaration lasts after the finger curls' tick interval `dt` in seconds: a few of
/// their ticks, so that a source ticking at another rate (the input tick, once a frame) is not lost
/// between two of them, and short enough that a hand let go opens at once.
pub(crate) fn max_age(dt: f32) -> Duration {
    let dt = if dt.is_finite() { dt } else { 0.0 };
    Duration::from_secs_f32((dt * 2.5).clamp(MIN_AGE_S, MAX_AGE_S))
}

/// A change of a hand's grip source, for the log.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct SourceChange {
    pub from: Option<GripSource>,
    pub to: Option<GripSource>,
    /// The changes made since the last logged one that were not logged.
    pub unlogged: u32,
}

/// The log of one hand's grip source, rate-limited: a change is logged at once unless the last was
/// logged less than [`LOG_INTERVAL`] ago, in which case the source the hand settles on is logged
/// when the interval has passed, with a count of the changes in between.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct SourceLog {
    logged: Option<GripSource>,
    seen: Option<GripSource>,
    at: Option<Instant>,
    unlogged: u32,
}

impl SourceLog {
    pub(crate) const NEW: SourceLog = SourceLog {
        logged: None,
        seen: None,
        at: None,
        unlogged: 0,
    };

    /// Note the hand's source `current` at `now`, and return the change to log, if any.
    pub(crate) fn observe(
        &mut self,
        now: Instant,
        current: Option<GripSource>,
    ) -> Option<SourceChange> {
        if current != self.seen {
            self.seen = current;
            self.unlogged += 1;
        }
        if current == self.logged {
            return None;
        }
        let due = self
            .at
            .is_none_or(|at| now.saturating_duration_since(at) >= LOG_INTERVAL);
        if !due {
            return None;
        }
        let change = SourceChange {
            from: self.logged,
            to: current,
            unlogged: self.unlogged.saturating_sub(1),
        };
        self.logged = current;
        self.at = Some(now);
        self.unlogged = 0;
        Some(change)
    }
}

/// The shortest gap between two logged changes of one hand's grip source.
pub(crate) const LOG_INTERVAL: Duration = Duration::from_millis(500);

const MIN_AGE_S: f32 = 0.03;
const MAX_AGE_S: f32 = 0.15;

type Declarations = [Option<(Grip, Instant)>; GripSource::ALL.len()];

/// Each hand's latest declaration per source, indexed by [`Hand::index`] and
/// [`GripSource::index`].
static DECLARED: Mutex<[Declarations; 2]> = Mutex::new([[None; GripSource::ALL.len()]; 2]);

/// The first source's declaration made within `max_age` of `now`.
fn winner(declared: &Declarations, now: Instant, max_age: Duration) -> Option<Grip> {
    declared
        .iter()
        .flatten()
        .find(|(_, at)| now.saturating_duration_since(*at) <= max_age)
        .map(|(grip, _)| *grip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources_are_indexed_in_order() {
        for (i, source) in GripSource::ALL.into_iter().enumerate() {
            assert_eq!(source.index(), i);
            assert!(!source.label().is_empty());
        }
    }

    #[test]
    fn the_first_fresh_source_wins() {
        let now = Instant::now();
        let age = Duration::from_millis(50);
        let mut declared: Declarations = [None; GripSource::ALL.len()];
        assert_eq!(winner(&declared, now, age), None);
        declared[GripSource::Pickup.index()] = Some((Grip::new(GripSource::Pickup), now));
        declared[GripSource::Grenade.index()] = Some((
            Grip::new(GripSource::Grenade),
            now - Duration::from_millis(80),
        ));
        assert_eq!(
            winner(&declared, now, age).map(|g| g.source),
            Some(GripSource::Pickup),
            "a stale declaration has let go"
        );
        declared[GripSource::Grenade.index()] = Some((Grip::new(GripSource::Grenade), now));
        assert_eq!(
            winner(&declared, now, age).map(|g| g.source),
            Some(GripSource::Grenade),
            "what the fingers hold wins"
        );
    }

    #[test]
    fn a_declaration_lasts_a_few_ticks() {
        assert_eq!(max_age(0.0), Duration::from_secs_f32(MIN_AGE_S));
        assert!((max_age(0.016).as_secs_f32() - 0.04).abs() < 1e-6);
        assert_eq!(max_age(1.0), Duration::from_secs_f32(MAX_AGE_S));
        assert_eq!(max_age(f32::NAN), Duration::from_secs_f32(MIN_AGE_S));
    }

    #[test]
    fn a_grip_scales_its_share_and_keeps_a_fit() {
        let grip = Grip::new(GripSource::Wheel).share(0.5).share(0.5);
        assert_eq!(grip.share, 0.25);
        assert_eq!(Grip::new(GripSource::Wheel).share(f32::NAN).share, 0.0);
        assert_eq!(Grip::new(GripSource::Wheel).share(3.0).share, 1.0);
        let fit = [Some(0.5), None, None, None, None];
        assert_eq!(Grip::new(GripSource::Pickup).fitted(Some(fit)).fit, fit);
        assert_eq!(Grip::new(GripSource::Pickup).fitted(None).fit, [None; 5]);
        assert_eq!(
            Grip::new(GripSource::Pickup).pose(GripPose::FlatPalm).pose,
            GripPose::FlatPalm
        );
    }

    #[test]
    fn sticky_holds_hold_and_grip_holds_follow_the_grip() {
        assert_eq!(GripSource::Gun.firmness(), Firmness::Held);
        assert_eq!(GripSource::GrappleLine.firmness(), Firmness::Held);
        assert_eq!(GripSource::Pickup.firmness(), Firmness::Grip);
        assert_eq!(GripSource::Climb.firmness(), Firmness::Grip);
        assert_eq!(GripSource::Stunt.firmness(), Firmness::Grip);
        assert!(GripSource::Foregrip.is_weapon() && !GripSource::Grenade.is_weapon());
        assert_eq!(
            GripSource::of_control(ControlKind::Throttle),
            GripSource::Lever
        );
        assert_eq!(
            GripSource::of_control(ControlKind::FlightStick),
            GripSource::Stick
        );
    }

    #[test]
    fn source_changes_are_logged_at_most_twice_a_second() {
        let start = Instant::now();
        let mut log = SourceLog::NEW;
        assert_eq!(log.observe(start, None), None, "an empty hand is not news");
        let first = log.observe(start, Some(GripSource::Gun)).unwrap();
        assert_eq!(
            (first.from, first.to, first.unlogged),
            (None, Some(GripSource::Gun), 0)
        );
        let soon = start + Duration::from_millis(100);
        assert_eq!(log.observe(soon, None), None, "too soon");
        assert_eq!(log.observe(soon, Some(GripSource::Pickup)), None);
        assert_eq!(log.observe(soon, Some(GripSource::Pickup)), None);
        let later = start + LOG_INTERVAL;
        let settled = log.observe(later, Some(GripSource::Pickup)).unwrap();
        assert_eq!(settled.from, Some(GripSource::Gun));
        assert_eq!(settled.to, Some(GripSource::Pickup));
        assert_eq!(settled.unlogged, 1, "the release in between was not logged");
        assert_eq!(log.observe(later, Some(GripSource::Pickup)), None);
        let back = later + LOG_INTERVAL;
        assert_eq!(
            log.observe(back, Some(GripSource::Gun)).unwrap().unlogged,
            0
        );
    }
}
