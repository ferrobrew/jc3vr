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
//! - **Acting.** The split acts whenever the bindings inject and both pistols are in the hands, in
//!   every mode. The mode governs only which triggers fire them ([`triggers_fire_pistols`]): on
//!   foot, swimming, falling, under the parachute, and in a passenger seat each trigger fires its
//!   own pistol; elsewhere (the wingsuit, the driver's seat) neither does.
//! - **Input.** The bindings hold the game's fire action while either trigger is pulled, so the
//!   game's own permission to shoot still reaches the weapon. The off-hand's trigger then fires
//!   the off-hand gun rather than retracting the tethers; the tethers retract with the off-hand's
//!   stick click instead ([`crate::controllers::bindings::tables`]). The triggers' press edges are
//!   latched per hand at the input tick ([`observe_triggers`]).
//! - **Firing.** The game updates one gun per tick, the one at the weapon's next gun
//!   ([`WeaponBase::m_NextWeaponToFire`]), and points the index at the other gun after every shot,
//!   so left alone a held fire action fires both pistols in turn. At the weapon's per-tick update
//!   ([`before_update`], from the gun aim's detour on the base update), the split decides
//!   ([`Split::decide`]) and always pins the index: to the chosen hand's gun with that hand's pull,
//!   so the update fires that gun, from its own muzzle, at that hand's aim point; otherwise to a
//!   gun that is not firing, with the fire input withheld when no hand may fire. A buffered shot
//!   is dropped when no hand has a live pull, since it would come out of whichever gun is pinned.
//!   When the input tick has gone stale, the dominant gun is pinned and the input passed through,
//!   so the worst case is the dominant pistol alone firing. The weapon counts ammunition, heat,
//!   and recoil once per shot, whichever gun fires.
//! - **Semi-automatic fire.** The weapon keeps one fired-this-pull flag, which only clears when its
//!   input drops, and the held fire action never drops while the other trigger is pulled. The
//!   split keeps a pull per hand instead: a semi-automatic weapon fires each hand's pull once, the
//!   older pull first, and the weapon's own flag (with its cancel flag and its buffered shot) is
//!   cleared when a fresh pull is first presented. A fully automatic weapon fires the held hands in
//!   turn. Both triggers together therefore fire both pistols alternately, never in the same tick:
//!   the engine fires at most one gun per update, and the guns share the weapon's fire interval,
//!   trigger state, and magazine, so a second gun fired in the same tick would bypass the fire
//!   rate and the semi-automatic rule. A pull inside the interval fires when the interval ends, and
//!   a reload stops both.

use std::time::{Duration, Instant};

use jc3gi::{
    character::{character::Character, inventory::WeaponSlot},
    equipment::weapon::{WeaponBase, WeaponInputs, WeaponState},
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::Hand,
    hands::late,
    mode::{AirborneVariant, Mode},
};

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

/// What the weapon's update is handed through the fire action.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FireInput {
    /// The game's fire input, unchanged.
    Pass,
    /// No fire input: no hand may fire this update.
    Withhold,
}

/// The weapon's state at its per-tick update, for [`Split::decide`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct UpdateRequest {
    /// Whether the game's fire input is pulled: the fire action held and shooting allowed.
    pub pulled: bool,
    /// Whether the input tick's triggers are current.
    pub current: bool,
    pub fully_automatic: bool,
    /// The gun each hand holds, indexed by [`Hand::index`].
    pub guns: [Option<usize>; 2],
    pub dominant: Hand,
}

/// What to do to the weapon before its update.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The gun to point the weapon's next gun at.
    pub gun: usize,
    /// The hand whose pull is handed to the weapon, when one is.
    pub chosen: Option<Hand>,
    pub input: FireInput,
    /// Clear the weapon's fired-this-pull, cancel, and buffered-shot state: the pull handed over
    /// is not the one that state belongs to.
    pub reset_pull: bool,
    /// Clear the weapon's buffered shot: no hand has a live pull.
    pub clear_buffer: bool,
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

    /// Decide what the weapon's update this tick is handed. `None` only when neither hand holds a
    /// gun, so there is nothing to pin.
    pub fn decide(&mut self, request: UpdateRequest) -> Option<Decision> {
        let UpdateRequest {
            pulled,
            current,
            fully_automatic,
            guns,
            dominant,
        } = request;
        let gun_of = |hand: Hand| guns[hand.index()].or(guns[hand.other().index()]);
        let resting = |chosen, input, live: Option<Hand>, split: &Self| {
            let hand = live.or(split.last_fired).unwrap_or(dominant);
            gun_of(hand).map(|gun| Decision {
                gun,
                chosen,
                input,
                reset_pull: false,
                clear_buffer: live.is_none(),
            })
        };
        if !current {
            return gun_of(dominant).map(|gun| Decision {
                gun,
                chosen: None,
                input: FireInput::Pass,
                reset_pull: false,
                clear_buffer: false,
            });
        }
        if !pulled {
            let live = self.candidate(fully_automatic);
            return resting(None, FireInput::Pass, live, self);
        }
        match self.choose(fully_automatic) {
            Some(choice) => match guns[choice.hand.index()] {
                Some(gun) => Some(Decision {
                    gun,
                    chosen: Some(choice.hand),
                    input: FireInput::Pass,
                    reset_pull: choice.fresh,
                    clear_buffer: false,
                }),
                None => resting(None, FireInput::Withhold, None, self),
            },
            None => resting(None, FireInput::Withhold, None, self),
        }
    }

    /// Choose the hand to hand the weapon this update ([`Self::candidate`]), marking its pull as
    /// presented. `None` when no hand qualifies.
    pub fn choose(&mut self, fully_automatic: bool) -> Option<Choice> {
        let hand = self.candidate(fully_automatic);
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

    /// The hand with a live pull the weapon would be handed: for a semi-automatic weapon, the hand
    /// with the oldest pull that has not fired; for a fully automatic one, a held hand, taking
    /// turns while both are held.
    fn candidate(&self, fully_automatic: bool) -> Option<Hand> {
        if fully_automatic {
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
        }
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
    /// The gun the last update pinned as the weapon's next gun.
    pub pinned: Option<usize>,
    /// Whether the guns' attach bones are swapped from the game's layout.
    pub swapped: bool,
}

pub fn status() -> DualWieldStatus {
    STATE.lock().status
}

/// Whether the hands' triggers fire the hand weapons in `mode`, so that with a pistol in each hand
/// each trigger fires its own. In the wingsuit and the driver's seat the triggers drive the suit
/// or the vehicle.
pub const fn triggers_fire_pistols(mode: Mode) -> bool {
    match mode {
        Mode::OnFoot(_) => true,
        Mode::Airborne(variant) => !matches!(variant, AirborneVariant::Wingsuit),
        Mode::Vehicle(seat) => !seat.driver,
        Mode::Ui => false,
    }
}

/// One input tick's triggers while the split acts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct TriggerTick {
    pub dominant: Hand,
    /// Each hand's trigger as it fires a pistol, indexed by [`Hand::index`]: released in a mode
    /// whose triggers do not fire them.
    pub triggers: [TriggerInput; 2],
}

/// Fold in the input tick's triggers, or `None` when the split does not act this tick: the toggle
/// is off, the bindings do not inject, or the hands do not both hold a pistol of the dual-wield
/// slot.
pub(crate) fn observe_triggers(tick: Option<TriggerTick>) {
    let mut state = STATE.lock();
    state.status.active = tick.is_some();
    let Some(tick) = tick else {
        state.status.triggers = [TriggerInput::default(); 2];
        state.split = Split::new();
        state.active_at = None;
        return;
    };
    state.status.triggers = tick.triggers;
    for hand in Hand::BOTH {
        state.split.observe(hand, tick.triggers[hand.index()]);
    }
    state.dominant = tick.dominant;
    state.active_at = Some(Instant::now());
}

/// Pin the gun that may fire and hand the weapon its pull ([`Split::decide`]), for the local
/// player's dual-wielded weapon while the split acts. Returns the inputs to update the weapon with
/// in place of `input`, or `None` to leave the update alone.
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
    let at = state.active_at?;
    let guns = unsafe { gun_by_hand(weapon) };
    let fully_automatic =
        unsafe { weapon.m_Tuning.as_ref() }.is_some_and(|tuning| tuning.m_fully_automatic);
    // The game's own fire action is held while either trigger is pulled, and it reaches the weapon
    // as zero whenever shooting is not allowed.
    let primary = input.m_PrimaryFireInput;
    let request = UpdateRequest {
        pulled: primary.is_finite() && primary > 0.0,
        current: at.elapsed() < MAX_INPUT_AGE,
        fully_automatic,
        guns,
        dominant: state.dominant,
    };
    let decision = state.split.decide(request);
    state.status.guns = guns;
    state.status.chosen = decision.and_then(|d| d.chosen);
    state.status.pinned = decision.map(|d| d.gun);
    drop(state);
    let decision = decision?;
    let gun = u8::try_from(decision.gun).ok()?;
    if decision.reset_pull {
        weapon.m_FiredThisInput = false;
        weapon.m_IsFiringCancelled = false;
    }
    if decision.reset_pull || decision.clear_buffer {
        weapon.m_InputBufferTimer = 0.0;
    }
    weapon.m_NextWeaponToFire = gun;
    Some(match decision.input {
        FireInput::Pass => *input,
        FireInput::Withhold => WeaponInputs {
            m_PrimaryFireInput: 0.0,
            ..*input
        },
    })
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
    dominant: Hand,
    /// The weapon whose attach bones are swapped from the game's layout.
    swapped: Option<usize>,
    status: DualWieldStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    split: Split::new(),
    active_at: None,
    dominant: Hand::Right,
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
        pinned: None,
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
    use crate::controllers::mode::{OnFootVariant, VehicleClass, VehicleSeat};

    /// Gun 0 in the right hand, gun 1 in the left.
    const GUNS: [Option<usize>; 2] = [Some(1), Some(0)];

    fn request(pulled: bool, fully_automatic: bool) -> UpdateRequest {
        UpdateRequest {
            pulled,
            current: true,
            fully_automatic,
            guns: GUNS,
            dominant: Hand::Right,
        }
    }

    fn gun(hand: Hand) -> usize {
        GUNS[hand.index()].unwrap()
    }

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

    #[test]
    fn each_trigger_fires_only_its_own_gun() {
        let cases = [
            (false, false, None),
            (true, false, Some(Hand::Left)),
            (false, true, Some(Hand::Right)),
        ];
        for ((left, right, fires), fully_automatic) in cases
            .into_iter()
            .flat_map(|case| [(case, false), (case, true)])
        {
            let mut split = Split::new();
            split.observe(
                Hand::Left,
                TriggerInput {
                    held: left,
                    pressed: left,
                },
            );
            split.observe(
                Hand::Right,
                TriggerInput {
                    held: right,
                    pressed: right,
                },
            );
            for _ in 0..4 {
                let d = split
                    .decide(request(left || right, fully_automatic))
                    .unwrap();
                assert_eq!(d.chosen, fires);
                if let Some(hand) = fires {
                    assert_eq!(d.gun, gun(hand));
                    assert_eq!(d.input, FireInput::Pass);
                    split.on_shot(hand);
                    // A semi-automatic gun fires once per press, so each shot needs a new pull.
                    if !fully_automatic {
                        split.observe(hand, pull());
                    }
                }
            }
        }
    }

    #[test]
    fn a_semi_automatic_hold_fires_once_and_then_withholds_on_its_own_gun() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        let d = split.decide(request(true, false)).unwrap();
        assert_eq!(
            (d.chosen, d.gun, d.reset_pull),
            (Some(Hand::Left), gun(Hand::Left), true)
        );
        split.on_shot(Hand::Left);
        split.observe(Hand::Left, hold());
        let d = split.decide(request(true, false)).unwrap();
        assert_eq!(d.chosen, None);
        assert_eq!(d.input, FireInput::Withhold);
        assert_eq!(d.gun, gun(Hand::Left), "the gun that fired stays pinned");
        assert!(d.clear_buffer);
    }

    #[test]
    fn both_semi_automatic_triggers_fire_both_guns_in_turn() {
        let mut split = Split::new();
        split.observe(Hand::Right, pull());
        split.observe(Hand::Left, pull());
        let first = split.decide(request(true, false)).unwrap();
        assert_eq!(first.chosen, Some(Hand::Right));
        split.on_shot(Hand::Right);
        let second = split.decide(request(true, false)).unwrap();
        assert_eq!(
            (second.chosen, second.gun),
            (Some(Hand::Left), gun(Hand::Left))
        );
        assert!(
            second.reset_pull,
            "the left pull is not the one the weapon fired"
        );
        split.on_shot(Hand::Left);
        let third = split.decide(request(true, false)).unwrap();
        assert_eq!(third.input, FireInput::Withhold);
    }

    #[test]
    fn both_fully_automatic_triggers_alternate_the_guns() {
        let mut split = Split::new();
        split.observe(Hand::Right, pull());
        split.observe(Hand::Left, pull());
        let mut fired = Vec::new();
        for _ in 0..4 {
            let d = split.decide(request(true, true)).unwrap();
            let hand = d.chosen.unwrap();
            assert_eq!(d.gun, gun(hand));
            split.on_shot(hand);
            fired.push(hand);
        }
        assert_eq!(fired, [Hand::Right, Hand::Left, Hand::Right, Hand::Left]);
    }

    #[test]
    fn no_fire_input_pins_without_firing() {
        let mut split = Split::new();
        let d = split.decide(request(false, false)).unwrap();
        assert_eq!(
            (d.gun, d.chosen, d.input),
            (gun(Hand::Right), None, FireInput::Pass)
        );
        assert!(
            d.clear_buffer,
            "no pull is live, so a buffered shot has no owner"
        );

        split.observe(Hand::Left, pull());
        let d = split.decide(request(false, false)).unwrap();
        assert_eq!(
            d.gun,
            gun(Hand::Left),
            "a live pull keeps its gun for a buffered shot"
        );
        assert!(!d.clear_buffer);
        let d = split.decide(request(true, false)).unwrap();
        assert!(
            d.reset_pull,
            "a pull withheld by the game is still unpresented"
        );
    }

    #[test]
    fn the_last_gun_to_fire_stays_pinned_at_rest() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        split.decide(request(true, false));
        split.on_shot(Hand::Left);
        split.observe(Hand::Left, TriggerInput::default());
        let d = split.decide(request(false, false)).unwrap();
        assert_eq!(d.gun, gun(Hand::Left));
    }

    #[test]
    fn a_stale_tick_pins_the_dominant_gun_and_passes_the_input() {
        let mut split = Split::new();
        split.observe(Hand::Right, pull());
        for dominant in Hand::BOTH {
            let d = split
                .decide(UpdateRequest {
                    current: false,
                    dominant,
                    ..request(true, false)
                })
                .unwrap();
            assert_eq!(
                (d.gun, d.chosen, d.input),
                (gun(dominant), None, FireInput::Pass)
            );
        }
    }

    #[test]
    fn a_hand_without_a_gun_withholds_and_pins_the_gun_there_is() {
        let mut split = Split::new();
        split.observe(Hand::Left, pull());
        let d = split
            .decide(UpdateRequest {
                guns: [None, Some(0)],
                ..request(true, false)
            })
            .unwrap();
        assert_eq!((d.gun, d.input), (0, FireInput::Withhold));
        let none = split.decide(UpdateRequest {
            guns: [None; 2],
            ..request(true, false)
        });
        assert_eq!(none, None);
    }

    #[test]
    fn the_modes_whose_triggers_fire_the_pistols() {
        let seat = |driver| {
            Mode::Vehicle(VehicleSeat {
                class: VehicleClass::Car,
                driver,
            })
        };
        let cases = [
            (Mode::OnFoot(OnFootVariant::Ground), true),
            (Mode::OnFoot(OnFootVariant::Swimming), true),
            (Mode::Airborne(AirborneVariant::Falling), true),
            (Mode::Airborne(AirborneVariant::Parachute), true),
            (Mode::Airborne(AirborneVariant::Wingsuit), false),
            (seat(false), true),
            (seat(true), false),
            (Mode::Ui, false),
        ];
        for (mode, fires) in cases {
            assert_eq!(triggers_fire_pistols(mode), fires, "{mode:?}");
        }
    }
}
