//! What the weapon handling reads from the crates above it, declared in one place
//! (`docs/architecture.md`, "Crate layering").
//!
//! The gameplay crate's install fills [`WeaponsContributions`] once, with one function per field,
//! so a registration that leaves one out fails to compile. The install's registration check
//! reports a missing registration ([`registered`]); the readers below find every field filled once
//! the install has run, and the crate's own tests, which register nothing, read none.

use std::{num::NonZeroUsize, sync::OnceLock};

use jc3vr_input::frame::Hand;

/// The functions the weapon handling calls in the crates above it.
#[derive(Debug, Clone, Copy)]
pub struct WeaponsContributions {
    /// Called after each shot of the mounted gun the hands aim
    /// ([`jc3vr_body::controllers::facts::mounted`]), on the thread that updates the weapon.
    pub mounted_shot: fn(),
    /// Called with the hand and why for each refused take of a weapon, on the game thread where the
    /// refusal is made, outside the take's own lock.
    pub take_refused: fn(Hand, &str),
    /// The loose object each hand carries, by [`Hand::index`]: a prop the physical hands hold,
    /// which the loadout table records as the hand's.
    pub held_props: fn() -> [Option<NonZeroUsize>; 2],
}

/// Register the contributions. One registration: a second one is refused and logged.
pub fn register(contributions: WeaponsContributions) {
    if CONTRIBUTIONS.set(contributions).is_err() {
        tracing::error!(
            "weapons: the contributions are already registered; keeping the first registration"
        );
    }
}

/// Whether the contributions are registered.
pub fn registered() -> bool {
    CONTRIBUTIONS.get().is_some()
}

/// The registered contributions, or `None` before the install (and in the crate's own tests).
pub(crate) fn get() -> Option<&'static WeaponsContributions> {
    CONTRIBUTIONS.get()
}

static CONTRIBUTIONS: OnceLock<WeaponsContributions> = OnceLock::new();
