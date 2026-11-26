//! The body IK's tuning, the `[body_ik]` section of the character tuning file
//! ([`CHARACTER`](crate::hooks::character::tuning::CHARACTER)): the HumanIK effector weights, the
//! lean model's shares and limits, and the view's floor.

use glam::Vec3;
use jc3vr_core::config::engine::{Invalid, nonnegative, range, vec3};
use serde::Deserialize;

/// The body IK's tuning.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyIkTuning {
    /// The translation-reach weight written to `m_TargetReachT[head]` (scaled by
    /// [`weight`](Self::weight)): how strongly the positional target pulls the upper body toward the
    /// head world target. Full reach: with body IK on, the solver — not the
    /// `UpdatePropEffects` override — is what places the head, so anything less leaves the rendered
    /// head short of where the player's head actually is.
    pub head_reach_t: f32,
    /// The rotation-reach weight written to `m_TargetReachR[head]` (scaled by
    /// [`weight`](Self::weight)) for the head's rotation target: how strongly the head is oriented
    /// toward the headpose forward.
    pub head_reach_r: f32,
    /// The pull weight written to `m_TargetPull[head]` (scaled by [`weight`](Self::weight)): how
    /// much reaching the head target propagates *down the chain* into the shoulders, chest, and
    /// spine rather than being absorbed by the neck alone. This is the knob that makes the upper
    /// body follow the head; with it at zero the solver reaches the target locally and the torso
    /// stays on the animated pose.
    pub head_pull: f32,
    /// The resistance weight written to `m_TargetResist[head]` (scaled by [`weight`](Self::weight))
    /// for the head's rotation target: how much the head effector resists being displaced by other
    /// effectors' pull, so the game's own aim and hand targets do not drag it off the headpose.
    pub head_resist: f32,
    /// How much of the player's lean the torso takes: the share applied to the swing about the base
    /// of the spine that would carry the animated head to where the player's head actually is. `1.0`
    /// articulates the spine the whole way, leaving nothing for the neck; lower values split the
    /// lean between the two. Being a rotation about the spine's base, it handles a lean in any
    /// direction — sideways, forward, or a duck — from the one construction, and it can never
    /// telescope the spine the way a straight displacement would.
    pub lean_share: f32,
    /// Where the torso pivots, between the base of the spine (`0.0`) and the hips (`1.0`).
    ///
    /// No single point suits both directions. A sideways lean is genuinely spinal, and the spine's
    /// base is the joint that bends. A forward lean is mostly a hip hinge, and pivoting up in the
    /// back tips the chest without carrying the torso forward, which reads as the lean barely doing
    /// anything. Dropping the pivot trades angle for travel: the same head displacement resolves to
    /// a smaller rotation about a longer arm, so the shoulders move further.
    pub pivot_drop: f32,
    /// The maximum torso lean, in degrees — an anatomical limit on the spine's range, and the bound
    /// that keeps the lean model inside the region it is valid over. The swing's angle grows toward
    /// 180 degrees as the head approaches the height of the pivot, about an axis that becomes
    /// ill-conditioned there, so without a cap a head dropped to waist height would fold the torso
    /// over backwards in a near-arbitrary direction.
    pub lean_max_deg: f32,
    /// How much of the head's body-relative yaw the torso takes, as a rotation about the spine base's
    /// vertical axis. Composes with [`lean_share`](Self::lean_share) into a single torso rotation.
    pub yaw_share: f32,
    /// The translation-reach weight written to `m_TargetReachT[shoulder]` (scaled by
    /// [`weight`](Self::weight)) for each shoulder carried by the torso rotation.
    pub shoulder_reach_t: f32,
    /// The pull weight written to `m_TargetPull[shoulder]` (scaled by [`weight`](Self::weight)): how
    /// much the shoulders' displacement carries on down into the chest and spine.
    pub shoulder_pull: f32,
    /// The translation-reach weight written to `m_TargetReachT[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each pinned hand: how rigidly the hands keep their animated
    /// position as the shoulders move.
    pub hand_reach_t: f32,
    /// The rotation-reach weight written to `m_TargetReachR[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each pinned hand: how rigidly the hands keep their animated
    /// *orientation*, as opposed to merely their position. Holding
    /// position alone lets the solver reach it from any wrist orientation, which spins the hands in
    /// place and takes the wielded weapon's aim with them.
    pub hand_reach_r: f32,
    /// The resistance weight written to `m_TargetResist[wrist]` (scaled by
    /// [`weight`](Self::weight)) for each pinned hand: how much the wrist refuses to be displaced by
    /// the shoulders' pull. Zero — the reach weights are the primary hold, and this is
    /// the knob to reach for if the hands still drift under a strong lean.
    pub hand_resist: f32,
    /// The rotation-reach weight written to `m_TargetReachR[chestEnd]` (scaled by
    /// [`weight`](Self::weight)), orienting the chest by the same torso rotation that places the
    /// shoulders.
    pub chest_reach_r: f32,
    /// A master multiplier on every reach, pull, and resist weight (`0..=1`).
    pub weight: f32,
    /// An optional character-model-space offset added to the head target position, for tuning where
    /// the body reaches relative to the headpose point. Zero.
    pub target_offset: Vec3,
    /// How far above the ground the view's head stays at the least, metres: the eyes hang a little
    /// below and in front of it, and the near plane needs a few centimetres past them.
    pub view_floor_clearance_m: f32,
    /// The farthest the character's root may move between two ticks for the synthetic head frame's
    /// filter to carry on, metres. A longer step is a teleport, and the filter starts over.
    pub max_root_step_m: f32,
}

impl BodyIkTuning {
    pub(crate) fn validate(&self) -> Result<(), Invalid> {
        let weights = [
            ("body_ik.head_reach_t", self.head_reach_t),
            ("body_ik.head_reach_r", self.head_reach_r),
            ("body_ik.head_pull", self.head_pull),
            ("body_ik.head_resist", self.head_resist),
            ("body_ik.lean_share", self.lean_share),
            ("body_ik.pivot_drop", self.pivot_drop),
            ("body_ik.yaw_share", self.yaw_share),
            ("body_ik.shoulder_reach_t", self.shoulder_reach_t),
            ("body_ik.shoulder_pull", self.shoulder_pull),
            ("body_ik.hand_reach_t", self.hand_reach_t),
            ("body_ik.hand_reach_r", self.hand_reach_r),
            ("body_ik.hand_resist", self.hand_resist),
            ("body_ik.chest_reach_r", self.chest_reach_r),
            ("body_ik.weight", self.weight),
        ];
        for (name, value) in weights {
            range(name, value, 0.0, 1.0)?;
        }
        range("body_ik.lean_max_deg", self.lean_max_deg, 0.0, 90.0)?;
        vec3("body_ik.target_offset", self.target_offset, 0.5)?;
        nonnegative(
            "body_ik.view_floor_clearance_m",
            self.view_floor_clearance_m,
            1.0,
        )?;
        range(
            "body_ik.max_root_step_m",
            self.max_root_step_m,
            0.01,
            1_000.0,
        )
    }
}
