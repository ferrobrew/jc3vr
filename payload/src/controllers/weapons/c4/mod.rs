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
//!   the charge is then dropped from the hand ([`PlantedExplosive::DropExplosive`]) with the hand's
//!   velocity plus the character's ([`PlantedExplosive::SetVelocity`]): the path the game takes for
//!   a charge placed under the parachute, from the hand instead of the right-hand bone. The base
//!   explosive's collision handler sticks it to what it hits.
//!
//! The selected explosive is never touched: the throw does not need the C4 weapon selected or
//! equipped. A charge is refused while as many are placed as may be
//! ([`PlantedExplosiveWeapon::CanPlantExplosive`]), while the game does not allow planting
//! ([`Inventory::GetAnimationAllowsPlantingExplosives`]), and while the game's own planting holds
//! the weapon. When the throw stops acting, a held charge is taken back
//! ([`PlantedExplosiveWeapon::DespawnPrimedExplosive`]); priming consumes nothing, so nothing is
//! refunded.
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

pub mod config;
pub mod press;
pub mod stage;
pub mod stick;
pub mod wheel;
pub mod wrist;

use std::time::Instant;

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{Character, WeaponInput},
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
        motion::{self, ReleaseEstimate},
        scheme,
        weapons::{
            c4::{
                stage::{InputEvent, Outcome, Refusal, Stage, WeaponAction, WeaponView},
                stick::{self as stick_by_hand, StickAt},
                wheel::{WheelHands, WheelStep},
            },
            config::C4Config,
            engine,
            grenade::stage::Release,
        },
    },
    headpose,
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

/// The hands that threw or pressed on a charge since the last call, by [`Hand::index`].
pub(crate) fn take_thrown() -> [bool; 2] {
    std::mem::take(&mut STATE.lock().thrown)
}

/// The hand holding a charge that may be pressed onto a surface now, with the hand's grip in the
/// world: the charge is held, has been out of the pouch long enough, and the hand has not opened.
pub(crate) fn stick_candidate(tuning: &C4Config) -> Option<(Hand, Pose)> {
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
        InputEvent::Release(hand, Release { velocity: at.push }),
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
    /// The VR world scale the hands compose with.
    pub world_scale: f32,
}

/// The input tick's half of the throw: fold the grab and the release into the stage.
pub(super) fn input_tick(inputs: &TickInputs, tuning: &C4Config) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let mut state = STATE.lock();
    if let Some(hand) = inputs.pouch_grab {
        state.stage = stage::on_input(state.stage, InputEvent::Grab(hand));
    }
    let planting = state.status.planting_mode;
    let pending = state
        .wheel
        .map(|(hand, since)| (hand, since.elapsed().as_secs_f32()));
    let hands = WheelHands {
        free: inputs.free,
        dominant: inputs.dominant,
    };
    match wheel::step(planting, state.stage, pending, hands) {
        WheelStep::Leave(hand) => {
            state.wheel = Some((hand, Instant::now()));
            drop(state);
            // SAFETY: the input tick runs on the main thread outside the world update.
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
        WheelStep::Take(hand) => {
            state.wheel = None;
            state.stage = stage::on_input(state.stage, InputEvent::Grab(hand));
        }
        WheelStep::Wait => {}
        WheelStep::Drop => {
            state.wheel = None;
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
        let release = release(hand, inputs.world_scale, tuning).unwrap_or(Release {
            velocity: Vec3::ZERO,
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
}

/// Stop the throw: a held charge is taken back at the weapon's next update, and a grab not yet
/// served is forgotten. A detonation request stands: it was asked for before the stop.
pub(super) fn stand_down() {
    let mut state = STATE.lock();
    state.stage = stage::on_input(state.stage, InputEvent::StandDown);
    state.status.stage = state.stage;
    state.wheel = None;
    state.preview = None;
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
    /// Where a charge pressed onto a surface leaves from, until it is placed.
    stick: Option<StickAt>,
    /// The surface the held charge is coming to, with its hand.
    preview: Option<(Hand, stick_by_hand::Contact)>,
    /// The hand a charge from the weapon wheel is for, and when the planting mode was left for it.
    wheel: Option<(Hand, Instant)>,
}

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
        events: Vec::new(),
        detonation_pending: false,
        detonations: 0,
        last_detonation: None,
    },
    grips: [None; 2],
    detonation: None,
    was_detonating: false,
    held_since: None,
    stick: None,
    preview: None,
    wheel: None,
});

/// The release of a throw by `hand`: the velocity of the held charge's point over the samples just
/// before now, turned into world axes and scaled. `None` without samples.
fn release(hand: Hand, world_scale: f32, tuning: &C4Config) -> Option<Release> {
    let samples = motion::history(hand);
    let now = motion::latest_time()?;
    let velocity = motion::release_velocity(
        &samples,
        now,
        hold_offset(hand, tuning),
        ReleaseEstimate {
            window_s: tuning.release_window_s,
            peak_span_s: tuning.release_peak_span_s,
        },
    )?;
    let world = headpose::xr::body_rotation() * velocity * world_scale * tuning.velocity_scale;
    Some(Release {
        velocity: world.clamp_length_max(tuning.max_speed_mps.max(0.0)),
    })
}

/// The held charge's offset in `hand`'s grip frame: the configured right-hand offset, mirrored for
/// the left.
pub(crate) fn hold_offset(hand: Hand, tuning: &C4Config) -> Vec3 {
    let o = tuning.hold_offset_m;
    match hand {
        Hand::Left => Vec3::new(-o.x, o.y, o.z),
        Hand::Right => o,
    }
}

/// The held charge's world matrix: the grip's world pose moved to the charge's point.
fn held_matrix(grip: Pose, offset: Vec3) -> Matrix4 {
    Matrix4::from(held::held_matrix(grip, offset))
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
    let (enabled, tuning) =
        Config::lock_query(|c| (c.controllers.enabled, c.controllers.weapons.c4));
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
                state.grips[h.index()].map(|g| held_matrix(g, hold_offset(h, &tuning)))
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
            let primed = act(w, user, action, grip);
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
        for hand in Hand::BOTH {
            haptics::play(hand, Event::C4Detonate);
        }
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
    let offset = hold_offset(hand, &Config::lock_query(|c| c.controllers.weapons.c4));
    let matrix = held_matrix(grip, offset);
    // SAFETY: the primed charge is live while the weak reference locks, on this thread.
    let charge = unsafe { primed_charge(&*weapon) };
    // The tick's place moves the charge's transform; its in-hand model is drawn at the hand's
    // render-time grip.
    held::hold(
        Slot::Charge,
        charge.map(|charge| Held {
            explosive: charge as usize,
            hand,
            offset,
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
        WeaponAction::Throw(release) => unsafe { throw(weapon, user, release, grip) },
        WeaponAction::Cancel => unsafe { weapon.DespawnPrimedExplosive() },
    }
    unsafe { primed_charge(weapon) }.is_some()
}

/// Make the primed charge live and drop it from the hand at `grip` (or where it is) with the
/// release velocity plus the character's.
///
/// # Safety
///
/// As [`act`], with a primed charge whose physics body exists.
unsafe fn throw(
    weapon: &mut PlantedExplosiveWeapon,
    user: &mut Character,
    release: Release,
    grip: Option<Matrix4>,
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
        + if carried.is_finite() {
            carried
        } else {
            Vec3::ZERO
        };
    let velocity = Vector3 {
        data: velocity.to_array(),
    };
    unsafe { charge.SetVelocity(&velocity) };
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

/// The weapon's first (and only) gun entry.
///
/// # Safety
///
/// `weapon` must be a live C4 weapon.
unsafe fn first_weapon_data(weapon: &mut PlantedExplosiveWeapon) -> Option<*mut WeaponData> {
    let base = unsafe { &*std::ptr::from_mut(weapon).cast::<WeaponBase>() };
    let list = &base.m_lWeaponData;
    if list.m_Begin.is_null() || list.m_Begin >= list.m_End {
        return None;
    }
    let data = unsafe { *list.m_Begin };
    (!data.is_null()).then_some(data)
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn fresh() -> State {
        State {
            stage: Stage::Idle,
            primed: None,
            thrown: [false; 2],
            status: C4Status::default(),
            grips: [None; 2],
            detonation: None,
            was_detonating: false,
            held_since: None,
            stick: None,
            preview: None,
            wheel: None,
        }
    }

    #[test]
    fn a_started_detonation_is_credited_to_the_request_or_the_game() {
        let mut state = fresh();
        state.detonation = Some(PendingDetonation {
            source: DetonationSource::WristButton,
            remaining_s: DETONATION_TIMEOUT_S,
        });
        settle_detonation(&mut state, true, 3, 0.03);
        assert_eq!(
            state.status.last_detonation,
            Some(Detonation {
                source: DetonationSource::WristButton,
                started: true,
                charges: 3,
            })
        );
        assert!(state.detonation.is_none() && !state.status.detonation_pending);
        settle_detonation(&mut state, true, 2, 0.03);
        assert_eq!(
            state.status.last_detonation.map(|d| d.source),
            Some(DetonationSource::GameInput)
        );
        assert_eq!(state.status.detonations, 2);
    }

    #[test]
    fn an_unanswered_request_times_out() {
        let mut state = fresh();
        state.detonation = Some(PendingDetonation {
            source: DetonationSource::WristButton,
            remaining_s: DETONATION_TIMEOUT_S,
        });
        settle_detonation(&mut state, false, 1, 0.1);
        assert!(state.status.detonation_pending);
        for _ in 0..10 {
            settle_detonation(&mut state, false, 1, 0.1);
        }
        assert!(!state.status.detonation_pending);
        assert_eq!(
            state.status.last_detonation,
            Some(Detonation {
                source: DetonationSource::WristButton,
                started: false,
                charges: 1,
            })
        );
        assert_eq!(state.status.detonations, 0);
    }

    #[test]
    fn the_held_charge_sits_at_the_mirrored_offset() {
        let tuning = C4Config {
            hold_offset_m: Vec3::new(0.02, 0.0, -0.05),
            ..C4Config::new()
        };
        assert_eq!(
            hold_offset(Hand::Left, &tuning),
            Vec3::new(-0.02, 0.0, -0.05)
        );
        let grip = Pose {
            position: Vec3::new(1.0, 2.0, 3.0),
            orientation: Quat::IDENTITY,
        };
        let m = Mat4::from(held_matrix(grip, hold_offset(Hand::Right, &tuning)));
        assert!(m.w_axis.truncate().distance(Vec3::new(1.02, 2.0, 2.95)) < 1e-6);
    }
}
