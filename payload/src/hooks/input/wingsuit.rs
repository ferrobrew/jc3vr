//! The wingsuit's stall flip removed, under
//! [`suppress_wingsuit_stall_flip`](crate::hooks::input::MovementConfig::suppress_wingsuit_stall_flip).
//!
//! When the wingsuit slows with its nose raised, the wingsuit's blend state plays a stall clip in
//! place of the flight blend, and with the nose raised further a full loop that recovers from it.
//! The physics body never loops: the loop is the clip's own rotation of the pose, so in VR the
//! body turns a circle the player never gestured, while the arms the player flies with stay where
//! they are. The blend state decides to stall through a single instruction that sets its stalling
//! flag ([`WingsuitBlendState_StallFlagSet`]); with its immediate patched to zero the blend state
//! never stalls, so neither clip plays and the stall's gates on the air brake and the wingsuit's
//! close act stay open. The player who pitches up and slows simply loses lift and drops, as the air
//! physics has them.
//!
//! The patch is a byte in the game's code, so it acts for every character in a wingsuit; only the
//! player flies one. It is applied while the toggle is on and the head pose comes from the headset,
//! and reverted otherwise, through the hooks' patcher, which also reverts it on unload.
//!
//! [`WingsuitBlendState_StallFlagSet`]: jc3gi::input::parachute::get_WingsuitBlendState_StallFlagSet

use std::sync::atomic::{AtomicBool, Ordering};

use jc3gi::input::parachute::get_WingsuitBlendState_StallFlagSet;

use crate::{
    config::Config,
    headpose::{self, Source},
};

/// Whether the stall flip is removed now.
pub fn suppressed() -> bool {
    PATCHED.load(Ordering::Relaxed)
}

/// Apply or revert the patch to match the toggle and the head pose's source. Called once per
/// frame on the game thread; it writes only when the wanted state changes.
pub(crate) fn sync() {
    let wanted = Config::lock_query(|c| c.movement.suppress_wingsuit_stall_flip)
        && headpose::is_active()
        && headpose::source() == Source::Vr;
    if wanted == PATCHED.load(Ordering::Relaxed) {
        return;
    }
    let Some(mut patcher) = crate::hooks::patcher() else {
        return;
    };
    // SAFETY: the extern names the immediate byte of an instruction in the game's code, which the
    // patcher makes writable for the write and restores on unpatch; a single byte is written whole.
    unsafe {
        let address = (get_WingsuitBlendState_StallFlagSet() as *mut u8) as usize;
        if wanted {
            patcher.patch(address, &[0]);
        } else {
            patcher.unpatch(address);
        }
    }
    PATCHED.store(wanted, Ordering::Relaxed);
    tracing::info!(
        target: "controllers",
        suppressed = wanted,
        "wingsuit: {} the stall flip",
        if wanted { "removed" } else { "restored" }
    );
}

static PATCHED: AtomicBool = AtomicBool::new(false);
