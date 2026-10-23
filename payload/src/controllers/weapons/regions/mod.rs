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
//!
//! Each hand carries its own gun: the wielded weapon or one armed in a hand, so a long gun drawn
//! by its barrel into one hand while the other holds a pistol stays carried there. A brace needs
//! the other hand free, so only the long gun in a hand whose other hand is free is braced
//! ([`crate::controllers::weapons::engine::lone_gun`]).

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

/// One hand on a gun in the hands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HandOnGun {
    pub region: GunRegion,
    pub role: HandRole,
}

/// The hands on the guns in the hands: each hand holding its own gun at its region (`holders`,
/// the grip unless it carries the gun, by [`Hand::index`]), and the bracing hand at its, when it
/// holds no gun of its own.
pub fn hands_on_guns(
    holders: [Option<GunRegion>; 2],
    brace: Option<(Hand, GunRegion)>,
) -> [Option<HandOnGun>; 2] {
    let mut hands = holders.map(|region| {
        region.map(|region| HandOnGun {
            region,
            role: if region.fires() {
                HandRole::Firing
            } else {
                HandRole::Carrying
            },
        })
    });
    if let Some((hand, region)) = brace.filter(|(h, _)| holders[h.index()].is_none()) {
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
    /// Whether a shape's read failed for the lone long gun on the last tick.
    pub unread: bool,
}

pub fn status() -> RegionsStatus {
    STATE.lock().status
}

/// How the hand carrying `weapon` holds it, for the rigid grip's placement of `hand`'s gun. A leaf
/// lock, so the placement may read it from the thread that runs it.
pub fn carry_for(weapon: usize, hand: Hand) -> Option<Carry> {
    STATE.lock().carries[hand.index()]
        .filter(|c| c.weapon == weapon)
        .map(|c| c.carry)
}

/// Which carry of `weapon` in `hand` is under way, when one is: each carry has its own mark, so a
/// gun body seated for one is not kept for another, or for the gun back at its grip. A leaf lock.
pub(crate) fn carry_mark(weapon: usize, hand: Hand) -> Option<u32> {
    STATE.lock().carries[hand.index()]
        .filter(|c| c.weapon == weapon)
        .map(|c| c.mark)
}

/// The hand carrying `weapon` by a part other than its grip, and how.
pub fn carrying(weapon: usize) -> Option<(Hand, Carry)> {
    let state = STATE.lock();
    Hand::BOTH.into_iter().find_map(|hand| {
        state.carries[hand.index()]
            .filter(|c| c.weapon == weapon)
            .map(|c| (hand, c.carry))
    })
}

/// How each hand carries its gun by a part other than its grip, by [`Hand::index`], when it does.
pub fn carries() -> [Option<Carry>; 2] {
    STATE.lock().carries.map(|c| c.map(|c| c.carry))
}

/// Start carrying `weapon` in `hand` as `carry` says.
pub(crate) fn begin_carry(weapon: usize, hand: Hand, carry: Carry, cause: CarryCause) {
    let mut state = STATE.lock();
    state.marks = state.marks.wrapping_add(1);
    state.carries[hand.index()] = Some(CarryHold {
        weapon,
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

/// Stop carrying `weapon`, for `reason`, when a hand carries it.
pub(crate) fn end_carry(weapon: usize, reason: &str) {
    let ended = {
        let mut state = STATE.lock();
        Hand::BOTH.into_iter().find_map(|hand| {
            let slot = &mut state.carries[hand.index()];
            slot.filter(|c| c.weapon == weapon)
                .and_then(|_| slot.take())
                .map(|c| (hand, c))
        })
    };
    if let Some((hand, c)) = ended {
        log_end(hand, &c, reason);
    }
}

/// Log the end of `hand`'s carry `c`, for `reason`.
fn log_end(hand: Hand, c: &CarryHold, reason: &str) {
    tracing::info!(
        target: "controllers",
        hand = hand.label(),
        region = c.carry.region.label(),
        reason,
        "regions: the gun is no longer carried by its {}",
        c.carry.region.label()
    );
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
    STATE.lock().draws[hand.index()] = Some(PendingDraw {
        weapon,
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
    STATE.lock().draws[hand.index()] = None;
}

/// What the input tick read of the guns in the hands, for the regions.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Observed {
    /// The long gun the regions act on in each hand, by [`Hand::index`].
    pub guns: [Option<usize>; 2],
    /// The lone long gun and its shape, when the shape was read.
    pub lone: Option<(usize, Option<GunShape>)>,
    pub now: Instant,
    /// How long a draw waits for its gun.
    pub draw_wait: Duration,
}

/// Follow the guns in the hands: end a carry whose gun left its hand, start a carry a draw waits
/// for once its gun is in the hand, and note the lone long gun's shape. Called once per input tick.
pub(crate) fn observe(observed: &Observed) {
    let mut arrived = Vec::new();
    let mut gone = Vec::new();
    {
        let mut guard = STATE.lock();
        let state = &mut *guard;
        for hand in Hand::BOTH {
            let gun = observed.guns[hand.index()];
            let slot = &mut state.draws[hand.index()];
            if let Some(draw) = *slot {
                let expired =
                    observed.now.saturating_duration_since(draw.since) > observed.draw_wait;
                if gun == Some(draw.weapon) {
                    *slot = None;
                    arrived.push((hand, draw));
                } else if expired {
                    *slot = None;
                }
            }
            let carry = &mut state.carries[hand.index()];
            if let Some(c) = *carry
                && let Some(reason) = carry_gone(c.weapon, gun)
            {
                *carry = None;
                gone.push((hand, c, reason));
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
    for (hand, c, reason) in gone {
        log_end(hand, &c, reason);
    }
    for (hand, draw) in arrived {
        begin_carry(draw.weapon, hand, draw.carry, CarryCause::Draw);
    }
}

/// Why a carry of `weapon` ends, with `gun` the long gun its hand now holds, or `None` while the
/// hand still holds it.
pub(crate) fn carry_gone(weapon: usize, gun: Option<usize>) -> Option<&'static str> {
    match gun {
        Some(w) if w == weapon => None,
        Some(_) => Some("another gun is in the hand"),
        None => Some("the gun left the hand"),
    }
}

/// Record the hands on the long guns in the hands for the readout.
pub(crate) fn record(hands: [Option<HandOnGun>; 2]) {
    STATE.lock().status.hands = hands;
}

/// Forget every carry and waiting brace or draw, for when the weapon handling stands down.
pub(crate) fn stand_down() {
    let mut state = STATE.lock();
    state.carries = [None; 2];
    state.rebrace = None;
    state.draws = [None; 2];
    state.status.hands = [None; 2];
    state.status.shape = None;
    state.status.unread = false;
}

/// A gun carried by a part other than its grip.
#[derive(Debug, Copy, Clone, PartialEq)]
struct CarryHold {
    weapon: usize,
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
    carry: Carry,
    since: Instant,
}

struct State {
    status: RegionsStatus,
    /// Each hand's carry, by [`Hand::index`].
    carries: [Option<CarryHold>; 2],
    /// The rebrace needs the other hand free, so there is one at most.
    rebrace: Option<Rebrace>,
    /// Each hand's draw waiting for its gun, by [`Hand::index`].
    draws: [Option<PendingDraw>; 2],
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
    carries: [None; 2],
    rebrace: None,
    draws: [None; 2],
    marks: 0,
});

#[cfg(test)]
mod tests;
