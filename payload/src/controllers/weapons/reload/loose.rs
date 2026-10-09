//! Magazines in the world: an ejected magazine is an object that keeps its rounds, and a magazine
//! lying in the world that fits the held gun can be picked up and seated with them.
//!
//! The game drops a gun's spare magazine prop, a rigid object, through one call
//! ([`WeaponData::ThrowAwayMagazineObject`]): the mod's eject and the game's own reload track
//! message both use it, for every character. A detour on it notes each prop it drops with the
//! prop's body, the body's owner, the gun's magazine spawn tag
//! ([`WeaponData::m_MagazineUniqueTag`], which names the magazine model, so a magazine fits every
//! gun of the same tag), and the rounds it holds:
//!
//! - a magazine the mod's eject drops holds the rounds the gun had, which the eject takes out of
//!   the gun ([`EjectRounds`]), so they are neither in the gun nor returned to the reserve;
//! - one the game's reload drops for the player holds none, since the game's reload returns the
//!   rounds still in the gun to the reserve;
//! - one another character's reload drops holds what that character's magazine held, which the
//!   game never counts anywhere else (a character other than the player has unlimited
//!   ammunition).
//!
//! A body still owned by the same prop proves the prop alive ([`sightings`]). While the reload
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
    game::GameObject,
    physics::{PfxRigidBody, PhysicsGameObject},
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        body::alignment::measure::Frames,
        frame::Hand,
        grab::{Candidate, GrabVolume},
        physical_hands,
        weapons::reload::config::LooseMagazineConfig,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&THROW_AWAY_MAGAZINE_OBJECT_BINDER)
}

/// A magazine noted in the world.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Loose {
    /// The prop's address, as a physics game object.
    pub object: NonZeroUsize,
    pub body: u32,
    /// The body's owner when the magazine was dropped, which a live prop still is.
    pub owner: NonZeroUsize,
    /// The magazine spawn tag of the gun it came from.
    pub tag: i32,
    pub rounds: i32,
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

/// The rounds the mod's eject moves out of the gun into the magazine it drops: set on the input
/// tick around its call to the drop, read by the drop's detour on the same thread.
pub(super) struct EjectRounds;

impl EjectRounds {
    /// Run `drop` with `rounds` as the dropped magazine's rounds.
    pub(super) fn with<R>(rounds: i32, drop: impl FnOnce() -> R) -> R {
        EJECT_ROUNDS.set(Some(rounds));
        let result = drop();
        EJECT_ROUNDS.set(None);
        result
    }
}

/// Read every noted magazine's body, forget the ones gone or too old, and return the live ones of
/// `tag` within `config.range_m` of `player`, carried into the cockpit frame by `frames`.
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
    let now = Instant::now();
    let mut state = STATE.lock();
    let held = state.held.map(|h| h.magazine.object);
    let mut seen = Vec::new();
    state.loose.retain(|(m, dropped)| {
        if now.saturating_duration_since(*dropped).as_secs_f32() > config.keep_s {
            return false;
        }
        let Ok((centre, _)) = (unsafe { physical_hands::body_pose(m.body, m.owner) }) else {
            return false;
        };
        if fits(m, tag, centre, player, config.range_m) && Some(m.object) != held {
            seen.push(Sighting {
                magazine: *m,
                cockpit: frames.world_to_cockpit(centre),
            });
        }
        true
    });
    state.counts.in_world = state.loose.len();
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
pub(super) fn take(hand: Hand, sighting: Sighting) {
    let mut state = STATE.lock();
    state.held = Some(Held {
        hand,
        magazine: sighting.magazine,
    });
    state.counts.picked_up += 1;
    tracing::info!(target: "controllers", ?hand, rounds = sighting.magazine.rounds, "reload: picked a magazine up from the world");
}

/// The magazine held as the round in hand, if it came from the world.
pub(super) fn held() -> Option<Loose> {
    STATE.lock().held.map(|h| h.magazine)
}

/// Carry the held magazine's body so that its top is at `top`, pointing along `axis`, world space,
/// over `dt`. Returns false when the magazine is gone from the world, which lets it go.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn carry(top: Vec3, axis: Vec3, dt: f32, config: &LooseMagazineConfig) -> bool {
    let Some(held) = STATE.lock().held else {
        return false;
    };
    let m = held.magazine;
    let Ok((centre, orientation)) = (unsafe { physical_hands::body_pose(m.body, m.owner) }) else {
        let_go();
        return false;
    };
    let target = top - axis.normalize_or_zero() * config.half_length_m;
    let (linear, angular) = carry_velocities(
        (centre, orientation),
        (target, upright_toward(orientation, axis)),
        dt,
        config.max_carry_mps,
    );
    // A failed write leaves the magazine where its physics has it until the next tick's check.
    let _ = unsafe { physical_hands::set_body_velocities(m.body, m.owner, linear, angular) };
    true
}

/// Let go of the held magazine: it stays in the world where it falls.
pub(super) fn let_go() {
    if let Some(held) = STATE.lock().held.take() {
        tracing::info!(target: "controllers", hand = ?held.hand, rounds = held.magazine.rounds, "reload: let go of the magazine from the world");
    }
}

/// The held magazine seated: disable its prop and forget it. Returns its rounds.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(super) unsafe fn seat() -> Option<i32> {
    let mut state = STATE.lock();
    let held = state.held.take()?;
    let m = held.magazine;
    state.loose.retain(|(l, _)| l.object != m.object);
    state.counts.seated += 1;
    state.counts.last_rounds = Some(m.rounds);
    drop(state);
    // The body check proves the prop live before it is touched.
    if unsafe { physical_hands::body_pose(m.body, m.owner) }.is_ok() {
        let object = (m.object.get() + PhysicsGameObject::GAME_OBJECT_BASE_OFFSET as usize)
            as *mut GameObject;
        unsafe { (*object).Enable(false) };
    }
    tracing::info!(target: "controllers", hand = ?held.hand, rounds = m.rounds, "reload: seated a magazine from the world");
    Some(m.rounds)
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

/// The most magazines noted at once; the oldest is forgotten past it.
const MAX_LOOSE: usize = 32;

thread_local! {
    static EJECT_ROUNDS: Cell<Option<i32>> = const { Cell::new(None) };
}

#[derive(Debug, Copy, Clone)]
struct Held {
    hand: Hand,
    magazine: Loose,
}

struct State {
    loose: Vec<(Loose, Instant)>,
    held: Option<Held>,
    counts: LooseCounts,
}

static STATE: Mutex<State> = Mutex::new(State {
    loose: Vec::new(),
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
    let enabled = Config::lock_query(|c| {
        let reload = c.controllers.weapons.reload;
        c.controllers.enabled && reload.enabled && reload.loose.enabled
    });
    // SAFETY: the gun is live for its own call.
    let Some(g) = (unsafe { gun.as_ref() }).filter(|_| enabled) else {
        return original();
    };
    let spare = g.m_MagazineObject.px;
    let tag = g.m_MagazineUniqueTag;
    // SAFETY: as above; a gun's owner and the character outlive the call.
    let game_rounds = unsafe {
        match character.as_ref() {
            Some(c) if c.m_IsLocalCharacter => 0,
            _ => g.m_Owner.as_ref().map_or(0, |w| w.m_MagazineAmmo.max(0)),
        }
    };
    let rounds = EJECT_ROUNDS.get().unwrap_or(game_rounds);
    let thrown = original();
    if thrown
        && let Some(object) = NonZeroUsize::new(spare as usize)
        // SAFETY: the prop is live: the drop just put it in the world and in the update.
        && let Some((body, owner)) = unsafe { body_of(object) }
    {
        let mut state = STATE.lock();
        state.loose.retain(|(l, _)| l.object != object);
        if state.loose.len() >= MAX_LOOSE {
            state.loose.remove(0);
        }
        state.loose.push((
            Loose {
                object,
                body,
                owner,
                tag,
                rounds,
            },
            Instant::now(),
        ));
        state.counts.dropped += 1;
    }
    thrown
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
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    fn magazine(tag: i32) -> Loose {
        Loose {
            object: NonZeroUsize::new(0x1000).unwrap(),
            body: 3,
            owner: NonZeroUsize::new(0x1000).unwrap(),
            tag,
            rounds: 12,
        }
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
