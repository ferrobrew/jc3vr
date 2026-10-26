//! Held guns as bodies: a gun in a hand whose body acts is a body of its own in the physics world,
//! joined to the hand bodies that hold it. The design is in `docs/mod/input/physical-guns.md`.
//!
//! - [`engine`]: the guns in the hands, as the weapon handling's input tick reads them, and the
//!   gun's own body, whose shape and mass the gun body takes;
//! - [`step`]: the gun bodies' lifecycle in the pre-simulation step: made, joined, kept on their
//!   seats, kicked by the shots, and destroyed;
//! - [`math`]: the pure parts: the mass, the seat and the deviation from it, the axes, and the shot.
//!
//! A gun's own body is in the world only while the weapon lies dropped or is mounted
//! ([`WeaponData::m_PfxInstance`]), and it belongs to the weapon, which the game may destroy at any
//! time. So the gun body is the mod's own: a body made with the gun's own shape (the new body takes
//! a reference of its own to it) and the gun's own mass, on the hand bodies' layer and in the
//! player's collision group, so it meets the world, characters, ragdolls, and vehicles, and not
//! the player, the hands, or the aim's ray. The joints join two bodies of the mod's own, so the
//! game destroying the weapon leaves nothing of the mod's naming a freed body; the gun body is
//! destroyed, its joints first, as soon as the gun leaves the hand, and with every hand body.
//!
//! Each tick runs in this order:
//!
//! 1. The weapon handling's input tick publishes each hand's gun ([`publish`]): the weapon, the
//!    gun, its own body, and which hand braces it on the foregrip. A gun counts only while the
//!    rigid grip pins it to the hand and no slide is moving it between the hand and the body.
//! 2. The hands those guns are in act ([`acting_hands`]), so their bodies are driven, with a
//!    holding hand's strength.
//! 3. The pre-simulation step makes each gun's body on its seat on the holding hand's body: the
//!    gun's last placement in the frame of the controller's palm the hand is driven to, so a hand
//!    body that lags its palm does not seat the gun off the hand. It waits for the hand body to
//!    settle on its palm, joins the gun body to it, and joins the bracing hand's body where that
//!    hand holds it. Each step it moves the gun body with the hand's as one rigid body, puts it
//!    back on its seat past a few centimetres or degrees of stray, measures the stray
//!    ([`deviation`]), which the rigid grip's pin carries the drawn gun by within a couple of
//!    degrees, and gives the gun body each shot's kick ([`queue_shot`]).
//!
//! The gun body's mass is its own body's (the weapon's pickup mass, shared between a dual-wield
//! pair's two guns) held to its weight class's, so that a rifle does not outweigh the hand that
//! swings it, and the hand holding it is driven with its full strength against the hand's and the
//! gun's mass together. The support and class-weight target is shared by the physical drive and
//! the drawn reference, so the gun stays near-rigid on a hand that droops under one-handed weight
//! and steadies with support.
//!
//! [`WeaponData::m_PfxInstance`]: jc3gi::equipment::weapon::WeaponData::m_PfxInstance

mod engine;
pub mod math;
pub(super) mod step;
pub(crate) mod targets;

use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use glam::Vec3;
use parking_lot::Mutex;

pub(crate) use self::engine::read_holds;
pub use self::math::MassSource;
use crate::controllers::{
    frame::{Hand, Pose},
    physical_hands::PhysicalHandsConfig,
    weapons::heft::HeftClass,
};

/// A gun in a hand, as the weapon handling's input tick read it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HeldGun {
    /// The weapon's address.
    pub weapon: NonZeroUsize,
    /// The gun's address (its `WeaponData`).
    pub gun: NonZeroUsize,
    /// The gun's own body's engine wrapper, as an address, which the body's instance must still
    /// be: it proves the gun alive when its body is made.
    pub wrapper: NonZeroUsize,
    /// The gun's own body's id.
    pub body: u32,
    pub class: HeftClass,
    /// How many guns the weapon has: the dual-wield pair's two share its own body's mass.
    pub guns: usize,
    /// The carry the hand holds the gun by, when it holds it by a part other than its grip
    /// ([`crate::controllers::weapons::regions::carry_mark`]): the seat is the carry's.
    pub carry: Option<u32>,
}

/// The engaged support contact in the gun's own frame, in game units.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct BraceContact {
    pub point: Vec3,
    /// The displacement from the support grip to its palm, in the palm's axes.
    pub palm_from_grip: Vec3,
}

/// The guns in the hands on one input tick, indexed by [`Hand::index`].
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GunHolds {
    /// The gun each hand holds.
    pub guns: [Option<HeldGun>; 2],
    /// Whether each hand braces the other hand's gun on its foregrip.
    pub braces: [bool; 2],
    /// The selected support region, indexed by the supporting hand.
    pub contacts: [Option<BraceContact>; 2],
}

impl GunHolds {
    /// The hands on a gun: each holding hand and each bracing hand.
    pub fn hands(&self) -> [bool; 2] {
        Hand::BOTH.map(|hand| {
            let i = hand.index();
            self.guns[i].is_some() || (self.braces[i] && self.guns[hand.other().index()].is_some())
        })
    }
}

/// One hand's gun body on the last step, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunBodyStatus {
    /// The weapon's address.
    pub weapon: usize,
    pub body_id: u32,
    /// Whether the body is in the simulation.
    pub in_world: bool,
    pub mass_kg: f32,
    pub mass_source: MassSource,
    /// The constraint that joins the gun to the holding hand's body.
    pub holder_joint: Option<u32>,
    /// The constraint that joins it to the bracing hand's body.
    pub brace_joint: Option<u32>,
    /// How far the gun body strays from its seat on the holding hand, metres and radians.
    pub deviation_m: f32,
    pub deviation_rad: f32,
    pub speed_mps: f32,
}

/// The held guns' readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GunsStatus {
    /// Indexed by the holding hand's [`Hand::index`].
    pub hands: [Option<GunBodyStatus>; 2],
    pub made: u64,
    pub destroyed: u64,
    /// Times a gun body strayed from its seat, or its hand was put somewhere, and it was put back.
    pub reseats: u64,
    /// Shots whose kick was given to a gun body.
    pub shots: u64,
    /// Contacts each hand's gun body started.
    pub contacts: [u64; 2],
    pub last_error: Option<String>,
}

pub fn status() -> GunsStatus {
    SHARED.lock().status.clone()
}

/// Whether held guns are bodies: their switch, under the hand bodies'.
pub(crate) fn enabled(config: &PhysicalHandsConfig) -> bool {
    super::enabled(config) && config.bodies.guns.enabled
}

/// Publish the guns in the hands. Called from the weapon handling's input tick.
pub(crate) fn publish(holds: GunHolds) {
    let mut shared = SHARED.lock();
    shared.holds = holds;
    shared.published = Some(Instant::now());
}

/// The guns in the hands for a step: the last publication while it is current and held guns are
/// bodies, and none otherwise.
pub(super) fn holds_for_step(config: &PhysicalHandsConfig) -> GunHolds {
    if !enabled(config) {
        return GunHolds::default();
    }
    SHARED
        .lock()
        .fresh_holds(Instant::now())
        .unwrap_or_default()
}

/// The hands that act because they hold or brace a gun, from the last publication: their bodies
/// are driven and joined to the gun's.
pub(crate) fn acting_hands(config: &PhysicalHandsConfig) -> [bool; 2] {
    if !enabled(config) {
        return [false; 2];
    }
    SHARED
        .lock()
        .fresh_holds(Instant::now())
        .map_or([false; 2], |h| h.hands())
}

/// The hands whose gun is a body on the last step, holding or bracing it through a joint. Their
/// support and weight target is shared with the physical drive; synthetic recoil and wall probes
/// leave those hands alone.
pub fn physical_hands() -> [bool; 2] {
    let shared = SHARED.lock();
    if !shared.stepped_lately(Instant::now()) {
        return [false; 2];
    }
    let mut hands = [false; 2];
    for hand in Hand::BOTH {
        if let Some(gun) = &shared.status.hands[hand.index()] {
            hands[hand.index()] |= gun.holder_joint.is_some();
            hands[hand.other().index()] |= gun.brace_joint.is_some();
        }
    }
    hands
}

/// How far the gun body of `weapon` in `hand` strays from its seat on the hand's body, in the
/// gun's own frame, held to the drawn gun's limits: the rigid grip's pin carries the drawn gun by
/// it, so the gun is drawn where its body is, within a couple of degrees of the hand's grip.
/// `None` without a joined gun body for that weapon in that hand.
pub(crate) fn deviation(hand: Hand, weapon: usize) -> Option<Pose> {
    let shared = SHARED.lock();
    if !shared.stepped_lately(Instant::now()) {
        return None;
    }
    shared.deviations[hand.index()]
        .filter(|(w, _)| *w == weapon)
        .map(|(_, d)| d)
}

/// Queue a shot of `weapon`'s gun in `hand` for the next step, which kicks the gun body. Called
/// from the per-shot seam, on the thread that updates the weapon.
pub(crate) fn queue_shot(hand: Hand, weapon: usize) {
    let mut shared = SHARED.lock();
    let queued = &mut shared.shots[hand.index()];
    *queued = match *queued {
        Some((w, n)) if w == weapon => Some((w, n.saturating_add(1))),
        _ => Some((weapon, 1)),
    };
}

/// Count a contact `hand`'s gun body started. Called from the blows' delivery.
pub(super) fn count_contact(hand: Hand) {
    SHARED.lock().status.contacts[hand.index()] += 1;
}

/// The mass of `hand`'s gun body on the last step, for a blow it strikes and for the drive of the
/// hand that holds it.
pub(super) fn mass(hand: Hand) -> Option<f32> {
    SHARED.lock().status.hands[hand.index()].map(|g| g.mass_kg)
}

/// How long a publication or a step's results stay current.
const STALE_AFTER: Duration = Duration::from_millis(200);

/// What the input tick, the step, and the readers share.
struct Shared {
    holds: GunHolds,
    published: Option<Instant>,
    /// Shots queued since the last step: each hand's weapon and the count.
    shots: [Option<(usize, u32)>; 2],
    /// Each holding hand's weapon and its gun body's deviation, from the last step.
    deviations: [Option<(usize, Pose)>; 2],
    stepped: Option<Instant>,
    status: GunsStatus,
}

impl Shared {
    fn fresh_holds(&self, now: Instant) -> Option<GunHolds> {
        self.published
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
            .then_some(self.holds)
    }

    fn stepped_lately(&self, now: Instant) -> bool {
        self.stepped
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    holds: GunHolds {
        guns: [None; 2],
        braces: [false; 2],
        contacts: [None; 2],
    },
    published: None,
    shots: [None; 2],
    deviations: [None; 2],
    stepped: None,
    status: GunsStatus {
        hands: [None; 2],
        made: 0,
        destroyed: 0,
        reseats: 0,
        shots: 0,
        contacts: [0; 2],
        last_error: None,
    },
});

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn gun(at: usize) -> HeldGun {
        HeldGun {
            weapon: NonZeroUsize::new(at).unwrap(),
            gun: NonZeroUsize::new(at + 8).unwrap(),
            wrapper: NonZeroUsize::new(at + 16).unwrap(),
            body: 7,
            class: HeftClass::Rifle,
            guns: 1,
            carry: None,
        }
    }

    #[test]
    fn the_hands_on_a_gun_hold_or_brace_it() {
        let one = GunHolds {
            guns: [None, Some(gun(0x1000))],
            braces: [false, false],
            contacts: [None; 2],
        };
        assert_eq!(one.hands(), [false, true]);
        let braced = GunHolds {
            braces: [true, false],
            ..one
        };
        assert_eq!(braced.hands(), [true, true]);
        // A brace without a gun in the other hand holds nothing.
        let alone = GunHolds {
            guns: [None, None],
            braces: [true, false],
            contacts: [None; 2],
        };
        assert_eq!(alone.hands(), [false, false]);
        let two = GunHolds {
            guns: [Some(gun(0x1000)), Some(gun(0x2000))],
            braces: [false, false],
            contacts: [None; 2],
        };
        assert_eq!(two.hands(), [true, true]);
    }
}
