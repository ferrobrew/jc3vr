//! The wingsuit's stall flip removed, and its barrel roll with the evade off, while the head pose
//! comes from the headset.
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
//! player flies one. It is applied while the head pose comes from the headset,
//! and reverted otherwise, through the hooks' patcher, which also reverts it on unload.
//!
//! The evade is a barrel roll ([`ACT_BOOST_WINGSUIT_BARREL_ROLL`]): the clip rolls the body a full
//! turn and carries it sideways. The arms throw it
//! (`controllers::vehicles::wingsuit_evade`), and the view rolls with the body. With the
//! evade switched off, it is a roll the player never asked for: the air physics queues it straight
//! on the animated model, so the `Character::QueueAct` filters never see it, and the animated
//! model's act queue asks [`drops_act`] instead, which drops it for the local player's model while
//! the head pose comes from the headset. The roll's directions and recovery are segments of its
//! clip, so the one act covers every evade.
//!
//! [`WingsuitBlendState_StallFlagSet`]: jc3gi::input::parachute::get_WingsuitBlendState_StallFlagSet
//! [`ACT_BOOST_WINGSUIT_BARREL_ROLL`]: jc3gi::animation::symbol_table::EventIdSymbolTable::ACT_BOOST_WINGSUIT_BARREL_ROLL

use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use jc3gi::{
    animation::symbol_table::EventIdSymbolTable, character::character::AnimatedModel,
    input::parachute::get_WingsuitBlendState_StallFlagSet,
};
use jc3vr_core::config::engine::ConfigSlot;
use jc3vr_view::{headpose, headpose::Source};
use serde::{Deserialize, Serialize};

use crate::{
    controllers::body::procedural::signals::is_local_model, hooks::character::resolve_act_id,
};

/// The boost wingsuit's evade, at `controllers.vehicles.wingsuit_evade` in the settings file: a
/// group the vehicles' settings hold, which the barrel-roll filter reads.
pub static WINGSUIT_EVADE_CONFIG: ConfigSlot<WingsuitEvadeConfig> =
    ConfigSlot::new("controllers.vehicles.wingsuit_evade");

/// The boost wingsuit's barrel-roll evade. Its throw is the vehicles' tuning.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WingsuitEvadeConfig {
    /// Throw the evade with the arms, and let the game's own evade roll the player's body. Off, the
    /// arms throw nothing and every barrel roll of the player's body is dropped, the gamepad's too.
    pub enabled: bool,
}

impl WingsuitEvadeConfig {
    pub const fn new() -> Self {
        Self { enabled: true }
    }
}

impl Default for WingsuitEvadeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the stall flip is removed now.
pub fn suppressed() -> bool {
    PATCHED.load(Ordering::Relaxed)
}

/// How many barrel rolls [`drops_act`] has dropped.
pub fn barrel_rolls_dropped() -> u64 {
    BARREL_ROLLS_DROPPED.load(Ordering::Relaxed)
}

/// Whether the animated model's act queue should drop `act`, queued on `model`: the barrel roll,
/// on the local player's model, while the head pose comes from the headset and the evade is
/// switched off. Called for every model's every act, so another model is turned away by its address
/// first.
pub(crate) fn drops_act(model: *const AnimatedModel, act: i32) -> bool {
    if !is_local_model(model) || barrel_roll_id() != Some(act) {
        return false;
    }
    let wanted = headpose::is_active()
        && headpose::source() == Source::Vr
        && !WINGSUIT_EVADE_CONFIG.query(|evade| evade.enabled);
    if wanted {
        BARREL_ROLLS_DROPPED.fetch_add(1, Ordering::Relaxed);
        tracing::info!(
            target: "controllers",
            "wingsuit: dropped the barrel roll, the evade is switched off"
        );
    }
    wanted
}

/// Apply or revert the patch to match the head pose's source. Called once per
/// frame on the game thread; it writes only when the wanted state changes.
pub fn sync() {
    let wanted = headpose::is_active() && headpose::source() == Source::Vr;
    if wanted == PATCHED.load(Ordering::Relaxed) {
        return;
    }
    let Some(mut patcher) = jc3vr_core::hooks::patcher() else {
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

static BARREL_ROLLS_DROPPED: AtomicU64 = AtomicU64::new(0);

/// The barrel roll's runtime id, once the act table is live. The name is an `NCharacter` id the
/// game registers at startup, so resolving it never adds a name to the table.
fn barrel_roll_id() -> Option<i32> {
    if let Some(id) = BARREL_ROLL_ID.get() {
        return Some(*id);
    }
    let id = resolve_act_id(EventIdSymbolTable::ACT_BOOST_WINGSUIT_BARREL_ROLL)?;
    Some(*BARREL_ROLL_ID.get_or_init(|| id))
}

static BARREL_ROLL_ID: OnceLock<i32> = OnceLock::new();
