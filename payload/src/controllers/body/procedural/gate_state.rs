//! What the solve gate keeps between ticks ([`State`]): the rig, the torso's blend, the clavicles'
//! followers, the wingsuit's arms, and the status, behind one lock, with the steps of the gate that
//! read and update them.

use jc3gi::character::character::{AnimationController, Character};
use parking_lot::Mutex;

use crate::controllers::{
    body::procedural::{
        ArmContext, INITIAL_STATUS, ProceduralStatus,
        authority::{Case, TorsoBlend, WeightFollower},
        config::ProceduralBodyConfig,
        joints::{LiveRig, RigCache},
        rig::Rig,
        solver::{
            arms::Side,
            clavicles::{self, ClavicleFollower, ClavicleTuning, ClavicleTurn},
            pose::RigPose,
            wings::WingTarget,
        },
        state::StateTracker,
        targets::TargetFade,
        wings::WingQueue,
    },
    hands::ik::deferred_wrist_targets,
};

pub(super) struct State {
    /// The rig of the skeleton the animation controller holds.
    pub(super) rig: RigCache,
    /// The torso's weight against the animation.
    pub(super) blend: TorsoBlend,
    /// What the hand-over table reads from the character, with the history it keeps.
    pub(super) tracker: StateTracker,
    /// The case and the torso's target last logged.
    pub(super) logged: Option<(Case, f32)>,
    /// The clavicles' turns toward the hands.
    pub(super) clavicles: ClavicleFollower,
    /// The fade of the owned bones' IK targets.
    pub(super) fade: TargetFade,
    /// The case the torso was last logged for, while it lasts.
    pub(super) on_hips_logged: Option<Case>,
    /// The wingsuit's arms' weight against the animation.
    pub(super) wings: WeightFollower,
    /// The wingsuit's last wrist targets, model space, which fade where they were as the arms ease
    /// out.
    pub(super) last_wings: Option<[Option<WingTarget>; 2]>,
    /// This tick's wingsuit wrist targets, queued once the gate lets go of its state.
    pub(super) wing_queue: Option<WingQueue>,
    /// Whether the wingsuit's arms have been logged since they last eased in.
    pub(super) wing_logged: bool,
    /// This tick's arm context, for the hand IK the gate runs next.
    pub(super) arms: Option<ArmContext>,
    pub(super) status: ProceduralStatus,
}

impl State {
    /// The rig of `controller`'s skeleton, checked against it.
    pub(super) unsafe fn rig_for(&mut self, controller: &AnimationController) -> Option<LiveRig> {
        let lookup = unsafe { self.rig.get(controller) };
        if lookup.rebuilt {
            self.status.rig_error = lookup.error;
        }
        lookup.rig
    }

    /// The clavicles' turns this tick: toward the wrist targets the hand IK queues next, eased.
    ///
    /// # Safety
    ///
    /// As [`crate::controllers::body::procedural::at_solve_gate`].
    pub(super) unsafe fn turn_clavicles(
        &mut self,
        character: &Character,
        rig: &Rig,
        pose: &RigPose,
        dt: f32,
        config: &ProceduralBodyConfig,
    ) -> [ClavicleTurn; 2] {
        let wanted = {
            let targets = unsafe { deferred_wrist_targets(character) };
            let tuning = ClavicleTuning {
                elevation: config.clavicle_elevation_deg.to_radians(),
                depression: config.clavicle_depression_deg.to_radians(),
                protraction: config.clavicle_protraction_deg.to_radians(),
                reach_start: config.clavicle_reach_start,
            };
            Side::BOTH.map(|side| {
                targets[side.index()].map_or(ClavicleTurn::ZERO, |(target, weight)| {
                    clavicles::wanted(pose, rig, side, target, weight, &tuning)
                })
            })
        };
        let turns = self.clavicles.step(wanted, dt, config.clavicle_follow_s);
        self.status.clavicles = turns;
        turns
    }

    /// Log a change of the hand-over table's case, or of the torso's target within one (a flight's
    /// hold on the torso running out), with the torso's weight it leaves.
    pub(super) fn log_case(&mut self, case: Case, target: f32, weight: f32) {
        if self.logged == Some((case, target)) {
            return;
        }
        self.logged = Some((case, target));
        tracing::info!(
            target: "body",
            from = self.status.case.label(),
            to = case.label(),
            torso = target,
            torso_now = weight,
            "procedural body: the hand-over case changes",
        );
    }
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    rig: RigCache::new(),
    blend: TorsoBlend::new(),
    tracker: StateTracker::new(),
    logged: None,
    clavicles: ClavicleFollower::new(),
    fade: TargetFade::new(),
    on_hips_logged: None,
    wings: WeightFollower::new(),
    last_wings: None,
    wing_queue: None,
    wing_logged: false,
    arms: None,
    status: INITIAL_STATUS,
});
