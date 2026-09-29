//! The game's precision aim, refused under the controller scheme.
//!
//! Precision aim does three things the headset cannot use: [`Player::SetAimMode`] adds the
//! precision-aim camera hint (a main-camera zoom), and the sniper rifle's own aim-mode override
//! shows the 2D sniper overlay and hides the holder, which hides the controller-driven hands with
//! it. Every entry into precision aim goes through [`Player::SetAimMode`] (the aim input, the sniper
//! rifle's return after a bolt-action cycle, and the guided rocket launcher's shot), so refusing
//! precision aim there closes every path at once, and the mode, the camera hint, the aim control's
//! aim type, and the weapon's own mode all stay on normal aim. The lens on the scope
//! ([`crate::controllers::weapons::scope`]) replaces the zoom.
//!
//! When the controller scheme takes over while the player is already in precision aim, the game's
//! own per-tick exit check ([`Player::UpdateAimMode`]) is followed by an exit of the mod's, through
//! the same setter, so the overlay and the hidden body are undone as the game undoes them.

use std::sync::atomic::{AtomicU64, Ordering};

use detours_macro::detour;
use jc3gi::character::player::{AimMode, Player};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, controllers::scheme};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_AIM_MODE_BINDER)
        .with_static_binder(&UPDATE_AIM_MODE_BINDER)
}

/// The suppression's counts since the payload loaded, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct SuppressCounters {
    /// Requests to enter precision aim that were refused.
    pub refused: u64,
    /// Exits from precision aim the mod made.
    pub exited: u64,
}

pub fn counters() -> SuppressCounters {
    SuppressCounters {
        refused: REFUSED.load(Ordering::Relaxed),
        exited: EXITED.load(Ordering::Relaxed),
    }
}

/// Whether precision aim is refused now.
pub fn suppressing() -> bool {
    Config::lock_query(|c| {
        c.controllers.enabled && c.controllers.weapons.scope.suppress_precision_aim
    }) && scheme::is_controllers()
}

/// The player's aim-mode setter. Passed through unless it would enter precision aim while the
/// suppression acts. The mode is taken as its raw value, since the refusal compares it before the
/// game has validated it.
#[detour(address = jc3gi::character::player::Player::SetAimMode_ADDRESS)]
fn set_aim_mode(player: *mut Player, mode: i32, add_camera_hint: bool) {
    if mode == AimMode::AIM_PRECISION as i32 && suppressing() {
        REFUSED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    SET_AIM_MODE
        .get()
        .unwrap()
        .call(player, mode, add_camera_hint);
}

/// The player's per-tick check that drops out of precision aim when it is no longer possible,
/// followed by the mod's own exit while the suppression acts.
#[detour(address = jc3gi::character::player::Player::UpdateAimMode_ADDRESS)]
fn update_aim_mode(player: *mut Player) {
    UPDATE_AIM_MODE.get().unwrap().call(player);
    // SAFETY: the game passes its live player; the aim mode is read as its raw value.
    let precision = unsafe { player.as_ref() }.is_some_and(|p| {
        let mode = unsafe { (&raw const p.m_AimMode).cast::<i32>().read() };
        mode == AimMode::AIM_PRECISION as i32
    });
    if precision && suppressing() {
        EXITED.fetch_add(1, Ordering::Relaxed);
        SET_AIM_MODE
            .get()
            .unwrap()
            .call(player, AimMode::AIM_NORMAL as i32, true);
    }
}

static REFUSED: AtomicU64 = AtomicU64::new(0);
static EXITED: AtomicU64 = AtomicU64::new(0);
