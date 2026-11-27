//! The local player's act arbiter: the one place that decides which of the local player's character
//! acts are dropped, replaced, or queued.
//!
//! The owners are the mod's subsystems that touch the player's acts ([`Owner`]), ordered by one
//! table ([`PRECEDENCE`]). A claim is a value: an owner, an act, what to do with it
//! ([`ActAction`]: drop, replace, or queue), and how long it stands ([`Expiry`]).
//!
//! - Filtering: both act-queue detours ask [`filter_character`] or [`filter_model`], which ask the
//!   filters at that entry ([`Entry`]) in precedence order; the first claim wins, and a later owner
//!   is not asked. [`Character::QueueAct`] forwards to [`AnimatedModel::QueueAct`], so an act queued
//!   on the character passes the character entry and then the model entry.
//! - Queuing: an owner that queues an act of its own [`submit`]s it and calls [`flush`], which is the
//!   only code that queues an act on, or pre-flights one for, the local player. Its acts go through
//!   [`Character::QueueAct`], so the filters see them too.
//!
//! Acts the mod queues on other characters (a punched rider, a seized gunner) are not the local
//! player's and stay with their subsystems.
//!
//! [`Character::QueueAct`]: jc3gi::character::character::Character::QueueAct
//! [`AnimatedModel::QueueAct`]: jc3gi::character::character::AnimatedModel::QueueAct

mod arbiter;
mod claim;

pub(crate) use crate::controllers::acts::arbiter::{filter_character, filter_model};
pub use crate::controllers::acts::{
    arbiter::{Flushed, flush, submit},
    claim::{
        ActAction, ActClaim, ActDecision, ActId, Entry, Expiry, Guard, Owner, PRECEDENCE, Refusal,
        Role,
    },
};

#[cfg(test)]
mod tests;
