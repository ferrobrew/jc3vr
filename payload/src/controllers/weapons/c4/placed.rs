//! Taking a placed charge back into the hand.
//!
//! Every charge the player has placed is on the C4 weapon's live list
//! ([`PlantedExplosiveWeapon::m_ActiveExplosives`]), which the weapon's serial update reads after
//! each of its updates ([`read`]). On the input tick each charge within [`OFFER_RANGE_M`] of the
//! player is a grab volume ([`GrabVolume::PlacedCharge`]) for each free hand, at the charge, in the
//! cockpit frame the volumes live in ([`begin_tick`]); while a throw holds a charge or waits for
//! one, nothing is offered. A grip press there takes the charge nearest the hand.
//!
//! The take swaps the placed charge for a primed one, so the hand holds a charge exactly as a grab
//! at the pouch leaves it: the throw asks for a charge, and before the weapon's next serial update
//! primes it, the placed charge comes off the live list
//! ([`PlantedExplosiveWeapon::RemoveFromActiveList`], which also unregisters its HUD hint and turns
//! off its light) and is removed ([`Explosive::RemoveSelf`]). The order matters: a charge removed
//! first is marked so that its destructor skips the live list, which would keep its entry, its
//! hint, and its light. With the placed charge gone the weapon has room for the primed one, so a
//! take works at the cap of placed charges. Pressed back onto the pouch, the charge is taken back as
//! any held charge is ([`crate::controllers::weapons::pouch_return`]); let go of, it is thrown, and
//! pressed onto a surface, it sticks there, once it has first been clear of every surface, so that
//! the one it came off does not take it straight back.
//!
//! A take is refused, the placed charge left where it is, when the charge is no longer placed
//! (it went off or was removed since the offer), while the game's own planting holds the weapon,
//! and while the game does not allow planting; the grabbing hand buzzes and the refusal is logged
//! at info level, as a refused grab at the pouch is.
//!
//! The grapple's snatch takes a placed charge into a hand through [`take`], choosing among
//! [`charges`].
//!
//! [`PlantedExplosiveWeapon::m_ActiveExplosives`]: jc3gi::equipment::weapon::PlantedExplosiveWeapon::m_ActiveExplosives
//! [`PlantedExplosiveWeapon::RemoveFromActiveList`]: jc3gi::equipment::weapon::PlantedExplosiveWeapon::RemoveFromActiveList
//! [`Explosive::RemoveSelf`]: jc3gi::equipment::explosive::Explosive::RemoveSelf
//! [`GrabVolume::PlacedCharge`]: crate::controllers::grab::GrabVolume::PlacedCharge

use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::{
        explosive::{Explosive, PlantedExplosive},
        weapon::PlantedExplosiveWeapon,
    },
};

use crate::controllers::{
    frame::Hand,
    grab::{Candidate, GrabVolume},
    hands::ActiveHands,
    weapons::{
        c4::{
            STATE, State, primed_charge,
            stage::{self, InputEvent, Refusal, Stage},
        },
        docks, engine,
    },
};

/// A charge the player placed, as the C4 weapon's last update listed it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PlacedCharge {
    /// The charge's address, at the start of the object.
    pub charge: usize,
    /// Where the charge is, world space.
    pub position: Vec3,
}

/// The placed charges at the C4 weapon's last update, in the order they were placed.
pub fn charges() -> Vec<PlacedCharge> {
    STATE.lock().placed.charges.clone()
}

/// Take the placed charge `charge` (a [`PlacedCharge::charge`]) into `hand`: the weapon's next
/// update swaps it for a primed charge in that hand, as a grip press on the charge does. Returns
/// `false`, with nothing changed, while a throw holds a charge or waits for one, or when `charge`
/// was not placed at the weapon's last update. A take accepted here can still be refused by the
/// weapon's update, which then buzzes `hand`.
pub(crate) fn take(hand: Hand, charge: usize) -> bool {
    STATE.lock().take_placed(hand, charge)
}

/// What the begin tick hands the offer.
pub(crate) struct BeginInputs<'a> {
    pub active: &'a ActiveHands,
    /// The hands holding nothing, by [`Hand::index`].
    pub free: [bool; 2],
    /// Whether the C4 throw acts this tick.
    pub acting: bool,
}

/// Offer each placed charge near the player to the free hands, and note the charge nearest each
/// of them for its grab.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn begin_tick(inputs: &BeginInputs) -> Vec<Candidate> {
    let mut state = STATE.lock();
    state.placed.offers = [None; 2];
    if !inputs.acting || state.stage != Stage::Idle || state.placed.charges.is_empty() {
        return Vec::new();
    }
    let Some(player) =
        (unsafe { engine::local_player() }).map(|player| Mat4::from(player.m_WorldMatrixT1))
    else {
        return Vec::new();
    };
    let Some(frames) = docks::cockpit_frames(inputs.active, player) else {
        return Vec::new();
    };
    let origin = player.w_axis.truncate();
    let near: Vec<(usize, Vec3)> = state
        .placed
        .charges
        .iter()
        .filter(|c| c.position.distance(origin) <= OFFER_RANGE_M)
        .map(|c| (c.charge, frames.cockpit.point_to_cockpit(c.position)))
        .collect();
    let grips = inputs
        .active
        .hands
        .map(|hand| hand.map(|hand| hand.grip.position));
    state.placed.offers = offers(&near, grips, inputs.free);
    candidates(&near, RADIUS_M, inputs.free)
}

/// The grab volumes of the charges `near` (each its address and its position in the cockpit
/// frame), for the hands `free` marks.
pub fn candidates(near: &[(usize, Vec3)], radius_m: f32, free: [bool; 2]) -> Vec<Candidate> {
    near.iter()
        .flat_map(|&(_, centre)| {
            Hand::BOTH
                .into_iter()
                .filter(move |h| free[h.index()])
                .map(move |hand| Candidate {
                    volume: GrabVolume::PlacedCharge,
                    centre,
                    radius: radius_m,
                    hand: Some(hand),
                })
        })
        .collect()
}

/// The charge among `near` that each free hand's grip is nearest, by [`Hand::index`]: the one its
/// grab takes. A hand without a grip, or not free, takes none.
pub fn offers(
    near: &[(usize, Vec3)],
    grips: [Option<Vec3>; 2],
    free: [bool; 2],
) -> [Option<usize>; 2] {
    Hand::BOTH.map(|hand| {
        let grip = grips[hand.index()].filter(|_| free[hand.index()])?;
        near.iter()
            .map(|&(charge, at)| (at.distance(grip), charge))
            .filter(|(distance, _)| distance.is_finite())
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, charge)| charge)
    })
}

/// Why a take of a placed charge is refused, if it is: the charge is no longer on the live list
/// (`on_list`), the game's own planting holds the weapon (`busy`), or the game does not allow
/// planting (`may_plant`).
pub fn refusal(on_list: bool, busy: bool, may_plant: bool) -> Option<Refusal> {
    if !on_list {
        Some(Refusal::NotPlaced)
    } else if busy {
        Some(Refusal::Busy)
    } else if !may_plant {
        Some(Refusal::NotAllowed)
    } else {
        None
    }
}

/// The take's share of the C4 state.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PlacedState {
    /// The placed charges at the weapon's last update.
    pub(super) charges: Vec<PlacedCharge>,
    /// The charge each hand's grab takes, from the last begin tick, by [`Hand::index`].
    pub(super) offers: [Option<usize>; 2],
    /// The placed charge the throw's hand asked for, taken at the weapon's next update.
    pub(super) take: Option<(Hand, usize)>,
    /// Whether the held charge came off a surface and has not yet been clear of every surface, so
    /// that pressing it on one does not stick it.
    pub(super) from_surface: bool,
}

impl PlacedState {
    pub(super) const NEW: Self = Self {
        charges: Vec::new(),
        offers: [None; 2],
        take: None,
        from_surface: false,
    };
}

impl State {
    /// Ask for the placed charge `charge` in `hand`, when the throw is idle and the charge was
    /// placed at the weapon's last update.
    pub(super) fn take_placed(&mut self, hand: Hand, charge: usize) -> bool {
        if self.stage != Stage::Idle || !self.placed.charges.iter().any(|c| c.charge == charge) {
            return false;
        }
        self.stage = stage::on_input(self.stage, InputEvent::Grab(hand));
        self.status.stage = self.stage;
        self.placed.take = Some((hand, charge));
        true
    }

    /// A grip press of `hand` on a placed charge: ask for the one the last begin tick offered it.
    pub(super) fn grab_placed(&mut self, hand: Hand) -> bool {
        self.placed.offers[hand.index()].is_some_and(|charge| self.take_placed(hand, charge))
    }

    /// The take to carry out at this weapon update: the one asked for, while the throw still waits
    /// for that hand's charge. A take whose request the throw has since dropped is forgotten.
    pub(super) fn due_take(&mut self) -> Option<(Hand, usize)> {
        let (hand, charge) = self.placed.take.take()?;
        matches!(self.stage, Stage::Wanted { hand: h, .. } if h == hand).then_some((hand, charge))
    }

    /// A take the weapon refused: the throw asks for nothing more, so no charge comes from the
    /// pouch in its place.
    pub(super) fn refuse_take(&mut self) {
        self.stage = Stage::Idle;
        self.status.stage = self.stage;
    }

    /// The placed charge `charge` was swapped for the charge the hand is about to hold.
    pub(super) fn took(&mut self, charge: usize) {
        self.placed.charges.retain(|c| c.charge != charge);
        self.status.taken += 1;
    }
}

/// What a weapon update did with a take.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum TakeStep {
    /// No take was due.
    None,
    /// The placed charge is gone, and the update primes the hand's charge in its place.
    Took,
    /// The take was refused, and the placed charge stays.
    Refused(Hand, Refusal),
}

/// Carry out a take due at this weapon update: refuse it, or take the placed charge off the live
/// list and remove it, so that the update primes the hand's charge in its place.
///
/// # Safety
///
/// The weapon and its user must be live, on the thread that updates the weapon.
pub(super) unsafe fn before_update(
    state: &mut State,
    weapon: &mut PlantedExplosiveWeapon,
    user: &Character,
) -> TakeStep {
    let Some((hand, charge)) = state.due_take() else {
        return TakeStep::None;
    };
    let on_list = unsafe { entries(weapon) }.any(|placed| placed as usize == charge);
    let busy = weapon.m_IsInPlantingMode || unsafe { primed_charge(weapon) }.is_some();
    let may_plant = unsafe { user.m_Inventory.GetAnimationAllowsPlantingExplosives() };
    if let Some(refusal) = refusal(on_list, busy, may_plant) {
        state.refuse_take();
        return TakeStep::Refused(hand, refusal);
    }
    let placed = charge as *mut PlantedExplosive;
    // SAFETY: the charge is on the weapon's live list, which its destructor would take it off, so
    // it is live.
    unsafe {
        weapon.RemoveFromActiveList(placed);
        (*placed.cast::<Explosive>()).RemoveSelf();
    }
    state.took(charge);
    tracing::info!(target: "controllers", hand = hand.label(), "c4: took a placed charge back into the hand");
    TakeStep::Took
}

/// The weapon's placed charges and where each is.
///
/// # Safety
///
/// The weapon must be live, on the thread that updates it.
pub(super) unsafe fn read(weapon: &PlantedExplosiveWeapon) -> Vec<PlacedCharge> {
    unsafe { entries(weapon) }
        .filter_map(|placed| {
            // SAFETY: a charge on the live list is live.
            let explosive = unsafe { placed.cast::<Explosive>().as_ref() }?;
            let position = Mat4::from(explosive.m_WorldMatrixT1).w_axis.truncate();
            position.is_finite().then_some(PlacedCharge {
                charge: placed as usize,
                position,
            })
        })
        .collect()
}

/// How near a placed charge a hand must come to take it, metres: about the charge's size, with the
/// slack the drawn hand needs.
pub const RADIUS_M: f32 = 0.1;

/// How far from the player a placed charge is offered, metres: beyond any reach, so that only the
/// few charges a hand might take are carried into the cockpit frame.
pub const OFFER_RANGE_M: f32 = 3.0;

/// The charges on the weapon's live list, null entries skipped.
///
/// # Safety
///
/// The weapon must be live, on the thread that updates it, and the list must not change while the
/// iterator lives.
unsafe fn entries(weapon: &PlantedExplosiveWeapon) -> impl Iterator<Item = *mut PlantedExplosive> {
    let list = &weapon.m_ActiveExplosives;
    let entries: &[_] = if list.begin.is_null() || list.is_empty() {
        &[]
    } else {
        // SAFETY: the vector's live elements, begin up to end.
        unsafe { std::slice::from_raw_parts(list.begin, list.len()) }
    };
    entries
        .iter()
        .map(|entry| entry.m_Explosive)
        .filter(|placed| !placed.is_null())
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: usize = 0x1000;
    const B: usize = 0x2000;

    #[test]
    fn each_free_hand_gets_a_volume_at_each_charge() {
        let near = [
            (A, Vec3::new(0.0, 1.0, -0.5)),
            (B, Vec3::new(1.0, 0.0, 0.0)),
        ];
        let both = candidates(&near, RADIUS_M, [true; 2]);
        assert_eq!(both.len(), 4);
        assert!(both.iter().all(|c| c.volume == GrabVolume::PlacedCharge));
        let left = candidates(&near, RADIUS_M, [true, false]);
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|c| c.hand == Some(Hand::Left)));
        assert!(candidates(&near, RADIUS_M, [false; 2]).is_empty());
    }

    #[test]
    fn each_hand_takes_the_charge_nearest_its_grip() {
        let near = [
            (A, Vec3::new(-0.3, 1.0, -0.4)),
            (B, Vec3::new(0.3, 1.0, -0.4)),
        ];
        let grips = [
            Some(Vec3::new(-0.25, 1.0, -0.4)),
            Some(Vec3::new(0.2, 1.0, -0.4)),
        ];
        assert_eq!(offers(&near, grips, [true; 2]), [Some(A), Some(B)]);
        assert_eq!(offers(&near, grips, [false, true]), [None, Some(B)]);
        assert_eq!(offers(&near, [None, grips[1]], [true; 2]), [None, Some(B)]);
        assert_eq!(offers(&[], grips, [true; 2]), [None; 2]);
    }

    #[test]
    fn a_take_is_refused_for_a_charge_gone_a_busy_weapon_or_no_permission() {
        assert_eq!(refusal(true, false, true), None);
        assert_eq!(refusal(false, false, true), Some(Refusal::NotPlaced));
        assert_eq!(refusal(false, true, false), Some(Refusal::NotPlaced));
        assert_eq!(refusal(true, true, true), Some(Refusal::Busy));
        assert_eq!(refusal(true, false, false), Some(Refusal::NotAllowed));
    }
}
