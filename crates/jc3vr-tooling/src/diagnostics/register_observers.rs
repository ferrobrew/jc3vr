//! Registers the samplers that observe the game where it acts: Rico's pose at the character hooks'
//! pose-finalization observer ([`jc3vr_gameplay::hooks::character::register_pose_observer`]), and
//! the bodies around him after each simulation step
//! ([`jc3vr_gameplay::hooks::sim::register_post_step`]).

use jc3vr_gameplay::hooks::{
    character::{PoseObserver, register_pose_observer},
    sim::register_post_step,
};

use crate::diagnostics::{bodies, pose};

pub(super) fn register() {
    register_pose_observer(PoseObserver {
        before_solve: pose::sample_pre_ik,
        after_pass: pose::sample_final,
    });
    register_post_step(bodies::after_step);
}
