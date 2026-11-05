//! C4 by hand: the charge thrown from the chest pouch, and the detonation from the wrist button,
//! with the engine side on [`PlantedExplosiveWeapon`] and [`PlantedExplosive`].
//!
//! A grip press at the C4 pouch primes a charge in whichever hand grabbed it, and opening the
//! fingers ([`crate::controllers::weapons::opening`]) throws it with that hand's velocity; it
//! sticks where it lands. The game's own C4 weapon does the
//! work, driven directly rather than through its planting mode (which equips the weapon, aims a
//! placement ghost with the camera, and places the charge at the aimed surface with an animation):
//!
//! - Before the weapon's serial update, the stage asks it to prime
//!   ([`PlantedExplosiveWeapon::PrimeExplosive`]), which spawns the charge as the game's planting
//!   does. Priming moves the weapon to its wielding state; the state it had is put back, so the
//!   weapon still reads as holstered to everything else.
//! - While the charge is held, the weapon's attachment update places its in-hand model and its
//!   transform at the hand instead of the character's right-hand bone.
//! - Once the grip opens and the charge's physics body exists, the weapon makes it live
//!   ([`PlantedExplosiveWeapon::Fire_Projectile`]), which counts it among the placed charges, and
//!   the charge is then dropped from the hand ([`PlantedExplosive::DropExplosive`]): the path the
//!   game takes for a charge placed under the parachute, from the hand instead of the right-hand
//!   bone. Its body then takes the motion a gun's let-go gives a gun
//!   ([`crate::controllers::weapons::grenade::throw_release`]), linear and angular, through the
//!   same owned-body setter. The base explosive's collision handler sticks it to what it hits.
//!
//! The selected explosive is never touched: the throw does not need the C4 weapon selected or
//! equipped. A charge is refused while as many are placed as may be
//! ([`PlantedExplosiveWeapon::CanPlantExplosive`]), while the game does not allow planting
//! ([`Inventory::GetAnimationAllowsPlantingExplosives`]), and while the game's own planting holds
//! the weapon. When the throw stops acting, a held charge is taken back
//! ([`PlantedExplosiveWeapon::DespawnPrimedExplosive`]); priming consumes nothing, so nothing is
//! refunded. A held charge pressed back onto the pouch
//! ([`crate::controllers::weapons::pouch_return`]) is taken back the same way: a primed charge is
//! not yet among the placed ones, so it holds no place against the limit, the detonator does not
//! count it, and once removed nothing can detonate it.
//!
//! Detonation goes through the game's own detonate input
//! ([`WeaponInput::E_INPUT_DETONATE_EXPLOSIVES`]): a request ([`request_detonation`], from the
//! wrist button in [`wrist`]) raises it for the weapon's serial update, which detonates every
//! placed charge as the game's own detonation does, until the weapon starts the detonation or the
//! request times out. The stick click's `DETONATE_EXPLOSIVE` reaches the same input through the
//! game's own hold rule.
//!
//! A held charge pressed onto a surface sticks there instead of being thrown ([`stick`]): it is
//! made live the same way, and placed just off the surface and pushed onto it, so that its own
//! collision sticks it. The surface the charge is coming to is marked where it would stick
//! ([`preview`]).
//!
//! The weapon wheel's explosives put the game in its planting mode, whose plant input the
//! controller layout leaves unbound. A free hand takes the charge instead ([`wheel`]): the
//! planting mode is left with the game's own holster ([`Character::HolsterWeapon`]), and once it
//! has ended the charge comes into that hand as one from the pouch does. A charge from the wheel
//! and one from the pouch are then the same charge.
//!
//! Every step is felt: a primed charge pulses its hand, a thrown one pulses the hand it leaves, a
//! grab the game refuses buzzes the grabbing hand (and is logged at info level), and a detonation
//! the weapon starts, from either source, pulses both hands and flashes the wrist button.
//!
//! [`Inventory::GetAnimationAllowsPlantingExplosives`]: jc3gi::character::inventory::Inventory::GetAnimationAllowsPlantingExplosives
//! [`Character::HolsterWeapon`]: jc3gi::character::character::Character::HolsterWeapon

pub mod press;
pub mod stage;
pub mod stick;
pub mod tuning;
pub mod wheel;
pub mod wrist;

use std::{ptr::NonNull, time::Instant};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::{
        character::{Character, WeaponInput},
        inventory::WeaponSlot,
    },
    equipment::{
        explosive::{Explosive, PlantedExplosive},
        weapon::{PlantedExplosiveWeapon, WeaponBase, WeaponData, WeaponInputs, WeaponState},
    },
    types::math::{Matrix4, Vector3},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        aim::targets::weak_target,
        frame::{Hand, Pose},
        hands::held::{self, Held, Slot},
        haptics::{self, Event},
        loadout::{ObjectAddress, live},
        motion, physical_hands, scheme,
        weapons::{
            c4::{
                stage::{InputEvent, Outcome, Refusal, Stage, WeaponAction, WeaponView},
                stick::{self as stick_by_hand, StickAt},
                tuning::{C4, C4Tuning},
                wheel::{WheelHands, WheelStep},
            },
            engine,
            grenade::{Release, catch::body_of, throw_release},
            pouch_return,
        },
    },
};

/// The C4 handling's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_FROM_USER_SERIAL_BINDER)
        .with_static_binder(&UPDATE_ATTACHMENT_TRANSFORMS_BINDER)
}

/// One reported step of a throw, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct C4Event {
    pub hand: Option<Hand>,
    pub outcome: Outcome,
}

/// What asked for a detonation.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DetonationSource {
    /// The wrist button.
    WristButton,
    /// The game's own detonate input: the stick click's `DETONATE_EXPLOSIVE`, held.
    GameInput,
}

impl DetonationSource {
    pub const fn label(self) -> &'static str {
        match self {
            DetonationSource::WristButton => "wrist button",
            DetonationSource::GameInput => "detonate button",
        }
    }
}

/// A detonation the weapon started, or a request it did not act on.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Detonation {
    pub source: DetonationSource,
    /// Whether the weapon started it; `false` for a request that timed out.
    pub started: bool,
    /// How many charges were placed when it started or timed out.
    pub charges: i32,
}

/// The C4 handling's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct C4Status {
    pub stage: Stage,
    /// Whether the C4 weapon's update ran for the local player since the payload loaded.
    pub weapon_seen: bool,
    /// The placed charges, and the most that may be placed, at the weapon's last update.
    pub charges: Option<(i32, i32)>,
    /// The game's permissions to plant and to detonate, at the weapon's last update.
    pub may_plant: bool,
    pub may_detonate: bool,
    /// Whether the weapon is in the game's own planting mode.
    pub planting_mode: bool,
    /// The last release's velocity relative to the character, world axes, metres per second.
    pub last_release: Option<Vec3>,
    pub primed: u64,
    pub thrown: u64,
    /// Charges pressed onto a surface by hand, of the thrown ones, and how many of those onto an
    /// object rather than the world's geometry.
    pub stuck: u64,
    pub stuck_on_objects: u64,
    /// Charges pressed back onto the pouch.
    pub returned: u64,
    /// The most recent throw steps, newest last.
    pub events: Vec<C4Event>,
    /// Whether a detonation request waits for the weapon.
    pub detonation_pending: bool,
    pub detonations: u64,
    pub last_detonation: Option<Detonation>,
}

pub fn status() -> C4Status {
    STATE.lock().status.clone()
}

/// Whether the C4 pouch has a charge to give: fewer charges are placed than may be, as the weapon's
/// last update read it, or it has not been read yet. A pouch with nothing to give is not offered.
pub(crate) fn in_stock() -> bool {
    STATE
        .lock()
        .status
        .charges
        .is_none_or(|(live, max)| live < max)
}

/// The placed charges at the weapon's last update, or `None` before it ran.
pub fn charges() -> Option<i32> {
    STATE.lock().status.charges.map(|(live, _)| live)
}

/// The placed charges and the most that may be placed at once, at the weapon's last update, or
/// `None` before it ran.
pub fn placed() -> Option<(i32, i32)> {
    STATE.lock().status.charges
}

/// Remove `owner`'s placed charges without detonating them, through the weapon's own removal
/// ([`PlantedExplosiveWeapon::RemoveAllExplosives`]), the one the game runs when a mission resets
/// or a character is disabled. A charge held in a hand is primed, not placed, so it stays. Returns
/// how many were removed, or `None` when `owner` has no C4 weapon.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread before the world update.
pub(crate) unsafe fn remove_placed(owner: &mut Character) -> Option<i32> {
    let weapon = unsafe { slot_c4(owner) }?;
    let removed = unsafe { weapon.NumActiveExplosives() };
    unsafe { weapon.RemoveAllExplosives() };
    let (live, max) = unsafe { (weapon.NumActiveExplosives(), weapon.MaxActiveExplosives()) };
    STATE.lock().status.charges = Some((live, max));
    Some(removed - live)
}

/// The hand a throw holds, from the grab at the pouch until the charge leaves it.
pub fn throwing_hand() -> Option<Hand> {
    STATE.lock().stage.hand()
}

/// The charge a hand holds, for the loadout table: the throw's primed charge in its hand.
pub(crate) fn held() -> Option<(Hand, usize)> {
    let state = STATE.lock();
    let hand = state
        .stage
        .hand()
        .filter(|_| state.stage.charge_in_hand())?;
    Some((hand, state.primed?))
}

/// The charge each hand holds that could go back into the pouch, by its address: a primed charge
/// the hand has not let go of.
pub(crate) fn returnable() -> [Option<usize>; 2] {
    let state = STATE.lock();
    let mut out = [None; 2];
    if let Stage::Held {
        hand,
        release: None,
    } = state.stage
    {
        out[hand.index()] = state.primed;
    }
    out
}

/// Pass the held charge from the other hand to `to`, as a held grenade or a gun is passed: the
/// loadout table moves the charge first, and the throw then belongs to `to`, reading its motion
/// from now. `false`, with nothing changed, when the other hand holds no charge or the table
/// refuses the move.
pub(crate) fn hand_over(to: Hand) -> bool {
    let Some((_, object)) = held().filter(|&(hand, _)| hand == to.other()) else {
        return false;
    };
    let Some(object) = ObjectAddress::from_address(object) else {
        return false;
    };
    if !live::hand_over(to, object) {
        return false;
    }
    let mut state = STATE.lock();
    state.stage = stage::on_input(state.stage, InputEvent::HandOver(to));
    state.held_since_s = motion::latest_time();
    state.preview = None;
    state.status.stage = state.stage;
    tracing::info!(target: "controllers", hand = to.label(), "c4: the charge passed to the other hand");
    true
}

/// The hands that threw or pressed on a charge since the last call, by [`Hand::index`].
pub(crate) fn take_thrown() -> [bool; 2] {
    std::mem::take(&mut STATE.lock().thrown)
}

/// The hand holding a charge that may be pressed onto a surface now, with the hand's grip in the
/// world: the charge is held, has been out of the pouch long enough, and the hand has not opened.
pub(crate) fn stick_candidate(tuning: &C4Tuning) -> Option<(Hand, Pose)> {
    let state = STATE.lock();
    let Stage::Held { hand, release } = state.stage else {
        return None;
    };
    let held_s = state.held_since?.elapsed().as_secs_f32();
    stick_by_hand::may_stick(held_s, release.is_some(), &tuning.stick)
        .then(|| state.grips[hand.index()].map(|grip| (hand, grip)))
        .flatten()
}

/// The surface the held charge is coming to, where it would stick if pressed on: the hand and the
/// contact, from the last aim update.
pub(crate) fn preview() -> Option<(Hand, stick_by_hand::Contact)> {
    STATE.lock().preview
}

/// Note the surface the held charge is coming to.
pub(crate) fn publish_preview(preview: Option<(Hand, stick_by_hand::Contact)>) {
    STATE.lock().preview = preview;
}

/// Press the charge `hand` holds onto a surface: the next weapon update places it at `at` and
/// pushes it on, as a release that throws it would. Nothing happens if the hand no longer holds a
/// charge it has not let go of.
pub(crate) fn press(hand: Hand, at: StickAt) {
    let mut state = STATE.lock();
    if state.stage
        != (Stage::Held {
            hand,
            release: None,
        })
    {
        return;
    }
    state.stage = stage::on_input(
        state.stage,
        InputEvent::Release(
            hand,
            Release {
                velocity: at.push,
                angular: Vec3::ZERO,
            },
        ),
    );
    state.status.stage = state.stage;
    state.stick = Some(at);
    tracing::info!(target: "controllers", ?hand, on_object = at.on_object, "c4: pressed the charge onto a surface");
}

/// Ask the weapon to detonate every placed charge, as the game's own detonation does.
pub(crate) fn request_detonation(source: DetonationSource) {
    let mut state = STATE.lock();
    state.detonation = Some(PendingDetonation {
        source,
        remaining_s: DETONATION_TIMEOUT_S,
    });
    state.status.detonation_pending = true;
}

/// What the input tick hands the throw.
pub(super) struct TickInputs {
    /// Whether the throw may act this tick: its toggle is on and the mode has free hands.
    pub acting: bool,
    /// The hand that grabbed the C4 pouch this tick.
    pub pouch_grab: Option<Hand>,
    /// The hands whose fingers opened this tick, indexed by [`Hand::index`].
    pub opened: [bool; 2],
    /// The hands holding nothing, by [`Hand::index`], for a charge from the weapon wheel.
    pub free: [bool; 2],
    /// The dominant hand, which takes a charge from the weapon wheel when it is free.
    pub dominant: Hand,
}

/// The input tick's half of the throw: fold the grab and the release into the stage.
pub(super) fn input_tick(inputs: &TickInputs, tuning: &C4Tuning) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let mut state = STATE.lock();
    if let Some(hand) = inputs.pouch_grab {
        state.stage = stage::on_input(state.stage, InputEvent::Grab(hand));
    }
    let mut returned = None;
    for (hand, pressed) in Hand::BOTH.into_iter().zip(pouch_return::take_charges()) {
        let before = state.stage;
        if pressed {
            state.stage = stage::on_input(before, InputEvent::Return(hand));
        }
        if state.stage != before {
            state.status.returned += 1;
            state.preview = None;
            returned = Some(hand);
            tracing::info!(target: "controllers", hand = hand.label(), "c4: put the charge back into the pouch");
        }
    }
    let planting = state.status.planting_mode;
    let hands = WheelHands {
        free: inputs.free,
        dominant: inputs.dominant,
    };
    match state.step_wheel(hands, Instant::now()) {
        WheelStep::Leave(hand) => {
            drop(state);
            // SAFETY: the game stage runs on the game thread before the world update.
            if let Some(owner) = unsafe { engine::local_player() } {
                unsafe { engine::holster(owner) };
            }
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                "c4: the weapon wheel's explosives leave the planting mode for a charge in the hand"
            );
            state = STATE.lock();
        }
        WheelStep::Take(_) | WheelStep::Wait => {}
        WheelStep::Drop => {
            tracing::info!(
                target: "controllers",
                planting,
                "c4: the weapon wheel's charge is not taken: no hand is free, or the planting mode did not end"
            );
        }
    }
    if let Some(hand) = state.stage.hand()
        && inputs.opened[hand.index()]
    {
        // Without a tracked sample of the throw, the charge is dropped where it is rather than kept
        // in a hand that has opened.
        let release = state.grips[hand.index()].map_or(Release::REST, |grip| {
            let point =
                held::held_matrix(grip, hold_local(hand, tuning)).transform_point3(tuning.centre_m);
            throw_release(
                hand,
                point,
                state.held_since_s.unwrap_or(f64::INFINITY),
                "c4",
                None,
            )
        });
        state.stage = stage::on_input(state.stage, InputEvent::Release(hand, release));
        state.status.last_release = Some(release.velocity);
        tracing::info!(
            target: "controllers",
            hand = hand.label(),
            speed_mps = release.velocity.length(),
            "c4: the fingers opened, and the charge leaves the hand"
        );
    }
    state.status.stage = state.stage;
    drop(state);
    if let Some(hand) = returned {
        haptics::play(hand, Event::PouchReturn);
    }
}

/// Stop the throw: a held charge is taken back at the weapon's next update, and a grab not yet
/// served is forgotten. A detonation request stands: it was asked for before the stop.
pub(crate) fn stand_down() {
    STATE.lock().stand_down();
}

/// Cancel only work owned by a hand whose input is unavailable, including a pending wheel charge.
pub(crate) fn input_unavailable(unavailable: [bool; 2]) {
    STATE.lock().input_unavailable(unavailable);
}

/// Note the controllers' grips in the world for the weapon's updates: where a held charge sits and
/// where a throw leaves from. `None` for a hand the aim update did not compose.
pub(crate) fn observe_hands(grips: [Option<Pose>; 2]) {
    let mut state = STATE.lock();
    state.grips = grips;
    // Without the hands, no surface is probed.
    if grips == [None; 2] {
        state.preview = None;
    }
}

/// How long a detonation request waits for the weapon to start the detonation, seconds: long
/// enough for the character to finish an act that keeps the detonate act out.
const DETONATION_TIMEOUT_S: f32 = 0.5;

/// How many throw steps the readout keeps.
const MAX_EVENTS: usize = 6;

/// A detonation request waiting for the weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
struct PendingDetonation {
    source: DetonationSource,
    remaining_s: f32,
}

struct State {
    stage: Stage,
    /// The primed charge in the throw's hand, as the weapon's last update left it.
    primed: Option<usize>,
    /// The hands that threw or pressed on a charge since the last read, by [`Hand::index`].
    thrown: [bool; 2],
    status: C4Status,
    /// The controllers' grips in the world, from the last aim update.
    grips: [Option<Pose>; 2],
    detonation: Option<PendingDetonation>,
    /// Whether the weapon had started a detonation on its last update.
    was_detonating: bool,
    /// When the held charge came out of the pouch.
    held_since: Option<Instant>,
    /// The same moment in display time, from which its throw reads the hand's motion.
    held_since_s: Option<f64>,
    /// Where a charge pressed onto a surface leaves from, until it is placed.
    stick: Option<StickAt>,
    /// The surface the held charge is coming to, with its hand.
    preview: Option<(Hand, stick_by_hand::Contact)>,
    /// The hand a charge from the weapon wheel is for, and when the planting mode was left for it.
    wheel: Option<WheelRequest>,
}

#[derive(Clone, Copy)]
enum WheelRequest {
    Pending { hand: Hand, since: Instant },
    Cancelled,
}

impl State {
    fn stand_down(&mut self) {
        self.stage = stage::on_input(self.stage, InputEvent::StandDown);
        self.status.stage = self.stage;
        self.wheel = None;
        self.preview = None;
    }

    fn input_unavailable(&mut self, unavailable: [bool; 2]) {
        if self
            .stage
            .hand()
            .is_some_and(|hand| unavailable[hand.index()])
        {
            self.stage = stage::on_input(self.stage, InputEvent::StandDown);
            self.status.stage = self.stage;
            self.preview = None;
        }
        if matches!(self.wheel, Some(WheelRequest::Pending { hand, .. }) if unavailable[hand.index()])
        {
            self.wheel = Some(WheelRequest::Cancelled);
        }
    }

    fn step_wheel(&mut self, hands: WheelHands, now: Instant) -> WheelStep {
        let pending = match self.wheel {
            Some(WheelRequest::Cancelled) => {
                if !self.status.planting_mode {
                    self.wheel = None;
                }
                return WheelStep::Wait;
            }
            Some(WheelRequest::Pending { hand, since }) => {
                Some((hand, now.saturating_duration_since(since).as_secs_f32()))
            }
            None => None,
        };
        let step = wheel::step(self.status.planting_mode, self.stage, pending, hands);
        match step {
            WheelStep::Leave(hand) => self.wheel = Some(WheelRequest::Pending { hand, since: now }),
            WheelStep::Take(hand) => {
                self.wheel = None;
                self.stage = stage::on_input(self.stage, InputEvent::Grab(hand));
            }
            WheelStep::Drop => self.wheel = None,
            WheelStep::Wait => {}
        }
        self.status.stage = self.stage;
        step
    }
}

#[cfg(test)]
mod input_tests;

static STATE: Mutex<State> = Mutex::new(State {
    stage: Stage::Idle,
    primed: None,
    thrown: [false; 2],
    status: C4Status {
        stage: Stage::Idle,
        weapon_seen: false,
        charges: None,
        may_plant: false,
        may_detonate: false,
        planting_mode: false,
        last_release: None,
        primed: 0,
        thrown: 0,
        stuck: 0,
        stuck_on_objects: 0,
        returned: 0,
        events: Vec::new(),
        detonation_pending: false,
        detonations: 0,
        last_detonation: None,
    },
    grips: [None; 2],
    detonation: None,
    was_detonating: false,
    held_since: None,
    held_since_s: None,
    stick: None,
    preview: None,
    wheel: None,
});

/// The held charge's in-hand model in `hand`'s grip frame.
pub(crate) fn hold_local(hand: Hand, tuning: &C4Tuning) -> Pose {
    tuning.hold.in_hand(hand)
}

/// The held charge's world matrix in a hand whose grip is `grip`.
fn held_matrix(grip: Pose, local: Pose) -> Matrix4 {
    Matrix4::from(held::held_matrix(grip, local))
}

/// Record a throw step's outcome.
fn report(state: &mut State, hand: Option<Hand>, outcome: Outcome) {
    tracing::debug!(target: "controllers", ?hand, outcome = %outcome.label(), "c4");
    match outcome {
        Outcome::Primed => state.status.primed += 1,
        Outcome::Thrown(_) => {
            state.status.thrown += 1;
            if let Some(hand) = hand {
                state.thrown[hand.index()] = true;
            }
        }
        _ => {}
    }
    state.status.events.push(C4Event { hand, outcome });
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

/// The C4 weapon's serial update from its user: the throw's action and the detonation request
/// before it, and both settled after it.
#[detour(address = jc3gi::equipment::weapon::PlantedExplosiveWeapon::UpdateFromUserPostSim_Serial_ADDRESS)]
fn update_from_user_serial(
    weapon: *mut PlantedExplosiveWeapon,
    dt: f32,
    aim_position: *const Vector3,
    input: *const WeaponInputs,
) {
    let original = move || {
        UPDATE_FROM_USER_SERIAL
            .get()
            .unwrap()
            .call(weapon, dt, aim_position, input)
    };
    // SAFETY: the weapon is live for its own update.
    let Some(user) = (unsafe { local_user(weapon) }) else {
        return original();
    };
    let (enabled, config) =
        Config::lock_query(|c| (c.controllers.enabled, c.controllers.weapons.c4));
    let tuning = C4Tuning::of(&config);
    let acting = enabled && scheme::is_controllers();

    // SAFETY: the weapon and its user are live, on the thread that updates the weapon; the
    // references end before the original update runs.
    let (reading, pulses, raised, previous) = unsafe {
        let (w, user) = (&mut *weapon, &mut *user);
        let reading = read_weapon(w, user);
        let mut state = STATE.lock();
        state.status.weapon_seen = true;
        state.status.charges = Some((reading.live, reading.max));
        state.status.may_plant = reading.may_plant;
        state.status.may_detonate = reading.may_detonate;
        state.status.planting_mode = reading.view.planting;
        let mut pulses = Vec::new();
        if acting || state.stage != Stage::Idle {
            let hand = state.stage.hand();
            let (next, action, outcome) = stage::before_update(state.stage, reading.view);
            if let Some(outcome) = outcome {
                report(&mut state, hand, outcome);
                pulses.extend(outcome_pulse(hand, outcome));
            }
            let grip = hand.and_then(|h| {
                state.grips[h.index()].map(|g| held_matrix(g, hold_local(h, &tuning)))
            });
            // A charge pressed onto a surface leaves from just off it, turned as the hand holds it.
            let stick = state
                .stick
                .filter(|_| matches!(action, WeaponAction::Throw(_)));
            let grip = match (stick, grip) {
                (Some(at), Some(grip)) => Some(with_translation(grip, at.position)),
                (Some(at), None) => {
                    Some(with_translation(Matrix4::from(Mat4::IDENTITY), at.position))
                }
                (None, grip) => grip,
            };
            let primed = act(w, user, action, grip, stick.is_some());
            let charge = primed_charge(w).map(|charge| charge as usize);
            let (next, outcome) = stage::after_action(next, action, primed);
            state.stage = next;
            state.status.stage = next;
            state.primed = charge.filter(|_| next.charge_in_hand());
            if let Some(outcome) = outcome {
                report(&mut state, hand, outcome);
                pulses.extend(outcome_pulse(hand, outcome));
                if outcome == Outcome::Primed {
                    state.held_since = Some(Instant::now());
                    state.held_since_s = motion::latest_time();
                }
                if let (Outcome::Thrown(_), Some(at)) = (outcome, stick) {
                    state.status.stuck += 1;
                    state.status.stuck_on_objects += u64::from(at.on_object);
                    tracing::info!(target: "controllers", ?hand, on_object = at.on_object, "c4: stuck the charge where it was pressed");
                    if let Some(hand) = hand {
                        pulses.push((hand, Event::C4Stick));
                    }
                }
            }
            if !next.charge_in_hand() {
                state.stick = None;
                state.held_since = None;
                state.held_since_s = None;
            }
        }
        // The detonate input is raised for the weapon's update only, and the game's own value is
        // put back after it.
        let raised = state.detonation.is_some() && acting;
        let previous = user.m_WeaponInputs[DETONATE_INPUT];
        if raised {
            user.m_WeaponInputs[DETONATE_INPUT] = previous.max(1.0);
        }
        (reading, pulses, raised, previous)
    };

    original();

    // SAFETY: as above, after the original update.
    let detonating = unsafe {
        if raised {
            (*user).m_WeaponInputs[DETONATE_INPUT] = previous;
        }
        (*weapon).m_AlreadyDetonatedOnThisInput
    };
    let mut state = STATE.lock();
    let started = detonating && !state.was_detonating;
    state.was_detonating = detonating;
    if started || state.detonation.is_some() {
        settle_detonation(&mut state, started, reading.live, dt);
    }
    drop(state);
    for (hand, event) in pulses {
        haptics::play(hand, event);
    }
    if started {
        wrist::note_detonation();
        haptics::play_hands(Hand::BOTH, Event::C4Detonate);
    }
}

/// The pulse a throw step's `outcome` gives the throwing `hand`: a prime and a throw pulse it, and
/// a refusal buzzes it and is logged at info level.
fn outcome_pulse(hand: Option<Hand>, outcome: Outcome) -> Option<(Hand, Event)> {
    let event = match outcome {
        Outcome::Primed => Event::C4Prime,
        Outcome::Thrown(_) => Event::C4Throw,
        Outcome::Refused(_) | Outcome::SpawnFailed => {
            tracing::info!(target: "controllers", ?hand, outcome = %outcome.label(), "c4 pouch");
            Event::PouchRefused
        }
        Outcome::Gone | Outcome::Cancelled => return None,
    };
    Some((hand?, event))
}

/// The character's detonate input's slot in its weapon inputs.
const DETONATE_INPUT: usize = WeaponInput::E_INPUT_DETONATE_EXPLOSIVES as usize;

/// Settle the detonation after the weapon's update: a started detonation is credited to the
/// pending request, or to the game's own input without one; a request that has waited too long is
/// dropped.
fn settle_detonation(state: &mut State, started: bool, charges: i32, dt: f32) {
    let pending = state.detonation.take();
    let event = if started {
        Some(Detonation {
            source: pending.map_or(DetonationSource::GameInput, |p| p.source),
            started: true,
            charges,
        })
    } else if let Some(mut pending) = pending {
        pending.remaining_s -= if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        if pending.remaining_s > 0.0 {
            state.detonation = Some(pending);
            None
        } else {
            Some(Detonation {
                source: pending.source,
                started: false,
                charges,
            })
        }
    } else {
        None
    };
    state.status.detonation_pending = state.detonation.is_some();
    if let Some(event) = event {
        tracing::debug!(target: "controllers", ?event, "c4 detonation");
        if event.started {
            state.status.detonations += 1;
        }
        state.status.last_detonation = Some(event);
    }
}

/// The weapon's attachment update: a charge held by the throw rides the hand rather than the
/// character's right-hand bone.
#[detour(address = jc3gi::equipment::weapon::PlantedExplosiveWeapon::UpdateAttachmentTransforms_ADDRESS)]
fn update_attachment_transforms(
    weapon: *mut PlantedExplosiveWeapon,
    p2: f32,
    p3: f32,
    transform_type: u32,
) {
    UPDATE_ATTACHMENT_TRANSFORMS
        .get()
        .unwrap()
        .call(weapon, p2, p3, transform_type);
    // SAFETY: the weapon is live for its own update.
    if unsafe { local_user(weapon) }.is_none() {
        return;
    }
    let (stage, grips) = {
        let state = STATE.lock();
        (state.stage, state.grips)
    };
    let Some(hand) = stage.hand().filter(|_| stage.charge_in_hand()) else {
        held::hold(Slot::Charge, None);
        return;
    };
    let Some(grip) = grips[hand.index()] else {
        held::hold(Slot::Charge, None);
        return;
    };
    let local = hold_local(hand, &C4);
    let matrix = held_matrix(grip, local);
    // SAFETY: the primed charge is live while the weak reference locks, on this thread.
    let charge = unsafe { primed_charge(&*weapon) };
    // The tick's place moves the charge's transform; its in-hand model is drawn at the hand's
    // render-time grip.
    held::hold(
        Slot::Charge,
        charge.map(|charge| Held {
            explosive: charge as usize,
            hand,
            local,
        }),
    );
    if let Some(charge) = charge {
        unsafe { place(charge, &matrix) };
    }
}

/// The weapon's user, when the weapon is the local player's.
///
/// # Safety
///
/// `weapon` must be null or a live C4 weapon.
unsafe fn local_user(weapon: *mut PlantedExplosiveWeapon) -> Option<*mut Character> {
    let base = unsafe { weapon.cast::<WeaponBase>().as_ref() }?;
    if !base.m_IsUserLocalPlayer {
        return None;
    }
    let user = base.m_User[0].cast::<Character>();
    (!user.is_null()).then_some(user)
}

/// What the serial update reads of the weapon before acting.
struct WeaponReading {
    view: WeaponView,
    live: i32,
    max: i32,
    may_plant: bool,
    may_detonate: bool,
}

/// Read the weapon and its user's permissions.
///
/// # Safety
///
/// The weapon and its user must be live, on the thread that updates the weapon.
unsafe fn read_weapon(weapon: &mut PlantedExplosiveWeapon, user: &Character) -> WeaponReading {
    let primed = unsafe { primed_charge(weapon) };
    let physics_ready = primed
        .and_then(|c| unsafe { c.cast::<Explosive>().as_ref() })
        .is_some_and(|e| !e.m_PfxInstance.is_null());
    let live = unsafe { weapon.NumActiveExplosives() };
    let max = unsafe { weapon.MaxActiveExplosives() };
    let may_plant = unsafe { user.m_Inventory.GetAnimationAllowsPlantingExplosives() };
    let may_detonate = unsafe { user.m_Inventory.GetAnimationAllowsTriggeringExplosives() };
    let refusal = if !may_plant {
        Some(Refusal::NotAllowed)
    } else if live >= max {
        Some(Refusal::NoRoom)
    } else {
        None
    };
    WeaponReading {
        view: WeaponView {
            primed: primed.is_some(),
            physics_ready,
            planting: weapon.m_IsInPlantingMode,
            refusal,
        },
        live,
        max,
        may_plant,
        may_detonate,
    }
}

/// Carry out `action` on the weapon, with the held charge at `grip` when the hand is known, and
/// return whether the weapon holds a primed charge afterwards.
///
/// # Safety
///
/// The weapon and its user must be live, on the thread that updates the weapon.
unsafe fn act(
    weapon: &mut PlantedExplosiveWeapon,
    user: &mut Character,
    action: WeaponAction,
    grip: Option<Matrix4>,
    pressed: bool,
) -> bool {
    match action {
        WeaponAction::None => {}
        WeaponAction::Prime => {
            if let Some(data) = unsafe { first_weapon_data(weapon) } {
                // Priming puts the weapon in its wielding state; everything else keeps reading it
                // in the state it was in.
                let base = unsafe { &mut *std::ptr::from_mut(weapon).cast::<WeaponBase>() };
                let state = base.m_State;
                unsafe { weapon.PrimeExplosive(data) };
                let base = unsafe { &mut *std::ptr::from_mut(weapon).cast::<WeaponBase>() };
                if state != WeaponState::E_WS_WIELDING {
                    base.m_State = state;
                }
                if let (Some(charge), Some(grip)) = (unsafe { primed_charge(weapon) }, grip) {
                    unsafe { place(charge, &grip) };
                }
            }
        }
        WeaponAction::Throw(release) => unsafe { throw(weapon, user, release, grip, pressed) },
        WeaponAction::Cancel => unsafe { weapon.DespawnPrimedExplosive() },
    }
    unsafe { primed_charge(weapon) }.is_some()
}

/// Make the primed charge live and drop it from the hand at `grip` (or where it is) with the
/// release's motion. A thrown release already carries the player's movement; a press's push is
/// relative to the character, so it takes the character's velocity as well.
///
/// # Safety
///
/// As [`act`], with a primed charge whose physics body exists.
unsafe fn throw(
    weapon: &mut PlantedExplosiveWeapon,
    user: &mut Character,
    release: Release,
    grip: Option<Matrix4>,
    pressed: bool,
) {
    let Some(data) = (unsafe { first_weapon_data(weapon) }) else {
        return;
    };
    let Some(charge) = (unsafe { primed_charge(weapon) }) else {
        return;
    };
    // A thrown charge is drawn where it flies from at once, not at the hand.
    held::hold(Slot::Charge, None);
    // Under the parachute the weapon drops the charge itself, from the right-hand bone.
    let parachute = unsafe { user.IsUsingParachute() };
    let carried = Vec3::from_array(user.m_MeasuredWorldVelocity.data);
    unsafe { weapon.Fire_Projectile(data) };
    // The weapon made this charge live only if it moved it to the last placed one.
    let placed = unsafe { weak_target(&weapon.m_LastPlantedExplosive) }.is_some_and(|object| {
        object.cast::<u8>().wrapping_sub(GAME_OBJECT_OFFSET) == charge.cast::<u8>()
    });
    if !placed {
        return;
    }
    // SAFETY: the charge is live: the weapon's last placed charge, on this thread.
    let explosive = unsafe { &mut *charge.cast::<Explosive>() };
    if let Some(grip) = grip {
        explosive.m_WorldMatrixT1 = grip;
        unsafe { explosive.SetPhysicsTransform(&grip) };
    }
    let charge = unsafe { &mut *charge };
    if !parachute {
        unsafe { charge.DropExplosive() };
    }
    let velocity = release.velocity
        + if pressed && carried.is_finite() {
            carried
        } else {
            Vec3::ZERO
        };
    // The dropped charge's body is in the simulation, so it is set as a let-go gun's is; the
    // engine's own setter, which sets only the linear part, is the fallback.
    let explosive = unsafe { &*std::ptr::from_mut(charge).cast::<Explosive>() };
    let set = unsafe { body_of(explosive) }.is_some_and(|(body, owner)| {
        unsafe { physical_hands::set_body_velocities(body, owner, velocity, release.angular) }
            .inspect_err(|error| {
                tracing::debug!(target: "controllers", %error, "c4: the charge's body refused the release motion");
            })
            .is_ok()
    });
    if !set {
        let velocity = Vector3 {
            data: velocity.to_array(),
        };
        unsafe { charge.SetVelocity(&velocity) };
    }
    if !parachute {
        let explosive = unsafe { &mut *std::ptr::from_mut(charge).cast::<Explosive>() };
        unsafe { explosive.PlayTrailEffect() };
    }
}

/// `matrix` with its translation replaced by `position`.
fn with_translation(matrix: Matrix4, position: Vec3) -> Matrix4 {
    let mut m = Mat4::from(matrix);
    m.w_axis = position.extend(1.0);
    Matrix4::from(m)
}

/// Put a held charge's in-hand model and its transform at `matrix`.
///
/// # Safety
///
/// `charge` must be live, on the thread that updates its weapon.
unsafe fn place(charge: *mut PlantedExplosive, matrix: &Matrix4) {
    unsafe {
        (*charge).SetInactiveModelMatrix(matrix);
        (*charge.cast::<Explosive>()).m_WorldMatrixT1 = *matrix;
    }
}

/// The offset of a charge's `CGameObject` subobject, which the weapon's weak references point at.
const GAME_OBJECT_OFFSET: usize = Explosive::GAME_OBJECT_BASE_OFFSET as usize;

/// The weapon's primed charge, while its weak reference is live.
///
/// # Safety
///
/// `weapon` must be a live C4 weapon, on the thread that updates it.
unsafe fn primed_charge(weapon: &PlantedExplosiveWeapon) -> Option<*mut PlantedExplosive> {
    let object = unsafe { weak_target(&weapon.m_PrimedExplosive) }?;
    Some(
        object
            .cast::<u8>()
            .wrapping_sub(GAME_OBJECT_OFFSET)
            .cast::<PlantedExplosive>(),
    )
}

/// The C4 weapon in `owner`'s planted-explosive slot, when the slot holds one.
///
/// # Safety
///
/// `owner` must be the live local player character, on the game thread.
unsafe fn slot_c4(owner: &mut Character) -> Option<&mut PlantedExplosiveWeapon> {
    let slot = WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE as usize;
    let weapon = unsafe { owner.m_Inventory.m_Weapons.get(slot)?.px.as_mut() }?;
    if !unsafe { (*engine::object_of(weapon)).IsType(PlantedExplosiveWeapon::TYPE_ID()) } {
        return None;
    }
    Some(unsafe { &mut *std::ptr::from_mut(weapon).cast::<PlantedExplosiveWeapon>() })
}

/// The weapon's first (and only) gun entry.
///
/// # Safety
///
/// `weapon` must be a live C4 weapon.
unsafe fn first_weapon_data(weapon: &mut PlantedExplosiveWeapon) -> Option<*mut WeaponData> {
    let base = unsafe { &*std::ptr::from_mut(weapon).cast::<WeaponBase>() };
    unsafe { engine::gun_ptr(base, 0) }.map(NonNull::as_ptr)
}

#[cfg(test)]
mod tests;
