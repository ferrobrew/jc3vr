//! A long gun grabbed anywhere along it. The design is in `docs/mod/input/physical-guns.md`,
//! "Grabbing a gun anywhere".
//!
//! Each gun's regions are derived from its model's box and its known points, all in the gun's own
//! frame ([`shape`]): the pistol grip at the attach point, the stock behind it, the magazine below
//! the well, the action at its handle, the handguard ahead of the receiver to past the foregrip,
//! and the barrel out to the muzzle. A hand that closes on a gun is classified into one of them
//! ([`classify`]), with the point it closes on (the anchor).
//!
//! A hand holding a gun on its own always holds it by its pistol grip, ready to shoot. What a hand
//! does with the gun follows the region:
//!
//! - a hand on the grip holds it and fires it, and it is the only hand that does;
//! - a second hand anywhere else braces it there ([`crate::controllers::weapons::foregrip`] with
//!   the anchor in place of the gun's own foregrip point), and a brace ahead of the grip steers it;
//! - a lone hand that takes a free gun anywhere on it (off the ground, off a dock, or from the
//!   other hand) takes it by the grip, the gun sliding into the palm aligned to shoot;
//! - the holding hand letting go while the other braces passes the gun to the bracing hand, which
//!   slides onto the grip from wherever it braced ([`grip_from_brace`]): the gun glides through the
//!   hand ([`slid_anchor`]) and turns onto the canonical grip over the same slide.
//!
//! The region definitions are the module's public surface, for any system that needs to know
//! where on a gun a hand is (the action's region among them). Only long guns (the two-handed,
//! special, and heavy slots) are braced along their length; a pistol keeps its grip.
//!
//! A brace needs the other hand free, so only the long gun in a hand whose other hand is free is
//! braced ([`crate::controllers::weapons::engine::lone_gun`]).

pub mod action;
pub mod config;
pub(crate) mod engine;
pub mod shape;

use std::time::{Duration, Instant};

use body_rig::smooth::smoothstep;
use glam::{Quat, Vec3};
use parking_lot::Mutex;

pub use self::{
    action::{action_back, action_volume},
    shape::{
        GripSlide, GunRegion, GunShape, RegionHit, classify, into_gun, out_of_gun, slide_offset,
        slide_on_bone, turn_on_wrist,
    },
};
use crate::controllers::frame::Hand;

/// How far outside the gun's model box a hand still touches the gun, metres.
pub const REACH_M: f32 = 0.05;

/// How far from the pistol grip a hand still holds the grip, metres.
pub const GRIP_RADIUS_M: f32 = 0.07;

/// What a hand does on a gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandRole {
    /// Holds it by its grip, and fires it.
    Firing,
    /// Holds it beside the hand that holds it.
    Bracing,
}

impl HandRole {
    pub const fn label(self) -> &'static str {
        match self {
            HandRole::Firing => "firing",
            HandRole::Bracing => "bracing",
        }
    }
}

/// One hand on a gun in the hands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HandOnGun {
    pub region: GunRegion,
    pub role: HandRole,
}

/// The hands on the guns in the hands: each hand holding its own gun (`holders`, by
/// [`Hand::index`]) at its grip, and the bracing hand at its region, when it holds no gun of its
/// own.
pub fn hands_on_guns(
    holders: [bool; 2],
    brace: Option<(Hand, GunRegion)>,
) -> [Option<HandOnGun>; 2] {
    let mut hands = holders.map(|holds| {
        holds.then_some(HandOnGun {
            region: GunRegion::Grip,
            role: HandRole::Firing,
        })
    });
    if let Some((hand, region)) = brace.filter(|(h, _)| !holders[h.index()]) {
        hands[hand.index()] = Some(HandOnGun {
            region,
            role: HandRole::Bracing,
        });
    }
    hands
}

/// The lone long gun's shape as read on the last tick, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ShapeRead {
    pub weapon: usize,
    /// The model box's length along the bore, metres.
    pub length_m: f32,
    pub foregrip: bool,
    pub magazine: bool,
    pub action: bool,
}

/// The regions' readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RegionsStatus {
    /// Each hand on a long gun in the hands, indexed by [`Hand::index`].
    pub hands: [Option<HandOnGun>; 2],
    pub shape: Option<ShapeRead>,
    /// Braces at a point the hand chose.
    pub braces: u64,
    /// Guns passed to the bracing hand, which slid onto the grip, when the holding hand let go.
    pub let_go: u64,
    /// Ticks a brace slid along the gun.
    pub slides: u64,
    /// Whether a shape's read failed for the lone long gun on the last tick.
    pub unread: bool,
}

pub fn status() -> RegionsStatus {
    STATE.lock().status
}

/// Where `weapon` is on its way onto `hand`'s grip, for the rigid grip's placement of `hand`'s gun:
/// the anchor and the turn where the slide has them now. A leaf lock, so the placement may read it
/// from the thread that runs it.
pub fn slide_for(weapon: usize, hand: Hand) -> Option<GripSlide> {
    let now = Instant::now();
    STATE.lock().slides[hand.index()]
        .filter(|s| s.weapon == weapon)
        .map(|s| s.at(now))
}

/// Which slide of `weapon` onto `hand`'s grip is under way, when one is: each slide has its own
/// mark, so a gun body seated for one is not kept for another, or for the gun at rest on its grip.
/// A leaf lock.
pub(crate) fn slide_mark(weapon: usize, hand: Hand) -> Option<u32> {
    let now = Instant::now();
    STATE.lock().slides[hand.index()]
        .filter(|s| s.weapon == weapon)
        .map(|s| s.mark_at(now))
}

/// The grip a bracing hand slides onto when the holding hand lets go, from wherever it braced:
/// the gun's point `from` (in the gun's frame) glides through the hand to the grip (the attach
/// point) over [`GRIP_SLIDE`] ([`slid_anchor`]), and the gun turns from `turn`, its rotation on the
/// wrist as the hand braced it, onto the canonical grip over the same slide, so it ends aligned to
/// shoot.
pub(crate) fn grip_from_brace(
    weapon: usize,
    hand: Hand,
    region: GunRegion,
    turn: Quat,
    from: Vec3,
    now: Instant,
) {
    let mut state = STATE.lock();
    state.marks = state.marks.wrapping_add(1);
    state.slides[hand.index()] = Some(SlideHold {
        weapon,
        from: if from.is_finite() { from } else { Vec3::ZERO },
        turn,
        mark: state.marks,
        since: now,
    });
    state.status.let_go += 1;
    drop(state);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        region = region.label(),
        from_m = ?from,
        "regions: the holding hand let go, and the {} hand slides from the {} onto the grip",
        hand.label(),
        region.label()
    );
}

/// The anchor of a hand sliding from `from` to `to` along a gun, `elapsed` into a slide of length
/// `over`: eased in and out, so the gun neither jumps into nor stops dead in the hand, and at `to`
/// once the slide is over.
pub fn slid_anchor(from: Vec3, to: Vec3, elapsed: Duration, over: Duration) -> Vec3 {
    from.lerp(to, eased_share(elapsed, over))
}

/// How long a bracing hand takes to slide onto the grip.
pub const GRIP_SLIDE: Duration = Duration::from_millis(250);

/// Note a brace at a point the hand chose.
pub(crate) fn braced(hand: Hand, region: GunRegion) {
    STATE.lock().status.braces += 1;
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        region = region.label(),
        "regions: the {} hand braced the gun at its {}",
        hand.label(),
        region.label()
    );
}

/// Note a brace sliding along the gun.
pub(crate) fn slid() {
    STATE.lock().status.slides += 1;
}

/// What the input tick read of the guns in the hands, for the regions.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Observed {
    /// The long gun the regions act on in each hand, by [`Hand::index`].
    pub guns: [Option<usize>; 2],
    /// The lone long gun and its shape, when the shape was read.
    pub lone: Option<(usize, Option<GunShape>)>,
}

/// Follow the guns in the hands: end a slide whose gun left its hand, and note the lone long gun's
/// shape. Called once per input tick.
pub(crate) fn observe(observed: &Observed) {
    let mut guard = STATE.lock();
    let state = &mut *guard;
    for hand in Hand::BOTH {
        let slot = &mut state.slides[hand.index()];
        if slot.is_some_and(|s| slide_gone(s.weapon, observed.guns[hand.index()])) {
            *slot = None;
        }
    }
    state.status.shape = observed.lone.and_then(|(w, shape)| {
        shape.map(|s| ShapeRead {
            weapon: w,
            length_m: s.max.z - s.min.z,
            foregrip: s.foregrip.is_some(),
            magazine: s.magazine.is_some(),
            action: s.action.is_some(),
        })
    });
    state.status.unread = observed.lone.is_some_and(|(_, shape)| shape.is_none());
}

/// Whether a slide of `weapon` ends, with `gun` the long gun its hand now holds.
pub(crate) fn slide_gone(weapon: usize, gun: Option<usize>) -> bool {
    gun != Some(weapon)
}

/// Record the hands on the long guns in the hands for the readout.
pub(crate) fn record(hands: [Option<HandOnGun>; 2]) {
    STATE.lock().status.hands = hands;
}

/// Forget every slide, for when the weapon handling stands down.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.slides = [None; 2];
    state.status.hands = [None; 2];
    state.status.shape = None;
    state.status.unread = false;
}

/// The share of a slide of length `over` done `elapsed` into it, eased in and out.
fn eased_share(elapsed: Duration, over: Duration) -> f32 {
    if over.is_zero() {
        return 1.0;
    }
    smoothstep(elapsed.as_secs_f32() / over.as_secs_f32())
}

/// A gun sliding through a hand onto its grip.
#[derive(Debug, Copy, Clone, PartialEq)]
struct SlideHold {
    weapon: usize,
    /// The anchor it started from, in the gun's frame.
    from: Vec3,
    /// The gun's rotation on the wrist when it started.
    turn: Quat,
    mark: u32,
    since: Instant,
}

impl SlideHold {
    /// The slide's mark at `now`. A slide that has ended counts as one of its own, so a gun body
    /// seated while the gun was gliding through the hand is seated again where it came to rest.
    fn mark_at(self, now: Instant) -> u32 {
        let settled = now.saturating_duration_since(self.since) >= GRIP_SLIDE;
        self.mark.wrapping_mul(2).wrapping_add(u32::from(settled))
    }

    /// Where the slide has the gun at `now`.
    fn at(self, now: Instant) -> GripSlide {
        let elapsed = now.saturating_duration_since(self.since);
        GripSlide {
            anchor: slid_anchor(self.from, Vec3::ZERO, elapsed, GRIP_SLIDE),
            turn: self.turn,
            aligned: eased_share(elapsed, GRIP_SLIDE),
        }
    }
}

struct State {
    status: RegionsStatus,
    /// Each hand's slide onto its grip, by [`Hand::index`].
    slides: [Option<SlideHold>; 2],
    /// The last slide's mark.
    marks: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: RegionsStatus {
        hands: [None; 2],
        shape: None,
        braces: 0,
        let_go: 0,
        slides: 0,
        unread: false,
    },
    slides: [None; 2],
    marks: 0,
});

#[cfg(test)]
mod tests;
