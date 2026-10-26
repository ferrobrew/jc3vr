//! Owned takeoff history and one camera result shared by the tree and render hooks.

use glam::{Mat4, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::hooks::camera::authored::{
    Limit, Refusal, STATUS, Status, UnsupportedCases,
    composition::{
        Baseline, DeltaRequest, InvalidPair, LocalSample, RootSample, RootTreatment, Sample, Stamp,
        Trs, compose_view,
    },
    native::{NativeSample, Source, read, trs},
    policy::{AcceptedCase, AcceptedRun, Frame, GraphState, Owner, Phase, Policy},
};

/// The ordinary camera and the tracked-only displacement on one side of the pose pair.
#[derive(Clone, Copy)]
pub(crate) struct TrackedView {
    pub world: Mat4,
    pub tracking_translation: Vec3,
}

/// Capture at the native camera-tree read phase. No pointer escapes this call.
pub(crate) unsafe fn capture(
    character: &Character,
    views: [TrackedView; 2],
    always_use_t1: bool,
) -> Option<[Mat4; 2]> {
    let native = match unsafe { read(character) } {
        Ok(sample) => sample,
        Err(reason) => {
            reset(reason);
            return None;
        }
    };
    let mut state = STATE.lock();
    match state.update(native, views, always_use_t1) {
        Ok(result) => result,
        Err(reason) => {
            state.clear();
            *STATUS.lock() = Status::NormalTracked {
                reason,
                unsupported: UnsupportedCases::LandingExplosionRecoveryFloorPickupHanging,
            };
            None
        }
    }
}

/// The render hook consumes the tree's result instead of resampling or applying another delta.
pub(crate) fn selected(character: &Character) -> Option<[Mat4; 2]> {
    let state = STATE.lock();
    (state.character == character as *const Character as u64)
        .then_some(state.selected)
        .flatten()
}

pub(crate) fn normal_reset() -> bool {
    STATE.lock().collapse_normal
}

pub(crate) fn take_normal_reset() -> bool {
    let mut state = STATE.lock();
    std::mem::take(&mut state.collapse_normal)
}

pub(crate) fn reset(reason: Refusal) {
    STATE.lock().clear();
    *STATUS.lock() = Status::NormalTracked {
        reason,
        unsupported: UnsupportedCases::LandingExplosionRecoveryFloorPickupHanging,
    };
}

#[derive(Default)]
struct Runtime {
    policy: Policy,
    character: u64,
    source: Option<Source>,
    graph_state: Option<GraphState>,
    pose_buffer: u64,
    serial: u64,
    run: u64,
    baseline: Option<Baseline>,
    previous: Option<Sample>,
    pair: Option<[Sample; 2]>,
    deltas: Option<[Mat4; 2]>,
    eye_offset: Vec3,
    selected: Option<[Mat4; 2]>,
    collapse_normal: bool,
}

impl Runtime {
    fn update(
        &mut self,
        native: NativeSample,
        views: [TrackedView; 2],
        always_use_t1: bool,
    ) -> Result<Option<[Mat4; 2]>, Refusal> {
        if self.character != native.character || self.source != Some(native.source) {
            self.clear();
            if native.state != GraphState::GroundInto {
                return Err(Refusal::SourceChanged);
            }
            self.character = native.character;
            self.source = Some(native.source);
            self.collapse_normal = false;
        }
        if self.graph_state == Some(GraphState::GroundFollowOn)
            && native.state == GraphState::GroundInto
        {
            return Err(Refusal::Policy(InvalidPair::InvalidTransition));
        }
        self.graph_state = Some(native.state);
        if let Some(previous) = self.previous
            && previous
                .root
                .transform
                .translation
                .distance(native.root.translation)
                > MAX_ROOT_STEP
        {
            self.policy.teleport();
            return Err(Refusal::Policy(InvalidPair::Teleport));
        }
        if self.pose_buffer != native.pose_buffer {
            self.serial = self.serial.wrapping_add(1);
            let stamp = Stamp {
                character: native.character,
                continuity: self.run,
                finalized: self.serial,
            };
            let sample = Sample {
                root: RootSample {
                    stamp,
                    transform: native.root,
                },
                local: LocalSample {
                    stamp,
                    transform: native.local,
                },
            };
            let baseline = match self.baseline {
                Some(baseline) => baseline,
                None => {
                    if native.state != GraphState::GroundInto {
                        return Err(Refusal::Policy(InvalidPair::InvalidTransition));
                    }
                    let root = native.root.matrix().map_err(Refusal::Policy)?;
                    self.eye_offset = root.inverse().transform_point3(
                        views[1].world.w_axis.truncate() - views[1].tracking_translation,
                    );
                    let baseline = Baseline {
                        run: self.run,
                        sample,
                    };
                    self.baseline = Some(baseline);
                    baseline
                }
            };
            let frame = Frame {
                accepted: AcceptedRun {
                    case: AcceptedCase::BoostedGroundTakeoff,
                    character: native.character,
                    continuity: self.run,
                    run: self.run,
                },
                state: native.state,
                sample,
                baseline,
                blend: None,
                source: native.source.camera_bone.into(),
                clip_time: None,
                request: DeltaRequest::AuthoredLocalRelativeToBaseline,
                root_treatment: RootTreatment::SeparateFromAuthoredLocal,
            };
            let decision = self.policy.observe(frame);
            if let Some(reason) = decision.reason {
                return Err(Refusal::Policy(reason));
            }
            if decision.owner() == Owner::NormalTracked || decision.reset {
                self.pair = None;
                self.deltas = None;
            }
            if let Some(pair) = decision.pair {
                self.deltas = Some(
                    pair.deltas(frame.request, frame.root_treatment)
                        .map_err(Refusal::Policy)?,
                );
                self.pair = self.previous.map(|previous| [previous, sample]);
            }
            self.previous = Some(sample);
            self.pose_buffer = native.pose_buffer;
            *STATUS.lock() = Status::BoostedGroundTakeoff {
                phase: decision.phase,
                limitation: Limit::ExactStatesOnlyTreeSampledNoBlendTail,
            };
        }
        let (Some(pair), Some(deltas)) = (self.pair, self.deltas) else {
            self.selected = None;
            return Ok(None);
        };
        let mut result = [Mat4::IDENTITY; 2];
        for side in 0..2 {
            result[side] = compose_tracked(
                pair[side].root.transform,
                self.eye_offset,
                deltas[side],
                views[side],
            )?;
        }
        if always_use_t1 {
            result[0] = result[1];
        }
        self.selected = Some(result);
        *STATUS.lock() = Status::BoostedGroundTakeoff {
            phase: Phase::Live,
            limitation: Limit::ExactStatesOnlyTreeSampledNoBlendTail,
        };
        Ok(self.selected)
    }

    fn clear(&mut self) {
        self.collapse_normal |= self.selected.is_some();
        self.policy.interrupt();
        self.run = self.run.wrapping_add(1);
        self.character = 0;
        self.source = None;
        self.pose_buffer = 0;
        self.baseline = None;
        self.previous = None;
        self.pair = None;
        self.deltas = None;
        self.selected = None;
    }
}

/// Freeze the animated eye offset, not the tracked displacement or the character root.
fn compose_tracked(
    root: Trs,
    eye_offset: Vec3,
    delta: Mat4,
    view: TrackedView,
) -> Result<Mat4, Refusal> {
    let root_matrix = root.matrix().map_err(Refusal::Policy)?;
    let orientation = trs(view.world)?.rotation;
    let base = trs(Mat4::from_rotation_translation(
        root.rotation,
        root_matrix.transform_point3(eye_offset),
    ))?;
    let head = Trs {
        translation: root.rotation.inverse() * view.tracking_translation,
        rotation: root.rotation.inverse() * orientation,
        scale: Vec3::ONE,
    };
    compose_view(base, delta, head).map_err(Refusal::Policy)
}

// A mod-side discontinuity limit, not a native movement threshold.
const MAX_ROOT_STEP: f32 = 8.0;

static STATE: Mutex<Runtime> = Mutex::new(Runtime {
    policy: Policy::new(),
    character: 0,
    source: None,
    graph_state: None,
    pose_buffer: 0,
    serial: 0,
    run: 0,
    baseline: None,
    previous: None,
    pair: None,
    deltas: None,
    eye_offset: Vec3::ZERO,
    selected: None,
    collapse_normal: false,
});

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    fn native(buffer: u64, state: GraphState, translation: Vec3) -> NativeSample {
        NativeSample {
            character: 10,
            source: Source {
                controller: 20,
                producer: 30,
                skeleton: 40,
                camera_bone: 50,
            },
            pose_buffer: buffer,
            state,
            root: Trs::IDENTITY,
            local: Trs {
                translation,
                ..Trs::IDENTITY
            },
        }
    }

    #[test]
    fn tree_capture_runs_the_policy_and_reuses_only_owned_history() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        assert!(
            state
                .update(
                    native(100, GraphState::GroundInto, Vec3::ZERO),
                    views,
                    false
                )
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .update(
                    native(100, GraphState::GroundInto, Vec3::ZERO),
                    views,
                    false
                )
                .unwrap()
                .is_none()
        );
        let pair = state
            .update(
                native(200, GraphState::GroundFollowOn, Vec3::X),
                views,
                false,
            )
            .unwrap()
            .unwrap();
        assert!(pair[0].w_axis.truncate().abs_diff_eq(Vec3::ZERO, 0.0001));
        assert!(pair[1].w_axis.truncate().abs_diff_eq(Vec3::X, 0.0001));
        let repeated = state
            .update(
                native(200, GraphState::GroundFollowOn, Vec3::X),
                views,
                false,
            )
            .unwrap()
            .unwrap();
        assert_eq!(pair, repeated);
        assert_eq!(state.serial, 2);
    }

    #[test]
    fn tracked_motion_is_composed_once_without_animated_anchor_bob() {
        let root = Trs {
            translation: Vec3::Y * 10.0,
            ..Trs::IDENTITY
        };
        let eye = Vec3::Y * 1.7;
        let head = Vec3::new(0.2, 0.1, -0.3);
        let rotation = Quat::from_rotation_x(0.4);
        let ordinary = Mat4::from_rotation_translation(rotation, Vec3::Y * 15.0 + head);
        let view = TrackedView {
            world: ordinary,
            tracking_translation: head,
        };
        let result = compose_tracked(root, eye, Mat4::IDENTITY, view).unwrap();
        assert!(
            result
                .w_axis
                .truncate()
                .abs_diff_eq(root.translation + eye + head, 0.0001)
        );
        assert!(result.x_axis.abs_diff_eq(ordinary.x_axis, 0.0001));
    }
}
