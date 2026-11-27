//! The shutdown participants of the subsystems that receive native callbacks from the engine into
//! payload code, and the steps that bracket their detours at hook installation and removal.
//!
//! Three subsystems receive them: inventory spawns, the magazine executor's serial callback, and
//! the human hull's collider callbacks (with the grenade locker's). Each registers a participant so
//! shutdown waits for its work to drain, opens its admission only once its detour is live, and,
//! after every hook is removed, proves no callback is still running in the image before it may be
//! unmapped.
//!
//! The steps after hook removal run in registration order and stop at the first that refuses, so
//! the magazine executor's admission closes first: it never refuses, and it must close even when a
//! check after it keeps the image mapped. The participants prepare for shutdown in registration
//! order, and the steps after enabling open admission in theirs.

use std::sync::Arc;

use jc3vr_body::hooks::character::magazine;
use jc3vr_core::{
    hooks::{register_post_enable, register_post_unpatch},
    lifecycle::{self, RegistrationError},
};
use jc3vr_physical::controllers::physical_hands::{human_hull, human_hull::HumanHullShutdown};
use jc3vr_weapons::controllers::weapons::{
    grenade, inventory::spawn_safety, reload::shutdown as magazine_shutdown,
};

/// Register the participants and their installation and removal steps. An error refuses hook
/// installation, and the refusal is already logged.
pub(crate) fn register() -> Result<(), RegistrationError> {
    if let Err(error) =
        lifecycle::register_participant(spawn_safety::PARTICIPANT, spawn_safety::participant())
    {
        tracing::error!(%error, "inventory spawn installation refused without shutdown preparation");
        return Err(error);
    }
    if let Err(error) = lifecycle::register_participant(
        magazine_shutdown::PARTICIPANT,
        magazine_shutdown::participant(),
    ) {
        tracing::error!(%error, "hook installation refused without shutdown preparation");
        return Err(error);
    }
    if let Err(error) =
        lifecycle::register_participant(human_hull::PARTICIPANT, Arc::new(HumanHullShutdown))
    {
        tracing::error!(%error, "human hull installation refused without shutdown preparation");
        return Err(error);
    }
    register_post_enable(magazine_shutdown::activate);
    register_post_enable(spawn_safety::activate);
    register_post_unpatch(magazine_shutdown::deactivate_after_unpatch);
    register_post_unpatch(spawn_callbacks_returned);
    register_post_unpatch(native_callbacks_returned);
    Ok(())
}

fn spawn_callbacks_returned() -> bool {
    match spawn_safety::post_unpatch_check() {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(%error, "hook removal: inventory spawn callbacks require the image to remain mapped");
            false
        }
    }
}

fn native_callbacks_returned() -> bool {
    if magazine::active_entries() != 0
        || human_hull::active_entries() != 0
        || grenade::locker_active_entries() != 0
    {
        tracing::error!(
            "native callback remains active after hook removal; the image must remain mapped"
        );
        return false;
    }
    true
}
