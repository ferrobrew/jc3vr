//! Finger curls on the rendered hands: the plan of record's finger curls
//! (`docs/mod/input/controllers-and-roomscale.md`), and the grip poses that wrap the fingers round
//! what the hand holds (`docs/mod/input/grip-poses.md`).
//!
//! - [`curl`]: each finger's curl and splay, from the tracked hand joints
//!   (`XR_EXT_hand_tracking`, located with the controller-conforming motion range when the runtime
//!   offers `XR_EXT_hand_joints_motion_range`) or estimated from the inputs the fingers rest on;
//! - [`grip`]: what each hand holds, as its sources declare it, and the pose for it;
//! - [`target`]: a hand's target shape for the tick, from its curls and its grip;
//! - [`pose`]: the shape turned into joint angles and the angles into model-space finger joints;
//! - [`config`]: the finger-curl settings, and [`tuning`] their fixed tuning.
//!
//! The curls are written once per sim tick, at the end of the pose-finalization pass (the post-call
//! `UpdatePropEffects` detour), with `AnimationController::SetJoint` over the finalized model-space
//! pose, as the head override is. The fingers do not need sub-tick latency, and the render-time
//! late update ([`crate::controllers::hands::late`]) already carries the finger bones rigidly with
//! the corrected hand in the skinning palette, so the curled fingers follow the controller at frame
//! rate without per-joint palette edits.
//!
//! The pose is absolute: each joint's bend is measured from a straight finger built from the
//! animated palm, and replaces the animated bend rather than adding to it, so an animation that
//! holds the fingers curled neither keeps an open hand curled nor pushes a fist into the palm.
//!
//! A hand is curled only while the controllers drive it: its wrist target was queued this tick, and
//! its fingers go from the animated pose to the curled one by the same tier weight, so a yielded
//! hand blends back to the animated fingers. A hand that holds something its source declared
//! ([`grip::declare`]) takes that thing's grip pose, blended in by how firmly it grips.

pub mod config;
pub mod curl;
pub mod grip;
pub mod pose;
pub mod target;
pub mod tuning;

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use jc3gi::character::character::{AnimationController, Character, Joint};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        fingers::{
            curl::{CurlSource, Finger},
            grip::{
                SourceLog,
                contact::{self, HandGeometry},
            },
            pose::{BoneJoint, HandShape, PalmFrame},
            target::{GripMemory, GripReport, TargetInputs},
            tuning::FINGERS,
        },
        frame::{self, Hand},
        hands::ik,
        scheme,
    },
};

/// One hand's finger curls on the last tick, for the debug UI and the status rows.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandFingersStatus {
    /// Where the curls came from; `None` when the hand was not curled.
    pub source: Option<CurlSource>,
    /// The smoothed shape the fingers were bent to.
    pub shape: HandShape,
    /// The tier weight the pose was blended by.
    pub weight: f32,
    /// What the hand held and how its fingers took it.
    pub grip: Option<GripReport>,
    /// Which fingers were driven, indexed by [`Finger::index`].
    pub driven: [bool; 5],
    /// Whether the finger bones were written.
    pub applied: bool,
    /// Whether a finger bone did not resolve by name.
    pub unresolved: bool,
}

/// The last tick's per-hand finger curls, indexed by [`Hand::index`]. A leaf slot of its own, so
/// that any thread (a status row's, mid-frame) may read it.
pub fn status() -> [HandFingersStatus; 2] {
    *PUBLISHED.lock()
}

/// `hand`'s index fingertip as last drawn, in the wrist frame the bones give
/// ([`crate::controllers::hands::pose::wrist_frame_from_bones`], origin at the wrist bone):
/// Rico's posed index finger, bent by the curls, extended past its distal joint
/// ([`pose::fingertip`]). The render-time late update carries the finger bones rigidly with the
/// wrist, so the point holds its place in the drawn wrist frame. `None` when the hand's fingers
/// were not written within `max_age` of `now`.
pub fn index_tip(hand: Hand, now: Instant, max_age: Duration) -> Option<Vec3> {
    INDEX_TIPS.lock()[hand.index()]
        .filter(|tip| now.saturating_duration_since(tip.at) <= max_age)
        .map(|tip| tip.wrist_local)
}

/// Curl the local player's fingers. Called at the end of the pose-finalization pass, after the
/// solve and the hands' wrist targets for the tick.
///
/// # Safety
///
/// `character` must be null or a live character; called from the post-call `UpdatePropEffects`
/// detour.
pub(crate) unsafe fn after_solve(character: *mut Character) {
    let Some(character) = (unsafe { character.as_mut() }).filter(|c| c.m_IsLocalCharacter) else {
        return;
    };
    let (config, enabled) = Config::lock_query(|c| (c.controllers.fingers, c.controllers.enabled));
    let mut state = STATE.lock();
    if !config.enabled || !enabled || !scheme::is_controllers() {
        *state = FingerState::EMPTY;
        *PUBLISHED.lock() = state.status;
        *INDEX_TIPS.lock() = [None; 2];
        return;
    }

    let now = Instant::now();
    let dt = state
        .last_tick
        .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
    state.last_tick = Some(now);

    let weights = ik::tick_records()
        .current
        .map(|target| target.map(|t| t.weight).filter(|&w| w > 0.0));
    let targets = frame::with_latest(|frame| {
        Hand::BOTH.map(|hand| {
            let f = frame.hand(hand);
            curl::select(config.source, hand, f).map(|curls| (curls, f.inputs))
        })
    })
    .unwrap_or([None; 2]);
    let max_age = grip::max_age(dt);
    let mut controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() };
    let mut tips = [None; 2];

    for hand in Hand::BOTH {
        let i = hand.index();
        let declared = grip::current(hand, now, max_age);
        if let Some(change) = state.logs[i].observe(now, declared.map(|g| g.source)) {
            tracing::info!(
                hand = hand.label(),
                from = change.from.map_or("nothing", |s| s.label()),
                to = change.to.map_or("nothing", |s| s.label()),
                pose = declared.map_or("open", |g| g.pose.label()),
                unlogged = change.unlogged,
                "fingers: the hand's grip source changed"
            );
        }
        let (Some(weight), Some(((source, measured), inputs))) = (weights[i], targets[i]) else {
            state.smoothed[i] = None;
            state.memory[i] = GripMemory::EMPTY;
            state.status[i] = HandFingersStatus::default();
            continue;
        };
        let target = target::hand_target(
            &TargetInputs {
                config: &FINGERS,
                declared,
                measured: &measured,
                source,
                inputs: &inputs,
                dt,
            },
            &mut state.memory[i],
        );
        let shape = state.smoothed[i].map_or(target.shape, |previous| {
            previous.smooth(target.shape, dt, FINGERS.smoothing_s)
        });
        state.smoothed[i] = Some(shape);
        let bend = pose::hand_bend(&shape, &FINGERS, weight, target.driven);
        let (outcome, tip) = match controller.as_deref_mut() {
            Some(controller) => unsafe { apply_hand(controller, hand, &bend) },
            None => (ApplyOutcome::NoController, None),
        };
        tips[i] = tip.map(|wrist_local| DrawnTip {
            wrist_local,
            at: now,
        });
        state.status[i] = HandFingersStatus {
            source: Some(source),
            shape,
            weight,
            grip: target.grip,
            driven: bend.driven,
            applied: outcome == ApplyOutcome::Applied,
            unresolved: outcome == ApplyOutcome::Unresolved,
        };
    }
    *PUBLISHED.lock() = state.status;
    *INDEX_TIPS.lock() = tips;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ApplyOutcome {
    Applied,
    Unresolved,
    /// The animated wrist and knuckles give no palm frame.
    Degenerate,
    NoController,
}

/// Write one hand's bent fingers over the finalized pose, and publish the hand's proportions for
/// the contact fit. Returns the outcome and, once written, the index fingertip in the bones' wrist
/// frame ([`index_tip`]). Every finger's joints are read before any is written, and each finger's are
/// written root outward, so the result does not depend on whether a write carries the bones beyond
/// it along.
unsafe fn apply_hand(
    controller: &mut AnimationController,
    hand: Hand,
    bend: &pose::HandBend,
) -> (ApplyOutcome, Option<Vec3>) {
    let bone = |name| unsafe { ik::bone_index(controller, name) };
    let Some(wrist) = bone(WRIST_NAMES[hand.index()]) else {
        return (ApplyOutcome::Unresolved, None);
    };
    let mut chains = [[0u32; 3]; 5];
    for finger in Finger::ALL {
        for (k, name) in CHAIN_NAMES[hand.index()][finger.index()].iter().enumerate() {
            let Some(index) = bone(name) else {
                return (ApplyOutcome::Unresolved, None);
            };
            chains[finger.index()][k] = index;
        }
    }
    let read = chains.map(|chain| chain.map(|b| unsafe { read_joint(controller, b) }));
    let joints = read.map(|chain| chain.map(|(_, j)| j));
    let knuckle = |finger: Finger| joints[finger.index()][0];
    let wrist_position = unsafe { read_joint(controller, wrist) }.1.position;
    let Some(palm) = PalmFrame::from_bones(
        hand,
        wrist_position,
        knuckle(Finger::Index).position,
        knuckle(Finger::Middle).position,
        knuckle(Finger::Little).position,
    ) else {
        return (ApplyOutcome::Degenerate, None);
    };
    if let Some(geometry) = HandGeometry::measure(&palm, &joints) {
        contact::publish_geometry(hand, geometry);
    }

    let mut written = joints;
    for finger in Finger::ALL.into_iter().filter(|f| bend.driven[f.index()]) {
        let i = finger.index();
        let bent = match finger {
            Finger::Thumb => pose::bend_thumb(
                joints[i],
                &palm,
                bend.bend[i],
                (bend.thumb_spread, bend.thumb_opposition),
                bend.weight,
            ),
            _ => pose::bend_finger(joints[i], &palm, bend.bend[i], bend.splay[i], bend.weight),
        };
        written[i] = bent;
        for ((&index, (mut joint, _)), new) in chains[i].iter().zip(read[i]).zip(bent) {
            joint.m_Translation.data = new.position.to_array();
            joint.m_Orientation.data = new.orientation.to_array();
            unsafe { controller.SetJoint(index, &mut joint) };
        }
    }
    // The knuckles are the bend's pivots, so the wrist frame is the same before and after.
    let tip = pose::fingertip(&written[Finger::Index.index()]).and_then(|tip| {
        pose::in_wrist_frame(
            hand,
            wrist_position,
            knuckle(Finger::Index).position,
            knuckle(Finger::Middle).position,
            tip,
        )
    });
    (ApplyOutcome::Applied, tip)
}

/// A bone's model-space joint, whole (to write back with its scale) and as a [`BoneJoint`].
unsafe fn read_joint(controller: &AnimationController, bone: u32) -> (Joint, BoneJoint) {
    let mut joint = Joint::default();
    unsafe { controller.GetJoint(bone, &mut joint) };
    let [x, y, z] = joint.m_Translation.data;
    let [qx, qy, qz, qw] = joint.m_Orientation.data;
    (
        joint,
        BoneJoint {
            position: Vec3::new(x, y, z),
            orientation: Quat::from_xyzw(qx, qy, qz, qw),
        },
    )
}

struct FingerState {
    smoothed: [Option<HandShape>; 2],
    memory: [GripMemory; 2],
    logs: [SourceLog; 2],
    last_tick: Option<Instant>,
    status: [HandFingersStatus; 2],
}

impl FingerState {
    const EMPTY: FingerState = FingerState {
        smoothed: [None; 2],
        memory: [GripMemory::EMPTY; 2],
        logs: [SourceLog::NEW; 2],
        last_tick: None,
        status: [HandFingersStatus {
            source: None,
            shape: HandShape {
                curl: [0.0; 5],
                splay: [0.0; 5],
                hook: [0.0; 5],
                opposition: 0.0,
                thumb_spread: 0.0,
            },
            weight: 0.0,
            grip: None,
            driven: [false; 5],
            applied: false,
            unresolved: false,
        }; 2],
    };
}

static STATE: Mutex<FingerState> = Mutex::new(FingerState::EMPTY);

/// The last tick's status, published for any thread.
static PUBLISHED: Mutex<[HandFingersStatus; 2]> = Mutex::new(FingerState::EMPTY.status);

/// The last tick's index fingertips, indexed by [`Hand::index`] ([`index_tip`]). A leaf slot of
/// its own, so that the input tick may read it.
static INDEX_TIPS: Mutex<[Option<DrawnTip>; 2]> = Mutex::new([None; 2]);

/// One hand's index fingertip in its bones' wrist frame, and when it was written.
#[derive(Debug, Copy, Clone, PartialEq)]
struct DrawnTip {
    wrist_local: Vec3,
    at: Instant,
}

/// The wrist bone per side.
const WRIST_NAMES: [&str; 2] = ["LeftHand", "RightHand"];

/// Each finger's three bones per side, root outward, indexed by [`Finger::index`]: the thumb's
/// base, middle, and last bones, and each finger's proximal, middle, and distal phalanges. The
/// ring and little fingers' in-hand metacarpals stay with the palm.
const CHAIN_NAMES: [[[&str; 3]; 5]; 2] = [
    [
        ["LeftHandThumb1", "LeftHandThumb2", "LeftHandThumb3"],
        ["LeftHandIndex1", "LeftHandIndex2", "LeftHandIndex3"],
        ["LeftHandMiddle1", "LeftHandMiddle2", "LeftHandMiddle3"],
        ["LeftHandRing1", "LeftHandRing2", "LeftHandRing3"],
        ["LeftHandPinky1", "LeftHandPinky2", "LeftHandPinky3"],
    ],
    [
        ["RightHandThumb1", "RightHandThumb2", "RightHandThumb3"],
        ["RightHandIndex1", "RightHandIndex2", "RightHandIndex3"],
        ["RightHandMiddle1", "RightHandMiddle2", "RightHandMiddle3"],
        ["RightHandRing1", "RightHandRing2", "RightHandRing3"],
        ["RightHandPinky1", "RightHandPinky2", "RightHandPinky3"],
    ],
];
