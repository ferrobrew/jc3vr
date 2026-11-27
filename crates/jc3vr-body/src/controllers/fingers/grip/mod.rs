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
//! grip::declare(hand, Grip::new(grip::WHEEL));
//! ```
//!
//! A source is a value its owning system defines: a label, the default pose, and how firmly it
//! holds. A declaration may name another pose ([`Grip::pose`]), scale the pose's share as its hold
//! blends in ([`Grip::share`]), or carry the contact fit's curls ([`Grip::fitted`]). The finger
//! curls read the declarations once per sim tick. A declaration lasts a few of their ticks, so a
//! source that stops declaring (the hold ended, or the source stood down) lets the hand go without
//! a release call of its own. When two sources declare for one hand, the first in the registered
//! order wins ([`crate::contributions::BodyContributions::grip_order`]): what the hand holds in its
//! fingers over what it holds on to.
//!
//! The declarations are a leaf lock: a source declares from its own tick, on whatever thread it
//! runs, holding its own state's lock or not, and nothing is taken under the slot.

pub mod blend;
pub mod catalogue;
pub mod contact;

use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use jc3vr_core::published::Stamp;
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::{
    contributions,
    controllers::{
        fingers::{
            grip::catalogue::GripPose,
            tuning::{FINGERS_TUNING, SourceDiametersTuning},
        },
        grab::cockpit::ControlKind,
    },
    tuning::FRESHNESS,
};

/// What put something in a hand: a label for the readouts and the log, the pose the hand takes on
/// it, and how firmly its hold holds. Each system that puts something in a hand defines its own
/// as a constant; two sources are the same when their labels are.
#[derive(Copy, Clone)]
pub struct GripSource {
    label: &'static str,
    pose: fn() -> GripPose,
    firmness: Firmness,
}

impl GripSource {
    /// A source labelled `label` whose hand takes `pose` and whose hold holds as `firmness` says:
    /// [`Firmness::Held`] for what stays in the hand after the grip lets go (whose hold is sticky
    /// or is ended by the source itself) and for a palm pressed on a surface, which no grip holds,
    /// and [`Firmness::Grip`] for what the grip has to keep holding.
    pub const fn new(label: &'static str, pose: fn() -> GripPose, firmness: Firmness) -> Self {
        Self {
            label,
            pose,
            firmness,
        }
    }

    pub const fn label(self) -> &'static str {
        self.label
    }

    /// The pose a hand takes on what this source puts in it.
    pub fn pose(self) -> GripPose {
        (self.pose)()
    }

    /// How firmly the source's hold holds.
    pub const fn firmness(self) -> Firmness {
        self.firmness
    }

    /// The source of a hand on the physical vehicle control `kind`.
    pub const fn of_control(kind: ControlKind) -> GripSource {
        match kind {
            ControlKind::Wheel | ControlKind::Yoke => WHEEL,
            ControlKind::Handlebars => HANDLEBARS,
            ControlKind::Cyclic | ControlKind::FlightStick => STICK,
            ControlKind::Collective | ControlKind::Throttle => LEVER,
        }
    }
}

impl PartialEq for GripSource {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
    }
}

impl Eq for GripSource {}

impl fmt::Debug for GripSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label)
    }
}

/// A steering wheel or a yoke.
pub const WHEEL: GripSource = GripSource::new("wheel", || bar(diameters().wheel_m), Firmness::Held);
pub const HANDLEBARS: GripSource = GripSource::new(
    "handlebars",
    || bar(diameters().handlebars_m),
    Firmness::Held,
);
/// A helicopter's cyclic or a plane's flight stick.
pub const STICK: GripSource = GripSource::new("stick", || bar(diameters().stick_m), Firmness::Held);
/// A helicopter's collective or a plane's throttle.
pub const LEVER: GripSource = GripSource::new("lever", || bar(diameters().lever_m), Firmness::Held);
/// The grapple line, while the grapple carries the player.
pub const GRAPPLE_LINE: GripSource = GripSource::new(
    "grapple line",
    || bar(diameters().grapple_line_m),
    Firmness::Held,
);
/// An empty palm pressed on a surface: a wall, a crate, or a ledge pushed down on.
pub const SURFACE: GripSource = GripSource::new("surface", || GripPose::FlatPalm, Firmness::Held);

/// The most sources the registered order may hold.
pub const MAX_SOURCES: usize = 32;

/// A round bar of `diameter_m` metres.
pub const fn bar(diameter_m: f32) -> GripPose {
    GripPose::Cylinder { diameter_m }
}

/// The bar diameters of the sources whose hold is a round bar.
pub fn diameters() -> &'static SourceDiametersTuning {
    &FINGERS_TUNING.get().diameters
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
    #[inline]
    pub fn new(source: GripSource) -> Self {
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
    #[inline]
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
    let Some(rank) = rank(grip.source) else {
        if !UNORDERED.swap(true, Ordering::Relaxed) {
            tracing::error!(
                source = grip.source.label(),
                "grip: a source outside the registered order declared a grip; it is ignored"
            );
        }
        return;
    };
    DECLARED.lock()[hand.index()][rank] = Some((grip, Stamp::now()));
}

/// The grip `hand` holds now: the winning declaration made within `max_age`.
pub fn current(hand: Hand, now: Stamp, max_age: Duration) -> Option<Grip> {
    let declared = DECLARED.lock()[hand.index()];
    winner(&declared, now, max_age)
}

/// Every source declaring for `hand` at `now`, in the order the declarations win, for the
/// readouts. A declaration counts within the longest age [`max_age`] gives, since the readouts do
/// not know the finger curls' tick interval.
pub fn declared(hand: Hand, now: Stamp) -> Vec<GripSource> {
    let declared = DECLARED.lock()[hand.index()];
    fresh(
        &declared,
        now,
        Duration::from_secs_f32(FRESHNESS.get().fingers.grip_max_s),
    )
    .map(|grip| grip.source)
    .collect()
}

/// How long a declaration lasts after the finger curls' tick interval `dt` in seconds: a few of
/// their ticks, so that a source ticking at another rate (the input tick, once a frame) is not lost
/// between two of them, and short enough that a hand let go opens at once.
pub fn max_age(dt: f32) -> Duration {
    let tuning = &FRESHNESS.get().fingers;
    let dt = if dt.is_finite() { dt } else { 0.0 };
    Duration::from_secs_f32((dt * tuning.grip_ticks).clamp(tuning.grip_min_s, tuning.grip_max_s))
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
    at: Option<Stamp>,
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
        now: Stamp,
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

type Declarations = [Option<(Grip, Stamp)>; MAX_SOURCES];

/// Each hand's latest declaration per source, indexed by [`Hand::index`] and the source's place in
/// the registered order ([`rank`]).
static DECLARED: Mutex<[Declarations; 2]> = Mutex::new([[None; MAX_SOURCES]; 2]);

/// Whether a source outside the registered order has declared, so it is logged once.
static UNORDERED: AtomicBool = AtomicBool::new(false);

/// `source`'s place in the registered order, the first winning.
fn rank(source: GripSource) -> Option<usize> {
    let order = contributions::get().map_or(&[][..], |contributions| contributions.grip_order);
    order
        .iter()
        .take(MAX_SOURCES)
        .position(|ordered| *ordered == source)
}

/// The first source's declaration made within `max_age` of `now`.
fn winner(declared: &Declarations, now: Stamp, max_age: Duration) -> Option<Grip> {
    fresh(declared, now, max_age).next()
}

/// The declarations made within `max_age` of `now`, in the order they win.
fn fresh(
    declared: &Declarations,
    now: Stamp,
    max_age: Duration,
) -> impl Iterator<Item = Grip> + '_ {
    declared
        .iter()
        .flatten()
        .filter(move |(_, at)| now.saturating_duration_since(*at) <= max_age)
        .map(|(grip, _)| *grip)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-ins for the sources the systems above define, in their winning order.
    const BALL: GripSource = GripSource::new("ball", || GripPose::GrenadePalm, Firmness::Held);
    const LEDGE: GripSource = GripSource::new("ledge", || GripPose::LedgeHook, Firmness::Grip);
    const LOOSE: GripSource = GripSource::new("loose", || GripPose::Wrap, Firmness::Grip);

    #[test]
    fn the_first_fresh_source_wins() {
        let earlier = Stamp::from_nanos(1_000_000_000);
        let now = earlier + Duration::from_millis(80);
        let age = Duration::from_millis(50);
        let mut declared: Declarations = [None; MAX_SOURCES];
        assert_eq!(winner(&declared, now, age), None);
        declared[2] = Some((Grip::new(LOOSE), now));
        declared[0] = Some((Grip::new(BALL), earlier));
        assert_eq!(
            winner(&declared, now, age).map(|g| g.source),
            Some(LOOSE),
            "a stale declaration has let go"
        );
        declared[0] = Some((Grip::new(BALL), now));
        assert_eq!(
            winner(&declared, now, age).map(|g| g.source),
            Some(BALL),
            "the earlier source in the order wins"
        );
    }

    #[test]
    fn every_fresh_source_is_listed_in_winning_order() {
        let earlier = Stamp::from_nanos(1_000_000_000);
        let now = earlier + Duration::from_millis(80);
        let age = Duration::from_millis(50);
        let mut declared: Declarations = [None; MAX_SOURCES];
        declared[1] = Some((Grip::new(LEDGE), now));
        declared[0] = Some((Grip::new(BALL), now));
        declared[2] = Some((Grip::new(LOOSE), earlier));
        let sources: Vec<GripSource> = fresh(&declared, now, age).map(|g| g.source).collect();
        assert_eq!(sources, [BALL, LEDGE]);
    }

    #[test]
    fn a_declaration_lasts_a_few_ticks() {
        assert_eq!(
            max_age(0.0),
            Duration::from_secs_f32(FRESHNESS.get().fingers.grip_min_s)
        );
        assert!(
            (max_age(0.016).as_secs_f32() - 0.016 * FRESHNESS.get().fingers.grip_ticks).abs()
                < 1e-6
        );
        assert_eq!(
            max_age(1.0),
            Duration::from_secs_f32(FRESHNESS.get().fingers.grip_max_s)
        );
        assert_eq!(
            max_age(f32::NAN),
            Duration::from_secs_f32(FRESHNESS.get().fingers.grip_min_s)
        );
    }

    #[test]
    fn a_grip_scales_its_share_and_keeps_a_fit() {
        let grip = Grip::new(WHEEL).share(0.5).share(0.5);
        assert_eq!(grip.share, 0.25);
        assert_eq!(Grip::new(WHEEL).share(f32::NAN).share, 0.0);
        assert_eq!(Grip::new(WHEEL).share(3.0).share, 1.0);
        let fit = [Some(0.5), None, None, None, None];
        assert_eq!(Grip::new(LOOSE).fitted(Some(fit)).fit, fit);
        assert_eq!(Grip::new(LOOSE).fitted(None).fit, [None; 5]);
        assert_eq!(
            Grip::new(LOOSE).pose(GripPose::FlatPalm).pose,
            GripPose::FlatPalm
        );
    }

    #[test]
    fn a_vehicle_control_takes_its_bar() {
        assert_eq!(GripSource::of_control(ControlKind::Throttle), LEVER);
        assert_eq!(GripSource::of_control(ControlKind::FlightStick), STICK);
        assert_eq!(GripSource::of_control(ControlKind::Yoke), WHEEL);
    }

    #[test]
    fn source_changes_are_logged_at_most_twice_a_second() {
        let start = Stamp::now();
        let mut log = SourceLog::NEW;
        assert_eq!(log.observe(start, None), None, "an empty hand is not news");
        let first = log.observe(start, Some(BALL)).unwrap();
        assert_eq!(
            (first.from, first.to, first.unlogged),
            (None, Some(BALL), 0)
        );
        let soon = start + Duration::from_millis(100);
        assert_eq!(log.observe(soon, None), None, "too soon");
        assert_eq!(log.observe(soon, Some(LOOSE)), None);
        assert_eq!(log.observe(soon, Some(LOOSE)), None);
        let later = start + LOG_INTERVAL;
        let settled = log.observe(later, Some(LOOSE)).unwrap();
        assert_eq!(settled.from, Some(BALL));
        assert_eq!(settled.to, Some(LOOSE));
        assert_eq!(settled.unlogged, 1, "the release in between was not logged");
        assert_eq!(log.observe(later, Some(LOOSE)), None);
        let back = later + LOG_INTERVAL;
        assert_eq!(log.observe(back, Some(BALL)).unwrap().unlogged, 0);
    }
}
