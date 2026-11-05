//! Taking a weapon from where it is into a hand. The design is in
//! `docs/mod/input/physical-weapons.md`, "Taking a weapon from a source".
//!
//! Every weapon a hand takes comes from a source ([`WeaponSource`]): lying in the world, on a rack
//! or set down in a locker by its placement rule, one gun of a rack's pistol pair, or a gun in a
//! living character's hands or holstered on its body. The take is the same whatever the source,
//! and differs only in its delivery ([`Delivery`]):
//!
//! - a hand that closes on the weapon within reach takes it at once ([`take`]): the source gives
//!   it up, and it slides from where it was into the hand through the direct pickup;
//! - the grapple's snatch takes it from range ([`free`]): the source gives it up into the world
//!   with its bodies, the line pulls it in, and the catching hand takes it ([`deliver`]).

use std::{fmt, num::NonZeroUsize};

use jc3gi::{character::inventory::WeaponSlot, equipment::weapon::WeaponBase};

use crate::controllers::{
    frame::Hand,
    loadout::{Delivery, TakeSource},
    weapons::{
        engine,
        hold::{
            SNAP_S,
            pickup::{self, Refusal, TakeOptions},
            rack::{self, paired::Member},
            refs::{Strong, Weak},
            steal::{self, CharacterGun},
            world::{TargetKind, WorldTarget},
        },
        inventory::engine::{weak_to, weapon_of},
    },
};

/// Where a weapon a hand takes is.
pub(crate) enum WeaponSource {
    /// Lying in the world with its bodies.
    Ground { weak: Weak },
    /// Held out by its placement rule: on a rack, or set down at a spot (a locker's).
    Rack { weapon: NonZeroUsize, weak: Weak },
    /// One gun of a rack's pistol pair.
    RackMember(Member),
    /// A gun in a living character's hands, or holstered on its body.
    Character(CharacterGun),
}

impl WeaponSource {
    /// The source a grip target in reach stands for, found again so that it is known alive and
    /// still offered.
    ///
    /// # Safety
    ///
    /// The game thread, before the world update.
    pub(crate) unsafe fn of_target(target: &WorldTarget) -> Result<WeaponSource, SourceRefusal> {
        match target.kind {
            TargetKind::Held { .. } => CharacterGun::held(target)
                .map(WeaponSource::Character)
                .ok_or(SourceRefusal::Gone),
            TargetKind::Rack { weapon } => {
                let alive = unsafe { rack::find(weapon) }.ok_or(SourceRefusal::Gone)?;
                let weapon = NonZeroUsize::new(weapon).ok_or(SourceRefusal::Gone)?;
                // SAFETY: the strong reference keeps the weapon alive.
                let weak = unsafe { weak_to(&*weapon_of(&alive)) };
                Ok(WeaponSource::Rack { weapon, weak })
            }
            TargetKind::RackMember {
                weapon,
                generation,
                member,
            } => {
                if !unsafe { rack::find(weapon) }.is_some_and(|alive| rack::racked(&alive)) {
                    return Err(SourceRefusal::Gone);
                }
                Ok(WeaponSource::RackMember(Member {
                    source: weapon,
                    generation,
                    index: member,
                }))
            }
        }
    }

    /// The source of a weapon lying in the world, from its live weapon.
    ///
    /// # Safety
    ///
    /// `weapon` must be live.
    pub(crate) unsafe fn ground(weapon: &WeaponBase) -> WeaponSource {
        WeaponSource::Ground {
            weak: unsafe { weak_to(weapon) },
        }
    }
}

/// A weapon its source gave up, lying in the world with its bodies, held alive.
pub(crate) struct Freed {
    pub weapon: *mut WeaponBase,
    /// Keeps the weapon alive while held.
    pub alive: Strong,
    /// Where it was taken from.
    pub source: TakeSource,
}

/// Why a source did not give its weapon up.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SourceRefusal {
    /// The weapon is gone from its source: despawned, taken, or no longer offered.
    Gone,
    /// A rack's copy is already on its way to a hand.
    CopyPending,
    /// The take into the hand refused it.
    Take(Refusal),
}

impl fmt::Display for SourceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceRefusal::Gone => {
                write!(f, "take from a source: the weapon is no longer there")
            }
            SourceRefusal::CopyPending => {
                write!(
                    f,
                    "take from a source: a rack's copy is already on its way to a hand"
                )
            }
            SourceRefusal::Take(refusal) => write!(f, "take from a source: {refusal}"),
        }
    }
}

impl std::error::Error for SourceRefusal {}

/// Take `source`'s weapon into `hand` within reach: the source gives it up, and it slides into the
/// hand from where it was. Returns the slot it belongs to.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn take(hand: Hand, source: &WeaponSource) -> Result<WeaponSlot, SourceRefusal> {
    let options = |source| TakeOptions {
        slide_s: SNAP_S,
        source,
        delivery: Delivery::Local,
    };
    match source {
        WeaponSource::Ground { weak } => {
            let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
            // SAFETY: the strong reference keeps the weapon alive.
            let weapon = unsafe { &mut *weapon_of(&alive) };
            unsafe { pickup::take(hand, weapon, options(TakeSource::World)) }
                .map_err(SourceRefusal::Take)
        }
        WeaponSource::Rack { weak, .. } => {
            let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
            unsafe { rack::take(hand, &alive, options(TakeSource::Rack)) }
        }
        WeaponSource::RackMember(member) => {
            let owner = unsafe { engine::local_player() }
                .ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
            unsafe { rack::paired::take(owner, hand, *member, options(TakeSource::Rack)) }
        }
        WeaponSource::Character(gun) => {
            let freed = unsafe {
                steal::release(gun, |owner, weapon| {
                    pickup::preflight(owner, weapon, hand)
                        .map(|_| ())
                        .map_err(SourceRefusal::Take)
                })
            }?;
            // SAFETY: the strong reference keeps the dropped gun alive.
            let weapon = unsafe { &mut *freed.weapon };
            let slot = unsafe { pickup::take(hand, weapon, options(freed.source)) }
                .map_err(SourceRefusal::Take)?;
            steal::note_steal();
            tracing::info!(
                target: "controllers",
                hand = hand.label(),
                "hold: took the gun out of a character's hands into the {} hand",
                hand.label()
            );
            Ok(slot)
        }
    }
}

/// Free `source`'s weapon for the grapple's snatch into `hand`: the source gives it up into the
/// world, for the line to pull in. A take the hand could not make is refused before the weapon
/// leaves its source.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn free(hand: Hand, source: &WeaponSource) -> Result<Freed, SourceRefusal> {
    match source {
        WeaponSource::Ground { weak } => {
            let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
            let weapon = weapon_of(&alive);
            let player = unsafe { engine::local_player() }
                .ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
            // SAFETY: the strong reference keeps the weapon alive.
            unsafe { pickup::takeable(player, &*weapon) }.map_err(SourceRefusal::Take)?;
            unsafe { pickup::route(&*weapon, Some(hand)) }.map_err(SourceRefusal::Take)?;
            Ok(Freed {
                weapon,
                alive,
                source: TakeSource::World,
            })
        }
        WeaponSource::Rack { weapon, weak } => unsafe {
            rack::remote::release(hand, *weapon, weak)
        },
        // The snatch does not offer a rack's pistols.
        WeaponSource::RackMember(_) => Err(SourceRefusal::Gone),
        WeaponSource::Character(gun) => unsafe {
            steal::release(gun, |owner, weapon| {
                pickup::preflight(owner, weapon, hand)
                    .map(|_| ())
                    .map_err(SourceRefusal::Take)
            })
        },
    }
}

/// Take `weapon`, which the snatch pulled in from `source` and lies in the world, into `hand`,
/// sliding the rest of the way into its grip over `slide_s`. Returns the slot it belongs to.
///
/// # Safety
///
/// `weapon` must be held alive, on the game thread before the world update.
pub(crate) unsafe fn deliver(
    hand: Hand,
    weapon: &mut WeaponBase,
    source: TakeSource,
    slide_s: f32,
) -> Result<WeaponSlot, Refusal> {
    unsafe {
        pickup::take(
            hand,
            weapon,
            TakeOptions {
                slide_s,
                source,
                delivery: Delivery::Remote,
            },
        )
    }
}
