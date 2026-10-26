//! The frame's snapshot of the late update's inputs, the correction it derives from them, and the
//! palette edit that carries the correction onto the skinning palette.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use glam::{Mat4, Vec3};
use jc3gi::{character::character::Character, graphics_engine::render_block::Matrix3x4};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::rig::origin_provenance::{self, OriginEvidence},
        frame::{self, ControllerFrame, HAND_JOINT_COUNT, Hand, HandJoint, Pose, TrackedPose},
        hands::{
            self, ActiveHands, ik,
            late::{
                bones::{self, HandBones, Solved},
                math::{
                    MAX_SHIFT_M, MAX_TURN_RAD, correction, interpolate_attached, lerp_transform,
                    pose_matrix, relative_metrics,
                },
            },
            palette::{self, PaletteLayout},
            pose as hand_pose,
            reach::{self, ReachConfig},
        },
        physical_hands::{DrawTime, physics},
    },
    headpose,
};

/// A pose composed with both sides of the render-time head pose pair: `previous` with the
/// previous-tick body frame and anchor, `current` with the current tick's. The engine interpolates
/// the camera between the two by `dtf`, and so does [`at`](Self::at).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PosePair {
    pub previous: Pose,
    pub current: Pose,
    /// The head pair that transports the fixed latest tracking pose into the world.
    pub transport: Option<[Mat4; 2]>,
}

impl PosePair {
    /// The pose at the interpolation fraction `dtf`.
    pub fn at(&self, dtf: f32) -> Pose {
        let Some(parent) = self.transport else {
            return hand_pose::interp(self.previous, self.current, dtf);
        };
        let matrix = interpolate_attached(
            parent,
            [pose_matrix(self.previous), pose_matrix(self.current)],
            dtf,
        );
        let (_, orientation, position) = matrix.to_scale_rotation_translation();
        Pose {
            position,
            orientation,
        }
    }
}

/// One hand's render-time world poses from the latest controller frame, which the late update
/// corrects the hand toward.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RenderHand {
    /// The character's wrist frame.
    pub wrist: PosePair,
    /// The controller's grip pose (moved with the wrist by the remap).
    pub grip: PosePair,
    /// The controller's aim pose (moved with the wrist by the remap).
    pub aim: PosePair,
}

/// One hand's world poses as the hand is drawn at a render interpolation fraction.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct DrawnHand {
    pub wrist: Pose,
    pub grip: Pose,
    pub aim: Pose,
}

/// The hands as they are drawn at the interpolation fraction `dtf`, indexed by [`Hand::index`]:
/// the solved wrist moved by the hand's correction, which is what the palette edit draws, with the
/// grip and the aim carried on it as the controller holds them. Anything placed in a hand or
/// anchored to it at render time takes these rather than the controller's own render-time poses,
/// so it stays on the hand where the hand falls short of the controller: an arm that cannot reach,
/// a hand blending to or from the animation. A hand the correction does not apply to is drawn where the tick solved it;
/// one the IK did not drive this tick takes the controller's render-time pose. `None` for an inert
/// or untracked hand.
pub fn drawn_hands(dtf: f32) -> [Option<DrawnHand>; 2] {
    FRAME.lock().map_or([None; 2], |frame| {
        Hand::BOTH.map(|hand| frame.drawn(hand, dtf))
    })
}

/// Copied fingertip inputs from the controller snapshot that composed a hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HandSource {
    pub grip: TrackedPose,
    pub aim: TrackedPose,
    pub joints: Option<[HandJoint; HAND_JOINT_COUNT]>,
    pub trigger_touch: bool,
}

/// The source inputs shared by both hands in one late-render snapshot.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct HandSources {
    pub sequence: u64,
    pub published_at: Instant,
    pub origin: OriginEvidence,
    pub world_scale: f32,
    pub hands: [Option<HandSource>; 2],
}

impl HandSources {
    pub(crate) fn from_frame(
        frame: &ControllerFrame,
        world_scale: f32,
        origin: OriginEvidence,
    ) -> Self {
        Self {
            sequence: frame.sequence,
            published_at: frame.published_at,
            origin,
            world_scale,
            hands: Hand::BOTH.map(|hand| {
                let source = frame.hand(hand);
                (frame.focused && source.active && source.grip.is_valid()).then_some(HandSource {
                    grip: source.grip,
                    aim: source.aim,
                    joints: source.joints,
                    trigger_touch: source.inputs.trigger_touch,
                })
            }),
        }
    }

    /// The composition still belongs to its captured origin and a recent publication.
    pub(crate) fn is_usable_at(self, now: Instant) -> bool {
        self.is_usable_with_origin(now, self.origin.finish())
    }

    fn is_usable_with_origin(self, now: Instant, current: OriginEvidence) -> bool {
        now >= self.published_at
            && now.duration_since(self.published_at) <= frame::MAX_FRAME_AGE
            && self.world_scale.is_finite()
            && self.world_scale > 0.0
            && matches!(self.origin, OriginEvidence::Known(_))
            && current == self.origin
    }
}

/// The drawn hands and their exact controller inputs, copied under one late-frame lock.
pub(crate) fn drawn_hands_with_sources(dtf: f32) -> Option<([Option<DrawnHand>; 2], HandSources)> {
    let frame = (*FRAME.lock())?;
    Some((
        Hand::BOTH.map(|hand| frame.drawn(hand, dtf)),
        frame.sources?,
    ))
}

/// Count a palette upload edited by a draw outside this module (the presentation layer's), for the
/// readout's tally.
pub(crate) fn count_palette_edit() {
    PALETTES_EDITED.fetch_add(1, Ordering::Relaxed);
}

/// Count a gun model matrix re-pushed by the weapon render detour, for the readout's tally.
pub(super) fn count_weapon_update() {
    WEAPONS_UPDATED.fetch_add(1, Ordering::Relaxed);
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
    crate::controllers::hands::late::weapon::begin_frame();
    // All model and palette consumers share this post-step measurement for the rendered frame.
    unsafe { physics::sample_render_offsets() };
    let origin = origin_provenance::begin(Ok(()));
    let Some(source) = frame::latest() else {
        *FRAME.lock() = None;
        return;
    };
    let Some(active) = hands::active_hands_from_frame(&source, DrawTime::Render) else {
        *FRAME.lock() = None;
        return;
    };
    let hands = compose_render_hands(&active);
    let sources = Some(HandSources::from_frame(
        &source,
        active.world_scale,
        origin.finish(),
    ));
    let solved = bones::solved();
    *FRAME.lock() = Some(LateFrame {
        hands,
        sources,
        ticks: ik::tick_records(),
        transport: character_transport(),
        solved,
        enabled: active.config.hands.late_update,
        reach: active.config.hands.reach,
    });
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

/// Interpolate a model attachment in the same parent frame as this frame's hand references.
pub(crate) fn interpolate_in_character(matrices: [Mat4; 2], dtf: f32) -> Mat4 {
    let transport = FRAME.lock().and_then(|frame| frame.transport);
    transport.map_or_else(
        || lerp_transform(matrices[0], matrices[1], dtf),
        |parent| interpolate_attached(parent, matrices, dtf),
    )
}

/// The current frame's snapshot, for the weapon render seam's own guard and per-gun lookups.
pub(super) fn current() -> Option<LateFrame> {
    *FRAME.lock()
}

/// The frame's snapshot of the late update's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct LateFrame {
    hands: [Option<RenderHand>; 2],
    sources: Option<HandSources>,
    ticks: ik::TickRecords,
    transport: Option<[Mat4; 2]>,
    solved: Solved,
    /// The late update's toggle; the render hands are published for the overlay either way.
    enabled: bool,
    /// The detach's settings.
    reach: ReachConfig,
}

impl LateFrame {
    /// The world-space correction of `hand` at `dtf` (see the module docs), scaled by the weight
    /// the tick's IK was asked at.
    pub(super) fn hand_delta(&self, hand: Hand, dtf: f32) -> Option<Pose> {
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

    /// `hand`'s poses as drawn at `dtf` (see [`drawn_hands`]).
    fn drawn(&self, hand: Hand, dtf: f32) -> Option<DrawnHand> {
        let render = self.hands[hand.index()]?;
        let (wrist, grip, aim) = (
            render.wrist.at(dtf),
            render.grip.at(dtf),
            render.aim.at(dtf),
        );
        let drawn = self.reference(hand, dtf).map_or(wrist, |reference| {
            let solved = Pose {
                position: reference.position - self.miss(hand, dtf).unwrap_or(Vec3::ZERO),
                ..reference
            };
            self.hand_delta(hand, dtf)
                .map_or(solved, |d| hand_pose::mul(d, solved))
        });
        let to_drawn = hand_pose::mul(drawn, hand_pose::inverse(wrist));
        Some(DrawnHand {
            wrist: drawn,
            grip: hand_pose::mul(to_drawn, grip),
            aim: hand_pose::mul(to_drawn, aim),
        })
    }

    /// The tick targets in the same interpolated parent frame as the rendered skeleton.
    fn reference(&self, hand: Hand, dtf: f32) -> Option<Pose> {
        let current = self.ticks.current[hand.index()]?;
        let previous = self.ticks.previous[hand.index()].unwrap_or(current);
        let Some(transport) = self.transport else {
            return Some(hand_pose::interp(previous.wrist, current.wrist, dtf));
        };
        let matrix = interpolate_attached(
            transport,
            [pose_matrix(previous.wrist), pose_matrix(current.wrist)],
            dtf,
        );
        let (_, orientation, position) = matrix.to_scale_rotation_translation();
        Some(Pose {
            position,
            orientation,
        })
    }

    /// The solved wrist's shortfall from its target, interpolated at `dtf`.
    fn miss(&self, hand: Hand, dtf: f32) -> Option<Vec3> {
        let current = self.solved.current[hand.index()]?.miss?;
        let previous = self.solved.previous[hand.index()]
            .and_then(|j| j.miss)
            .unwrap_or(current);
        Some(self.transport.map_or_else(
            || previous.lerp(current, dtf),
            |parent| {
                let local_previous = parent[0].inverse().transform_vector3(previous);
                let local_current = parent[1].inverse().transform_vector3(current);
                lerp_transform(parent[0], parent[1], dtf)
                    .transform_vector3(local_previous.lerp(local_current, dtf))
            },
        ))
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

    pub(super) fn hand_status(&self, hand: Hand, dtf: f32) -> HandLateStatus {
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
        let reference = self.reference(hand, dtf).unwrap();
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

fn character_transport() -> Option<[Mat4; 2]> {
    let current = unsafe { Character::GetLocalPlayerCharacter() };
    if !matches_local_character(current as usize, ik::local_character()) {
        return None;
    }
    // The game-thread lookup owns the lifetime check; the IK address is only an identity.
    let character = unsafe { current.as_ref() }?;
    let pair = [
        Mat4::from(character.m_WorldMatrixT0),
        Mat4::from(character.m_WorldMatrixT1),
    ];
    pair.iter()
        .all(|m| m.is_finite() && m.determinant().abs() > f32::EPSILON)
        .then_some(pair)
}

fn matches_local_character(current: usize, recorded: usize) -> bool {
    current != 0 && current == recorded
}

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
            transport: Some([
                Mat4::from_rotation_translation(previous_head.orientation, previous_head.position),
                Mat4::from_rotation_translation(current_head.orientation, current_head.position),
            ]),
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

static FRAME: Mutex<Option<LateFrame>> = Mutex::new(None);
static WEAPONS_UPDATED: AtomicU64 = AtomicU64::new(0);
static PALETTES_EDITED: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;
    use crate::controllers::hands::late::bones::ArmJoints;

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
            transport: None,
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
            sources: None,
            ticks: ik::TickRecords {
                previous: [None, target(previous_tick)],
                current: [None, target(current_tick)],
            },
            transport: None,
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
    fn hand_sources_reject_age_and_origin_discontinuities() {
        let now = Instant::now();
        let [origin, changed] = origin_provenance::test_origins();
        let sources = HandSources {
            sequence: 1,
            published_at: now,
            origin,
            world_scale: 1.0,
            hands: [None; 2],
        };
        assert!(sources.is_usable_with_origin(now, origin));
        assert!(!sources.is_usable_with_origin(now, changed));
        assert!(!sources.is_usable_with_origin(now - frame::MAX_FRAME_AGE, origin));
        assert!(!sources.is_usable_with_origin(
            now + frame::MAX_FRAME_AGE + std::time::Duration::from_millis(1),
            origin,
        ));
    }

    #[test]
    fn character_transport_requires_the_current_local_ik_identity() {
        assert!(!matches_local_character(0, 0));
        assert!(!matches_local_character(0, 17));
        assert!(!matches_local_character(17, 0));
        assert!(!matches_local_character(17, 23));
        assert!(matches_local_character(17, 17));
    }

    #[test]
    fn a_turning_platform_uses_the_same_parent_frame_for_the_reference_and_correction() {
        let parent = [
            Mat4::IDENTITY,
            Mat4::from_rotation_translation(Quat::from_rotation_y(0.8), Vec3::X * 3.0),
        ];
        let local = Pose {
            position: Vec3::new(0.4, 1.0, -0.5),
            orientation: Quat::IDENTITY,
        };
        let from_matrix = |matrix: Mat4| {
            let (_, orientation, position) = matrix.to_scale_rotation_translation();
            Pose {
                position,
                orientation,
            }
        };
        let ticks = [
            from_matrix(parent[0] * pose_matrix(local)),
            from_matrix(parent[1] * pose_matrix(local)),
        ];
        let pair = PosePair {
            previous: ticks[0],
            current: ticks[1],
            transport: Some(parent),
        };
        let mut f = frame(ticks[1], ticks[0], ticks[1], 1.0);
        f.transport = Some(parent);
        f.hands[Hand::Right.index()] = Some(RenderHand {
            wrist: pair,
            grip: pair,
            aim: pair,
        });
        for t in [0.0, 0.25, 0.5, 1.0] {
            let expected = lerp_transform(parent[0], parent[1], t) * pose_matrix(local);
            let reference = f.reference(Hand::Right, t).unwrap();
            assert!(pose_matrix(reference).abs_diff_eq(expected, 1e-5));
            let delta = f.hand_delta(Hand::Right, t).unwrap();
            assert!(pose_matrix(delta).abs_diff_eq(Mat4::IDENTITY, 1e-5));
            assert!(pose_matrix(f.drawn(Hand::Right, t).unwrap().grip).abs_diff_eq(expected, 1e-5));
        }
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
    fn the_drawn_hand_is_the_controller_s_when_the_arm_reaches_it() {
        let f = frame(pose(0.5, 0.2), pose(0.3, 0.0), pose(0.4, 0.1), 1.0);
        let drawn = f.drawn(Hand::Right, 0.5).unwrap();
        let now = pose(0.5, 0.2);
        assert!(drawn.wrist.position.abs_diff_eq(now.position, 1e-5));
        assert!(drawn.wrist.orientation.angle_between(now.orientation) < 1e-4);
        assert!(drawn.grip.position.abs_diff_eq(now.position, 1e-5));
        assert!(f.drawn(Hand::Left, 0.5).is_none());
    }

    #[test]
    fn the_drawn_hand_falls_short_where_the_arm_does_and_carries_its_grip() {
        let p = pose(0.3, 0.0);
        let mut f = with_miss(frame(p, p, p, 1.0), Vec3::new(0.05, 0.0, 0.0));
        let grip = Pose {
            position: p.position + Vec3::new(0.0, 0.0, -0.05),
            ..p
        };
        let pair = PosePair {
            previous: grip,
            current: grip,
            transport: None,
        };
        f.hands[Hand::Right.index()].as_mut().unwrap().grip = pair;
        let drawn = f.drawn(Hand::Right, 0.5).unwrap();
        // The detach covers all but its threshold of the miss.
        assert!((drawn.wrist.position.x - 0.29).abs() < 1e-5);
        assert!(
            (drawn.grip.position - drawn.wrist.position)
                .abs_diff_eq(Vec3::new(0.0, 0.0, -0.05), 1e-5)
        );
        // Without the late update the hand is drawn where the tick solved it.
        f.enabled = false;
        let solved = f.drawn(Hand::Right, 0.5).unwrap();
        assert!((solved.wrist.position.x - 0.25).abs() < 1e-5);
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
        let mut off = frame(pose(0.35, 0.0), pose(0.3, 0.0), pose(0.3, 0.0), 1.0);
        off.enabled = false;
        assert!(off.hand_delta(Hand::Right, 1.0).is_none());
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
