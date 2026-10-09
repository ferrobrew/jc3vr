//! The render-time late update: the held weapon's model and the hand and forearm bones in the
//! skinning palette, re-posed from the latest controller pose.
//!
//! The IK places the hands at the sim tick, and the engine interpolates the rendered pose between
//! the last two ticks, so the rendered hands trail the controllers by one to two ticks. Once per
//! rendered frame ([`begin_frame`], from `Game::UpdateRender` after the VR pose is published) this
//! module snapshots the latest controller poses composed with the render-time head pose pair, and
//! the targets the last two ticks' IK was asked to reach. At render time, for the engine's
//! interpolation fraction `dtf`, each hand's correction is
//!
//! `D = now(dtf) × reference(dtf)⁻¹`,
//!
//! where `now` interpolates the latest controller pose between the head pose pair's two sides (as
//! the camera does) and `reference` interpolates the two ticks' targets (as the skinning palette
//! and the weapon's cached transforms do). The physical move between those poses is bounded before
//! the correction is weighted, and the weighted pose is derived by interpolating `reference` toward
//! `now` before computing `D`. The grip offset is on both sides, so it cancels.
//!
//! The rendered hand sits on the *solved* wrist, which falls short of the tick's target when the
//! arm cannot reach it. With the detach on, `reference` is taken from the solved wrist instead: the
//! target moved back along the shortfall by the part of it the detach covers
//! ([`reach::detach_offset`]: past a small threshold, up to a cap), so `D` carries the rendered hand
//! that far off the end of the arm toward the controller, and the forearm stretches along its axis
//! to stay attached ([`palette::forearm_reach`]). Past the cap, the hand stays at the cap.
//!
//! Both render seams apply the same `D`, so the gun and the hand never separate:
//!
//! - the post-call `CWeaponBase::UpdateRender` detour re-pushes the gun's model matrix as `D` times
//!   the engine's own interpolated matrix, for a gun the local player wields in a hand attach bone;
//! - the character render-block hooks pre-multiply the hand bones' palette entries by `D` (in model
//!   space), and the forearm bones' by a swing about the elbow that follows the corrected wrist
//!   ([`palette_edit`]);
//! - the gun's separate model parts (a magazine, a slide, a spinning barrel) are re-pushed by
//!   [`crate::controllers::hands::parts`] with the correction of the hand they ride.
//!
//! The muzzle flash, attached effects, and shot origin stay on the sim-rate transform.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicU64, Ordering},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    character::character::{AnimationController, SafeBoneIndex},
    equipment::weapon::{WeaponBase, WeaponState},
    game::GameObjectRenderContext,
    graphics_engine::render_block::Matrix3x4,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::{
        frame::{Hand, Pose},
        hands::{
            self, ActiveHands, ik,
            palette::{self, PaletteLayout},
            pose as hand_pose,
            reach::{self, ReachConfig},
        },
        weapons::hold::second,
    },
    headpose,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WEAPON_UPDATE_RENDER_BINDER)
}

/// The largest skeleton bone index accepted from a name lookup; the engine's bone indices are
/// `i16`-sized.
pub const MAX_BONE_INDEX: u32 = 0x7FFF;

/// A pose composed with both sides of the render-time head pose pair: `previous` with the
/// previous-tick body frame and anchor, `current` with the current tick's. The engine interpolates
/// the camera between the two by `dtf`, and so does [`at`](Self::at).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PosePair {
    pub previous: Pose,
    pub current: Pose,
}

impl PosePair {
    /// The pose at the interpolation fraction `dtf`.
    pub fn at(&self, dtf: f32) -> Pose {
        hand_pose::interp(self.previous, self.current, dtf)
    }
}

/// One hand's render-time world poses from the latest controller frame, for the late update and
/// the overlay layer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RenderHand {
    /// The character's wrist frame.
    pub wrist: PosePair,
    /// The controller's grip pose (moved with the wrist by the remap).
    pub grip: PosePair,
    /// The controller's aim pose (moved with the wrist by the remap).
    pub aim: PosePair,
}

/// The render-time hand poses of the frame in flight, indexed by [`Hand::index`]; `None` for an
/// inert or untracked hand.
pub fn render_hands() -> [Option<RenderHand>; 2] {
    FRAME.lock().map_or([None; 2], |frame| frame.hands)
}

/// Why a hand was or was not corrected at the sampled render interpolation fraction.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum LateReason {
    #[default]
    Disabled,
    NoTrackedRenderHand,
    NoSimTarget,
    InvalidPoseOrWeight,
    ExcessTranslation,
    ExcessRotation,
    Applied,
}

impl LateReason {
    /// The stable label written by controller diagnostics.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::NoTrackedRenderHand => "no_tracked_render_hand",
            Self::NoSimTarget => "no_sim_target",
            Self::InvalidPoseOrWeight => "invalid_pose_or_weight",
            Self::ExcessTranslation => "excess_translation",
            Self::ExcessRotation => "excess_rotation",
            Self::Applied => "applied",
        }
    }
}

/// One hand's late-correction status at the camera's last render interpolation fraction.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandLateStatus {
    pub reason: LateReason,
    /// The physical distance between the latest and reference wrist positions, in metres.
    pub physical_displacement_m: Option<f32>,
    /// The relative wrist rotation between the latest and reference poses, in degrees.
    pub relative_rotation_deg: Option<f32>,
    /// The current simulation tick's requested wrist position, in world metres.
    pub requested_wrist_position: Option<Vec3>,
    /// The interpolated simulation reference wrist position, in world metres.
    pub reference_wrist_position: Option<Vec3>,
    /// The latest render wrist position, in world metres.
    pub latest_wrist_position: Option<Vec3>,
    /// The interpolated solved wrist position, in character model metres.
    ///
    /// The late render seam has no model instance matrix, so this is deliberately not presented as a
    /// world-space position.
    pub achieved_wrist_position_model: Option<Vec3>,
    /// The interpolated shortfall of the solved wrist from its target, metres.
    pub solve_miss_m: Option<f32>,
    /// How far the rendered hand is detached from the solved wrist toward the target, metres.
    pub detach_m: Option<f32>,
}

/// The late update's per-hand readout and cumulative counters.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct LateStatus {
    /// Structured status for each hand, indexed by [`Hand::index`].
    pub hands: [HandLateStatus; 2],
    /// Each hand's applied physical displacement in metres and relative rotation in degrees.
    /// `None` when the hand was not corrected. Kept for the existing debug UI callers.
    pub deltas: [Option<(f32, f32)>; 2],
    /// Gun model matrices re-pushed since the payload loaded.
    pub weapons_updated: u64,
    /// Palette uploads edited since the payload loaded.
    pub palettes_edited: u64,
}

/// The late update's readout.
pub fn status() -> LateStatus {
    let frame = *FRAME.lock();
    let dtf = crate::hooks::camera::last_dtf();
    let hands = frame.map_or([HandLateStatus::default(); 2], |frame| {
        Hand::BOTH.map(|hand| frame.hand_status(hand, dtf))
    });
    LateStatus {
        hands,
        deltas: hands.map(|hand| {
            match (
                hand.reason,
                hand.physical_displacement_m,
                hand.relative_rotation_deg,
            ) {
                (LateReason::Applied, Some(displacement), Some(rotation)) => {
                    Some((displacement, rotation))
                }
                _ => None,
            }
        }),
        weapons_updated: WEAPONS_UPDATED.load(Ordering::Relaxed),
        palettes_edited: PALETTES_EDITED.load(Ordering::Relaxed),
    }
}

/// Snapshot the frame's late-update inputs. Called once per rendered frame from `Game::UpdateRender`
/// after the VR head pose pair is published and before the engine's render update.
pub(crate) fn begin_frame() {
    let Some(active) = hands::render_active_hands() else {
        *FRAME.lock() = None;
        return;
    };
    let hands = compose_render_hands(&active);
    let solved = *SOLVED.lock();
    *FRAME.lock() = Some(LateFrame {
        hands,
        ticks: ik::tick_records(),
        solved,
        enabled: active.config.hands.late_update,
        reach: active.config.hands.reach,
    });
}

/// The bones the palette edit touches for one hand: the hand group (the wrist, the attach bones,
/// and the fingers), which takes the hand's whole correction, and the forearm group, which swings
/// about the elbow. Zero marks a bone that did not resolve.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct HandBones {
    hand: [u16; HAND_BONE_NAMES[0].len()],
    forearm: [u16; FOREARM_BONE_NAMES[0].len()],
}

impl HandBones {
    /// Resolve `hand`'s bones by name.
    ///
    /// # Safety
    ///
    /// `controller` must be the character's live animation controller, on the thread that owns it.
    pub(crate) unsafe fn resolve(controller: &AnimationController, hand: Hand) -> Self {
        let resolve = |name| unsafe { ik::bone_index(controller, name) }.map_or(0, |i| i as u16);
        HandBones {
            hand: HAND_BONE_NAMES[hand.index()].map(resolve),
            forearm: FOREARM_BONE_NAMES[hand.index()].map(resolve),
        }
    }

    /// The wrist bone (the hand group's root).
    pub fn wrist(&self) -> u16 {
        self.hand[0]
    }

    /// The elbow's bone (the forearm, whose origin is the elbow).
    pub fn elbow(&self) -> u16 {
        self.forearm[0]
    }

    fn hand_bones(&self) -> impl Iterator<Item = usize> + '_ {
        self.hand.iter().filter(|&&b| b != 0).map(|&b| b as usize)
    }

    fn forearm_bones(&self) -> impl Iterator<Item = usize> + '_ {
        self.forearm
            .iter()
            .filter(|&&b| b != 0)
            .map(|&b| b as usize)
    }
}

/// The post-solve model-space elbow and wrist of one arm, and the solved wrist's shortfall from the
/// tick's target.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ArmJoints {
    pub elbow: Vec3,
    pub wrist: Vec3,
    /// The tick's world-space wrist target less the solved wrist in the world; `None` when the
    /// hand had no target this tick.
    pub miss: Option<Vec3>,
}

/// Publish a tick's resolved bones and post-solve arm joints; the previous tick's joints shift
/// back.
pub(crate) fn publish_solved(bones: [HandBones; 2], joints: [Option<ArmJoints>; 2]) {
    let mut solved = SOLVED.lock();
    solved.bones = bones;
    solved.previous = solved.current;
    solved.current = joints;
}

/// A draw's palette edit: per hand, the model-space transforms for the hand group and the forearm
/// group, and the bones they apply to.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PaletteEdit {
    bones: [HandBones; 2],
    hands: [Option<HandPaletteEdit>; 2],
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct HandPaletteEdit {
    hand: Mat4,
    forearm: Mat4,
}

impl PaletteEdit {
    /// Whether the edit changes `bone`'s entry.
    pub fn touches(&self, bone: usize) -> bool {
        self.edited_bones().any(|(b, _)| b == bone)
    }

    /// Pre-multiply the edited entries of `entries`, which spans the batch's referenced bones.
    pub fn apply(&self, entries: &mut [Matrix3x4], layout: PaletteLayout) {
        for (bone, edit) in self.edited_bones() {
            if let Some(entry) = entries.get_mut(bone) {
                palette::premultiply_entry(&mut entry.m, layout, edit);
            }
        }
        PALETTES_EDITED.fetch_add(1, Ordering::Relaxed);
    }

    fn edited_bones(&self) -> impl Iterator<Item = (usize, &Mat4)> + '_ {
        Hand::BOTH.into_iter().flat_map(move |hand| {
            let bones = &self.bones[hand.index()];
            self.hands[hand.index()].iter().flat_map(move |edit| {
                bones
                    .hand_bones()
                    .map(move |b| (b, &edit.hand))
                    .chain(bones.forearm_bones().map(move |b| (b, &edit.forearm)))
            })
        })
    }
}

/// The palette edit for a draw of the local player's body whose instance matrix is `instance`, at
/// the interpolation fraction `dtf`, or `None` when no hand is corrected.
pub(crate) fn palette_edit(instance: Mat4, dtf: f32) -> Option<PaletteEdit> {
    let frame = (*FRAME.lock())?;
    let to_model = instance.inverse();
    let hands = Hand::BOTH.map(|hand| {
        let d = pose_matrix(frame.hand_delta(hand, dtf)?);
        let d_model = to_model * d * instance;
        let joints = frame.solved.current[hand.index()]?;
        let previous = frame.solved.previous[hand.index()].unwrap_or(joints);
        let elbow = previous.elbow.lerp(joints.elbow, dtf);
        let wrist = previous.wrist.lerp(joints.wrist, dtf);
        let stretch = frame.reach.detach && frame.reach.stretch_forearm;
        Some(HandPaletteEdit {
            hand: d_model,
            forearm: palette::forearm_reach(elbow, wrist, d_model.transform_point3(wrist), stretch),
        })
    });
    hands.iter().any(Option::is_some).then_some(PaletteEdit {
        bones: frame.solved.bones,
        hands,
    })
}

/// The world-space correction of `hand` at the interpolation fraction `dtf`, as a matrix, for a
/// render seam that re-pushes a model riding that hand; `None` when the hand is not corrected.
pub(crate) fn hand_correction(hand: Hand, dtf: f32) -> Option<Mat4> {
    let frame = (*FRAME.lock())?;
    frame.hand_delta(hand, dtf).map(pose_matrix)
}

/// The engine's weapon render update, then the gun's model matrix re-pushed with the hand's
/// correction.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::UpdateRender_ADDRESS)]
fn weapon_update_render(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    WEAPON_UPDATE_RENDER
        .get()
        .unwrap()
        .call(game_object, context);
    unsafe { late_update_weapon(game_object, context) };
}

/// The frame's snapshot of the late update's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
struct LateFrame {
    hands: [Option<RenderHand>; 2],
    ticks: ik::TickRecords,
    solved: Solved,
    /// The late update's toggle; the render hands are published for the overlay either way.
    enabled: bool,
    /// The detach's settings.
    reach: ReachConfig,
}

impl LateFrame {
    /// The world-space correction of `hand` at `dtf` (see the module docs), scaled by the weight
    /// the tick's IK was asked at.
    fn hand_delta(&self, hand: Hand, dtf: f32) -> Option<Pose> {
        let status = self.hand_status(hand, dtf);
        (status.reason == LateReason::Applied).then(|| {
            let now = self.hands[hand.index()].unwrap().wrist.at(dtf);
            let current = self.ticks.current[hand.index()].unwrap();
            let reference = self.reference(hand, dtf).unwrap();
            let detached = Pose {
                position: reference.position - self.detach(hand, dtf),
                ..reference
            };
            // The detached reference can fail the plausibility bound the status passed only by the
            // cap's margin; the hand then stays on the arm rather than going uncorrected.
            correction(now, detached, current.weight)
                .or_else(|| correction(now, reference, current.weight))
                .unwrap()
        })
    }

    /// The tick targets interpolated at `dtf`.
    fn reference(&self, hand: Hand, dtf: f32) -> Option<Pose> {
        let current = self.ticks.current[hand.index()]?;
        let previous = self.ticks.previous[hand.index()].unwrap_or(current);
        Some(hand_pose::interp(previous.wrist, current.wrist, dtf))
    }

    /// The solved wrist's shortfall from its target, interpolated at `dtf`.
    fn miss(&self, hand: Hand, dtf: f32) -> Option<Vec3> {
        let current = self.solved.current[hand.index()]?.miss?;
        let previous = self.solved.previous[hand.index()]
            .and_then(|j| j.miss)
            .unwrap_or(current);
        Some(previous.lerp(current, dtf))
    }

    /// How far the rendered hand leaves the solved wrist toward the target at `dtf`, in the world.
    fn detach(&self, hand: Hand, dtf: f32) -> Vec3 {
        if !self.reach.detach {
            return Vec3::ZERO;
        }
        self.miss(hand, dtf).map_or(Vec3::ZERO, |miss| {
            reach::detach_offset(miss, self.reach.detach_threshold_m, self.reach.detach_cap_m)
        })
    }

    fn hand_status(&self, hand: Hand, dtf: f32) -> HandLateStatus {
        if !self.enabled {
            return HandLateStatus::default();
        }
        let Some(render_hand) = self.hands[hand.index()] else {
            return HandLateStatus {
                reason: LateReason::NoTrackedRenderHand,
                ..HandLateStatus::default()
            };
        };
        let Some(current) = self.ticks.current[hand.index()] else {
            return HandLateStatus {
                reason: LateReason::NoSimTarget,
                ..HandLateStatus::default()
            };
        };
        let previous = self.ticks.previous[hand.index()].unwrap_or(current);
        let reference = hand_pose::interp(previous.wrist, current.wrist, dtf);
        let now = render_hand.wrist.at(dtf);
        let metrics = relative_metrics(now, reference);
        let (physical_displacement_m, relative_rotation_deg) = metrics
            .map(|(displacement, rotation)| (Some(displacement), Some(rotation)))
            .unwrap_or((None, None));
        let reason = match metrics {
            _ if !current.weight.is_finite() => LateReason::InvalidPoseOrWeight,
            None => LateReason::InvalidPoseOrWeight,
            Some((displacement, _)) if displacement >= MAX_SHIFT_M => LateReason::ExcessTranslation,
            Some((_, rotation)) if rotation >= MAX_TURN_RAD.to_degrees() => {
                LateReason::ExcessRotation
            }
            Some(_) => LateReason::Applied,
        };
        let solved = (self.solved.previous[hand.index()].or(self.solved.current[hand.index()]))
            .zip(self.solved.current[hand.index()])
            .map(|(previous, current)| previous.wrist.lerp(current.wrist, dtf));
        HandLateStatus {
            reason,
            physical_displacement_m,
            relative_rotation_deg,
            requested_wrist_position: Some(current.wrist.position),
            reference_wrist_position: Some(reference.position),
            latest_wrist_position: Some(now.position),
            achieved_wrist_position_model: solved,
            solve_miss_m: self.miss(hand, dtf).map(Vec3::length),
            detach_m: (reason == LateReason::Applied).then(|| self.detach(hand, dtf).length()),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Solved {
    bones: [HandBones; 2],
    previous: [Option<ArmJoints>; 2],
    current: [Option<ArmJoints>; 2],
}

/// Derive a bounded correction from the physical move between the reference and current wrist.
///
/// The relative position and rotation are validated before the correction is weighted. This keeps
/// a stationary wrist rotation valid even when the world origin is far away, and rejects
/// discontinuities before they can be interpolated into a plausible-looking correction.
pub(crate) fn correction(now: Pose, reference: Pose, weight: f32) -> Option<Pose> {
    (weight.is_finite() && plausible(now, reference))
        .then(|| hand_pose::weighted_delta(now, reference, weight))
}

/// The physical wrist displacement and relative rotation, or `None` for a non-finite pose.
fn relative_metrics(now: Pose, reference: Pose) -> Option<(f32, f32)> {
    (now.position.is_finite()
        && reference.position.is_finite()
        && now.orientation.is_finite()
        && reference.orientation.is_finite())
    .then(|| {
        let displacement = (now.position - reference.position).length();
        let mut relative = hand_pose::delta(now, reference).orientation;
        if relative.w < 0.0 {
            relative = -relative;
        }
        let (_, angle) = relative.to_axis_angle();
        (displacement, angle.to_degrees())
    })
    .filter(|(displacement, rotation)| displacement.is_finite() && rotation.is_finite())
}

/// Whether the wrist moved by a tick or two rather than discontinuously (a teleport, a recenter, or
/// a respawn), which must not be smeared onto the rendered hand.
pub(crate) fn plausible(now: Pose, reference: Pose) -> bool {
    relative_metrics(now, reference).is_some_and(|(displacement, rotation)| {
        displacement < MAX_SHIFT_M && rotation < MAX_TURN_RAD.to_degrees()
    })
}

const MAX_SHIFT_M: f32 = 0.5;
const MAX_TURN_RAD: f32 = std::f32::consts::FRAC_PI_2;

fn compose_render_hands(active: &ActiveHands) -> [Option<RenderHand>; 2] {
    let (previous_head, current_head) = (headpose::query_prev(), headpose::query());
    let cockpit_head = active.head;
    let pair = |pose: Pose| {
        let side = |head: headpose::HeadPose| {
            hand_pose::compose_world(
                head.position,
                hand_pose::body_of_head(head.orientation, cockpit_head.orientation),
                cockpit_head.position,
                pose,
                active.world_scale,
            )
        };
        PosePair {
            previous: side(previous_head),
            current: side(current_head),
        }
    };
    active.hands.map(|hand| {
        hand.map(|h| RenderHand {
            wrist: pair(h.wrist),
            grip: pair(h.grip),
            aim: pair(h.aim),
        })
    })
}

/// Re-push the model matrix of each gun of `game_object`'s weapon that the local player wields in
/// a hand attach bone, with that hand's correction.
unsafe fn late_update_weapon(game_object: *mut c_void, context: *const GameObjectRenderContext) {
    let (Some(context), false) = (unsafe { context.as_ref() }, game_object.is_null()) else {
        return;
    };
    let weapon = unsafe {
        &*(game_object
            .cast::<u8>()
            .sub(WeaponBase::GAME_OBJECT_BASE_OFFSET as usize)
            .cast::<WeaponBase>())
    };
    let local = ik::local_character();
    // `m_User` is a `weak_ptr<CCharacter>`; its first word is the character. Checked before the
    // frame lock, since this runs for every weapon in the world.
    if local == 0 || weapon.m_User[0] as usize != local {
        return;
    }
    let Some(frame) = *FRAME.lock() else {
        return;
    };
    let state = weapon.m_State;
    if !matches!(
        state,
        WeaponState::E_WS_WIELDING | WeaponState::E_WS_DUAL_WIELDING
    ) && !second::is(weapon as *const WeaponBase as usize)
    {
        return;
    }
    let dtf = context.m_Dtf;
    let guns = &weapon.m_lWeaponData;
    let mut cursor = guns.m_Begin;
    while !cursor.is_null() && cursor < guns.m_End {
        let gun = unsafe { (*cursor).as_ref() };
        cursor = unsafe { cursor.add(1) };
        let Some(gun) = gun else {
            continue;
        };
        let Some(&bone) = gun.m_lBoneAttachementPerWeaponState.get(state as usize) else {
            continue;
        };
        let Some(hand) = attach_hand(bone) else {
            continue;
        };
        let (Some(d), Some(instance)) = (frame.hand_delta(hand, dtf), unsafe {
            gun.m_BaseModelInstance.as_mut()
        }) else {
            continue;
        };
        let interpolated = lerp_transform(
            Mat4::from(gun.m_WorldMatrixT0),
            Mat4::from(gun.m_WorldMatrixT1),
            dtf,
        );
        let matrix = Matrix4::from(pose_matrix(d) * interpolated);
        unsafe { instance.UpdateMatrix(&matrix) };
        WEAPONS_UPDATED.fetch_add(1, Ordering::Relaxed);
    }
}

/// The hand an attach bone (an `NBone::ESafeBoneIndex`) belongs to, or `None` for a holster or
/// other non-hand bone.
pub(crate) fn attach_hand(bone: u32) -> Option<Hand> {
    [
        (SafeBoneIndex::ATTACH_HAND_LEFT as u32, Hand::Left),
        (SafeBoneIndex::ATTACH_HAND_LEFT2 as u32, Hand::Left),
        (SafeBoneIndex::ATTACH_HAND_RIGHT as u32, Hand::Right),
        (SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32, Hand::Right),
    ]
    .into_iter()
    .find(|&(safe, _)| safe == bone)
    .map(|(_, hand)| hand)
}

/// The engine's transform interpolation (`CTransform::Lerp`): the translation and scale lerped and
/// the rotation slerped.
pub(crate) fn lerp_transform(a: Mat4, b: Mat4, t: f32) -> Mat4 {
    let (sa, ra, ta) = a.to_scale_rotation_translation();
    let (sb, rb, tb) = b.to_scale_rotation_translation();
    Mat4::from_scale_rotation_translation(sa.lerp(sb, t), ra.slerp(rb, t), ta.lerp(tb, t))
}

pub(crate) fn pose_matrix(p: Pose) -> Mat4 {
    Mat4::from_rotation_translation(p.orientation, p.position)
}

/// The hand group per side: the wrist first, then the attach bones and the seventeen finger bones.
const HAND_BONE_NAMES: [[&str; 20]; 2] = [
    [
        "LeftHand",
        "LeftHandAttach",
        "LeftHandAttach2",
        "LeftHandThumb1",
        "LeftHandThumb2",
        "LeftHandThumb3",
        "LeftHandIndex1",
        "LeftHandIndex2",
        "LeftHandIndex3",
        "LeftHandMiddle1",
        "LeftHandMiddle2",
        "LeftHandMiddle3",
        "LeftInHandRing",
        "LeftHandRing1",
        "LeftHandRing2",
        "LeftHandRing3",
        "LeftInHandPinky",
        "LeftHandPinky1",
        "LeftHandPinky2",
        "LeftHandPinky3",
    ],
    [
        "RightHand",
        "RightHandAttach",
        "RightHandAttach2",
        "RightHandThumb1",
        "RightHandThumb2",
        "RightHandThumb3",
        "RightHandIndex1",
        "RightHandIndex2",
        "RightHandIndex3",
        "RightHandMiddle1",
        "RightHandMiddle2",
        "RightHandMiddle3",
        "RightInHandRing",
        "RightHandRing1",
        "RightHandRing2",
        "RightHandRing3",
        "RightInHandPinky",
        "RightHandPinky1",
        "RightHandPinky2",
        "RightHandPinky3",
    ],
];

/// The forearm group per side: the forearm (whose origin is the elbow) first, then its roll bones.
const FOREARM_BONE_NAMES: [[&str; 4]; 2] = [
    [
        "LeftForeArm",
        "LeftForeArmRoll",
        "df_LeftForeArmRollB01",
        "df_LeftForeArmRollC01",
    ],
    [
        "RightForeArm",
        "RightForeArmRoll",
        "df_RightForeArmRollB01",
        "df_RightForeArmRollC01",
    ],
];

static FRAME: Mutex<Option<LateFrame>> = Mutex::new(None);
static SOLVED: Mutex<Solved> = Mutex::new(Solved {
    bones: [HandBones {
        hand: [0; 20],
        forearm: [0; 4],
    }; 2],
    previous: [None; 2],
    current: [None; 2],
});
static WEAPONS_UPDATED: AtomicU64 = AtomicU64::new(0);
static PALETTES_EDITED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn pose(x: f32, yaw: f32) -> Pose {
        Pose {
            position: Vec3::new(x, 1.0, 0.0),
            orientation: Quat::from_rotation_y(yaw),
        }
    }

    fn frame(now: Pose, previous_tick: Pose, current_tick: Pose, weight: f32) -> LateFrame {
        let still = PosePair {
            previous: now,
            current: now,
        };
        let target = |wrist| Some(ik::TickTarget { wrist, weight });
        LateFrame {
            hands: [
                None,
                Some(RenderHand {
                    wrist: still,
                    grip: still,
                    aim: still,
                }),
            ],
            ticks: ik::TickRecords {
                previous: [None, target(previous_tick)],
                current: [None, target(current_tick)],
            },
            solved: Solved::default(),
            enabled: true,
            reach: ReachConfig::new(),
        }
    }

    /// `f` with the right wrist solved `miss` short of its targets on both ticks.
    fn with_miss(mut f: LateFrame, miss: Vec3) -> LateFrame {
        let joints = Some(ArmJoints {
            elbow: Vec3::ZERO,
            wrist: Vec3::X,
            miss: Some(miss),
        });
        f.solved.previous[Hand::Right.index()] = joints;
        f.solved.current[Hand::Right.index()] = joints;
        f
    }

    #[test]
    fn a_short_solve_detaches_the_hand_beyond_the_threshold() {
        let p = pose(0.3, 0.0);
        let f = with_miss(frame(p, p, p, 1.0), Vec3::new(0.05, 0.0, 0.0));
        let d = f.hand_delta(Hand::Right, 0.5).unwrap();
        assert!((d.position.x - 0.04).abs() < 1e-5);
        let status = f.hand_status(Hand::Right, 0.5);
        assert!((status.solve_miss_m.unwrap() - 0.05).abs() < 1e-5);
        assert!((status.detach_m.unwrap() - 0.04).abs() < 1e-5);
    }

    #[test]
    fn the_detach_is_capped_and_adds_to_the_motion_since_the_tick() {
        let f = with_miss(
            frame(pose(0.5, 0.0), pose(0.4, 0.0), pose(0.4, 0.0), 1.0),
            Vec3::new(0.0, 0.0, -0.5),
        );
        let d = f.hand_delta(Hand::Right, 0.5).unwrap();
        assert!((d.position.x - 0.1).abs() < 1e-5);
        assert!((d.position.z + 0.1).abs() < 1e-5);
    }

    #[test]
    fn the_detach_off_keeps_the_hand_on_the_arm() {
        let p = pose(0.3, 0.0);
        let mut f = with_miss(frame(p, p, p, 1.0), Vec3::new(0.05, 0.0, 0.0));
        f.reach.detach = false;
        let d = f.hand_delta(Hand::Right, 0.5).unwrap();
        assert!(d.position.length() < 1e-5);
    }

    #[test]
    fn a_hand_at_its_tick_target_needs_no_correction() {
        let p = pose(0.3, 0.2);
        let d = frame(p, p, p, 1.0).hand_delta(Hand::Right, 0.4).unwrap();
        assert!(d.position.length() < 1e-5);
        assert!(frame(p, p, p, 1.0).hand_delta(Hand::Left, 0.4).is_none());
    }

    #[test]
    fn the_correction_covers_the_motion_since_the_interpolated_ticks() {
        let f = frame(pose(0.5, 0.0), pose(0.3, 0.0), pose(0.4, 0.0), 1.0);
        let d = f.hand_delta(Hand::Right, 0.5).unwrap();
        assert!((d.position.x - 0.15).abs() < 1e-5);
        let half = frame(pose(0.5, 0.0), pose(0.3, 0.0), pose(0.4, 0.0), 0.5)
            .hand_delta(Hand::Right, 0.5)
            .unwrap();
        assert!((half.position.x - 0.075).abs() < 1e-5);
    }

    #[test]
    fn a_stationary_rotating_wrist_is_corrected_at_a_translated_origin() {
        let reference = Pose {
            position: Vec3::new(1000.0, 200.0, -300.0),
            orientation: Quat::IDENTITY,
        };
        let now = Pose {
            orientation: Quat::from_rotation_y(0.4),
            ..reference
        };
        let d = correction(now, reference, 0.5).unwrap();
        let carried = hand_pose::mul(d, reference);
        assert!(carried.position.distance(reference.position) < 1e-5);
        assert!(
            (carried
                .orientation
                .angle_between(Quat::from_rotation_y(0.2)))
            .abs()
                < 1e-5
        );
    }

    #[test]
    fn status_reports_physical_metrics_and_applied_reason() {
        let reference = Pose {
            position: Vec3::new(1000.0, 200.0, -300.0),
            orientation: Quat::IDENTITY,
        };
        let now = Pose {
            orientation: -Quat::from_rotation_y(0.4),
            ..reference
        };
        let f = frame(now, reference, reference, 0.5);
        let status = f.hand_status(Hand::Right, 0.5);
        assert_eq!(status.reason, LateReason::Applied);
        assert_eq!(status.physical_displacement_m, Some(0.0));
        assert!((status.relative_rotation_deg.unwrap() - 22.9183).abs() < 1e-3);
        assert_eq!(status.latest_wrist_position, Some(reference.position));
        assert_eq!(status.reference_wrist_position, Some(reference.position));
    }

    #[test]
    fn status_reasons_keep_the_guard_independent_of_weight() {
        let excessive = frame(pose(5.0, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 0.01);
        assert_eq!(
            excessive.hand_status(Hand::Right, 1.0).reason,
            LateReason::ExcessTranslation
        );
        let excessive_turn = frame(
            pose(0.3, std::f32::consts::PI),
            pose(0.3, 0.0),
            pose(0.3, 0.0),
            0.01,
        );
        assert_eq!(
            excessive_turn.hand_status(Hand::Right, 1.0).reason,
            LateReason::ExcessRotation
        );
        assert_eq!(
            frame(pose(0.3, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), f32::NAN)
                .hand_status(Hand::Right, 1.0)
                .reason,
            LateReason::InvalidPoseOrWeight
        );
    }

    #[test]
    fn status_reports_missing_inputs_and_disabled_state() {
        let mut disabled = frame(pose(0.35, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0);
        disabled.enabled = false;
        assert_eq!(
            disabled.hand_status(Hand::Right, 1.0).reason,
            LateReason::Disabled
        );
        assert_eq!(
            frame(pose(0.3, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0)
                .hand_status(Hand::Left, 1.0)
                .reason,
            LateReason::NoTrackedRenderHand
        );
        let mut no_target = frame(pose(0.3, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0);
        no_target.ticks.current[Hand::Right.index()] = None;
        assert_eq!(
            no_target.hand_status(Hand::Right, 1.0).reason,
            LateReason::NoSimTarget
        );
    }

    #[test]
    fn invalid_or_discontinuous_motion_is_not_corrected() {
        let translated = frame(pose(5.0, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0);
        assert!(translated.hand_delta(Hand::Right, 1.0).is_none());
        let reference = pose(0.3, 0.0);
        assert!(
            correction(
                Pose {
                    orientation: Quat::from_rotation_y(std::f32::consts::PI),
                    ..reference
                },
                reference,
                1.0,
            )
            .is_none()
        );
        assert!(correction(reference, reference, f32::NAN).is_none());
        let mut off = frame(pose(0.35, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0);
        off.enabled = false;
        assert!(off.hand_delta(Hand::Right, 1.0).is_none());
    }

    #[test]
    fn the_weapon_interpolation_matches_the_endpoints() {
        let a = Mat4::from_rotation_translation(Quat::from_rotation_x(0.2), Vec3::X);
        let b = Mat4::from_rotation_translation(Quat::from_rotation_x(0.6), Vec3::Y);
        assert!(lerp_transform(a, b, 0.0).abs_diff_eq(a, 1e-5));
        assert!(lerp_transform(a, b, 1.0).abs_diff_eq(b, 1e-5));
    }

    #[test]
    fn attach_bones_map_to_hands() {
        assert_eq!(
            attach_hand(SafeBoneIndex::ATTACH_HAND_RIGHT2 as u32),
            Some(Hand::Right)
        );
        assert_eq!(
            attach_hand(SafeBoneIndex::ATTACH_HAND_LEFT as u32),
            Some(Hand::Left)
        );
        assert_eq!(attach_hand(SafeBoneIndex::HEAD as u32), None);
    }

    #[test]
    fn the_edit_touches_only_the_resolved_bones_of_corrected_hands() {
        let mut bones = [HandBones::default(); 2];
        bones[1].hand[0] = 40;
        bones[1].forearm[0] = 38;
        bones[0].hand[0] = 20;
        let edit = PaletteEdit {
            bones,
            hands: [
                None,
                Some(HandPaletteEdit {
                    hand: Mat4::from_translation(Vec3::X),
                    forearm: Mat4::IDENTITY,
                }),
            ],
        };
        assert!(edit.touches(40));
        assert!(edit.touches(38));
        assert!(!edit.touches(20));
        assert!(!edit.touches(0));
    }
}
