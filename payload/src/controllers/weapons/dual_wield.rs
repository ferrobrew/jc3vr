//! The dual-wield split: the plan of record's "Dual-wield"
//! (`docs/mod/input/controllers-and-roomscale.md`), with the engine side in
//! `docs/engine/gameplay/aim-pipeline.md`.
//!
//! The dual-wield slot is one weapon with a gun per hand. Each gun rides its own hand attach bone,
//! a child of that hand's wrist, so the controller-driven wrists carry the guns and the late update
//! corrects each by its own hand. On foot the game has one fire input and alternates the guns after
//! every shot. The split gives each hand's trigger its own gun:
//!
//! - **Placement.** Gun 0 rides the dominant hand and gun 1 the other: when the game's layout has
//!   them the other way round (a left-handed layout), the two guns' attach bones for the
//!   dual-wielding state are swapped, and swapped back when the split stops acting ([`place`]).
//! - **Input.** The bindings hold the game's fire action while either trigger is pulled, so the
//!   game's own permission to shoot still reaches the weapon. On foot, the off-hand's trigger then
//!   fires the off-hand gun rather than retracting the tethers; the tethers retract with the
//!   off-hand's stick click instead ([`crate::controllers::bindings::tables`]). The triggers' press
//!   edges are latched per hand at the input tick ([`observe_triggers`]).
//! - **Firing.** At the weapon's per-tick update ([`before_update`], from the gun aim's detour on
//!   the base update), the split chooses a hand ([`Split::choose`]), points the weapon's next gun
//!   ([`WeaponBase::m_NextWeaponToFire`]) at that hand's gun, and hands the weapon that hand's pull,
//!   so the update fires that gun, from its own muzzle, at that hand's aim point. The weapon counts
//!   ammunition, heat, and recoil once per shot, whichever gun fires.
//! - **Semi-automatic fire.** The weapon keeps one fired-this-pull flag, which only clears when its
//!   input drops, and the held fire action never drops while the other trigger is pulled. The
//!   split keeps a pull per hand instead: a semi-automatic weapon fires each hand's pull once, the
//!   older pull first, and the weapon's own flag (with its cancel flag and its buffered shot) is
//!   cleared when a fresh pull is first presented. A fully automatic weapon fires the held hands in
//!   turn. The two guns share the weapon's fire interval, so a pull inside it fires when the
//!   interval ends, and a reload stops both.

use std::time::{Duration, Instant};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponInputs, WeaponState},
};
use parking_lot::Mutex;

use crate::controllers::{aim::gun, frame::Hand, hands::late};

/// One hand's trigger at an input tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TriggerInput {
    /// Pulled past the press threshold at some point in the tick.
    pub held: bool,
    /// Pulled from released during the tick.
    pub pressed: bool,
}

/// A hand's pull that has not fired yet.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Pending {
    /// The order the pulls began in.
    order: u64,
    /// Whether the weapon has been handed this pull.
    presented: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
struct HandTrigger {
    held: bool,
    pending: Option<Pending>,
}

/// The hand the split hands the weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Choice {
    pub hand: Hand,
    /// Whether this is a pull the weapon has not been handed before, so that its own
    /// fired-this-pull state belongs to another pull.
    pub fresh: bool,
}

/// The per-hand pulls. Pure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Split {
    hands: [HandTrigger; 2],
    next_order: u64,
    last_fired: Option<Hand>,
    last_chosen: Option<Hand>,
}

impl Split {
    pub const fn new() -> Self {
        Self {
            hands: [HandTrigger {
                held: false,
                pending: None,
            }; 2],
            next_order: 0,
            last_fired: None,
            last_chosen: None,
        }
    }

    /// Fold in one input tick of a hand's trigger: a new pull begins at a press edge (or when the
    /// trigger is first seen held), and a released trigger drops its pull.
    pub fn observe(&mut self, hand: Hand, input: TriggerInput) {
        let h = &mut self.hands[hand.index()];
        if !input.held {
            *h = HandTrigger::default();
            return;
        }
        if input.pressed || !h.held {
            h.pending = Some(Pending {
                order: self.next_order,
                presented: false,
            });
            self.next_order += 1;
        }
        h.held = true;
    }

    /// Choose the hand to hand the weapon this update: for a semi-automatic weapon, the hand with
    /// the oldest pull that has not fired; for a fully automatic one, a held hand, taking turns
    /// while both are held. `None` when no hand qualifies.
    pub fn choose(&mut self, fully_automatic: bool) -> Option<Choice> {
        let hand = if fully_automatic {
            let held = Hand::BOTH.map(|h| self.hands[h.index()].held);
            match held {
                [true, true] => Some(self.last_fired.map_or(Hand::Right, Hand::other)),
                [true, false] => Some(Hand::Left),
                [false, true] => Some(Hand::Right),
                [false, false] => None,
            }
        } else {
            Hand::BOTH
                .into_iter()
                .filter_map(|h| self.hands[h.index()].pending.map(|p| (h, p.order)))
                .min_by_key(|&(_, order)| order)
                .map(|(h, _)| h)
        };
        let Some(hand) = hand else {
            self.last_chosen = None;
            return None;
        };
        let pending = &mut self.hands[hand.index()].pending;
        let unpresented = pending.is_some_and(|p| !p.presented);
        if let Some(p) = pending {
            p.presented = true;
        }
        let fresh = self.last_chosen != Some(hand) || unpresented;
        self.last_chosen = Some(hand);
        Some(Choice { hand, fresh })
    }

    /// A shot from `hand`'s gun: its pull has fired.
    pub fn on_shot(&mut self, hand: Hand) {
        self.hands[hand.index()].pending = None;
        self.last_fired = Some(hand);
    }
}

/// The split's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct DualWieldStatus {
    /// Whether the split acted on the last input tick.
    pub active: bool,
    /// Each hand's trigger at the last input tick, indexed by [`Hand::index`].
    pub triggers: [TriggerInput; 2],
    /// The gun each hand's trigger fires, and how many shots it has fired.
    pub guns: [Option<usize>; 2],
    pub shots: [u64; 2],
    /// The hand the last update handed the weapon.
    pub chosen: Option<Hand>,
    /// Whether the guns' attach bones are swapped from the game's layout.
    pub swapped: bool,
}

pub fn status() -> DualWieldStatus {
    STATE.lock().status
}

/// Fold in the input tick's triggers. `active` is whether the split acts this tick: the toggle, the
/// bindings injecting, and both hands holding a pistol of the dual-wield slot.
pub(crate) fn observe_triggers(active: bool, triggers: [TriggerInput; 2]) {
    let mut state = STATE.lock();
    state.status.active = active;
    state.status.triggers = triggers;
    if !active {
        state.split = Split::new();
        state.active_at = None;
        return;
    }
    for hand in Hand::BOTH {
        state.split.observe(hand, triggers[hand.index()]);
    }
    state.active_at = Some(Instant::now());
}

/// Hand the weapon the chosen hand's pull, for the local player's dual-wielded weapon while the
/// split acts. Returns the inputs to update the weapon with in place of `input`, or `None` to leave
/// the update alone.
///
/// # Safety
///
/// `weapon` must be live, called from its per-tick update before the original runs.
pub(crate) unsafe fn before_update(
    weapon: &mut WeaponBase,
    input: &WeaponInputs,
) -> Option<WeaponInputs> {
    if !(weapon.m_IsUserLocalPlayer
        && weapon.m_EquipSlot == WeaponSlot::E_SLOT_DUAL_WIELD
        && weapon.m_State == WeaponState::E_WS_DUAL_WIELDING)
    {
        return None;
    }
    let mut state = STATE.lock();
    if !state
        .active_at
        .is_some_and(|at| at.elapsed() < MAX_INPUT_AGE)
    {
        return None;
    }
    // The game's own fire action is held while either trigger is pulled, and it reaches the weapon
    // as zero whenever shooting is not allowed.
    if input.m_PrimaryFireInput.is_nan() || input.m_PrimaryFireInput <= 0.0 {
        state.status.chosen = None;
        return None;
    }
    let guns = unsafe { gun_by_hand(weapon) };
    state.status.guns = guns;
    let fully_automatic =
        unsafe { weapon.m_Tuning.as_ref() }.is_some_and(|tuning| tuning.m_fully_automatic);
    let choice = state
        .split
        .choose(fully_automatic)
        .and_then(|c| guns[c.hand.index()].map(|gun| (c, gun)));
    state.status.chosen = choice.map(|(c, _)| c.hand);
    let Some((choice, gun)) = choice else {
        return Some(WeaponInputs {
            m_PrimaryFireInput: 0.0,
            ..*input
        });
    };
    if choice.fresh {
        weapon.m_FiredThisInput = false;
        weapon.m_IsFiringCancelled = false;
        weapon.m_InputBufferTimer = 0.0;
    }
    weapon.m_NextWeaponToFire = gun as u8;
    Some(*input)
}

/// A shot from `hand`'s gun of the player's weapon, from the per-shot seam.
pub(crate) fn on_shot(hand: Hand) {
    let mut state = STATE.lock();
    if state.active_at.is_none() {
        return;
    }
    state.split.on_shot(hand);
    state.status.shots[hand.index()] += 1;
}

/// Put gun 0 of the player's dual-wield slot in `dominant`'s hand and gun 1 in the other while
/// `active`; otherwise put back the game's layout if the split changed it.
///
/// # Safety
///
/// `owner` must be the live local player character, on the main thread outside the world update.
pub(crate) unsafe fn place(owner: &Character, dominant: Hand, active: bool) {
    let weapon = owner
        .m_Inventory
        .m_Weapons
        .get(WeaponSlot::E_SLOT_DUAL_WIELD as usize)
        .map_or(std::ptr::null_mut(), |w| w.px);
    let mut state = STATE.lock();
    let Some(weapon) = (unsafe { weapon.as_mut() }) else {
        state.swapped = None;
        state.status.swapped = false;
        return;
    };
    let address = weapon as *mut WeaponBase as usize;
    if state.swapped.is_some_and(|a| a != address) {
        // The slot holds another weapon now; the one swapped before is no longer the player's.
        state.swapped = None;
    }
    let swapped = state.swapped.is_some();
    let needs_swap = if active {
        let first = unsafe { dual_bones(weapon) }.and_then(|[b0, _]| late::attach_hand(b0));
        first.is_some_and(|hand| hand != dominant)
    } else {
        swapped
    };
    if needs_swap && unsafe { swap_dual_bones(weapon) } {
        state.swapped = if swapped { None } else { Some(address) };
        tracing::debug!(target: "controllers", restored = swapped, "dual wield: swapped the pistols' hands");
    }
    state.status.swapped = state.swapped.is_some();
}

/// How long the input tick's triggers stay current for the weapon update.
const MAX_INPUT_AGE: Duration = Duration::from_millis(250);

struct State {
    split: Split,
    active_at: Option<Instant>,
    /// The weapon whose attach bones are swapped from the game's layout.
    swapped: Option<usize>,
    status: DualWieldStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    split: Split::new(),
    active_at: None,
    swapped: None,
    status: DualWieldStatus {
        active: false,
        triggers: [TriggerInput {
            held: false,
            pressed: false,
        }; 2],
        guns: [None; 2],
        shots: [0; 2],
        chosen: None,
        swapped: false,
    },
});

/// The gun each hand holds in the dual-wielding state, indexed by [`Hand::index`].
unsafe fn gun_by_hand(weapon: &WeaponBase) -> [Option<usize>; 2] {
    let mut guns = [None; 2];
    for (index, gun) in unsafe { gun::guns(weapon) }.enumerate() {
        let hand = gun.and_then(|g| {
            g.m_lBoneAttachementPerWeaponState
                .get(WeaponState::E_WS_DUAL_WIELDING as usize)
                .and_then(|&bone| late::attach_hand(bone))
        });
        if let Some(hand) = hand {
            guns[hand.index()].get_or_insert(index);
        }
    }
    guns
}

/// The first two guns' attach bones for the dual-wielding state.
unsafe fn dual_bones(weapon: &WeaponBase) -> Option<[u32; 2]> {
    let mut guns = unsafe { gun::guns(weapon) };
    let bone = |gun: Option<Option<&jc3gi::equipment::weapon::WeaponData>>| {
        gun.flatten().and_then(|g| {
            g.m_lBoneAttachementPerWeaponState
                .get(WeaponState::E_WS_DUAL_WIELDING as usize)
                .copied()
        })
    };
    Some([bone(guns.next())?, bone(guns.next())?])
}

/// Swap the first two guns' dual-wielding attach bones, when both are hand bones on different
/// hands. Returns whether they were swapped.
unsafe fn swap_dual_bones(weapon: &mut WeaponBase) -> bool {
    let Some([a, b]) = (unsafe { dual_bones(weapon) }) else {
        return false;
    };
    let (Some(ha), Some(hb)) = (late::attach_hand(a), late::attach_hand(b)) else {
        return false;
    };
    if ha == hb {
        return false;
    }
    let list = &weapon.m_lWeaponData;
    let slot = WeaponState::E_WS_DUAL_WIELDING as usize;
    for (index, bone) in [(0, b), (1, a)] {
        // SAFETY: `dual_bones` found both guns, so the list holds at least two live entries.
        let gun = unsafe { (*list.m_Begin.add(index)).as_mut() };
        if let Some(entry) = gun.and_then(|g| g.m_lBoneAttachementPerWeaponState.get_mut(slot)) {
            *entry = bone;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull() -> TriggerInput {
        TriggerInput {
            held: true,
            pressed: true,
        }
    }

    fn hold() -> TriggerInput {
        TriggerInput {
            held: true,
            pressed: false,
        }
    }

    #[test]
    fn a_semi_automatic_pull_fires_once() {
        let mut split = Split::new();
        split.observe(Hand::Right, pull());
        let c = split.choose(false).unwrap();
        assert_eq!(c.hand, Hand::Right);
        assert!(c.fresh);
        split.on_shot(Hand::Right);
        split.observe(Hand::Right, hold());
        assert_eq!(split.choose(false), None);
        split.observe(Hand::Right, TriggerInput::default());
        split.observe(Hand::Right, pull());
        assert_eq!(split.choose(false).map(|c| c.hand), Some(Hand::Right));
    }

    #[test]
    fn the_other_hand_fires_while_the_first_is_still_held() {
        let mut split = Split::new();
        split.observe(Hand::Right, pull());
        split.choose(false);
        split.on_shot(Hand::Right);
        split.observe(Hand::Right, hold());
        split.observe(Hand::Left, pull());
        let c = split.choose(false).unwrap();
        assert_eq!(c.hand, Hand::Left);
        assert!(c.fresh, "a switch of hands resets the weapon's pull");
    }

    #[test]
    fn a_pull_waiting_out_the_fire_interval_is_kept_and_not_fresh_again() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        assert!(split.choose(false).unwrap().fresh);
        split.observe(Hand::Left, hold());
        let again = split.choose(false).unwrap();
        assert_eq!(again.hand, Hand::Left);
        assert!(!again.fresh);
    }

    #[test]
    fn the_older_of_two_pulls_fires_first() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        split.observe(Hand::Right, pull());
        assert_eq!(split.choose(false).unwrap().hand, Hand::Left);
        split.on_shot(Hand::Left);
        assert_eq!(split.choose(false).unwrap().hand, Hand::Right);
    }

    #[test]
    fn fully_automatic_fire_takes_turns() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        split.observe(Hand::Right, pull());
        let first = split.choose(true).unwrap().hand;
        split.on_shot(first);
        let second = split.choose(true).unwrap().hand;
        assert_ne!(first, second);
        split.on_shot(second);
        assert_eq!(split.choose(true).unwrap().hand, first);
        split.observe(Hand::Left, TriggerInput::default());
        split.on_shot(Hand::Right);
        assert_eq!(split.choose(true).unwrap().hand, Hand::Right);
    }

    #[test]
    fn a_release_drops_the_pull() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        split.observe(Hand::Left, TriggerInput::default());
        assert_eq!(split.choose(false), None);
        assert_eq!(split.choose(true), None);
    }
}
