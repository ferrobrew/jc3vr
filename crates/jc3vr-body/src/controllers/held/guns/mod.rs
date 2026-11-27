//! The held guns the hand bodies make bodies of, as the weapon handling publishes them, and what
//! the bodies' step reports back: the store the weapon handling's input tick and the hand bodies'
//! pre-simulation step share. The design is in `docs/mod/input/physical-guns.md`.
//!
//! - the publication: the guns in the hands for the next step ([`publish`], [`holds_at`]) and the
//!   shots queued for it ([`queue_shot`], [`take_shots`]);
//! - the step's record ([`record_step`], [`stand_down`], and the counts) and the results the
//!   hands, the rigid grip, and the foregrip read ([`physical_hands`], [`deviation`],
//!   [`take_support_break`], [`mass`], [`carried_mass`]), and the readout ([`status`]);
//! - the support and weight targets the weapon handling composes for the held guns, registered
//!   once at install ([`crate::contributions::BodyContributions::gun_targets`]) and applied by the
//!   hand bodies' drive ([`apply_targets`]);
//! - [`HeftClass`]: the weight class a weapon's tuning comes from.
//!
//! The step counts the publication's age in physics steps ([`crate::controllers::physics::steps`])
//! and the results' in wall-clock time, so a hitch of the whole simulation tick never drops a held
//! gun and a reader between the steps sees results only while the steps run.

mod class;

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3vr_core::published::{Stamp, Step};
use jc3vr_input::frame::{Hand, Pose};
use parking_lot::Mutex;

pub use self::class::HeftClass;
use crate::{
    contributions,
    controllers::{hands::CockpitHand, physics::steps},
    tuning::FRESHNESS,
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
    /// The slide onto the grip the gun is in, while one is under way or has just come to rest, as
    /// the weapon handling marks it: the seat is the slide's.
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

/// Where a gun body's mass comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MassSource {
    /// The gun's own body.
    Body,
    /// The class's fallback: the gun's own body gives no finite, positive mass.
    Class,
}

impl MassSource {
    pub const fn label(self) -> &'static str {
        match self {
            MassSource::Body => "its own body",
            MassSource::Class => "its class",
        }
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

impl GunBodyStatus {
    /// The mass the holding hand's drive carries besides its own: the gun body's, and the bracing
    /// hand's (`bracing_hand_kg`) while a brace joins it.
    pub fn carried_kg(&self, bracing_hand_kg: f32) -> f32 {
        let bracing = if self.brace_joint.is_some() && bracing_hand_kg.is_finite() {
            bracing_hand_kg.max(0.0)
        } else {
            0.0
        };
        self.mass_kg + bracing
    }
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

/// What one step did to the gun bodies, recorded at its end ([`record_step`]).
pub struct StepRecord {
    /// When the step ran.
    pub at: Stamp,
    /// Each holding hand's gun body, by the holding hand.
    pub statuses: [Option<GunBodyStatus>; 2],
    /// Each holding hand's weapon and its gun body's deviation, held to the drawn gun's limits.
    pub deviations: [Option<(usize, Pose)>; 2],
    /// Each bracing hand whose brace broke on this step, by the bracing hand.
    pub support_broke: [bool; 2],
    pub made: u64,
    pub destroyed: u64,
    pub reseats: u64,
    pub shots: u64,
    pub error: Option<String>,
}

pub fn status() -> GunsStatus {
    SHARED.lock().status.clone()
}

/// Publish the guns in the hands. Called from the weapon handling's input tick.
pub fn publish(holds: GunHolds) {
    let mut shared = SHARED.lock();
    shared.holds = holds;
    shared.published = Some(Step::now());
}

/// The guns in the hands as last published, while the publication is current at step `step`.
pub fn holds_at(step: Step) -> Option<GunHolds> {
    SHARED.lock().fresh_holds(step)
}

/// The hands whose gun is a body on the last step, holding or bracing it through a joint. Their
/// support and weight target is shared with the physical drive; synthetic recoil and wall probes
/// leave those hands alone.
pub fn physical_hands() -> [bool; 2] {
    let shared = SHARED.lock();
    if !shared.stepped_lately(Stamp::now()) {
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
pub fn deviation(hand: Hand, weapon: usize) -> Option<Pose> {
    let shared = SHARED.lock();
    if !shared.stepped_lately(Stamp::now()) {
        return None;
    }
    shared.deviations[hand.index()]
        .filter(|(w, _)| *w == weapon)
        .map(|(_, d)| d)
}

/// Queue a shot of `weapon`'s gun in `hand` for the next step, which kicks the gun body. Called
/// from the per-shot seam, on the thread that updates the weapon.
pub fn queue_shot(hand: Hand, weapon: usize) {
    let mut shared = SHARED.lock();
    let queued = &mut shared.shots[hand.index()];
    *queued = match *queued {
        Some((w, n)) if w == weapon => Some((w, n.saturating_add(1))),
        _ => Some((weapon, 1)),
    };
}

/// Take the shots queued since the last step: each hand's weapon and the count.
pub fn take_shots() -> [Option<(usize, u32)>; 2] {
    std::mem::take(&mut SHARED.lock().shots)
}

/// Record what a step did to the gun bodies.
pub fn record_step(record: StepRecord) {
    let mut shared = SHARED.lock();
    for (flag, now_broken) in shared.support_broke.iter_mut().zip(record.support_broke) {
        *flag |= now_broken;
    }
    shared.deviations = record.deviations;
    shared.stepped = Some(record.at);
    let status = &mut shared.status;
    status.hands = record.statuses;
    status.made += record.made;
    status.destroyed += record.destroyed;
    status.reseats += record.reseats;
    status.shots += record.shots;
    if let Some(error) = record.error {
        status.last_error = Some(error);
    }
}

/// Forget the last step's gun bodies and the shots queued, for a step that acts on no hand.
pub fn stand_down() {
    let mut shared = SHARED.lock();
    shared.shots = [None; 2];
    shared.deviations = [None; 2];
    shared.status.hands = [None; 2];
}

/// Count gun bodies destroyed outside a step's own record.
pub fn count_destroyed(destroyed: u64) {
    SHARED.lock().status.destroyed += destroyed;
}

/// Count a contact `hand`'s gun body started. Called from the blows' delivery.
pub fn count_contact(hand: Hand) {
    SHARED.lock().status.contacts[hand.index()] += 1;
}

/// The mass of `hand`'s gun body on the last step, for a blow it strikes.
pub fn mass(hand: Hand) -> Option<f32> {
    SHARED.lock().status.hands[hand.index()].map(|g| g.mass_kg)
}

/// The mass the drive of the hand holding `hand`'s gun carries besides its own, from the last
/// step: the gun body's, and a bracing hand's of `bracing_hand_kg` while the brace holds it, since
/// that hand rides the gun.
pub fn carried_mass(hand: Hand, bracing_hand_kg: f32) -> Option<f32> {
    SHARED.lock().status.hands[hand.index()].map(|g| g.carried_kg(bracing_hand_kg))
}

/// Whether the brace `hand` held broke since the last call, because its tracked palm left the gun:
/// the two-handed hold lets go. Called once per input tick by the foregrip.
pub fn take_support_break(hand: Hand) -> bool {
    std::mem::take(&mut SHARED.lock().support_broke[hand.index()])
}

/// The support and weight targets of the held guns: called with the cockpit-frame hands and the
/// head's position, it poses a two-handed or heavy gun's hands. The weapon handling composes it
/// with its own configuration.
pub(crate) type TargetsFn = fn(&mut [Option<CockpitHand>; 2], Vec3);

/// Give `hands` the held guns' support and weight targets, as the registered targets pose them
/// ([`crate::contributions::BodyContributions::gun_targets`]); the install refuses to finish
/// without them.
pub fn apply_targets(hands: &mut [Option<CockpitHand>; 2], head: Vec3) {
    if let Some(apply) = contributions::get().and_then(|contributions| contributions.gun_targets) {
        apply(hands, head);
    }
}

/// What the input tick, the step, and the readers share.
struct Shared {
    holds: GunHolds,
    /// The step the holds were published at.
    published: Option<Step>,
    /// Shots queued since the last step: each hand's weapon and the count.
    shots: [Option<(usize, u32)>; 2],
    /// Each holding hand's weapon and its gun body's deviation, from the last step.
    deviations: [Option<(usize, Pose)>; 2],
    /// Each bracing hand whose brace broke since the foregrip last looked, by the bracing hand.
    support_broke: [bool; 2],
    stepped: Option<Stamp>,
    status: GunsStatus,
}

impl Shared {
    /// The holds while their publication is current at step `step`.
    fn fresh_holds(&self, step: Step) -> Option<GunHolds> {
        steps::window()
            .holds(self.published, step)
            .then_some(self.holds)
    }

    fn stepped_lately(&self, now: Stamp) -> bool {
        FRESHNESS
            .get()
            .held
            .guns_ms
            .window()
            .holds(self.stepped, now)
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
    support_broke: [false; 2],
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
    fn the_holding_hand_carries_a_bracing_hand_with_its_gun() {
        let held = GunBodyStatus {
            weapon: 0x1000,
            body_id: 7,
            in_world: true,
            mass_kg: 4.0,
            mass_source: MassSource::Body,
            holder_joint: Some(1),
            brace_joint: None,
            deviation_m: 0.0,
            deviation_rad: 0.0,
            speed_mps: 0.0,
        };
        assert_eq!(held.carried_kg(3.0), 4.0);
        let braced = GunBodyStatus {
            brace_joint: Some(2),
            ..held
        };
        assert_eq!(braced.carried_kg(3.0), 7.0);
        assert_eq!(braced.carried_kg(f32::NAN), 4.0);
        assert_eq!(braced.carried_kg(-1.0), 4.0);
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
