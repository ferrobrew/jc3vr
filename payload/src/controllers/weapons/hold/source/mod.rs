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
//!   with its bodies, the line pulls it in, and the catching hand takes it ([`deliver`]). A source
//!   that makes the weapon it gives (a rack's copy, a pair's staged pistol) gives it a few frames
//!   later ([`poll`]), placed where it hung.

use std::{fmt, num::NonZeroUsize};

use jc3gi::{
    character::inventory::WeaponSlot,
    equipment::weapon::{WeaponBase, WeaponState},
};

use crate::controllers::{
    frame::Hand,
    loadout::{Delivery, TakeSource},
    weapons::{
        dual_wield::single,
        engine::{self, slot_of},
        hold::{
            SNAP_S,
            pickup::{self, Refusal, TakeOptions},
            rack::{self, paired::Member},
            refs::{Strong, Weak},
            steal::{self, CharacterGun},
            world::{TargetKind, WorldTarget},
        },
        inventory::engine::{strong_to, weak_to, weapon_of},
    },
};

/// Where a weapon a hand takes is.
pub(crate) enum WeaponSource {
    /// Lying in the world with its bodies.
    Ground { weak: Weak },
    /// Held out by its placement rule: on a rack, or set down at a spot (a locker's). `gun` is the
    /// gun aimed at, the first or the second of a rack's pistol pair, for the snatch, which takes a
    /// pair's pistols by their rack's weapon.
    Rack {
        weapon: NonZeroUsize,
        weak: Weak,
        gun: u8,
    },
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
                Ok(WeaponSource::Rack {
                    weapon,
                    weak,
                    gun: 0,
                })
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
    /// A rack's copies and pistol members are closed until the game restarts: an earlier request
    /// could not be proven retired.
    CopiesClosed,
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
            SourceRefusal::CopiesClosed => write!(
                f,
                "take from a source: a rack's copies are closed until the game restarts"
            ),
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
            let (claim, admitted) = unsafe { rack::paired::claim(owner, hand, *member) }?;
            // SAFETY: the claim holds the pistol alive.
            let weapon = unsafe { &mut *claim.weapon() };
            let taken = unsafe {
                pickup::deliver(owner, hand, weapon, admitted, options(TakeSource::Rack))
            };
            unsafe { claim.settle(taken.is_ok()) };
            taken.map_err(SourceRefusal::Take)
        }
        WeaponSource::Character(gun) => {
            let freed = unsafe {
                steal::release(gun, |owner, weapon| {
                    pickup::preflight(owner, weapon, hand, false)
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

/// What a source gave up for the snatch.
pub(crate) enum Yield {
    /// The weapon, lying in the world.
    Ready(Freed),
    /// A weapon the source is still making: a rack's copy, or a pistol of a pair not yet staged.
    /// [`poll`] collects it.
    Pending(Ticket),
}

/// A weapon a source is making for the snatch.
pub(crate) enum Ticket {
    /// The copy a rack with unlimited stock is spawning.
    Copy(Member),
    /// A pistol of the pair at `source`, staged and claimed once it is ready.
    Member { member: Member, source: Weak },
}

/// Free `source`'s weapon for the grapple's snatch into `hand`: the source gives it up into the
/// world, for the line to pull in, or starts making it. A take the hand could not make is refused
/// before anything leaves the source.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn free(hand: Hand, source: &WeaponSource) -> Result<Yield, SourceRefusal> {
    let player =
        unsafe { engine::local_player() }.ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
    let admit = |weapon: &WeaponBase, copy: bool| {
        unsafe { pickup::preflight(player, weapon, hand, copy) }
            .map(|_| ())
            .map_err(SourceRefusal::Take)
    };
    match source {
        WeaponSource::Ground { weak } => {
            let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
            let weapon = weapon_of(&alive);
            // SAFETY: the strong reference keeps the weapon alive.
            admit(unsafe { &*weapon }, false)?;
            Ok(Yield::Ready(Freed {
                weapon,
                alive,
                source: TakeSource::World,
            }))
        }
        WeaponSource::Rack { weapon, weak, gun } => {
            let alive = weak.lock().ok_or(SourceRefusal::Gone)?;
            // SAFETY: the strong reference keeps the weapon alive.
            let display = unsafe { &mut *weapon_of(&alive) };
            if !unsafe { rack::remote::waiting(display) } {
                return Err(SourceRefusal::Gone);
            }
            let racked = display.m_State == WeaponState::E_WS_WEAPON_RACK;
            let pistols =
                racked && unsafe { slot_of(display) } == Some(WeaponSlot::E_SLOT_DUAL_WIELD);
            if pistols && unsafe { rack::paired::prepare(player, display, alive) } {
                admit(display, true)?;
                let member =
                    rack::paired::member_for(weapon.get(), *gun).ok_or(SourceRefusal::Gone)?;
                tracing::info!(target: "controllers", hand = hand.label(), member = member.index,
                    "snatch: claiming a pistol of a rack's pair for the {} hand", hand.label());
                return Ok(Yield::Pending(Ticket::Member {
                    member,
                    source: weak.clone(),
                }));
            }
            if racked && display.m_InfinitePickup {
                admit(display, true)?;
                let member =
                    unsafe { rack::copy::request(player, hand, Delivery::Remote, display) }?;
                return Ok(Yield::Pending(Ticket::Copy(member)));
            }
            unsafe { rack::remote::release(hand, *weapon, weak) }.map(Yield::Ready)
        }
        // A pair's pistols reach the snatch as their rack's weapon, by the gun aimed at.
        WeaponSource::RackMember(_) => Err(SourceRefusal::Gone),
        WeaponSource::Character(gun) => {
            unsafe { steal::release(gun, |_, weapon| admit(weapon, false)) }.map(Yield::Ready)
        }
    }
}

/// The weapon `ticket` waits for, once its source has made it: `None` while it has not, else the
/// weapon lying in the world where it hung, or why it never will be.
///
/// # Safety
///
/// The game thread, before the world update.
pub(crate) unsafe fn poll(hand: Hand, ticket: &Ticket) -> Option<Result<Freed, SourceRefusal>> {
    match ticket {
        Ticket::Copy(member) => rack::copy::collect(*member),
        Ticket::Member { member, source } => {
            let result = unsafe { claim_remote(hand, *member, source) };
            result.transpose()
        }
    }
}

/// Claim the pair's pistol `member` for the snatch into `hand` once it is staged, and drop it into
/// the world where it hung. Keeps the pair observed meanwhile, so its staging goes on.
unsafe fn claim_remote(
    hand: Hand,
    member: Member,
    source: &Weak,
) -> Result<Option<Freed>, SourceRefusal> {
    let owner =
        unsafe { engine::local_player() }.ok_or(SourceRefusal::Take(Refusal::NoCharacter))?;
    let alive = source.lock().ok_or(SourceRefusal::Gone)?;
    // SAFETY: the strong reference keeps the weapon alive.
    let display = unsafe { &*weapon_of(&alive) };
    if !unsafe { rack::paired::prepare(owner, display, alive) } {
        return Err(SourceRefusal::Gone);
    }
    if !rack::paired::ready(member) {
        return Ok(None);
    }
    let (claim, _) = unsafe { rack::paired::claim(owner, hand, member) }?;
    // SAFETY: the claim holds the pistol alive.
    let weapon = unsafe { &mut *claim.weapon() };
    let Some(alive) = (unsafe { strong_to(weapon) }) else {
        unsafe { claim.settle(false) };
        return Err(SourceRefusal::Gone);
    };
    // The pistol has left its rack: a missed catch leaves it lying where it falls, with its share
    // of the pair's ammunition.
    unsafe { claim.settle(true) };
    unsafe { weapon.SetVisible(true, true, false) };
    unsafe { single::hide_extra(weapon) };
    // Into the world where it hung, with its bodies, for the line to pull in.
    unsafe { weapon.OnDrop() };
    tracing::info!(target: "controllers", hand = hand.label(), member = member.index,
        "snatch: freed a pistol of a rack's pair for the {} hand", hand.label());
    Ok(Some(Freed {
        weapon,
        alive,
        source: TakeSource::Rack,
    }))
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
