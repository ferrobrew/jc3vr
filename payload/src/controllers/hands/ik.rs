//! The wrist IK targets: each controller-driven hand's wrist queued on HumanIK's `MAIN` pass at
//! the pre-solve seam, the entry of the pose-finalization pass (see
//! `docs/engine/character/humanik.md`).
//!
//! Per hand and tick:
//!
//! - the target is the wrist frame from [`crate::controllers::hands::pose`], composed into the
//!   world through the head and brought into the character's model space through its current world
//!   matrix;
//! - the position target overwrites the effector's translation, and the rotation target is the
//!   delta from the animated wrist frame (built from the animated knuckles) to the target frame,
//!   which the engine pre-multiplies onto the pre-solve orientation;
//! - both are queued at `UPPER_BODY` so the combined pass step does not depend on the order in
//!   which targets were queued, without interpolation or blend-out because they are re-queued every
//!   tick, and with the reaches scaled by the hand's tier weight;
//! - the position target's pull ramps up as the target nears the full length of the character's
//!   measured arm ([`reach::pull_ramp`]), so reach propagates into the shoulder and chest only when
//!   the arm alone cannot get there, and the body IK's pin on that shoulder yields by the same ramp
//!   so it does not hold the girdle back;
//! - the `SECONDARY` pass is switched off for the hand ([`crate::controllers::hands::secondary`])
//!   at that pass's own entry ([`before_secondary`]), after the state tasks that write its keys,
//!   and the wrist's current reach is zeroed there so that a stale `SECONDARY` target cannot pull
//!   it back.
//!
//! The wielded weapon rides the hand's attach bone, a child of the wrist, and the engine refreshes
//! the weapon's cached transform from that bone right after the `MAIN` solve, so the gun follows
//! the IK-driven wrist within the same tick without a joint override.
//!
//! Each tick's world-space targets are recorded ([`tick_records`]) for the render-time late update,
//! which corrects the rendered hands and weapon by the controllers' motion since. Before anything is
//! queued, the character's arms are measured from the pre-solve pose ([`rig::measure`]); after the
//! solve, each driven hand's shortfall (the target less the solved wrist) is published with the
//! solved joints for the late update's detach.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use glam::{Mat4, Vec3};
use jc3gi::{
    animation::ik::{Effector, Pass, SolveStep},
    character::character::{
        AnimationController, Character, CharacterLodFlags, Joint, SafeBoneIndex,
        get_Character_EnableHIK,
    },
    hash::hashlittle,
    types::math::Vector3,
};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        frame::{Hand, Pose},
        hands::{
            self, ActiveHands, late, pose as hand_pose, reach,
            rig::{self, TickArm},
            secondary::{self, SecondaryParts, Switched},
            tiers,
        },
        roomscale::stance::unarmed,
    },
    headpose,
    hooks::{self, character::body_ik},
};

/// The hands the controllers drive this tick, decided before the body IK queues its hand pins so
/// that a pin never competes with a controller target.
#[derive(Clone)]
pub struct HandPlan {
    active: Option<ActiveHands>,
    weights: [f32; 2],
}

impl HandPlan {
    /// Whether the controllers drive `hand` this tick, at any weight.
    pub fn drives(&self, hand: Hand) -> bool {
        self.weights[hand.index()] > 0.0
    }

    /// [`drives`](Self::drives) for both hands, indexed by [`Hand::index`].
    pub fn driven(&self) -> [bool; 2] {
        Hand::BOTH.map(|hand| self.drives(hand))
    }
}

/// Decide which hands the controllers drive this tick: hand IK on, the hands active
/// ([`hands::active_hands`]), the hand tracked, its tier weight above zero, and the character in
/// gameplay with the headpose driving it.
pub fn plan() -> HandPlan {
    let active = hands::active_hands()
        .filter(|a| a.config.hands.ik && hooks::in_gameplay() && headpose::is_active());
    let tier_weights = tiers::weights();
    let weights = Hand::BOTH.map(|hand| {
        active
            .as_ref()
            .and_then(|a| a.hands[hand.index()])
            .map_or(0.0, |_| tier_weights[hand.index()])
    });
    HandPlan { active, weights }
}

/// What the hand IK did for one hand on its last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandIkStatus {
    /// Whether position and rotation targets were queued.
    pub queued: bool,
    /// Why nothing was queued, when nothing was.
    pub skip: Option<HandIkSkip>,
    /// The tier weight the reaches were scaled by.
    pub weight: f32,
    /// The distance from the animated wrist to the queued target, metres.
    pub reach_distance: f32,
    /// The angle of the queued rotation target, degrees.
    pub rotation_deg: f32,
    /// The target's distance from the character's pre-solve shoulder joint, as a fraction of his
    /// measured arm length; `None` before the arm is measured.
    pub extension: Option<f32>,
    /// The pull the position target was queued with, after the tier weight.
    pub pull: f32,
    /// How far the body IK's shoulder pin on this side was released for the reach, `0..=1`.
    pub shoulder_yield: f32,
    /// Whether the `SECONDARY` pass's wrist target was switched off this tick.
    pub secondary_wrist_off: bool,
    /// Whether the weapon off-hand grip was switched off this tick.
    pub secondary_grip_off: bool,
    /// Whether a game system held a live `MAIN` target on the wrist, which the controller target
    /// replaced.
    pub overrode_live_target: bool,
}

/// Why no wrist target was queued for a hand.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HandIkSkip {
    /// Hand IK is off, the hands are inert, the hand is untracked, or its tier yields it fully.
    NotDriven,
    /// HumanIK is globally disabled or the character is at reduced LOD.
    IkGatedOff,
    /// No valid head anchor yet.
    NoAnchor,
    /// The wrist bone does not map to the expected effector.
    UnmappedWrist,
    /// The knuckle bones did not resolve, or the hand is degenerate.
    NoHandFrame,
}

impl HandIkSkip {
    pub const fn label(self) -> &'static str {
        match self {
            HandIkSkip::NotDriven => "not driven",
            HandIkSkip::IkGatedOff => "HumanIK gated off",
            HandIkSkip::NoAnchor => "no head anchor",
            HandIkSkip::UnmappedWrist => "wrist has no effector",
            HandIkSkip::NoHandFrame => "knuckle bones unresolved",
        }
    }
}

/// The last tick's per-hand status, indexed by [`Hand::index`].
pub fn status() -> [HandIkStatus; 2] {
    *STATUS.lock()
}

/// One hand's recorded target for a tick: the world-space wrist frame the IK was asked to reach,
/// and the weight it was asked at.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TickTarget {
    pub wrist: Pose,
    pub weight: f32,
}

/// The last two ticks' targets per hand: `previous` is the tick before `current`.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct TickRecords {
    pub previous: [Option<TickTarget>; 2],
    pub current: [Option<TickTarget>; 2],
}

/// The last two ticks' recorded targets.
pub fn tick_records() -> TickRecords {
    *TICKS.lock()
}

/// The local player character's address, published every tick for the render-phase weapon hook's
/// ownership test (which must not call into the engine from a render job).
pub(crate) fn local_character() -> usize {
    LOCAL_CHARACTER.load(Ordering::Relaxed)
}

/// Queue the wrist targets for the hands `plan` drives, switch the `SECONDARY` pass off for them,
/// and record the tick. Called at the entry of the pose-finalization pass, after the body IK has
/// published this tick's head anchors.
///
/// # Safety
///
/// `character` must be null or a live character; called from the pre-call finalize detour.
pub(crate) unsafe fn queue(character: *mut Character, plan: &HandPlan) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    LOCAL_CHARACTER.store(character as *mut Character as usize, Ordering::Relaxed);

    let smoothing = Config::lock_query(|c| c.controllers.hands.reach.shoulder_smoothing_s);
    let arms = match unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() } {
        Some(controller) => unsafe { rig::measure(character, controller, smoothing) },
        None => [None; 2],
    };
    let shoulders_pinned = body_ik::shoulders_driven();

    let mut status = [HandIkStatus::default(); 2];
    let mut targets = [None; 2];
    for hand in Hand::BOTH {
        status[hand.index()] = match (&plan.active, plan.drives(hand)) {
            (Some(active), true) => {
                let request = HandRequest {
                    hand,
                    weight: plan.weights[hand.index()],
                    arm: arms[hand.index()],
                    shoulder_pinned: shoulders_pinned[hand.index()],
                };
                unsafe { queue_hand(character, active, request) }.map_or_else(
                    |skip| HandIkStatus {
                        skip: Some(skip),
                        ..HandIkStatus::default()
                    },
                    |(status, target)| {
                        targets[hand.index()] = Some(target);
                        status
                    },
                )
            }
            _ => HandIkStatus {
                skip: Some(HandIkSkip::NotDriven),
                ..HandIkStatus::default()
            },
        };
    }

    // The switch acts for every driven hand, whether or not its target could be queued: the
    // controllers own the arm either way, and the game's secondary targets would otherwise move a
    // hand the player is holding elsewhere. It acts at the `SECONDARY` pass's entry, after the
    // state tasks that write the keys inside this pass.
    let suppress = plan
        .active
        .as_ref()
        .is_some_and(|active| active.config.hands.suppress_secondary_ik);
    SECONDARY_PLAN.set(Some(SecondaryPlan {
        character: character as *mut Character as usize,
        driven: if suppress { plan.driven() } else { [false; 2] },
    }));

    let mut ticks = TICKS.lock();
    ticks.previous = ticks.current;
    ticks.current = targets;
    drop(ticks);
    *STATUS.lock() = status;
}

/// Switch the `SECONDARY` pass off for the hands [`queue`] recorded as driven this tick, and switch
/// the off-hand weapon grip off for both hands while the weapon-agnostic body releases it
/// ([`unarmed::release_grip`]). Called at the entry of the `SECONDARY` pass, after every writer of
/// its keys, including the state tasks the pose-finalization pass runs after its entry.
///
/// # Safety
///
/// `character` must be null or a live character; called from the pre-call `SECONDARY` pass detour.
pub(crate) unsafe fn before_secondary(character: *mut Character) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let address = character as *mut Character as usize;
    let driven = SECONDARY_PLAN
        .take()
        .filter(|plan| plan.character == address)
        .map_or([false; 2], |plan| plan.driven);
    let release_grip = unarmed::release_grip();
    let mut switched = [Switched::default(); 2];
    for hand in Hand::BOTH {
        let parts = SecondaryParts {
            wrist_target: driven[hand.index()],
            weapon_grip: driven[hand.index()] || release_grip,
        };
        if parts.wrist_target || parts.weapon_grip {
            switched[hand.index()] =
                unsafe { secondary::switch_off(&mut character.m_Blackboard, hand, parts) };
        }
        if parts.wrist_target {
            secondary::release_wrist(&mut character.m_HIK, hand);
        }
    }
    let mut status = STATUS.lock();
    for hand in Hand::BOTH {
        let s = &mut status[hand.index()];
        s.secondary_wrist_off = switched[hand.index()].wrist_target;
        s.secondary_grip_off = switched[hand.index()].weapon_grip;
    }
}

/// Forget this thread's `SECONDARY` plan at the end of a pose-finalization pass, so that a pass
/// whose `SECONDARY` pass did not run (HumanIK switched off) leaves nothing for a later one.
pub(crate) fn end_finalize() {
    SECONDARY_PLAN.set(None);
}

/// Publish the post-solve pose the late update swings the forearm about, and the bone indices it
/// edits. Called at the end of the pose-finalization pass, after the solve.
///
/// # Safety
///
/// `character` must be null or a live character; called from the post-call `UpdatePropEffects`
/// detour.
pub(crate) unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let Some(controller) = (unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() })
    else {
        return;
    };
    let bones = Hand::BOTH.map(|hand| unsafe { late::HandBones::resolve(controller, hand) });
    let to_world = Mat4::from(character.m_WorldMatrixT1);
    let targets = TICKS.lock().current;
    let joints = Hand::BOTH.map(|hand| {
        let b = &bones[hand.index()];
        (b.elbow() != 0 && b.wrist() != 0).then(|| {
            let wrist = unsafe { joint(controller, b.wrist() as u32) }.0;
            late::ArmJoints {
                elbow: unsafe { joint(controller, b.elbow() as u32) }.0,
                wrist,
                miss: targets[hand.index()]
                    .map(|t| t.wrist.position - to_world.transform_point3(wrist))
                    .filter(|m| m.is_finite()),
            }
        })
    });
    late::publish_solved(bones, joints);
}

/// What one hand's targets are queued from, besides the active hands.
struct HandRequest {
    hand: Hand,
    /// The tier weight.
    weight: f32,
    /// The character's arm on this side as read this tick, if measured.
    arm: Option<TickArm>,
    /// Whether the body IK queued a position target on this side's shoulder this tick.
    shoulder_pinned: bool,
}

/// Queue one hand's targets. Returns the status and the recorded world target, or why nothing was
/// queued.
unsafe fn queue_hand(
    character: &mut Character,
    active: &ActiveHands,
    request: HandRequest,
) -> Result<(HandIkStatus, TickTarget), HandIkSkip> {
    let HandRequest {
        hand,
        weight,
        arm,
        shoulder_pinned,
    } = request;
    let cockpit = active.hands[hand.index()].ok_or(HandIkSkip::NotDriven)?;
    // The same gates the engine checks before the solve: a target queued with no solve is never
    // consumed or cleared.
    let hik_enabled = unsafe { *get_Character_EnableHIK() };
    if !hik_enabled
        || character
            .m_LodFlags
            .contains(CharacterLodFlags::REDUCED_LOD)
    {
        return Err(HandIkSkip::IkGatedOff);
    }
    if headpose::anchor().is_none() {
        return Err(HandIkSkip::NoAnchor);
    }
    let controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() }
        .ok_or(HandIkSkip::NoHandFrame)?;

    let (effector, wrist_safe) = match hand {
        Hand::Left => (Effector::LEFT_WRIST as i32, SafeBoneIndex::LEFT_HAND),
        Hand::Right => (Effector::RIGHT_WRIST as i32, SafeBoneIndex::RIGHT_HAND),
    };
    let wrist_bone = unsafe { character.GetSafeIndex(wrist_safe) };
    if unsafe { character.m_HIK.GetEffectorIdFromBoneIndex(wrist_bone) } != effector {
        log_once(
            &UNMAPPED_LOGGED,
            "a wrist bone does not map to its HumanIK effector",
        );
        return Err(HandIkSkip::UnmappedWrist);
    }

    let names = knuckle_names(hand);
    let index_bone = unsafe { bone_index(controller, names[0]) };
    let middle_bone = unsafe { bone_index(controller, names[1]) };
    let (Some(index_bone), Some(middle_bone)) = (index_bone, middle_bone) else {
        log_once(&KNUCKLES_LOGGED, "the knuckle bones did not resolve");
        return Err(HandIkSkip::NoHandFrame);
    };
    let (wrist_position, _) = unsafe { joint(controller, wrist_bone) };
    let (index_position, _) = unsafe { joint(controller, index_bone) };
    let (middle_position, _) = unsafe { joint(controller, middle_bone) };
    let animated_frame =
        hand_pose::wrist_frame_from_bones(hand, wrist_position, index_position, middle_position)
            .ok_or(HandIkSkip::NoHandFrame)?;

    let head = headpose::query();
    let body = headpose::xr::body_rotation();
    let world = hand_pose::compose_world(
        head.position,
        body,
        active.head.position,
        cockpit.wrist,
        active.world_scale,
    );
    let to_model = Mat4::from(character.m_WorldMatrixT1).inverse();
    let (_, target_rotation, target_position) = (to_model
        * Mat4::from_rotation_translation(world.orientation, world.position))
    .to_scale_rotation_translation();

    let overrode_live_target =
        unsafe { body_ik::effector_live_targeted(&character.m_HIK, effector) };
    if overrode_live_target {
        log_once(
            &LIVE_TARGET_LOGGED,
            "a game system holds a live MAIN target on a controller-driven wrist",
        );
    }

    let cfg = &active.config.hands;
    let reach_cfg = &cfg.reach;
    let extension = arm
        .filter(|a| a.length > 0.0)
        .map(|a| target_position.distance(a.shoulder_model) / a.length);
    let ramp = extension.filter(|_| reach_cfg.reach_pull).map_or(0.0, |e| {
        reach::pull_ramp(e, reach_cfg.pull_start, reach_cfg.pull_full)
    });
    let pull = (cfg.pull.max(ramp * reach_cfg.pull_max) * weight).clamp(0.0, 1.0);
    let hik = &mut character.m_HIK;
    let slot = effector as usize;
    let position = Vector3 {
        data: target_position.to_array(),
    };
    unsafe {
        hik.AddEffectorTargetPosition(
            effector,
            &position,
            SolveStep::UPPER_BODY,
            Pass::MAIN,
            false,
            0.0,
            false,
            0.0,
        );
    }
    hik.m_TargetReachT[slot] = (cfg.reach_t * weight).clamp(0.0, 1.0);
    hik.m_TargetPull[slot] = pull;

    // The body IK's shoulder target holds the girdle where the lean put it, which would cancel the
    // pull; it yields by the same ramp. Its reach was written this tick, so scaling it is safe.
    let shoulder_yield = if shoulder_pinned {
        (ramp * weight).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if shoulder_yield > 0.0 {
        let shoulder = match hand {
            Hand::Left => Effector::LEFT_SHOULDER,
            Hand::Right => Effector::RIGHT_SHOULDER,
        } as usize;
        hik.m_TargetReachT[shoulder] *= 1.0 - shoulder_yield;
    }

    let (axis, angle) = hand_pose::wrist_rotation_delta(target_rotation, animated_frame);
    let axis = if axis.is_finite() { axis } else { Vec3::Y };
    let axis = Vector3 {
        data: axis.to_array(),
    };
    unsafe {
        hik.AddEffectorTargetRotationVector(
            effector,
            angle,
            &axis,
            SolveStep::UPPER_BODY,
            Pass::MAIN,
            false,
            0.0,
            false,
            0.0,
        );
    }
    hik.m_TargetReachR[slot] = (cfg.reach_r * weight).clamp(0.0, 1.0);
    // A rotation target promotes the target resist to the live resist, so the slot is always
    // written, or a past writer's value leaks in.
    hik.m_TargetResist[slot] = (cfg.resist * weight).clamp(0.0, 1.0);

    Ok((
        HandIkStatus {
            queued: true,
            skip: None,
            weight,
            reach_distance: target_position.distance(wrist_position),
            rotation_deg: angle.to_degrees(),
            extension,
            pull,
            shoulder_yield,
            overrode_live_target,
            ..HandIkStatus::default()
        },
        TickTarget {
            wrist: world,
            weight,
        },
    ))
}

/// The model-space position and orientation of `bone`.
unsafe fn joint(controller: &mut AnimationController, bone: u32) -> (Vec3, glam::Quat) {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone, &mut joint) };
    let [x, y, z] = joint.m_Translation.data;
    let [qx, qy, qz, qw] = joint.m_Orientation.data;
    (Vec3::new(x, y, z), glam::Quat::from_xyzw(qx, qy, qz, qw))
}

/// A bone's skeleton index by name, or `None` when the lookup fails (the root index, which a
/// failed lookup can resolve to, or an implausible index).
pub(crate) unsafe fn bone_index(controller: &AnimationController, name: &str) -> Option<u32> {
    let index = unsafe { controller.GetBoneIndex(hashlittle(name.as_bytes()) as u32) };
    (index != 0 && index <= late::MAX_BONE_INDEX).then_some(index)
}

/// The index and middle knuckles (the proximal phalanges) of `hand`.
fn knuckle_names(hand: Hand) -> [&'static str; 2] {
    match hand {
        Hand::Left => ["LeftHandIndex1", "LeftHandMiddle1"],
        Hand::Right => ["RightHandIndex1", "RightHandMiddle1"],
    }
}

fn log_once(flag: &AtomicBool, message: &str) {
    if !flag.swap(true, Ordering::Relaxed) {
        tracing::warn!(target: "controllers", "hand IK: {message}");
    }
}

static STATUS: Mutex<[HandIkStatus; 2]> = Mutex::new(
    [HandIkStatus {
        queued: false,
        skip: None,
        weight: 0.0,
        reach_distance: 0.0,
        rotation_deg: 0.0,
        extension: None,
        pull: 0.0,
        shoulder_yield: 0.0,
        secondary_wrist_off: false,
        secondary_grip_off: false,
        overrode_live_target: false,
    }; 2],
);
static TICKS: Mutex<TickRecords> = Mutex::new(TickRecords {
    previous: [None; 2],
    current: [None; 2],
});
static LOCAL_CHARACTER: AtomicUsize = AtomicUsize::new(0);

/// The hands whose `SECONDARY` targets [`before_secondary`] switches off, recorded by [`queue`] for
/// the same pose-finalization pass, which runs on one thread.
#[derive(Copy, Clone)]
struct SecondaryPlan {
    /// The character the plan was made for.
    character: usize,
    driven: [bool; 2],
}

thread_local! {
    static SECONDARY_PLAN: Cell<Option<SecondaryPlan>> = const { Cell::new(None) };
}
static UNMAPPED_LOGGED: AtomicBool = AtomicBool::new(false);
static KNUCKLES_LOGGED: AtomicBool = AtomicBool::new(false);
static LIVE_TARGET_LOGGED: AtomicBool = AtomicBool::new(false);
