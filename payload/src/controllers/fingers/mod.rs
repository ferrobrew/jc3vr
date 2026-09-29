//! Finger curls on the rendered hands: the plan of record's finger curls
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! - [`curl`]: each finger's curl and splay, from the tracked hand joints
//!   (`XR_EXT_hand_tracking`, located with the controller-conforming motion range when the runtime
//!   offers `XR_EXT_hand_joints_motion_range`) or estimated from the inputs the fingers rest on;
//! - [`pose`]: the curls turned into joint angles and the angles into model-space finger joints;
//! - [`config`]: the finger-curl settings.
//!
//! The curls are written once per sim tick, at the end of the pose-finalization pass (the post-call
//! `UpdatePropEffects` detour), with `AnimationController::SetJoint` over the finalized model-space
//! pose, as the head override is. The fingers do not need sub-tick latency, and the render-time
//! late update ([`crate::controllers::hands::late`]) already carries the finger bones rigidly with
//! the corrected hand in the skinning palette, so the curled fingers follow the controller at frame
//! rate without per-joint palette edits.
//!
//! A hand is curled only while the controllers drive it: its wrist target was queued this tick, and
//! its curl is scaled by the same tier weight, so a yielded hand blends back to the animated
//! fingers. A hand that holds a weapon (a gun in its attach bone, or the off-hand on the foregrip)
//! drives only its trigger finger unless
//! [`drive_all_when_holding`](config::FingersConfig::drive_all_when_holding) is set, because the
//! game's grip animation already wraps the other fingers round the weapon.

pub mod config;
pub mod curl;
pub mod pose;

use std::time::Instant;

use glam::{Quat, Vec3};
use jc3gi::character::character::{AnimationController, Character, Joint};
use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        fingers::{
            curl::{CurlSource, Finger, FingerCurls},
            pose::{BoneJoint, PalmFrame},
        },
        frame::{self, Hand},
        hands::ik,
        scheme,
        weapons::{self, foregrip},
    },
};

/// One hand's finger curls on the last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandFingersStatus {
    /// Where the curls came from; `None` when the hand was not curled.
    pub source: Option<CurlSource>,
    /// The smoothed curls.
    pub curls: FingerCurls,
    /// The tier weight the bend was scaled by.
    pub weight: f32,
    /// Whether the hand held a weapon, so only its trigger finger was driven.
    pub holding: bool,
    /// Which fingers were driven, indexed by [`Finger::index`].
    pub driven: [bool; 5],
    /// Whether the finger bones were written.
    pub applied: bool,
    /// Whether a finger bone did not resolve by name.
    pub unresolved: bool,
}

/// The last tick's per-hand finger curls, indexed by [`Hand::index`].
pub fn status() -> [HandFingersStatus; 2] {
    STATE.lock().status
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
        Hand::BOTH.map(|hand| curl::select(config.source, hand, frame.hand(hand)))
    })
    .unwrap_or([None; 2]);
    let holding = holding_hands();
    let mut controller = unsafe { character.m_AnimatedModel.m_AnimationController.as_mut() };

    for hand in Hand::BOTH {
        let i = hand.index();
        let (Some(weight), Some((source, target))) = (weights[i], targets[i]) else {
            state.smoothed[i] = None;
            state.status[i] = HandFingersStatus::default();
            continue;
        };
        let curls = state.smoothed[i].map_or(target, |previous| {
            curl::smooth(previous, target, dt, config.smoothing_s)
        });
        state.smoothed[i] = Some(curls);
        let bend = pose::hand_bend(&curls, &config, weight, holding[i]);
        let outcome = match controller.as_deref_mut() {
            Some(controller) => unsafe { apply_hand(controller, hand, &bend) },
            None => ApplyOutcome::NoController,
        };
        state.status[i] = HandFingersStatus {
            source: Some(source),
            curls,
            weight,
            holding: holding[i],
            driven: bend.driven,
            applied: outcome == ApplyOutcome::Applied,
            unresolved: outcome == ApplyOutcome::Unresolved,
        };
    }
}

/// The hands that hold a weapon, indexed by [`Hand::index`]: each hand with a gun in its attach
/// bone while the weapon handling runs, and the off-hand while it holds the foregrip.
fn holding_hands() -> [bool; 2] {
    let weapons = weapons::status();
    let mut holding = weapons
        .wielded
        .filter(|_| weapons.active)
        .map_or([false; 2], |w| w.weapon.wielded.hands);
    if let Some(hold) = foregrip::status().hold.filter(|h| h.engaged) {
        holding[hold.holder.other().index()] = true;
    }
    holding
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ApplyOutcome {
    Applied,
    Unresolved,
    /// The animated wrist and knuckles give no palm frame.
    Degenerate,
    NoController,
}

/// Write one hand's bent fingers over the finalized pose. Each finger's joints are read before any
/// is written and written root outward, so the result does not depend on whether a write carries
/// the bones beyond it along.
unsafe fn apply_hand(
    controller: &mut AnimationController,
    hand: Hand,
    bend: &pose::HandBend,
) -> ApplyOutcome {
    let bone = |name| unsafe { ik::bone_index(controller, name) };
    let Some(wrist) = bone(WRIST_NAMES[hand.index()]) else {
        return ApplyOutcome::Unresolved;
    };
    let mut chains = [[0u32; 3]; 5];
    for finger in Finger::ALL {
        for (k, name) in CHAIN_NAMES[hand.index()][finger.index()].iter().enumerate() {
            let Some(index) = bone(name) else {
                return ApplyOutcome::Unresolved;
            };
            chains[finger.index()][k] = index;
        }
    }
    let knuckle = |finger: Finger| unsafe { read_joint(controller, chains[finger.index()][0]) }.1;
    let Some(palm) = PalmFrame::from_bones(
        hand,
        unsafe { read_joint(controller, wrist) }.1.position,
        knuckle(Finger::Index).position,
        knuckle(Finger::Middle).position,
        knuckle(Finger::Little).position,
    ) else {
        return ApplyOutcome::Degenerate;
    };

    for finger in Finger::ALL.into_iter().filter(|f| bend.driven[f.index()]) {
        let i = finger.index();
        let read = chains[i].map(|b| unsafe { read_joint(controller, b) });
        let joints = read.map(|(_, j)| j);
        let bent = match finger {
            Finger::Thumb => pose::bend_thumb(joints, &palm, bend.bend[i]),
            _ => pose::bend_finger(joints, &palm, bend.bend[i], bend.splay[i]),
        };
        for ((&index, (mut joint, _)), new) in chains[i].iter().zip(read).zip(bent) {
            joint.m_Translation.data = new.position.to_array();
            joint.m_Orientation.data = new.orientation.to_array();
            unsafe { controller.SetJoint(index, &mut joint) };
        }
    }
    ApplyOutcome::Applied
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
    smoothed: [Option<FingerCurls>; 2],
    last_tick: Option<Instant>,
    status: [HandFingersStatus; 2],
}

impl FingerState {
    const EMPTY: FingerState = FingerState {
        smoothed: [None; 2],
        last_tick: None,
        status: [HandFingersStatus {
            source: None,
            curls: FingerCurls {
                curl: [0.0; 5],
                splay: [0.0; 5],
            },
            weight: 0.0,
            holding: false,
            driven: [false; 5],
            applied: false,
            unresolved: false,
        }; 2],
    };
}

static STATE: Mutex<FingerState> = Mutex::new(FingerState::EMPTY);

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
