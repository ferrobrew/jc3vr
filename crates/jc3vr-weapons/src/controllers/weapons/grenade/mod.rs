//! Independent safe grenades, their pins, and native live grenades.
//!
//! A pouch draw takes one grenade from the game's own stock ([`stock`]) and a native grenade from
//! the grenade weapon's own pool, left inert, so the item the player sees, carries, stows, and
//! drops is the engine's own object from that moment on. Transfers, holster moves, and releases
//! keep that item and its actor while its pin is in, and pressed back onto the pouch with its pin in
//! it returns to the stock. Pulling the pin starts the fuse ([`pin`]): a trigger press on the hand
//! holding it, the other hand's squeeze on the pin, or the teeth, with the grenade held at the mouth
//! while the other hand is occupied. Nothing else does, and a grenade thrown or dropped with its pin
//! in stays inert. Native live grenades can still be caught and thrown on.

mod beacon;
pub mod catch;
mod fuse_log;
mod locker;
mod native;
mod pin;
pub(crate) mod safe;
pub mod stock;
pub mod tuning;
mod world;

use std::{cell::Cell, num::NonZeroUsize, time::Duration};

use detours_macro::detour;
use glam::Vec3;
use jc3gi::{
    equipment::{
        explosive::{Explosive, Grenade},
        weapon::{ThrownGrenadeWeapon, WeaponBase, WeaponInputs, WeaponState},
    },
    input::input_action_map::Action,
    types::math::{Matrix4, Vector3},
};
use jc3vr_body::controllers::{
    aim::targets::weak_target,
    body::rig::frames::Frames,
    grab::{Candidate, GrabVolume, resolve::nearest_within},
    hands::{
        ActiveHands, held,
        held::{Held, Slot},
    },
    haptics, intent,
    intent::{
        controls::ButtonControl,
        hold::{Desired, MapKind},
    },
    physics,
    physics::{local_player::LocalPlayer, refs::Weak},
    telemetry,
    telemetry::Thrown,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{
    frame::{Hand, Pose},
    haptics::cues::Event,
    motion,
};
use jc3vr_view::controllers::overlay::{
    primitive::{Anchor, DepthMode, Facing, HandPose, Primitive, Shape, Source, SpriteSize},
    store,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    loadout::{ItemId, Place, TakeSource, live, safe_items::SafeItemView},
    weapons::{
        docks, engine,
        grenade::{
            catch::{CatchCounts, Caught, Sighting},
            native::PinPull,
            pin::{PinSource, TeethPull, TeethTick},
            stock::Stock,
            tuning::{GRENADE, GrenadeTuning},
        },
        hold::{act, tuning::HOLD},
        inventory::engine::weapon_of,
        pouch_return, reload,
    },
};

/// The grenade hooks, aggregated by the weapon controller.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_FROM_USER_BINDER)
        .with_static_binder(&GRENADE_PROPEL_TOWARD_BINDER)
        .with_hook_library(beacon::hook_library())
        .with_hook_library(native::hook_library())
        .with_hook_library(locker::hook_library())
        .with_hook_library(world::hook_library())
        .with_hook_library(fuse_log::hook_library())
}

/// Hook removal keeps the image mapped while a locker source callback remains active.
pub fn locker_active_entries() -> usize {
    locker::active_entries()
}

/// Observe a live world weapon during native placement or rendering for safe grenade sources.
///
/// # Safety
///
/// The caller runs the weapon's native pass and keeps it live for this call.
pub unsafe fn observe_world_weapon(weapon: &WeaponBase) {
    unsafe { locker::observe(weapon) };
}

/// The held live grenade's fuse.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Fuse {
    pub remaining_s: f32,
    pub total_s: f32,
}

/// The motion a release throws its item with, world axes: what a gun's let-go gives the gun
/// ([`throw_release`]).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Release {
    /// Metres per second, the player's own movement included.
    pub velocity: Vec3,
    /// Radians per second.
    pub angular: Vec3,
}

impl Release {
    pub const REST: Self = Self {
        velocity: Vec3::ZERO,
        angular: Vec3::ZERO,
    };
}

/// The release of the `item` `hand` holds at `point_world`, held since `since_s` (display time): the
/// estimate and the cap a gun's let-go takes ([`act::release_at`], the hold's
/// [`max_throw_mps`](crate::controllers::weapons::hold::tuning::HoldTuning::max_throw_mps)), so a
/// grenade, a charge, and a gun leave the hand with the same motion for the same swing. The release
/// is recorded as a throw of `item`, `armed` when the item has a live state.
pub(crate) fn throw_release(
    hand: Hand,
    point_world: Vec3,
    since_s: f64,
    item: &'static str,
    armed: Option<bool>,
) -> Release {
    let estimate = act::release_at(hand, point_world, since_s);
    let requested = estimate.capped(HOLD.get().hold.max_throw_mps);
    telemetry::note_throw_at(
        Thrown {
            hand,
            item,
            linear: requested.linear,
            angular: requested.angular,
            time_scale: estimate.time_scale.get(),
            source: Some(estimate.source.label()),
            armed,
        },
        point_world,
        None,
    );
    Release {
        velocity: requested.linear,
        angular: requested.angular,
    }
}

/// One reported native throw step.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrenadeEvent {
    pub hand: Option<Hand>,
    pub release: Release,
}

/// The grenade controller's readout. Safe held items have no live fuse.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GrenadeStatus {
    pub fuse: Option<Fuse>,
    pub last_release: Option<Vec3>,
    pub weapon_seen: bool,
    /// The game's grenade stock at the last input tick, which the pouch draws from and returns to.
    pub stock: Option<Stock>,
    /// Grenades pressed back onto the pouch and taken, and those the pouch refused because the
    /// stock was full.
    pub returned: u64,
    pub returns_refused: u64,
    pub primed: u64,
    pub thrown: u64,
    pub gone: u64,
    pub caught_hand: Option<Hand>,
    pub catch: CatchCounts,
    pub events: Vec<GrenadeEvent>,
}

/// Whether the pouch has a grenade to give: the game's stock holds one, or the game's unlimited
/// ammunition applies. A pouch with nothing to give is neither offered nor drawn.
pub(crate) fn in_stock() -> bool {
    stock::in_stock()
}

/// The grenade `hand` holds that could go back into the pouch: one of the loadout's with its pin
/// in.
pub(crate) fn returnable(hand: Hand) -> Option<ItemId> {
    safe::in_hand(hand)
        .filter(|_| safe::pin_in(hand))
        .map(|item| item.item)
}

pub fn status() -> GrenadeStatus {
    let mut status = STATE.lock().status.clone();
    status.catch = catch::counts();
    status.caught_hand = catch::caught().map(|caught| caught.hand);
    status
}

/// Native caught grenades only. Actor-free items are already owned by the loadout table.
pub(crate) fn held() -> Option<(Hand, usize, TakeSource)> {
    catch::caught().map(|caught| (caught.hand, caught.explosive, TakeSource::Catch))
}

/// The hands whose native grenades launched since the previous read.
pub(crate) fn take_thrown() -> [bool; 2] {
    let pouch = std::mem::take(&mut STATE.lock().thrown);
    let caught = catch::take_thrown();
    Hand::BOTH.map(|hand| pouch[hand.index()] || caught[hand.index()])
}

/// The grapple's snatch brought the inert grenade whose body is `body`, owned by `owner`, to
/// `hand`: the next input tick takes it into that hand through the loadout, as a hand closing on it
/// would. `false` when no inert grenade lying in the world has that body.
pub fn snatched(hand: Hand, owner: NonZeroUsize, body: u32) -> bool {
    let Some(item) = safe::inert_in_world(owner, body) else {
        return false;
    };
    STATE.lock().snatched[hand.index()] = Some(item);
    true
}

/// The grapple's snatch brought `weapon`, a finite native hand grenade lying in the world
/// ([`finite_world_pickup`]), to `hand`: the next input tick takes it into that hand, as a hand
/// closing on it does ([`take_world_pickup`]).
pub fn snatched_from_world(hand: Hand, weapon: Weak) {
    STATE.lock().snatched_pickups[hand.index()] = Some(weapon);
}

/// Whether `weapon` is a finite native hand grenade lying in the world, such as one a character
/// dropped: a grenade the snatch can bring to the hand and the world's drop replaces, rather than a
/// grenade on a display ([`WeaponBase::m_InfinitePickup`]), which stays where it is until a hand
/// takes it.
#[inline]
pub fn finite_world_pickup(weapon: &WeaponBase) -> bool {
    // Read as the raw value: a state the enum does not name is not dropped.
    let state = unsafe { (&raw const weapon.m_State).cast::<i32>().read() };
    let grenade = unsafe { engine::is_hand_grenade(weapon) };
    grenade && state == WeaponState::E_WS_DROPPED as i32 && !weapon.m_InfinitePickup
}

/// Take the finite native hand grenade `weapon` lying in the world into `hand` as one grenade: a
/// safe item is admitted to the hand ([`safe::draw`], under its allocation rules), and only then is
/// the native grenade consumed ([`consume_world_pickup`]), so a refused admission leaves it lying
/// and one native grenade always becomes one item.
///
/// # Safety
///
/// `weapon` must be held alive.
pub(super) unsafe fn take_world_pickup(
    game: &GameTick<'_>,
    hand: Hand,
    weapon: &mut WeaponBase,
    tuning: &GrenadeTuning,
) -> bool {
    if !finite_world_pickup(weapon) || holds(hand) {
        return false;
    }
    if !safe::draw(game, hand, tuning) {
        return false;
    }
    unsafe { consume_world_pickup(weapon) };
    true
}

/// Consume a native hand grenade lying in the world that a hand took: its rounds are emptied, so
/// the game's proximity collection can no longer credit them to the reserve, and it is despawned
/// with its bodies out of the physics world, so it no longer shows, collides, or offers itself.
///
/// # Safety
///
/// The game thread, before the world update; `weapon` must be held alive.
unsafe fn consume_world_pickup(weapon: &mut WeaponBase) {
    unsafe { weapon.SetMagazineAmmo(0) };
    weapon.m_StoredNonMagazineAmmo = 0;
    unsafe { weapon.Despawn() };
    unsafe { weapon.RemoveFromWorld() };
    tracing::info!(target: "controllers", weapon = weapon as *mut WeaponBase as usize, "grenade: a hand took a native grenade lying in the world");
}

/// Whether the physics game object `owner`'s body `body` is an inert grenade lying in the world,
/// which the snatch brings to the hand as a grenade rather than as a loose object.
#[inline]
pub fn inert_in_world(owner: NonZeroUsize, body: u32) -> bool {
    safe::inert_in_world(owner, body).is_some()
}

/// The inert grenades lying in the world, each with its physics game object, its body, and its
/// grab point in the world, for the snatch to offer near its aim as it offers weapons lying there.
#[inline]
pub fn inert_lying() -> Vec<(NonZeroUsize, u32, Vec3)> {
    safe::loose_bodies()
}

/// Whether `hand` holds a grenade from the pouch whose pin is still in.
#[inline]
pub fn pin_in_hand(hand: Hand) -> bool {
    safe::pin_in(hand)
}

/// Both hands can hold independent safe grenades.
pub fn holds(hand: Hand) -> bool {
    safe::in_hand(hand).is_some()
        || beacon::held().is_some_and(|(held, _)| held == hand)
        || catch::caught().is_some_and(|caught| caught.hand == hand)
}

pub(super) struct BeginInputs<'a> {
    pub active: &'a ActiveHands,
    pub free: [bool; 2],
    pub acting: bool,
}

/// Offer inert loose models and native live grenades to free hands.
pub(super) fn begin_tick(
    game: &GameTick<'_>,
    inputs: &BeginInputs,
    tuning: &GrenadeTuning,
) -> Vec<Candidate> {
    {
        let mut state = STATE.lock();
        state.sightings.clear();
        state.locker_frames = None;
        state.mouth = [None; 2];
    }
    let _ = stock::refresh(game);
    locker::clear();
    if !inputs.acting {
        return Vec::new();
    }
    let Some(mut owner) = LocalPlayer::get(game) else {
        return Vec::new();
    };
    let player = owner.frame();
    let Some(frames) = docks::cockpit_frames(inputs.active, player) else {
        return Vec::new();
    };
    STATE.lock().locker_frames = Some(frames);
    STATE.lock().mouth = at_mouth(inputs, tuning);
    let mut candidates = Vec::new();
    // SAFETY: the local player, in the game tick.
    candidates.extend(unsafe {
        locker::begin(
            owner.character_mut(),
            inputs.free,
            inputs
                .active
                .hands
                .map(|hand| hand.map(|hand| hand.grip.position)),
            |point| frames.cockpit.point_to_cockpit(point),
        )
    });
    for world in safe::loose() {
        for hand in Hand::BOTH {
            if inputs.free[hand.index()] && !holds(hand) {
                candidates.push(Candidate {
                    volume: GrabVolume::LiveGrenade,
                    centre: frames.cockpit.point_to_cockpit(world),
                    radius: tuning.catch.radius_m,
                    hand: Some(hand),
                });
            }
        }
    }
    if catch::caught().is_none() {
        let seen = unsafe {
            catch::sightings(player.w_axis.truncate(), &tuning.catch, |point| {
                frames.cockpit.point_to_cockpit(point)
            })
        };
        let free = Hand::BOTH.map(|hand| inputs.free[hand.index()] && !holds(hand));
        candidates.extend(catch::candidates(&seen, tuning.catch.radius_m, free));
        STATE.lock().sightings = seen;
    }
    candidates
}

/// The pins of the grenades in the hands, each offered to the other hand while that hand is free,
/// with the hand that holds the grenade: a squeeze there pulls the pin ([`pin`]).
pub(super) fn pin_candidates(
    inputs: &BeginInputs,
    tuning: &GrenadeTuning,
) -> Vec<(Hand, Candidate)> {
    if !inputs.acting {
        return Vec::new();
    }
    Hand::BOTH
        .into_iter()
        .filter(|&holder| inputs.free[holder.other().index()] && safe::pin_in(holder))
        .filter_map(|holder| {
            let grip = inputs.active.hands[holder.index()]?.grip;
            let point = pin::pin_point(grip, holder, inputs.active.world_scale, tuning)?;
            Some((holder, pin::candidate(holder, point, &tuning.pin)))
        })
        .collect()
}

pub(super) struct TickInputs {
    pub acting: bool,
    /// The hands whose grenade's pin the other hand squeezed this tick, by [`Hand::index`] of the
    /// hand holding the grenade.
    pub pin_grabs: [bool; 2],
    pub pouch_grab: Option<Hand>,
    pub locker_grabs: [bool; 2],
    pub catch_grab: Option<Hand>,
    pub cockpit_grips: [Option<Vec3>; 2],
    pub triggers: [ButtonControl; 2],
    pub dominant: Hand,
    pub opened: [bool; 2],
    pub dt: f32,
}

/// Consume the resolved pouch grab and grip opening in the actual input tick.
pub(super) fn input_tick(game: &GameTick<'_>, inputs: &TickInputs, desired: &mut Vec<Desired>) {
    if !inputs.acting {
        stand_down();
        return;
    }
    let tuning = GRENADE.get();
    let grips = STATE.lock().grips;
    beacon::input_tick(inputs, grips);
    for hand in Hand::BOTH {
        if inputs.locker_grabs[hand.index()] && !holds(hand) {
            let frames = STATE.lock().locker_frames;
            let result = frames
                .ok_or(locker::Refusal::SourceChanged)
                .and_then(|frames| {
                    locker::take(
                        game,
                        hand,
                        inputs.cockpit_grips[hand.index()],
                        |point| frames.cockpit.point_to_cockpit(point),
                        tuning,
                    )
                });
            match result {
                Ok(()) => haptics::play(hand, Event::GrenadePrime),
                Err(error) => {
                    tracing::debug!(target: "controllers", hand = hand.label(), %error, "grenade: locker draw refused");
                    haptics::play(hand, Event::PouchRefused);
                }
            }
        }
    }
    if let Some(hand) = inputs.pouch_grab
        && !holds(hand)
    {
        if draw_from_pouch(game, hand, tuning) {
            haptics::play(hand, Event::GrenadePrime);
        } else {
            haptics::play(hand, Event::PouchRefused);
        }
    }
    for (hand, pressed) in Hand::BOTH.into_iter().zip(pouch_return::take_grenades()) {
        if pressed {
            return_to_pouch(game, hand);
        }
    }
    world::input_tick(game);
    let pickups = std::mem::take(&mut STATE.lock().snatched_pickups);
    for (hand, weapon) in Hand::BOTH.into_iter().zip(pickups) {
        let Some(alive) = weapon.as_ref().and_then(Weak::lock) else {
            continue;
        };
        // SAFETY: the strong reference keeps the weapon alive.
        let took = unsafe { take_world_pickup(game, hand, &mut *weapon_of(&alive), tuning) };
        if took {
            haptics::play(hand, Event::GrenadeCatch);
        } else {
            tracing::debug!(target: "controllers", hand = hand.label(), "grenade: the snatched world grenade could not be taken into the hand");
        }
    }
    let snatched = std::mem::take(&mut STATE.lock().snatched);
    for (hand, item) in Hand::BOTH.into_iter().zip(snatched) {
        if let Some(item) = item
            && !holds(hand)
        {
            if safe::take_item(game, hand, item, tuning) {
                haptics::play(hand, Event::GrenadeCatch);
            } else {
                tracing::debug!(target: "controllers", hand = hand.label(), "grenade: the snatched grenade could not be taken into the hand");
            }
        }
    }
    if let Some(hand) = inputs.catch_grab
        && !holds(hand)
    {
        let frames = STATE.lock().locker_frames;
        let took_safe = frames
            .zip(inputs.cockpit_grips[hand.index()])
            .is_some_and(|(frames, grip)| safe::take_loose(game, hand, grip, &frames, tuning));
        if !took_safe
            && catch::caught().is_none()
            && let Some(grip) = inputs.cockpit_grips[hand.index()]
        {
            let seen = STATE.lock().sightings.clone();
            catch_one(hand, grip, &seen, tuning);
        }
    }
    let teeth = {
        let mut state = STATE.lock();
        let mouth = state.mouth;
        Hand::BOTH.map(|hand| {
            let seen = mouth[hand.index()];
            state.teeth[hand.index()].step(
                TeethTick {
                    pin_in: safe::pin_in(hand),
                    at_mouth: seen.is_some_and(|seen| seen.at_mouth),
                    other_occupied: seen.is_some_and(|seen| seen.other_occupied),
                    dt: inputs.dt,
                },
                &tuning.pin,
            )
        })
    };
    for hand in Hand::BOTH {
        let source = if inputs.triggers[hand.index()].pressed {
            PinSource::Trigger
        } else if inputs.pin_grabs[hand.index()] {
            PinSource::OtherHand
        } else if teeth[hand.index()] {
            PinSource::Teeth
        } else {
            continue;
        };
        // SAFETY: the game stage runs on the game thread before the world update.
        match unsafe { safe::pull_pin(hand) } {
            Some(PinPull::Pulled) => {
                haptics::play(hand, Event::GrenadePin);
                if source == PinSource::OtherHand {
                    haptics::play(hand.other(), Event::GrenadePin);
                }
                fuse_log::lit(hand, source);
            }
            Some(PinPull::NotReady) => {
                tracing::debug!(target: "controllers", hand = hand.label(), by = source.label(), "grenade: the pin stays in until the grenade has its physics body");
            }
            Some(PinPull::AlreadyOut) | None => {
                if source != PinSource::Trigger {
                    tracing::debug!(target: "controllers", hand = hand.label(), by = source.label(), "grenade: no pin to pull");
                }
            }
        }
    }
    for hand in Hand::BOTH {
        if inputs.opened[hand.index()] {
            safe::opened(game, hand);
        }
    }
    if Hand::BOTH.into_iter().any(holds) {
        desired.retain(|desired| {
            !(desired.target.map == MapKind::Player
                && desired.target.action == Action::THROW_GRENADE)
        });
    }
    fuse_log::observe(grips);
    // SAFETY: the game stage runs on the game thread before the world update.
    let caught = unsafe { carry_caught(inputs, tuning) };
    let urgent = most_urgent(
        caught
            .into_iter()
            .chain(Hand::BOTH.into_iter().filter_map(|hand| {
                safe::fuse_in_hand(hand).map(|(remaining_s, total_s)| {
                    (
                        hand,
                        Fuse {
                            remaining_s,
                            total_s,
                        },
                    )
                })
            })),
    );
    let mut state = STATE.lock();
    state.status.fuse = urgent.map(|(_, fuse)| fuse);
    state.status.stock = stock::last();
    let (mut primitives, pulse) = warning(&mut state, urgent, inputs.dt).unwrap_or_default();
    drop(state);
    if primitives.is_empty() {
        store::clear(Source::GrenadeWarning);
    } else {
        let ttl = Duration::from_millis(tuning.warning.ttl_ms);
        store::submit(Source::GrenadeWarning, &mut primitives, ttl);
    }
    if let Some(hand) = pulse {
        haptics::play(hand, Event::GrenadeWarning);
    }
}

/// Cancel only the unavailable hands' throws before masked grips can look like finger opening.
pub fn input_unavailable(unavailable: [bool; 2]) {
    beacon::input_unavailable(unavailable);
    safe::stand_down_hands(unavailable);
    if catch::caught().is_some_and(|caught| unavailable[caught.hand.index()]) {
        catch::let_go(false);
    }
}

/// A loss of input releases safe items inertly and leaves a caught grenade's fuse unchanged.
pub(super) fn stand_down() {
    beacon::stand_down();
    locker::clear();
    world::clear();
    safe::stand_down();
    let mut state = STATE.lock();
    state.next_pulse_s = 0.0;
    state.sightings.clear();
    state.locker_frames = None;
    state.snatched = [None; 2];
    state.snatched_pickups = [None, None];
    state.mouth = [None; 2];
    state.teeth = [TeethPull::NEW; 2];
    state.status.fuse = None;
    drop(state);
    if catch::caught().is_some() {
        catch::let_go(false);
    }
    store::clear(Source::GrenadeWarning);
}

/// Note tracked controller world poses for safe rendering and live catches.
pub fn observe_hands(grips: [Option<Pose>; 2]) {
    STATE.lock().grips = grips;
    safe::observe(grips, GRENADE.get());
}

/// Draw the items' native actors at the frame's interpolation fraction `dtf`; their own render
/// updates show them.
///
/// # Safety
///
/// The game thread, before the world update.
pub unsafe fn render_frame(dtf: f32) {
    unsafe { beacon::render_frame() };
    unsafe { safe::render_frame(dtf, GRENADE.get()) };
}

pub(super) fn report_native_throw(hand: Hand, release: Release) {
    let mut state = STATE.lock();
    state.status.primed += 1;
    state.status.thrown += 1;
    state.status.last_release = Some(release.velocity);
    state.thrown[hand.index()] = true;
    state.status.events.push(GrenadeEvent {
        hand: Some(hand),
        release,
    });
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

/// Draw a grenade from the pouch: one from the game's stock, as an item in `hand`. The stock is
/// read before the item is made and taken after, so a refused allocation takes nothing and an empty
/// stock makes nothing.
///
/// In the input tick's game stage.
fn draw_from_pouch(game: &GameTick<'_>, hand: Hand, tuning: &GrenadeTuning) -> bool {
    if !stock::refresh(game).is_some_and(Stock::may_draw) {
        tracing::info!(target: "controllers", hand = hand.label(), "grenade: the pouch is empty");
        return false;
    }
    if !safe::draw(game, hand, tuning) {
        return false;
    }
    if !stock::take(game) {
        // The stock changed between the read and the take: the item goes back as it came.
        tracing::warn!(target: "controllers", hand = hand.label(), "grenade: the stock emptied during the draw; the grenade goes back");
        safe::return_held(hand);
        return false;
    }
    true
}

/// Put the grenade `hand` holds back into the pouch: its item and actor retire, and the stock goes
/// up by one. A full stock refuses with the pouch's buzz and leaves the grenade in the hand; a
/// grenade whose pin is out never goes back.
///
/// In the input tick's game stage.
fn return_to_pouch(game: &GameTick<'_>, hand: Hand) {
    if returnable(hand).is_none() {
        return;
    }
    if let Err(error) = stock::may_give_back(game) {
        tracing::info!(target: "controllers", hand = hand.label(), %error, "grenade: the pouch refused the grenade");
        STATE.lock().status.returns_refused += 1;
        // The brief refusal buzz went unnoticed on the 14 November take, and six grenades fell.
        haptics::play(hand, Event::PouchFull);
        // A click as well, the dry fire of the gun the other hand holds, or else of a gun carried
        // on the body, so the refusal is heard.
        let carried = live::bound()
            .into_iter()
            .filter(|(_, place)| !matches!(place, Place::World(_)))
            .map(|(object, _)| object.get());
        let clicked = live::in_hand(hand.other())
            .into_iter()
            .chain(carried)
            .any(|gun| unsafe { reload::refusal_click(gun) });
        tracing::debug!(target: "controllers", clicked, "grenade: the full pouch's click");
        return;
    }
    if !safe::return_held(hand) {
        tracing::debug!(target: "controllers", hand = hand.label(), "grenade: the held grenade could not be put away");
        return;
    }
    if let Err(error) = stock::give_back(game) {
        // Read a moment ago as having room, on the same thread: only a change from elsewhere in the
        // game between the two reads lands here.
        tracing::warn!(target: "controllers", hand = hand.label(), %error, "grenade: the stock filled while the grenade was put away");
    }
    STATE.lock().status.returned += 1;
    haptics::play(hand, Event::PouchReturn);
    tracing::info!(target: "controllers", hand = hand.label(), "grenade: put the grenade back into the pouch");
}

const MAX_EVENTS: usize = 6;

struct State {
    thrown: [bool; 2],
    /// The inert grenades the snatch brought to each hand, for the next input tick to take.
    snatched: [Option<SafeItemView>; 2],
    /// The finite native grenades lying in the world the snatch brought to each hand, for the next
    /// input tick to take.
    snatched_pickups: [Option<Weak>; 2],
    sightings: Vec<Sighting>,
    locker_frames: Option<Frames>,
    status: GrenadeStatus,
    grips: [Option<Pose>; 2],
    next_pulse_s: f32,
    /// Where each hand's grenade stood to the mouth at the tick's start, for the teeth pull.
    mouth: [Option<Mouth>; 2],
    teeth: [TeethPull; 2],
}

/// How a hand's grenade stood to the mouth at the tick's start ([`at_mouth`]).
#[derive(Debug, Copy, Clone, PartialEq)]
struct Mouth {
    at_mouth: bool,
    other_occupied: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    thrown: [false; 2],
    snatched: [None; 2],
    snatched_pickups: [None, None],
    sightings: Vec::new(),
    locker_frames: None,
    status: GrenadeStatus {
        fuse: None,
        last_release: None,
        weapon_seen: false,
        stock: None,
        returned: 0,
        returns_refused: 0,
        primed: 0,
        thrown: 0,
        gone: 0,
        caught_hand: None,
        catch: CatchCounts {
            tracked: 0,
            in_reach: 0,
            caught: 0,
            thrown: 0,
            lost: 0,
        },
        events: Vec::new(),
    },
    grips: [None; 2],
    next_pulse_s: 0.0,
    mouth: [None; 2],
    teeth: [TeethPull::NEW; 2],
});

#[derive(Copy, Clone)]
struct PlannedLaunch {
    explosive: usize,
    transform: Option<Matrix4>,
    release: Release,
    catchable: bool,
}

thread_local! {
    static LAUNCH: Cell<Option<PlannedLaunch>> = const { Cell::new(None) };
}

/// Where each hand's grenade with its pin in stands to the mouth: whether its pin is at the mouth,
/// and whether the other hand is occupied, holding something ([`BeginInputs::free`]), closed on
/// something, or untracked. `None` for a hand without such a grenade or without a pose.
fn at_mouth(inputs: &BeginInputs, tuning: &GrenadeTuning) -> [Option<Mouth>; 2] {
    let mouth = pin::mouth_point(inputs.active.head, &tuning.pin);
    Hand::BOTH.map(|hand| {
        if !safe::pin_in(hand) {
            return None;
        }
        let grip = inputs.active.hands[hand.index()]?.grip;
        let pin = pin::pin_point(grip, hand, inputs.active.world_scale, tuning)?;
        let other = hand.other();
        Some(Mouth {
            at_mouth: pin::at_mouth(pin, mouth, &tuning.pin),
            other_occupied: !inputs.free[other.index()]
                || intent::hand_closed(other)
                || inputs.active.hands[other.index()].is_none(),
        })
    })
}

/// The held grenade's model in `hand`'s grip frame.
fn hold_local(hand: Hand, tuning: &GrenadeTuning) -> Pose {
    tuning.hold.in_hand(hand)
}

fn warning(
    state: &mut State,
    caught: Option<(Hand, Fuse)>,
    dt: f32,
) -> Option<(Vec<Primitive>, Option<Hand>)> {
    let tuning = GRENADE.get();
    let warning = &tuning.warning;
    let Some((hand, fuse)) = caught else {
        state.next_pulse_s = 0.0;
        return None;
    };
    let urgency = fuse_urgency(fuse.remaining_s, fuse.total_s);
    let (calm, urgent) = (warning.calm_rgba, warning.urgent_rgba);
    let primitives = vec![Primitive::Sprite {
        at: Anchor::Hand {
            hand,
            pose: HandPose::Grip,
            offset: hold_local(hand, tuning).position,
        },
        size: SpriteSize::World(warning.ring_size_m + warning.ring_size_growth_m * urgency),
        facing: Facing::Billboard,
        shape: Shape::Ring {
            thickness: warning.ring_thickness + warning.ring_thickness_growth * urgency,
        },
        color: std::array::from_fn(|index| calm[index] + (urgent[index] - calm[index]) * urgency),
        depth: DepthMode::XRay {
            occluded_alpha: warning.occluded_alpha,
        },
    }];
    let mut pulse = None;
    state.next_pulse_s -= dt;
    if state.next_pulse_s <= 0.0 {
        let [slow, fast] = warning.interval_s;
        state.next_pulse_s = slow + (fast - slow) * urgency;
        pulse = Some(hand);
    }
    Some((primitives, pulse))
}

/// The fuse in a hand nearest to running out, of a caught grenade's and the held items' whose pins
/// are out: the one the warning shows.
fn most_urgent(fuses: impl Iterator<Item = (Hand, Fuse)>) -> Option<(Hand, Fuse)> {
    fuses
        .filter(|(_, fuse)| fuse.remaining_s.is_finite())
        .min_by(|a, b| a.1.remaining_s.total_cmp(&b.1.remaining_s))
}

/// How near a running fuse is to its end: the share of its `total_s` that has burnt, from zero when
/// it was lit to one when it runs out, so that the warning rises from the pin on whatever the
/// fuse's length.
pub fn fuse_urgency(remaining_s: f32, total_s: f32) -> f32 {
    if !remaining_s.is_finite() || !total_s.is_finite() || total_s <= 0.0 {
        return 0.0;
    }
    (1.0 - remaining_s / total_s).clamp(0.0, 1.0)
}

#[detour(address = jc3gi::equipment::weapon::ThrownGrenadeWeapon::UpdateFromUserPostSim_Parallel_ADDRESS)]
fn update_from_user(
    weapon: *mut ThrownGrenadeWeapon,
    dt: f32,
    aim_position: *const Vector3,
    input: *const WeaponInputs,
) {
    UPDATE_FROM_USER
        .get()
        .unwrap()
        .call(weapon, dt, aim_position, input);
    // SAFETY: the grenade weapon is live for its own update.
    let Some(base) = (unsafe { weapon.cast::<WeaponBase>().as_ref() }) else {
        return;
    };
    if !base.m_IsUserLocalPlayer {
        return;
    }
    STATE.lock().status.weapon_seen = true;
    unsafe { beacon::observe(weapon) };
}

#[detour(address = jc3gi::equipment::explosive::Grenade::PropelToward_ADDRESS)]
fn grenade_propel_toward(
    grenade: *mut Grenade,
    target_position: *const Vector3,
    relative_velocity: *const Vector3,
    target_is_on_roof: bool,
    exact_target_aiming: bool,
) {
    GRENADE_PROPEL_TOWARD.get().unwrap().call(
        grenade,
        target_position,
        relative_velocity,
        target_is_on_roof,
        exact_target_aiming,
    );
    let launch = LAUNCH
        .get()
        .filter(|launch| launch.explosive == grenade as usize);
    // SAFETY: the grenade is live for its launch.
    if launch.is_none_or(|launch| launch.catchable) && !beacon::propelling(grenade as usize) {
        unsafe { catch::register(grenade.cast::<Explosive>(), launch.is_some()) };
    }
    let Some(launch) = launch else {
        return;
    };
    LAUNCH.set(None);
    // SAFETY: a grenade is an explosive at the same address.
    let Some(explosive) = (unsafe { grenade.cast::<Explosive>().as_mut() }) else {
        return;
    };
    if let Some(transform) = launch.transform {
        explosive.m_WorldMatrixT1 = transform;
        unsafe { explosive.SetPhysicsTransform(&transform) };
    }
    // The release already carries the player's movement, so the thrower's velocity the launch adds
    // is replaced rather than added to, and the canned spin gives way to the hand's.
    explosive.m_DeferredInitialVelocity = Vector3 {
        data: launch.release.velocity.to_array(),
    };
    explosive.m_InitialAngularVelocity = Vector3 {
        data: launch.release.angular.to_array(),
    };
}

unsafe fn primed_explosive(weapon: &ThrownGrenadeWeapon) -> Option<*mut Explosive> {
    let object = unsafe { weak_target(&weapon.m_PrimedGrenade) }?;
    Some(
        object
            .cast::<u8>()
            .wrapping_sub(Explosive::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<Explosive>(),
    )
}

fn catch_one(hand: Hand, grip: Vec3, seen: &[Sighting], tuning: &GrenadeTuning) {
    if let Some(sighting) = nearest_within(seen, |s| s.cockpit, grip, tuning.catch.radius_m) {
        catch::set_caught(Some(Caught {
            hand,
            explosive: sighting.explosive,
            body: sighting.body,
            owner: sighting.owner,
            since_s: motion::latest_time().unwrap_or(f64::NEG_INFINITY),
        }));
        haptics::play(hand, Event::GrenadeCatch);
    }
}

unsafe fn carry_caught(inputs: &TickInputs, tuning: &GrenadeTuning) -> Option<(Hand, Fuse)> {
    let caught = catch::caught()?;
    let hand = caught.hand;
    let Ok((centre, _)) = (unsafe { physics::body_pose(caught.body, caught.owner) }) else {
        catch::let_go(false);
        return None;
    };
    if inputs.opened[hand.index()] {
        // The grenade's body is already in the simulation, so it is thrown as a gun is: its own
        // body takes the release motion at once.
        let Release { velocity, angular } =
            throw_release(hand, centre, caught.since_s, "caught grenade", Some(true));
        let _ =
            unsafe { physics::set_body_velocities(caught.body, caught.owner, velocity, angular) };
        catch::let_go(true);
        STATE.lock().status.last_release = Some(velocity);
        return None;
    }
    let world_grip = STATE.lock().grips[hand.index()];
    held::hold(
        Slot::CaughtGrenade,
        Some(Held {
            explosive: caught.explosive,
            hand,
            local: hold_local(hand, tuning),
        }),
    );
    if let Some(grip) = world_grip {
        let target = grip.position + grip.orientation * hold_local(hand, tuning).position;
        let velocity = catch::carry_velocity(centre, target, inputs.dt, tuning.catch.max_carry_mps);
        let _ = unsafe {
            physics::set_body_velocities(caught.body, caught.owner, velocity, Vec3::ZERO)
        };
    }
    // SAFETY: body_pose proved the native grenade live on the game thread before the world update.
    let explosive = unsafe { &*(caught.explosive as *const Explosive) };
    Some((
        hand,
        Fuse {
            remaining_s: explosive.m_DetonationTimer,
            total_s: explosive.m_StartDetonationTimer,
        },
    ))
}

#[cfg(test)]
mod tests {
    use jc3vr_body::controllers::{
        fingers::grip::catalogue::{GripPose, cylinder_curl},
        hands::held::away_from_palm,
    };

    use super::*;
    use crate::controllers::weapons::grenade::tuning::GRENADE_BODY_RADIUS_M;

    #[test]
    fn the_fuse_warning_rises_from_the_pin() {
        assert_eq!(fuse_urgency(8.0, 8.0), 0.0);
        assert!((fuse_urgency(6.0, 8.0) - 0.25).abs() < 1e-6);
        assert!((fuse_urgency(1.0, 8.0) - 0.875).abs() < 1e-6);
        assert_eq!(fuse_urgency(-0.1, 8.0), 1.0);
        assert_eq!(fuse_urgency(9.0, 8.0), 0.0);
    }

    #[test]
    fn a_fuse_without_usable_timings_does_not_warn() {
        assert_eq!(fuse_urgency(f32::NAN, 8.0), 0.0);
        assert_eq!(fuse_urgency(1.0, 0.0), 0.0);
        assert_eq!(fuse_urgency(1.0, f32::NAN), 0.0);
    }

    #[test]
    fn the_warning_shows_the_fuse_nearest_to_running_out() {
        let fuse = |remaining_s| Fuse {
            remaining_s,
            total_s: 4.0,
        };
        assert_eq!(most_urgent(std::iter::empty()), None);
        let fuses = [
            (Hand::Left, fuse(3.0)),
            (Hand::Right, fuse(1.5)),
            (Hand::Left, fuse(f32::NAN)),
        ];
        assert_eq!(
            most_urgent(fuses.into_iter()),
            Some((Hand::Right, fuse(1.5)))
        );
    }

    #[test]
    fn the_held_grenade_stands_upright_with_its_lever_on_the_palm() {
        for hand in Hand::BOTH {
            let local = hold_local(hand, GRENADE.get());
            // The fuse head points up the finger tube, out of the top of the fist.
            assert!((local.orientation * Vec3::Y).abs_diff_eq(Vec3::NEG_Z, 1e-5));
            // The lever faces back into the palm.
            assert!((local.orientation * Vec3::Z).abs_diff_eq(-away_from_palm(hand), 1e-5));
            // The body is out of the palm by at least its radius.
            assert!(local.position.dot(away_from_palm(hand)) >= GRENADE_BODY_RADIUS_M);
        }
    }

    #[test]
    fn the_grenade_palm_wraps_the_grenades_body() {
        // Upright, the body is a cylinder up the finger tube, so the fingers close as far as round
        // a bar of the body's diameter.
        let wrap = cylinder_curl(2.0 * GRENADE_BODY_RADIUS_M);
        let shape = GripPose::GrenadePalm.shape();
        for curl in &shape.curl[1..] {
            assert!((curl - wrap).abs() < 0.06, "{curl} against {wrap}");
        }
    }
}
