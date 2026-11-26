//! The hand-off between a hand that reaches a weapon lying in the world and the weapon handling
//! that takes it: why a direct pickup refuses a weapon ([`Refusal`], [`TakeRefusal`]), and the
//! weapon handling's side of the hand-off ([`PickupSink`]), which the physical hands call when a
//! pickup grab lands on a weapon.
//!
//! The weapon handling registers its sink at install ([`register_pickup_sink`]); calling through
//! it before then is a programming error and panics, and the controllers' install refuses to
//! finish without it ([`sink_registered`]).

use std::{fmt, sync::OnceLock};

use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;

/// Why a direct pickup did not take the weapon.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There is no local player.
    NoCharacter,
    /// The weapon is no longer lying in the world.
    NotDropped,
    /// The weapon an interaction offers is neither on its rack nor lying in the world: its raw state.
    NotOffered { state: i32 },
    /// The weapon's slot is out of range.
    UnknownSlot,
    /// A hand grenade: the game collects it by proximity.
    Grenade,
    /// The weapon gives a copy when picked up.
    InfinitePickup,
    /// The character's animation forbids a pickup now.
    AnimationForbids,
    /// The pickup ran, but the weapon is not in the hands afterwards.
    NotWielded,
    /// The hands cannot take it now.
    Hands(TakeRefusal),
    /// The weapon did not become the player's, or the table did not take it into the hand.
    NotTaken,
}

impl Refusal {
    /// Whether the game's use-item action should be tried instead: the direct pickup did not touch
    /// the weapon.
    pub fn falls_back(self) -> bool {
        !matches!(
            self,
            Refusal::NotWielded | Refusal::Hands(_) | Refusal::NotTaken
        )
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let why = match self {
            Refusal::NoCharacter => "there is no local player",
            Refusal::NotDropped => "the weapon is not lying in the world",
            Refusal::NotOffered { state } => {
                return write!(
                    f,
                    "weapon pickup: the offered weapon is neither on its rack nor lying in the world (state {state})"
                );
            }
            Refusal::UnknownSlot => "the weapon's slot is out of range",
            Refusal::Grenade => "a hand grenade is collected by the game",
            Refusal::InfinitePickup => "the weapon gives a copy when picked up",
            Refusal::AnimationForbids => "the animation forbids a pickup",
            Refusal::NotWielded => "the weapon is not in the hands after the pickup",
            Refusal::Hands(refusal) => return write!(f, "weapon pickup: {refusal}"),
            Refusal::NotTaken => "the table did not take the weapon into the hand",
        };
        write!(f, "weapon pickup: {why}")
    }
}

impl std::error::Error for Refusal {}

/// Why a take did not happen.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TakeRefusal {
    /// The grabbing hand holds something already.
    HandFull(Hand),
    /// The game's use-item pickup found no free hand.
    BothHandsFull,
}

impl fmt::Display for TakeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TakeRefusal::HandFull(hand) => {
                write!(f, "take: the {} hand holds something already", hand.label())
            }
            TakeRefusal::BothHandsFull => write!(f, "take: both hands hold something"),
        }
    }
}

impl std::error::Error for TakeRefusal {}

/// The weapon handling's side of the hand-off, called in this order for each weapon a pickup grab
/// lands on: [`take_ground`](PickupSink::take_ground), then [`record`](PickupSink::record), then,
/// for a refusal that [falls back](Refusal::falls_back), [`grant`](PickupSink::grant) before the
/// caller clicks the use-item action.
#[derive(Clone, Copy)]
pub struct PickupSink {
    /// Take a weapon lying in the world straight into a hand.
    ///
    /// # Safety
    ///
    /// As [`take_ground`].
    pub take_ground: unsafe fn(&GameTick<'_>, Hand, &WeaponBase) -> Result<(), Refusal>,
    /// Count a direct pickup, or a pickup left to the use-item action with its refusal.
    pub record: fn(Option<String>),
    /// Let the use-item click that follows through to the game's pickup.
    pub grant: fn(),
}

/// Register the weapon handling's sink; the first registration stands.
pub fn register_pickup_sink(sink: PickupSink) {
    if SINK.set(sink).is_err() {
        tracing::warn!("held: a pickup sink is already registered; the new one is ignored");
    }
}

/// Whether the weapon handling has registered its sink.
pub fn sink_registered() -> bool {
    SINK.get().is_some()
}

/// Take `weapon`, lying in the world, straight into `hand`.
///
/// # Safety
///
/// `weapon` must be live.
pub unsafe fn take_ground(
    game: &GameTick<'_>,
    hand: Hand,
    weapon: &WeaponBase,
) -> Result<(), Refusal> {
    unsafe { (sink().take_ground)(game, hand, weapon) }
}

/// A direct pickup happened, or was left to the game's use-item action because of `refusal`.
pub fn record(refusal: Option<String>) {
    (sink().record)(refusal)
}

/// Let the use-item clicks of a short window through to the game's pickup: the caller is clicking
/// the action to have the game pick up a weapon a hand reached.
pub fn grant() {
    (sink().grant)()
}

fn sink() -> &'static PickupSink {
    SINK.get()
        .expect("held: no pickup sink is registered; the controllers' install registers it")
}

static SINK: OnceLock<PickupSink> = OnceLock::new();
