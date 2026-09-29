//! Crouching, in the plan's stages.
//!
//! 1. The visual crouch: when the head drops, the hips drop with it and the feet stay planted, so
//!    that the body folds through the legs instead of bending the spine over. It extends the body
//!    IK's head target in the same HumanIK pass: a positional target on the hips effector, lowered
//!    by a share of the head's drop, and the ankle effectors pinned where the animation put them.
//!    The pass solves the full body, because the body IK's torso targets already raise the pass to
//!    the upper-body step and the pass takes the highest step queued; only the full-body step
//!    includes the legs as well.
//! 2. The capsule: the game's own crouching proxy state, requested through the character's pending
//!    proxy state, which the engine applies on the simulation thread after the state tasks run. The
//!    request is made only while the pending state is the character's default or already the
//!    crouch, so the game's own proxy states (swimming, the parachute, vehicles, ragdoll) always win,
//!    and it is released only when a ray up from the feet finds the headroom to stand.
//!
//! In the seated posture the crouch toggle lowers the eyes by a preset
//! ([`crate::controllers::roomscale::posture`]), which the visual crouch then folds the body for.

use jc3gi::{
    animation::ik::{Effector, HumanIK, Pass, SolveStep},
    character::character::{Character, CharacterLodFlags, Joint, get_Character_EnableHIK},
    physics::ProxyState,
    types::math::Vector3,
};

use crate::hooks::character::body_ik::bone_for_effector;

/// Whether the character counts as crouched, with hysteresis on the head's drop.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct CrouchTracker {
    crouched: bool,
}

impl CrouchTracker {
    pub const fn new() -> Self {
        Self { crouched: false }
    }

    /// Fold in the head's `drop` below the standing eye height (metres): crouched once it passes
    /// `enter_m`, standing once it is back above `exit_m`.
    pub fn update(&mut self, drop: f32, enter_m: f32, exit_m: f32) -> bool {
        let exit_m = exit_m.min(enter_m);
        self.crouched = if self.crouched {
            drop > exit_m
        } else {
            drop > enter_m
        };
        self.crouched
    }
}

/// How far the hips drop for a head `drop` (metres), taking `share` of it.
pub fn hips_drop(drop: f32, share: f32) -> f32 {
    if drop.is_finite() && share.is_finite() {
        drop.max(0.0) * share.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// What the capsule request writes this tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CapsuleStep {
    /// The value to write to the pending proxy state, if any.
    pub write: Option<i32>,
    /// Whether the mod holds the crouch request after the write.
    pub owned: bool,
}

/// Decide the capsule's pending proxy state. `want` is whether the character should be in the crouch
/// capsule, `owned` whether the mod's request is standing, and `pending` and `default` the
/// character's pending and default proxy states.
pub fn capsule_step(want: bool, owned: bool, pending: i32, default: i32) -> CapsuleStep {
    let crouching = ProxyState::CROUCHING as i32;
    match (want, pending) {
        // The game wants another state: stand down without touching it.
        (_, p) if p != default && p != crouching => CapsuleStep {
            write: None,
            owned: false,
        },
        // Re-asserted every tick, since a graph state's exit may restore the default mid-crouch.
        (true, p) if p == default => CapsuleStep {
            write: Some(crouching),
            owned: true,
        },
        (true, _) => CapsuleStep { write: None, owned },
        (false, p) if owned && p == crouching => CapsuleStep {
            write: Some(default),
            owned: false,
        },
        (false, _) => CapsuleStep {
            write: None,
            owned: false,
        },
    }
}

/// Queue the visual crouch's HumanIK targets for the local player, before the pose-finalization
/// pass solves: the hips lowered by `hips_drop` metres and the ankles pinned. Returns whether
/// targets were queued. Every engine pointer is null-guarded.
///
/// # Safety
///
/// `character` must be null or the live local character, at the entry of its pose-finalization
/// pass.
pub unsafe fn queue_ik(character: *mut Character, hips_drop: f32) -> bool {
    unsafe {
        let Some(character) = character.as_mut().filter(|c| c.m_IsLocalCharacter) else {
            return false;
        };
        if hips_drop.is_nan() || hips_drop <= 0.0 || !*get_Character_EnableHIK() {
            return false;
        }
        if character
            .m_LodFlags
            .contains(CharacterLodFlags::REDUCED_LOD)
        {
            return false;
        }
        let Some(controller) = character.m_AnimatedModel.m_AnimationController.as_ref() else {
            return false;
        };
        let hik = &mut character.m_HIK;
        let joint_position = |effector: i32| {
            let bone = bone_for_effector(hik, effector)?;
            let mut joint = Joint::default();
            controller.GetJoint(bone, &mut joint);
            Some(glam::Vec3::from_array(joint.m_Translation.data))
        };
        let hips_effector = Effector::HIPS as i32;
        let Some(hips) = joint_position(hips_effector) else {
            return false;
        };
        let ankles = [Effector::LEFT_ANKLE as i32, Effector::RIGHT_ANKLE as i32]
            .map(|effector| joint_position(effector).map(|p| (effector, p)));

        // Model space shares the character's up axis, so the drop is along -Y.
        queue_position(hik, hips_effector, hips - glam::Vec3::Y * hips_drop, 0.0);
        for (effector, position) in ankles.into_iter().flatten() {
            queue_position(hik, effector, position, 0.0);
        }
        true
    }
}

/// How fast the crouch targets' reach eases in and out, per second.
const REACH_RATE: f32 = 4.0;

/// Queue a full-reach positional target on `effector` in the `MAIN` pass, at the full-body step.
unsafe fn queue_position(hik: &mut HumanIK, effector: i32, position: glam::Vec3, pull: f32) {
    let Ok(index) = usize::try_from(effector) else {
        return;
    };
    let target = Vector3 {
        data: position.to_array(),
    };
    unsafe {
        hik.AddEffectorTargetPosition(
            effector,
            &target,
            SolveStep::FULL_BODY,
            Pass::MAIN,
            true,
            REACH_RATE,
            true,
            REACH_RATE,
        );
    }
    hik.m_TargetReachT[index] = 1.0;
    hik.m_TargetPull[index] = pull;
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: i32 = ProxyState::PLAYER_STANDING as i32;
    const CROUCH: i32 = ProxyState::CROUCHING as i32;
    const SWIM: i32 = ProxyState::SWIMMING as i32;

    #[test]
    fn the_tracker_has_hysteresis() {
        let mut t = CrouchTracker::default();
        assert!(!t.update(0.4, 0.45, 0.3));
        assert!(t.update(0.5, 0.45, 0.3));
        assert!(t.update(0.35, 0.45, 0.3));
        assert!(!t.update(0.25, 0.45, 0.3));
    }

    #[test]
    fn the_hips_take_their_share_of_a_drop() {
        assert!((hips_drop(0.5, 0.8) - 0.4).abs() < 1e-6);
        assert_eq!(hips_drop(-0.2, 0.8), 0.0);
        assert_eq!(hips_drop(f32::NAN, 0.8), 0.0);
    }

    #[test]
    fn a_crouch_is_requested_over_the_default_and_re_asserted() {
        let step = capsule_step(true, false, DEFAULT, DEFAULT);
        assert_eq!(step.write, Some(CROUCH));
        assert!(step.owned);
        let held = capsule_step(true, true, CROUCH, DEFAULT);
        assert_eq!(held.write, None);
        assert!(held.owned);
        let restored_by_the_graph = capsule_step(true, true, DEFAULT, DEFAULT);
        assert_eq!(restored_by_the_graph.write, Some(CROUCH));
    }

    #[test]
    fn the_games_own_states_win() {
        let step = capsule_step(true, true, SWIM, DEFAULT);
        assert_eq!(step.write, None);
        assert!(!step.owned);
        let release = capsule_step(false, true, SWIM, DEFAULT);
        assert_eq!(release.write, None);
    }

    #[test]
    fn standing_releases_only_the_mods_own_crouch() {
        let release = capsule_step(false, true, CROUCH, DEFAULT);
        assert_eq!(release.write, Some(DEFAULT));
        assert!(!release.owned);
        let not_ours = capsule_step(false, false, CROUCH, DEFAULT);
        assert_eq!(not_ours.write, None);
    }
}
