//! A long gun grabbed anywhere along it. The design is in `docs/mod/input/physical-guns.md`,
//! "Grabbing a gun anywhere".
//!
//! Each gun's regions are derived from its model's box and its known points, all in the gun's own
//! frame ([`shape`]): the pistol grip at the attach point, the stock behind it, the magazine below
//! the well, the action at its handle, the handguard ahead of the receiver to past the foregrip,
//! and the barrel out to the muzzle. A hand that closes on a gun is classified into one of them
//! ([`classify`]), with the point it closes on (the anchor) and the gun's rotation on its wrist.
//!
//! What a hand does with the gun follows the region:
//!
//! - a hand on the grip holds it as before and fires it, and it is the only hand that does;
//! - a second hand anywhere else braces it there ([`crate::controllers::weapons::foregrip`] with
//!   the anchor in place of the gun's own foregrip point), and a brace ahead of the grip steers it;
//! - a lone hand anywhere but the grip carries it ([`Carry`]): the rigid grip
//!   ([`crate::controllers::hands::grip`]) pins the gun so its anchor sits in that hand's palm,
//!   turned as the hand took it, and its trigger fires nothing;
//! - a hand that takes the grip from a carrying hand, or slides along to it from a brace, becomes
//!   the firing hand, and the hand it took the gun from braces it where it was;
//! - the holding hand letting go while the other braces leaves the gun carried by the brace.
//!
//! The region definitions are the module's public surface, for any system that needs to know
//! where on a gun a hand is (the action's region among them). Only long guns (the two-handed,
//! special, and heavy slots) are held this way; a pistol keeps its grip.

pub mod action;
pub mod config;
pub(crate) mod engine;
pub mod shape;

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

pub use self::{
    action::{action_back, action_volume},
    shape::{
        Carry, GunRegion, GunShape, RegionHit, carry_offset, classify, into_gun, out_of_gun,
        turn_on_wrist,
    },
};
use crate::controllers::frame::Hand;

/// What a hand does on a gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandRole {
    /// Holds it by its grip, and fires it.
    Firing,
    /// Holds it beside the hand that holds it.
    Bracing,
    /// Holds it alone at a part other than its grip, and cannot fire it.
    Carrying,
}

impl HandRole {
    pub const fn label(self) -> &'static str {
        match self {
            HandRole::Firing => "firing",
            HandRole::Bracing => "bracing",
            HandRole::Carrying => "carrying",
        }
    }
}

/// One hand on the wielded gun.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HandOnGun {
    pub region: GunRegion,
    pub role: HandRole,
}

/// The hands on a gun: the holding hand at its region (the grip unless it carries), and the
/// bracing hand at its, indexed by [`Hand::index`].
pub fn hands_on_gun(
    holder: Option<(Hand, GunRegion)>,
    brace: Option<(Hand, GunRegion)>,
) -> [Option<HandOnGun>; 2] {
    let mut hands = [None; 2];
    if let Some((hand, region)) = holder {
        let role = if region.fires() {
            HandRole::Firing
        } else {
            HandRole::Carrying
        };
        hands[hand.index()] = Some(HandOnGun { region, role });
    }
    if let Some((hand, region)) = brace.filter(|(h, _)| holder.is_none_or(|(o, _)| o != *h)) {
        hands[hand.index()] = Some(HandOnGun {
            region,
            role: HandRole::Bracing,
        });
    }
    hands
}

/// Where a hand closed on a gun lying in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WorldTouch {
    /// Where the hand reached the gun, in the world.
    pub point: Vec3,
    /// The hand's wrist rotation in the world.
    pub wrist: Quat,
    pub grip_radius: f32,
    pub reach: f32,
}

/// What took a gun by a part other than its grip.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CarryCause {
    /// Picked up off the ground there.
    Pickup,
    /// Drawn off its dock there.
    Draw,
    /// Left in the bracing hand when the holding hand let go.
    LetGo,
}

impl CarryCause {
    pub const fn label(self) -> &'static str {
        match self {
            CarryCause::Pickup => "pickup",
            CarryCause::Draw => "draw",
            CarryCause::LetGo => "the holding hand let go",
        }
    }
}

/// The wielded long gun's shape as read on the last tick, for the readout.
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
    /// Each hand on the wielded gun, indexed by [`Hand::index`].
    pub hands: [Option<HandOnGun>; 2],
    pub shape: Option<ShapeRead>,
    /// Guns taken by a part other than the grip.
    pub carries: u64,
    /// Braces at a point the hand chose.
    pub braces: u64,
    /// Grips taken from a carrying hand, or slid along to.
    pub grips_taken: u64,
    /// Guns left carried in the bracing hand when the holding hand let go.
    pub let_go: u64,
    /// Ticks a brace slid along the gun.
    pub slides: u64,
    /// Whether a shape's read failed for the wielded long gun on the last tick.
    pub unread: bool,
}

pub fn status() -> RegionsStatus {
    STATE.lock().status
}

/// How the hand carrying `weapon` holds it, for the rigid grip's placement of `hand`'s gun. A leaf
/// lock, so the placement may read it from the thread that runs it.
pub fn carry_for(weapon: usize, hand: Hand) -> Option<Carry> {
    STATE
        .lock()
        .carry
        .filter(|c| c.weapon == weapon && c.hand == hand)
        .map(|c| c.carry)
}

/// Which carry of `weapon` in `hand` is under way, when one is: each carry has its own mark, so a
/// gun body seated for one is not kept for another, or for the gun back at its grip. A leaf lock.
pub(crate) fn carry_mark(weapon: usize, hand: Hand) -> Option<u32> {
    STATE
        .lock()
        .carry
        .filter(|c| c.weapon == weapon && c.hand == hand)
        .map(|c| c.mark)
}

/// The hand carrying `weapon` by a part other than its grip, and how.
pub fn carrying(weapon: usize) -> Option<(Hand, Carry)> {
    STATE
        .lock()
        .carry
        .filter(|c| c.weapon == weapon)
        .map(|c| (c.hand, c.carry))
}

/// Start carrying `weapon` in `hand` as `carry` says.
pub(crate) fn begin_carry(weapon: usize, hand: Hand, carry: Carry, cause: CarryCause) {
    let mut state = STATE.lock();
    state.marks = state.marks.wrapping_add(1);
    state.carry = Some(CarryHold {
        weapon,
        hand,
        carry,
        mark: state.marks,
    });
    state.status.carries += 1;
    if cause == CarryCause::LetGo {
        state.status.let_go += 1;
    }
    drop(state);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        region = carry.region.label(),
        cause = cause.label(),
        anchor_m = ?carry.anchor,
        "regions: the {} hand took the gun by its {}",
        hand.label(),
        carry.region.label()
    );
}

/// Stop carrying, for `reason`, when a gun is carried.
pub(crate) fn end_carry(reason: &str) {
    let ended = STATE.lock().carry.take();
    if let Some(c) = ended {
        tracing::info!(
            target: "controllers",
            hand = c.hand.label(),
            region = c.carry.region.label(),
            reason,
            "regions: the gun is no longer carried by its {}",
            c.carry.region.label()
        );
    }
}

/// Note that `hand` took the grip of `weapon` from the other hand, which held it at `region`, and
/// let the other hand brace it at `anchor` (in the gun's frame) once the gun is measured in its new
/// hand.
pub(crate) fn took_grip(weapon: usize, hand: Hand, region: GunRegion, anchor: Vec3, now: Instant) {
    let mut state = STATE.lock();
    state.status.grips_taken += 1;
    state.rebrace = Some(Rebrace {
        weapon,
        hand: hand.other(),
        region,
        anchor,
        since: now,
    });
    drop(state);
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        from = region.label(),
        "regions: the {} hand took the gun's grip; the other braces it at its {}",
        hand.label(),
        region.label()
    );
}

/// The brace waiting for `hand` on `weapon`, taken once: the region and the anchor in the gun's
/// frame. A brace waiting longer than `wait` is given up.
pub(crate) fn take_rebrace(
    weapon: usize,
    hand: Hand,
    now: Instant,
    wait: Duration,
) -> Option<(GunRegion, Vec3)> {
    let mut state = STATE.lock();
    let rebrace = state.rebrace?;
    if now.saturating_duration_since(rebrace.since) > wait || rebrace.weapon != weapon {
        state.rebrace = None;
        return None;
    }
    if rebrace.hand != hand {
        return None;
    }
    state.rebrace = None;
    Some((rebrace.region, rebrace.anchor))
}

/// Give up a waiting brace, when its hand let go before it took hold.
pub(crate) fn drop_rebrace() {
    STATE.lock().rebrace = None;
}

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

/// Let the draw `hand` started at `weapon`'s barrel or stock take it there once it is in the hand.
pub(crate) fn pend_draw(weapon: usize, hand: Hand, carry: Carry, now: Instant) {
    STATE.lock().draw = Some(PendingDraw {
        weapon,
        hand,
        carry,
        since: now,
    });
    tracing::debug!(
        target: "controllers",
        hand = hand.label(),
        region = carry.region.label(),
        "regions: a draw at the gun's {} waits for the gun",
        carry.region.label()
    );
}

/// Forget the draw `hand` left waiting to take a docked gun by a part other than its grip, for a
/// grab at a holster that touched no such part.
pub(crate) fn cancel_draw(hand: Hand) {
    let mut state = STATE.lock();
    if state.draw.is_some_and(|d| d.hand == hand) {
        state.draw = None;
    }
}

/// What the input tick read of the wielded weapon, for the regions.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Observed {
    /// The wielded weapon and its holding hand.
    pub weapon: Option<(usize, Option<Hand>)>,
    /// Whether the wielded weapon is a long gun the regions act on.
    pub long: bool,
    /// Its shape, when it is a long gun and the shape was read.
    pub shape: Option<GunShape>,
    pub now: Instant,
    /// How long a draw waits for its gun.
    pub draw_wait: Duration,
}

/// Follow the wielded weapon: end a carry whose gun left its hand, start a carry a draw waits for
/// once its gun is in the hand, and note the shape. Called once per input tick.
pub(crate) fn observe(observed: &Observed) {
    let mut state = STATE.lock();
    let wielded = observed.weapon.filter(|_| observed.long);
    if let Some(draw) = state.draw {
        let arrived = wielded.is_some_and(|(w, h)| w == draw.weapon && h == Some(draw.hand));
        let expired = observed.now.saturating_duration_since(draw.since) > observed.draw_wait;
        if arrived {
            state.draw = None;
            drop(state);
            begin_carry(draw.weapon, draw.hand, draw.carry, CarryCause::Draw);
            state = STATE.lock();
        } else if expired {
            state.draw = None;
        }
    }
    let gone = state.carry.and_then(|c| match wielded {
        Some((w, _)) if w != c.weapon => Some("another gun is in the hands"),
        Some((_, Some(h))) if h != c.hand => Some("the gun went to the other hand"),
        Some(_) => None,
        None => Some("the gun left the hands"),
    });
    state.status.shape = wielded.zip(observed.shape).map(|((w, _), s)| ShapeRead {
        weapon: w,
        length_m: s.max.z - s.min.z,
        foregrip: s.foregrip.is_some(),
        magazine: s.magazine.is_some(),
        action: s.action.is_some(),
    });
    state.status.unread = wielded.is_some() && observed.shape.is_none();
    drop(state);
    if let Some(reason) = gone {
        end_carry(reason);
    }
}

/// Record the hands on the wielded gun for the readout.
pub(crate) fn record(hands: [Option<HandOnGun>; 2]) {
    STATE.lock().status.hands = hands;
}

/// Forget every carry and waiting brace or draw, for when the weapon handling stands down.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.carry = None;
    state.rebrace = None;
    state.draw = None;
    state.status.hands = [None; 2];
    state.status.shape = None;
    state.status.unread = false;
}

/// A gun carried by a part other than its grip.
#[derive(Debug, Copy, Clone, PartialEq)]
struct CarryHold {
    weapon: usize,
    hand: Hand,
    carry: Carry,
    mark: u32,
}

/// A brace waiting for the gun to be measured in its new holding hand.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Rebrace {
    weapon: usize,
    hand: Hand,
    region: GunRegion,
    anchor: Vec3,
    since: Instant,
}

/// A draw at a docked gun's barrel or stock, waiting for the gun to come into the hand.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PendingDraw {
    weapon: usize,
    hand: Hand,
    carry: Carry,
    since: Instant,
}

struct State {
    status: RegionsStatus,
    carry: Option<CarryHold>,
    rebrace: Option<Rebrace>,
    draw: Option<PendingDraw>,
    /// The last carry's mark.
    marks: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: RegionsStatus {
        hands: [None; 2],
        shape: None,
        carries: 0,
        braces: 0,
        grips_taken: 0,
        let_go: 0,
        slides: 0,
        unread: false,
    },
    carry: None,
    rebrace: None,
    draw: None,
    marks: 0,
});

#[cfg(test)]
mod tests;
