//! Animation hooks: hold clip time to suppress Rico's idle breathing.
//!
//! With the head driven by the HMD, the base idle clip's breathing and sway can move the body
//! independently of the player's tracked pose (issue #33). These hooks pass zero delta time to
//! `AnimationController::UpdateAnimationsTime` and `AnimationController::UpdateAnimations` while
//! suppression is enabled and the local player's active body rule state is the on-foot `S_IDLE`.
//! The engine selects between time-only advancement and synchronous pose evaluation. Each hook
//! still forwards its native call; holding clip time does not freeze graph state, blend weights, IK,
//! or the full pose.
//! Every other state and character receives the original delta time. The periodic idle fidget is
//! handled separately in `crate::hooks::input::locomotion`.

use std::sync::LazyLock;

use detours_macro::detour;
use jc3gi::{
    character::character::{AnimationController, Character},
    hash::hashlittle,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

/// The type id (`hashlittle("S_IDLE")`, the engine's own `HashString`) of the on-foot base idle rule
/// state, resolved via the game's own hash so it stays correct rather than hard-coding the value.
static S_IDLE_TYPE_ID: LazyLock<u32> = LazyLock::new(|| hashlittle(b"S_IDLE") as u32);

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&UPDATE_ANIMATIONS_TIME_BINDER)
        .with_static_binder(&UPDATE_ANIMATIONS_BINDER)
}

/// Hold clip time on the controller's time-only path when idle breathing suppression applies.
/// The native call and its initialized guard remain in place.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimationsTime_ADDRESS
)]
fn update_animations_time(controller: *mut AnimationController, dt: f32) {
    let dt = if unsafe { should_freeze(controller) } {
        0.0
    } else {
        dt
    };
    UPDATE_ANIMATIONS_TIME.get().unwrap().call(controller, dt);
    if let Some(controller) = unsafe { controller.as_ref() } {
        unsafe { crate::hooks::camera::authored::animation_update(controller, false) };
    }
}

/// Hold clip time during synchronous pose evaluation when idle breathing suppression applies.
/// `PoseProducer::Update` still evaluates the selected bones; graph, blend, and IK changes can
/// still change the pose.
#[detour(
    address = jc3gi::character::character::AnimationController::UpdateAnimations_ADDRESS
)]
fn update_animations(controller: *mut AnimationController, dt: f32, num_of_bones: i32) {
    let dt = if unsafe { should_freeze(controller) } {
        0.0
    } else {
        dt
    };
    UPDATE_ANIMATIONS
        .get()
        .unwrap()
        .call(controller, dt, num_of_bones);
    if let Some(controller) = unsafe { controller.as_ref() } {
        unsafe { crate::hooks::camera::authored::animation_update(controller, true) };
    }
}

/// Whether this controller belongs to the local player, that player's body rule state is the
/// on-foot `S_IDLE`, and breathing suppression is enabled. Controller identity is checked before
/// the config lock and state walk.
unsafe fn should_freeze(controller: *mut AnimationController) -> bool {
    unsafe {
        let Some(player) = Character::GetLocalPlayerCharacter().as_ref() else {
            return false;
        };
        idle_breathing_suppressed(
            std::ptr::eq(
                player.m_AnimatedModel.m_AnimationController as *const AnimationController,
                controller,
            ),
            || Config::lock_query(|c| c.movement.suppress_idle_breathing),
            || (active_state_type_id(player), *S_IDLE_TYPE_ID),
        )
    }
}

/// Lazy reads preserve the non-local and disabled fast paths without native calls in policy tests.
fn idle_breathing_suppressed(
    is_local_controller: bool,
    enabled: impl FnOnce() -> bool,
    state_ids: impl FnOnce() -> (Option<u32>, u32),
) -> bool {
    is_local_controller && enabled() && {
        let (active_state, idle_state) = state_ids();
        active_state == Some(idle_state)
    }
}

/// The character's active animation rule-state id (`hashlittle(state_name)`, e.g. `S_IDLE`), read
/// through its first (body) rule system's state machine, mirroring `CCharacter::IsInVehicleAttachState`.
/// `None` if any link in the chain is absent (loading, no rule system).
pub(crate) unsafe fn active_state_type_id(character: &Character) -> Option<u32> {
    unsafe {
        Some(
            character
                .m_AnimatedModel
                .m_RuleSystems
                .as_slice()
                .first()?
                .as_ref()?
                .m_StateMachineInstance
                .as_ref()?
                .m_CurrentState
                .as_ref()?
                .m_HashID
                .m_Hash,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn local_on_foot_idle_holds_clip_time_when_enabled() {
        for idle_state in [0, u32::MAX] {
            assert!(idle_breathing_suppressed(
                true,
                || true,
                || (Some(idle_state), idle_state),
            ));
        }
    }

    #[test]
    fn disabled_suppression_skips_the_state_read() {
        assert!(!idle_breathing_suppressed(
            true,
            || false,
            || panic!("disabled suppression must not read the state"),
        ));
    }

    #[test]
    fn other_body_rule_states_leave_clip_time_running() {
        for idle_state in [0_u32, u32::MAX] {
            assert!(!idle_breathing_suppressed(
                true,
                || true,
                || (Some(idle_state.wrapping_add(1)), idle_state),
            ));
        }
    }

    #[test]
    fn a_missing_body_rule_state_leaves_clip_time_running() {
        assert!(!idle_breathing_suppressed(
            true,
            || true,
            || (None, u32::MAX),
        ));
    }

    #[test]
    fn a_non_local_controller_skips_config_and_state_reads() {
        assert!(!idle_breathing_suppressed(
            false,
            || panic!("another controller must not read the config"),
            || panic!("another controller must not read the state"),
        ));
    }

    #[test]
    fn the_enabled_setting_is_read_before_the_state() {
        let config_read = Cell::new(false);
        assert!(idle_breathing_suppressed(
            true,
            || {
                config_read.set(true);
                true
            },
            || {
                assert!(config_read.get());
                (Some(17), 17)
            },
        ));
    }
}
