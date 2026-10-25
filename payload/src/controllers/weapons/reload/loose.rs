//! Magazines in the world: an ejected magazine is an object that keeps its rounds, and a magazine
//! lying in the world that fits the held gun can be picked up and seated with them.
//!
//! The game drops a gun's spare magazine prop, a rigid object, through one call
//! ([`WeaponData::ThrowAwayMagazineObject`]): the mod's eject and the game's own reload track
//! message both use it, for every character. A reserved manual drop transfers its known carried
//! rounds. Local automatic and empty NPC drops can reserve empty pickup records before native
//! execution. A full store refuses manual drops unchanged, but runs automatic calls unchanged and
//! untracked. Nonempty NPC import is blocked: native throw leaves the weapon's rounds counted,
//! and native finite reload returns them to reserve. Copying them would duplicate ammunition.
//!
//! Reservations, held items, unread results, and world records share one fixed cap. No native prop
//! retirement is guessed. Unavailable physics preserves accounting. Losing body ownership
//! quarantines a record; reused body ids cannot restore its authority without lifetime proof.
//!
//! While the reload
//! wants a magazine for the held gun (its magazine is out), each noted magazine of the gun's tag
//! within reach is a grab volume for the hand without a gun. A grip press there takes it as the
//! round in hand, from where it lay ([`take`]): its body is carried to the hand each tick
//! ([`carry`]), so the magazine the player sees in the hand is the one that was on the ground.
//! Seated, it loads its own rounds and the prop is disabled ([`seat`]); let go of, it falls from
//! the hand with the motion the carry gave it and stays in the world.
//!
//! [`WeaponData::ThrowAwayMagazineObject`]: jc3gi::equipment::weapon::WeaponData::ThrowAwayMagazineObject
//! [`WeaponData::m_MagazineUniqueTag`]: jc3gi::equipment::weapon::WeaponData::m_MagazineUniqueTag

use std::{cell::Cell, num::NonZeroUsize, time::Instant};

use detours_macro::detour;
use glam::{Quat, Vec3};
use jc3gi::{
    character::character::Character,
    equipment::weapon::WeaponData,
    physics::{PfxRigidBody, PhysicsGameObject},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        body::rig::frames::Frames,
        frame::Hand,
        grab::{Candidate, GrabVolume},
        physical_hands,
        weapons::reload::{
            config::LooseMagazineConfig,
            loose::records::{DropKey, Records, Token, WorldHandle},
        },
    },
};

mod body;
mod records;

use crate::controllers::weapons::reload::loose::body::{Bodies, Carry, NativeBodies};

/// Whether a held operation committed, must wait, or has no represented item.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum Operation<T> {
    Ready(T),
    Deferred,
    Gone,
}

/// A failed pose read alone never proves the prop's lifetime ended.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Maintenance {
    Live,
    Unavailable {
        identity_lost: bool,
    },
    /// Model an explicit lifetime invalidation, not a recycled body id.
    #[cfg(test)]
    Gone,
}

fn body_status(result: Result<(), physical_hands::BodyError>) -> Maintenance {
    match result {
        Ok(()) => Maintenance::Live,
        Err(physical_hands::BodyError::NoBody | physical_hands::BodyError::Reowned) => {
            Maintenance::Unavailable {
                identity_lost: true,
            }
        }
        Err(
            physical_hands::BodyError::NoPhysicsWorld
            | physical_hands::BodyError::NotDynamic
            | physical_hands::BodyError::BadMotion,
        ) => Maintenance::Unavailable {
            identity_lost: false,
        },
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&THROW_AWAY_MAGAZINE_OBJECT_BINDER)
}

/// A magazine noted in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Loose {
    /// The committed record generation; copied native fields do not establish ownership.
    handle: WorldHandle,
    /// The prop's address, as a physics game object.
    pub object: NonZeroUsize,
    pub body: u32,
    /// The body's owner when the magazine was dropped, which a live prop still is.
    pub owner: NonZeroUsize,
    /// The magazine spawn tag of the gun it came from.
    pub tag: i32,
    pub rounds: i32,
}

impl Loose {
    #[cfg(test)]
    pub(crate) const fn test(
        object: NonZeroUsize,
        body: u32,
        owner: NonZeroUsize,
        tag: i32,
        rounds: i32,
    ) -> Self {
        Self {
            handle: WorldHandle::test(0),
            object,
            body,
            owner,
            tag,
            rounds,
        }
    }
}

/// Native fields captured before the store assigns committed ownership.
pub(super) struct Captured {
    object: NonZeroUsize,
    body: u32,
    owner: NonZeroUsize,
    tag: i32,
    rounds: i32,
}

/// A noted magazine the input tick saw, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Sighting {
    pub magazine: Loose,
    /// The body's centre, cockpit frame.
    pub cockpit: Vec3,
}

/// The magazines' counts, for the readout.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct LooseCounts {
    /// Magazines noted and still in the world.
    pub in_world: usize,
    /// Of those, the ones that fit the held gun within reach on the last tick.
    pub in_reach: usize,
    pub dropped: u64,
    pub picked_up: u64,
    pub seated: u64,
    /// The rounds of the last magazine seated from the world.
    pub last_rounds: Option<i32>,
}

/// A reserved slot becomes the exact retained world item on a successful manual throw.
/// Dropping the lease releases unused capacity or just unprotects the committed world record.
pub(super) struct Retention {
    token: Token,
    key: DropKey,
}

impl Retention {
    pub(super) fn reserve(gun: usize, spare: usize) -> Option<Self> {
        let key = DropKey { gun, spare };
        let token = STATE.lock().records.reserve(key)?;
        Some(Self { token, key })
    }

    pub(super) fn matches(&self, gun: usize, spare: usize) -> bool {
        self.key == DropKey { gun, spare } && STATE.lock().records.matches(self.token, self.key)
    }
}

/// Capacity refusal must occur before the supplied engine operation.
pub(super) fn with_admission<R, T>(
    reserve: impl FnOnce() -> Option<R>,
    refused: T,
    execute: impl FnOnce(&R) -> T,
) -> T {
    let Some(reservation) = reserve() else {
        return refused;
    };
    execute(&reservation)
}

impl Drop for Retention {
    fn drop(&mut self) {
        STATE.lock().records.release(self.token);
    }
}

/// Known carried rounds scoped around the reserved call, read on that same thread by its detour.
pub(super) struct EjectRounds;

impl EjectRounds {
    /// Run `drop` only with already reserved capacity for its exact spare.
    pub(super) fn with<R>(
        retention: &Retention,
        rounds: i32,
        drop: impl FnOnce() -> R,
    ) -> (R, Option<Loose>) {
        let expected = ExpectedDrop {
            key: retention.key,
            token: retention.token,
            rounds,
        };
        let previous_rounds = EJECT_ROUNDS.replace(Some(expected));
        let previous_prop = EJECT_PROP.replace(None);
        let result = drop();
        let prop = EJECT_PROP.replace(previous_prop);
        EJECT_ROUNDS.set(previous_rounds);
        (result, prop)
    }
}

/// Check every occupied world slot once per reload input tick, regardless of reload eligibility.
/// Reservations are not world objects. Failed body reads preserve accounting; ownership loss
/// quarantines pickup and seating. Only usable, unheld, unleased empty records can age out.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn maintain(config: &LooseMagazineConfig) {
    let mut state = STATE.lock();
    state.maintain(Instant::now(), config.keep_s, |m| {
        body_status(unsafe { physical_hands::body_pose(m.body, m.owner) }.map(|_| ()))
    });
}

/// Return retained world items of `tag` within `config.range_m` of `player`, in cockpit space.
/// Maintenance checks accounting independently, even when this query is never called.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn sightings(
    player: Vec3,
    tag: i32,
    frames: &Frames,
    config: &LooseMagazineConfig,
) -> Vec<Sighting> {
    let mut state = STATE.lock();
    let held = state.held.map(|h| h.magazine.handle);
    let magazines: Vec<_> = state
        .records
        .magazines()
        .filter(|m| Some(m.handle) != held && state.records.usable(*m))
        .collect();
    let mut seen = Vec::new();
    for magazine in magazines {
        let result = unsafe { physical_hands::body_pose(magazine.body, magazine.owner) };
        if let Operation::Ready((centre, _)) = state.observe(magazine, result)
            && fits(&magazine, tag, centre, player, config.range_m)
        {
            seen.push(Sighting {
                magazine,
                cockpit: frames.world_to_cockpit(centre),
            });
        }
    }
    state.counts.in_reach = seen.len();
    seen
}

/// Whether the magazine `m`, its body at `centre`, fits a gun of `tag` and lies within `range_m` of
/// `player`.
pub fn fits(m: &Loose, tag: i32, centre: Vec3, player: Vec3, range_m: f32) -> bool {
    m.tag == tag
        && m.tag != WeaponData::NO_MAGAZINE_UNIQUE_TAG
        && centre.distance(player) <= range_m
}

/// The grab volumes of the sightings, for `hand`.
pub fn candidates(seen: &[Sighting], radius_m: f32, hand: Hand) -> Vec<Candidate> {
    seen.iter()
        .map(|s| Candidate {
            volume: GrabVolume::WorldMagazine,
            centre: s.cockpit,
            radius: radius_m,
            hand: Some(hand),
        })
        .collect()
}

/// The sighting nearest `grip`, within `radius_m`.
pub fn nearest(seen: &[Sighting], grip: Vec3, radius_m: f32) -> Option<Sighting> {
    seen.iter()
        .map(|s| (s.cockpit.distance(grip), s))
        .filter(|(d, _)| d.is_finite() && *d <= radius_m)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, s)| *s)
}

/// Take the magazine of `sighting` into `hand` as the round in hand.
pub(super) fn take(hand: Hand, sighting: Sighting) -> bool {
    let mut state = STATE.lock();
    if !state.take(hand, sighting.magazine) {
        return false;
    }
    state.counts.picked_up += 1;
    tracing::info!(target: "controllers", ?hand, rounds = sighting.magazine.rounds, "reload: picked a magazine up from the world");
    true
}

/// The magazine held as the round in hand, if it came from the world.
pub(super) fn held() -> Option<Loose> {
    STATE.lock().held.map(|h| h.magazine)
}

/// Carry the held magazine's body so that its top is at `top`, pointing along `axis`, world space,
/// over `dt`. Unavailable or quarantined bodies defer while preserving held ownership.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn carry(
    top: Vec3,
    axis: Vec3,
    dt: f32,
    config: &LooseMagazineConfig,
) -> Operation<()> {
    STATE.lock().carry_with(
        Carry {
            top,
            axis,
            dt,
            config,
        },
        &mut NativeBodies,
    )
}

/// Let go of the held magazine: it stays in the world where it falls.
pub(super) fn let_go() {
    if let Some(held) = STATE.lock().held.take() {
        tracing::info!(target: "controllers", hand = ?held.hand, rounds = held.magazine.rounds, "reload: let go of the magazine from the world");
    }
}

/// Commit seating once, disable the prop, and return its rounds. Deferred leaves the hold intact.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn seat() -> Operation<i32> {
    let result = STATE.lock().seat_with(&mut NativeBodies);
    match result {
        Operation::Ready(magazine) => {
            NativeBodies.disable(magazine);
            let mut state = STATE.lock();
            state.counts.seated += 1;
            state.counts.last_rounds = Some(magazine.rounds);
            tracing::info!(target: "controllers", rounds = magazine.rounds, "reload: seated a magazine from the world");
            Operation::Ready(magazine.rounds)
        }
        Operation::Deferred => Operation::Deferred,
        Operation::Gone => Operation::Gone,
    }
}

pub fn counts() -> LooseCounts {
    STATE.lock().counts
}

/// The velocities that carry a body from `from` (centre, orientation) to `to` over `dt`, the
/// linear part capped at `max_mps`.
pub fn carry_velocities(
    from: (Vec3, Quat),
    to: (Vec3, Quat),
    dt: f32,
    max_mps: f32,
) -> (Vec3, Vec3) {
    if !(dt > 0.0 && from.0.is_finite() && to.0.is_finite()) {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let linear = ((to.0 - from.0) / dt).clamp_length_max(max_mps.max(0.0));
    let turn = (to.1 * from.1.inverse()).normalize();
    // The shorter way round.
    let turn = if turn.w < 0.0 { -turn } else { turn };
    let (axis, angle) = turn.to_axis_angle();
    let angular = if angle.is_finite() && axis.is_finite() {
        (axis * angle / dt).clamp_length_max(MAX_CARRY_TURN_RADPS)
    } else {
        Vec3::ZERO
    };
    (linear, angular)
}

/// `orientation` turned the least way that points its +Y along `axis`: a magazine's top is its +Y,
/// and it should point out of the hand into the well.
pub fn upright_toward(orientation: Quat, axis: Vec3) -> Quat {
    let Some(axis) = axis.try_normalize() else {
        return orientation;
    };
    let up = orientation * Vec3::Y;
    (Quat::from_rotation_arc(up, axis) * orientation).normalize()
}

/// The fastest a held magazine is turned toward the hand's pose, radians per second.
const MAX_CARRY_TURN_RADPS: f32 = 30.0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct ExpectedDrop {
    key: DropKey,
    token: Token,
    rounds: i32,
}

thread_local! {
    static EJECT_ROUNDS: Cell<Option<ExpectedDrop>> = const { Cell::new(None) };
    static EJECT_PROP: Cell<Option<Loose>> = const { Cell::new(None) };
}

#[derive(Debug, Copy, Clone)]
struct Held {
    hand: Hand,
    magazine: Loose,
}

struct State {
    records: Records,
    held: Option<Held>,
    counts: LooseCounts,
}

impl State {
    fn take(&mut self, hand: Hand, magazine: Loose) -> bool {
        if self.held.is_some() || !self.records.usable(magazine) {
            return false;
        }
        self.held = Some(Held { hand, magazine });
        true
    }

    fn maintain(&mut self, now: Instant, keep_s: f32, status: impl FnMut(Loose) -> Maintenance) {
        self.records
            .maintain(now, keep_s, self.held.map(|h| h.magazine.handle), status);
        if self
            .held
            .is_some_and(|h| !self.records.contains(h.magazine))
        {
            self.held = None;
        }
        self.counts.in_world = self.records.magazines().count();
        self.counts.in_reach = 0;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    records: Records::new(),
    held: None,
    counts: LooseCounts {
        in_world: 0,
        in_reach: 0,
        dropped: 0,
        picked_up: 0,
        seated: 0,
        last_rounds: None,
    },
});

/// The gun's magazine drop: the prop it drops noted with its rounds.
#[detour(address = jc3gi::equipment::weapon::WeaponData::ThrowAwayMagazineObject_ADDRESS)]
fn throw_away_magazine_object(
    gun: *mut WeaponData,
    character: *mut Character,
    model_name_hash: u32,
) -> bool {
    let original = || {
        THROW_AWAY_MAGAZINE_OBJECT
            .get()
            .unwrap()
            .call(gun, character, model_name_hash)
    };
    let expected = EJECT_ROUNDS.get();
    let admitted = if expected.is_none_or(|e| e.key.gun != gun as usize) {
        // The original detour imported NPC rounds without removing them from native accounting.
        // Only a local automatic drop or an already empty NPC magazine is proved import-safe.
        unsafe { gun.as_ref() }.and_then(|g| {
            let enabled = Config::lock_query(|c| c.controllers.weapons.reload.loose.enabled);
            let local = unsafe { character.as_ref() }.is_some_and(|c| c.m_IsLocalCharacter);
            let rounds = unsafe { g.m_Owner.as_ref() }.map(|w| w.m_MagazineAmmo.max(0));
            automatic_rounds(enabled, local, rounds).and_then(|rounds| {
                Retention::reserve(gun as usize, g.m_MagazineObject.px as usize)
                    .map(|lease| (lease, rounds))
            })
        })
    } else {
        None
    };
    let automatic = admitted.as_ref().map(|(lease, rounds)| ExpectedDrop {
        key: lease.key,
        token: lease.token,
        rounds: *rounds,
    });
    let (thrown, retained) = dispatch_drop(
        expected.filter(|e| e.key.gun == gun as usize).or(automatic),
        automatic.is_some(),
        || {
            // No gun reference crosses the original, which may re-enter its owner.
            unsafe { gun.as_ref() }.map(|g| {
                (
                    DropKey {
                        gun: gun as usize,
                        spare: g.m_MagazineObject.px as usize,
                    },
                    g.m_MagazineUniqueTag,
                )
            })
        },
        |expected, key| STATE.lock().records.begin(expected.token, key),
        original,
        |expected, tag| {
            let object = NonZeroUsize::new(expected.key.spare)?;
            // SAFETY: the native drop just enabled this prop and put it into the update.
            let (body, owner) = unsafe { body_of(object) }?;
            let captured = Captured {
                object,
                body,
                owner,
                tag,
                rounds: expected.rounds.max(0),
            };
            let mut state = STATE.lock();
            let magazine = state
                .records
                .commit(expected.token, captured, Instant::now())?;
            state.counts.dropped += 1;
            state.counts.in_world = state.records.magazines().count();
            Some(magazine)
        },
    );
    if automatic.is_none()
        && let Some(magazine) = retained
    {
        EJECT_PROP.set(Some(magazine));
    }
    thrown
}

fn automatic_rounds(enabled: bool, local: bool, native_rounds: Option<i32>) -> Option<i32> {
    (enabled && (local || native_rounds == Some(0))).then_some(0)
}

fn dispatch_drop(
    expected: Option<ExpectedDrop>,
    native_fallback: bool,
    read: impl FnOnce() -> Option<(DropKey, i32)>,
    begin: impl FnOnce(ExpectedDrop, DropKey) -> bool,
    original: impl FnOnce() -> bool,
    record: impl FnOnce(ExpectedDrop, i32) -> Option<Loose>,
) -> (bool, Option<Loose>) {
    let Some(expected) = expected else {
        return (original(), None);
    };
    let Some((key, tag)) = read() else {
        return (native_fallback && original(), None);
    };
    if key != expected.key {
        return (original(), None);
    }
    if tag == WeaponData::NO_MAGAZINE_UNIQUE_TAG || !begin(expected, key) {
        return (native_fallback && original(), None);
    }
    let thrown = original();
    (thrown, if thrown { record(expected, tag) } else { None })
}

/// The prop's body and the body's owner.
///
/// # Safety
///
/// `object` must be a live rigid object.
unsafe fn body_of(object: NonZeroUsize) -> Option<(u32, NonZeroUsize)> {
    let physics = unsafe { &mut *(object.get() as *mut PhysicsGameObject) };
    let instance = unsafe { physics.GetIPfxInstance() };
    let owner = NonZeroUsize::new(unsafe { instance.as_ref() }?.m_GameObject as usize)?;
    // A rigid object's instance is its one body's wrapper.
    let body = unsafe { (*instance.cast::<PfxRigidBody>()).m_BodyId };
    Some((body, owner))
}

#[cfg(test)]
mod policy_tests;

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    fn magazine(tag: i32) -> Loose {
        Loose {
            handle: WorldHandle::test(0),
            object: NonZeroUsize::new(0x1000).unwrap(),
            body: 3,
            owner: NonZeroUsize::new(0x1000).unwrap(),
            tag,
            rounds: 12,
        }
    }

    #[test]
    fn an_eject_returns_only_its_own_prop_and_restores_nested_scopes() {
        let outer = Loose {
            rounds: 11,
            ..magazine(7)
        };
        let inner = Loose {
            rounds: 3,
            ..magazine(8)
        };
        let a = Retention::reserve(9001, 9002).unwrap();
        let b = Retention::reserve(9003, 9004).unwrap();
        let (result, retained) = EjectRounds::with(&a, outer.rounds, || {
            assert_eq!(EJECT_ROUNDS.get().unwrap().key, a.key);
            EJECT_PROP.set(Some(outer));
            let (inner_result, captured) = EjectRounds::with(&b, inner.rounds, || {
                assert_eq!(EJECT_ROUNDS.get().unwrap().key, b.key);
                EJECT_PROP.set(Some(inner));
                true
            });
            assert!(inner_result);
            assert_eq!(captured, Some(inner));
            assert_eq!(EJECT_ROUNDS.get().unwrap().key, a.key);
            assert_eq!(EJECT_PROP.get(), Some(outer));
            false
        });
        assert!(!result);
        assert_eq!(retained, Some(outer));
        assert_eq!(EJECT_ROUNDS.get(), None);
        assert_eq!(EJECT_PROP.get(), None);
        assert_eq!(EjectRounds::with(&a, 9, || true), (true, None));
    }

    #[test]
    fn a_magazine_fits_its_own_tag_within_reach() {
        let m = magazine(7);
        assert!(fits(&m, 7, Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), 2.0));
        assert!(!fits(&m, 8, Vec3::ZERO, Vec3::ZERO, 2.0));
        assert!(!fits(&m, 7, Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0), 2.0));
        let none = magazine(WeaponData::NO_MAGAZINE_UNIQUE_TAG);
        assert!(!fits(
            &none,
            WeaponData::NO_MAGAZINE_UNIQUE_TAG,
            Vec3::ZERO,
            Vec3::ZERO,
            2.0
        ));
    }

    #[test]
    fn the_nearest_magazine_in_reach_is_taken() {
        let seen = [0.08, 0.03, 0.5].map(|x| Sighting {
            magazine: magazine(1),
            cockpit: Vec3::new(x, 0.0, 0.0),
        });
        assert_eq!(nearest(&seen, Vec3::ZERO, 0.1), Some(seen[1]));
        assert_eq!(nearest(&seen, Vec3::new(2.0, 0.0, 0.0), 0.1), None);
        let volumes = candidates(&seen, 0.1, Hand::Left);
        assert_eq!(volumes.len(), 3);
        assert!(volumes.iter().all(|c| c.hand == Some(Hand::Left)));
    }

    #[test]
    fn the_carry_moves_and_turns_the_magazine_to_the_hand() {
        let from = (Vec3::ZERO, Quat::IDENTITY);
        let to = (Vec3::new(0.0, 0.1, 0.0), Quat::from_rotation_z(0.1));
        let (linear, angular) = carry_velocities(from, to, 0.01, 20.0);
        assert!((linear - Vec3::new(0.0, 10.0, 0.0)).length() < 1e-4);
        assert!((angular - Vec3::new(0.0, 0.0, 10.0)).length() < 1e-3);
        let (capped, _) = carry_velocities(from, (Vec3::X, Quat::IDENTITY), 0.01, 20.0);
        assert!((capped.length() - 20.0).abs() < 1e-3);
        assert_eq!(
            carry_velocities(from, to, 0.0, 20.0),
            (Vec3::ZERO, Vec3::ZERO)
        );
    }

    #[test]
    fn a_magazine_is_turned_to_point_its_top_along_the_axis() {
        let turned = upright_toward(Quat::IDENTITY, Vec3::X);
        assert!((turned * Vec3::Y - Vec3::X).length() < 1e-5);
        let lying = Quat::from_rotation_x(FRAC_PI_2);
        let turned = upright_toward(lying, Vec3::Y);
        assert!((turned * Vec3::Y - Vec3::Y).length() < 1e-5);
        assert_eq!(upright_toward(lying, Vec3::ZERO), lying);
    }
}
