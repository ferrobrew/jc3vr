//! Owned named-case history and one camera result shared by the tree and render hooks.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    controllers::roomscale::teleport,
    hooks::camera::authored::{
        Limit, Refusal, STATUS, Status,
        composition::{
            Baseline, DeltaRequest, InvalidPair, LocalSample, RootSample, RootTreatment, Sample,
            Stamp, Trs, compose_view,
        },
        evidence::Clip,
        native::{NativeSample, Source, read, trs},
        policy::{
            AcceptedRun, Frame, GraphState, NamedCase, NativeContribution, Owner, Phase, Policy,
        },
    },
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
    let now = Instant::now();
    let native = unsafe { read(character) };
    STATE
        .lock()
        .capture(native, views, always_use_t1, now, teleport::epoch())
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
    *STATUS.lock() = Status::NormalTracked { reason };
}

#[derive(Default)]
struct Runtime {
    policy: Policy,
    character: u64,
    source: Option<Source>,
    graph_state: Option<GraphState>,
    pose_buffer: u64,
    finalized: u64,
    clips: Vec<Clip>,
    clip_source: u64,
    outgoing: bool,
    serial: u64,
    run: u64,
    baseline: Option<Baseline>,
    previous: Option<Sample>,
    pair: Option<[Sample; 2]>,
    deltas: Option<[Mat4; 2]>,
    eye_offset: Vec3,
    selected: Option<[Mat4; 2]>,
    correction: Option<Trs>,
    handback: Option<Handback>,
    continuity: Option<Continuity>,
    collapse_normal: bool,
}

#[derive(Clone, Copy)]
struct Handback {
    correction: Trs,
    started: Instant,
    case: NamedCase,
}

#[derive(Clone, Copy)]
struct Continuity {
    character: u64,
    source: Source,
    root: Trs,
    teleport_epoch: u64,
}

impl Runtime {
    fn capture(
        &mut self,
        native: Result<NativeSample, Refusal>,
        views: [TrackedView; 2],
        always_use_t1: bool,
        now: Instant,
        teleport_epoch: u64,
    ) -> Option<[Mat4; 2]> {
        let result = native.and_then(|native| {
            let continuity = Continuity {
                character: native.character,
                source: native.source,
                root: native.root,
                teleport_epoch,
            };
            self.validate_continuity(continuity)?;
            let result = self.update(native, views, always_use_t1);
            self.continuity = Some(continuity);
            result
        });
        match result {
            Ok(result) => result,
            Err(reason)
                if self.continuity.is_some()
                    && matches!(
                        reason,
                        Refusal::OrdinaryOrUnsupportedState
                            | Refusal::IdleTailWeightNotBound
                            | Refusal::NativeTailComplete
                    ) =>
            {
                self.release(reason, views, always_use_t1, now)
            }
            Err(reason) => {
                self.clear();
                *STATUS.lock() = Status::NormalTracked { reason };
                None
            }
        }
    }

    fn validate_continuity(&self, current: Continuity) -> Result<(), Refusal> {
        current.root.matrix().map_err(Refusal::Policy)?;
        if let Some(previous) = self.continuity {
            if previous.character != current.character {
                return Err(Refusal::Policy(InvalidPair::CharacterChanged));
            }
            if previous.source != current.source {
                return Err(Refusal::Policy(InvalidPair::SourceChanged));
            }
            if previous.teleport_epoch != current.teleport_epoch
                || previous.root.translation.distance(current.root.translation) > MAX_ROOT_STEP
            {
                return Err(Refusal::Policy(InvalidPair::Teleport));
            }
        }
        Ok(())
    }

    fn update(
        &mut self,
        native: NativeSample,
        views: [TrackedView; 2],
        always_use_t1: bool,
    ) -> Result<Option<[Mat4; 2]>, Refusal> {
        native.animation.validate()?;
        let outgoing = matches!(
            native.state,
            Err(Refusal::OrdinaryOrUnsupportedState | Refusal::IdleTailWeightNotBound)
        );
        let graph_state = if outgoing {
            self.graph_state.ok_or(native.state.unwrap_err())?
        } else {
            native.state?
        };
        let case = graph_state
            .named_case()
            .ok_or(Refusal::OrdinaryOrUnsupportedState)?;
        let clips = if outgoing {
            self.clips
                .iter()
                .filter_map(|previous| {
                    native
                        .animation
                        .samplers
                        .iter()
                        .find(|clip| clip.source == previous.source)
                        .copied()
                })
                .collect::<Vec<_>>()
        } else {
            native.animation.playing.clone()
        };
        if self.previous.is_some()
            && self.finalized == native.finalized
            && (self.graph_state != Some(graph_state)
                || self.clips != clips
                || self.outgoing != outgoing)
        {
            return Err(Refusal::IncoherentAnimation);
        }
        if outgoing && !clips.iter().copied().any(Clip::contributes) {
            return Err(Refusal::NativeTailComplete);
        }
        let same_state = self.graph_state == Some(graph_state);
        let source_changed = !outgoing
            && (clips.len() != self.clips.len()
                || clips.iter().any(|clip| {
                    !self
                        .clips
                        .iter()
                        .any(|previous| previous.source == clip.source)
                }));
        let clip_restart = same_state
            && !outgoing
            && (self.outgoing
                || clips.len() != self.clips.len()
                || clips.iter().any(|clip| {
                    self.clips
                        .iter()
                        .find(|previous| previous.source == clip.source)
                        .is_none_or(|previous| clip.restarted(*previous))
                }));
        if outgoing
            && clips.iter().any(|clip| {
                self.clips
                    .iter()
                    .find(|previous| previous.source == clip.source)
                    .is_some_and(|previous| clip.restarted(*previous))
            })
        {
            return Err(Refusal::InvalidClipTime);
        }
        if self.character != native.character
            || self.source != Some(native.source)
            || clip_restart
            || self
                .graph_state
                .is_some_and(|previous| !previous.continues(graph_state))
        {
            self.clear();
            if !graph_state.can_enter() {
                return Err(Refusal::Policy(InvalidPair::InvalidTransition));
            }
            self.character = native.character;
            self.source = Some(native.source);
        }
        if self.previous.is_some() && native.finalized < self.finalized {
            return Err(Refusal::IncoherentAnimation);
        }
        if source_changed || clip_restart || self.previous.is_none() {
            self.clip_source = native.finalized;
        }
        self.graph_state = Some(graph_state);
        self.clips = clips;
        self.outgoing = outgoing;
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
        if self.previous.is_none() || self.finalized != native.finalized {
            self.serial = self.serial.wrapping_add(1);
            let stamp = Stamp {
                character: native.character,
                continuity: self.run,
                finalized: native.finalized,
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
                    if !graph_state.can_enter() {
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
                    case,
                    character: native.character,
                    continuity: self.run,
                    run: self.run,
                },
                state: graph_state,
                sample,
                baseline,
                blend: None,
                native: Some(NativeContribution {
                    stamp,
                    run: self.run,
                    source: self.clip_source,
                    outgoing,
                    contributing: self.clips.iter().copied().any(Clip::contributes),
                }),
                source: self.clip_source,
                clip_time: self.clips.first().map(|clip| clip.time),
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
            self.finalized = native.finalized;
            *STATUS.lock() = Status::NamedCase {
                case,
                phase: decision.phase,
                limitation: Limit::SynchronousSourceVerifiedNativeBlend,
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
        self.correction = Some(trs(views[1].world.inverse() * result[1])?);
        self.handback = None;
        self.collapse_normal = false;
        self.selected = Some(result);
        *STATUS.lock() = Status::NamedCase {
            case,
            phase: if outgoing { Phase::Tail } else { Phase::Live },
            limitation: Limit::SynchronousSourceVerifiedNativeBlend,
        };
        Ok(self.selected)
    }

    fn release(
        &mut self,
        reason: Refusal,
        views: [TrackedView; 2],
        always_use_t1: bool,
        now: Instant,
    ) -> Option<[Mat4; 2]> {
        if self.handback.is_none() {
            let handback = self
                .correction
                .zip(self.graph_state)
                .and_then(|(correction, state)| {
                    state.named_case().map(|case| Handback {
                        correction,
                        started: now,
                        case,
                    })
                });
            if let Some(handback) = handback {
                let character = self.character;
                let continuity = self.continuity;
                self.clear();
                self.character = character;
                self.continuity = continuity;
                self.handback = Some(handback);
            } else {
                self.clear();
                *STATUS.lock() = Status::NormalTracked { reason };
                return None;
            }
        }
        let handback = self.handback?;
        let remaining = 1.0
            - (now
                .saturating_duration_since(handback.started)
                .as_secs_f32()
                / HANDBACK_DURATION.as_secs_f32())
            .min(1.0);
        if remaining <= 0.0 {
            self.clear();
            *STATUS.lock() = Status::NormalTracked { reason };
            return None;
        }
        let amount = remaining * remaining * (3.0 - 2.0 * remaining);
        let correction = Trs {
            translation: handback.correction.translation * amount,
            rotation: Quat::IDENTITY.slerp(handback.correction.rotation, amount),
            scale: Vec3::ONE.lerp(handback.correction.scale, amount),
        };
        let result = correction.matrix().ok().and_then(|correction| {
            let mut result = views.map(|view| view.world * correction);
            if always_use_t1 {
                result[0] = result[1];
            }
            result
                .iter()
                .all(|matrix| trs(*matrix).is_ok())
                .then_some(result)
        });
        let Some(result) = result else {
            self.clear();
            *STATUS.lock() = Status::NormalTracked { reason };
            return None;
        };
        self.correction = Some(correction);
        self.selected = Some(result);
        self.collapse_normal = false;
        *STATUS.lock() = Status::Handback {
            case: handback.case,
            reason,
        };
        self.selected
    }

    fn clear(&mut self) {
        self.collapse_normal |= self.selected.is_some();
        self.policy.interrupt();
        self.run = self.run.wrapping_add(1);
        self.character = 0;
        self.source = None;
        self.graph_state = None;
        self.pose_buffer = 0;
        self.finalized = 0;
        self.clips.clear();
        self.clip_source = 0;
        self.outgoing = false;
        self.baseline = None;
        self.previous = None;
        self.pair = None;
        self.deltas = None;
        self.selected = None;
        self.correction = None;
        self.handback = None;
        self.continuity = None;
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
const HANDBACK_DURATION: Duration = Duration::from_millis(250);

static STATE: Mutex<Runtime> = Mutex::new(Runtime {
    policy: Policy::new(),
    character: 0,
    source: None,
    graph_state: None,
    pose_buffer: 0,
    finalized: 0,
    clips: Vec::new(),
    clip_source: 0,
    outgoing: false,
    serial: 0,
    run: 0,
    baseline: None,
    previous: None,
    pair: None,
    deltas: None,
    eye_offset: Vec3::ZERO,
    selected: None,
    correction: None,
    handback: None,
    continuity: None,
    collapse_normal: false,
});

#[cfg(test)]
mod tests {
    use super::*;
    #[path = "adapter.rs"]
    mod adapter;

    use crate::hooks::camera::authored::{
        evidence::{Animation, ClipSource},
        policy::{HangingState, LandingState, RecoveryState},
    };

    fn native(buffer: u64, state: GraphState, translation: Vec3) -> NativeSample {
        let graph_serial: u64 = match state {
            GraphState::GroundInto => 1,
            GraphState::GroundFollowOn => 2,
            GraphState::Landing(LandingState::Hard) => 3,
            GraphState::Landing(_) => 4,
            GraphState::Recovery(RecoveryState::PoseMatching) => 5,
            GraphState::Recovery(RecoveryState::BlendIn) => 6,
            GraphState::Recovery(_) => 7,
            GraphState::FloorPickup => 8,
            GraphState::Hanging(HangingState::GrappleAttached) => 9,
            GraphState::Hanging(HangingState::StuntIdle) => 10,
            _ => 11,
        };
        NativeSample {
            character: 10,
            source: Source {
                controller: 20,
                producer: 30,
                producer_owner: 31,
                skeleton_resource: 41,
                skeleton: 40,
                driver: 42,
                rule: 43,
                blender: 44,
                camera_bone: 50,
            },
            pose_buffer: buffer,
            finalized: buffer * 100 + graph_serial,
            animation: Animation {
                state: Some(1),
                playing: vec![clip(buffer as f32 / 1000.0)],
                samplers: vec![clip(buffer as f32 / 1000.0)],
            },
            state: Ok(state),
            root: Trs::IDENTITY,
            local: Trs {
                translation,
                ..Trs::IDENTITY
            },
        }
    }

    fn clip(time: f32) -> Clip {
        Clip {
            source: ClipSource {
                instance: 60,
                owner: 61,
                animation: 62,
                binding: 63,
            },
            time,
            duration: 2.0,
            looping: false,
            overflows: 0,
            underflows: 0,
            weight: 1.0,
            camera_track_weight: 1.0,
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
    fn every_named_family_runs_the_same_native_sample_path() {
        for graph in [
            GraphState::GroundInto,
            GraphState::Landing(LandingState::Hard),
            GraphState::Recovery(RecoveryState::GetUp),
            GraphState::FloorPickup,
            GraphState::Hanging(HangingState::GrappleAttached),
        ] {
            let mut state = Runtime::default();
            let views = [TrackedView {
                world: Mat4::IDENTITY,
                tracking_translation: Vec3::ZERO,
            }; 2];
            assert!(
                state
                    .update(native(100, graph, Vec3::ZERO), views, false)
                    .unwrap()
                    .is_none()
            );
            let pair = state
                .update(native(200, graph, Vec3::X), views, false)
                .unwrap()
                .unwrap();
            assert!(pair[0].w_axis.truncate().abs_diff_eq(Vec3::ZERO, 0.0001));
            assert!(pair[1].w_axis.truncate().abs_diff_eq(Vec3::X, 0.0001));
        }
    }

    #[test]
    fn identity_hanging_channel_still_has_an_active_runtime_pair() {
        let mut state = Runtime::default();
        let graph = GraphState::Hanging(HangingState::GrappleAttached);
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        state
            .update(native(100, graph, Vec3::ZERO), views, false)
            .unwrap();
        assert_eq!(
            state
                .update(native(200, graph, Vec3::ZERO), views, false)
                .unwrap(),
            Some([Mat4::IDENTITY; 2])
        );
        assert!(state.deltas.is_some());
    }

    #[test]
    fn clear_allows_repeated_takeoff_runs_after_the_follow_on() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        for _ in 0..3 {
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
                        native(200, GraphState::GroundFollowOn, Vec3::X),
                        views,
                        false
                    )
                    .unwrap()
                    .is_some()
            );
            state.clear();
            assert_eq!(state.graph_state, None);
        }
    }

    #[test]
    fn new_family_and_distinct_same_family_motion_rebase_even_on_the_same_buffer() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        let first = GraphState::Landing(LandingState::Hard);
        state
            .update(native(100, first, Vec3::ZERO), views, false)
            .unwrap();
        state
            .update(native(200, first, Vec3::X), views, false)
            .unwrap();
        for graph in [
            GraphState::Landing(LandingState::FastFall),
            GraphState::FloorPickup,
            GraphState::Hanging(HangingState::GrappleAttached),
            GraphState::Hanging(HangingState::StuntIdle),
        ] {
            let previous_run = state.run;
            assert!(
                state
                    .update(native(200, graph, Vec3::Y * 3.0), views, false)
                    .unwrap()
                    .is_none()
            );
            assert!(state.run > previous_run);
            assert_eq!(
                state.baseline.unwrap().sample.local.transform.translation,
                Vec3::Y * 3.0
            );
            let pair = state
                .update(native(300, graph, Vec3::Y * 4.0), views, false)
                .unwrap()
                .unwrap();
            assert!(pair[0].w_axis.truncate().abs_diff_eq(Vec3::ZERO, 0.0001));
            assert!(pair[1].w_axis.truncate().abs_diff_eq(Vec3::Y, 0.0001));
        }
    }

    #[test]
    fn recovery_handoffs_keep_the_entry_baseline_and_reverse_order_restarts() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        for (buffer, recovery) in [
            (100, RecoveryState::PoseMatching),
            (200, RecoveryState::BlendIn),
            (300, RecoveryState::GetUp),
        ] {
            state
                .update(
                    native(
                        buffer,
                        GraphState::Recovery(recovery),
                        Vec3::X * buffer as f32,
                    ),
                    views,
                    false,
                )
                .unwrap();
        }
        assert_eq!(
            state.baseline.unwrap().sample.local.transform.translation,
            Vec3::X * 100.0
        );
        assert!(
            state
                .update(
                    native(
                        300,
                        GraphState::Recovery(RecoveryState::PoseMatching),
                        Vec3::ZERO
                    ),
                    views,
                    false
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state.baseline.unwrap().sample.local.transform.translation,
            Vec3::ZERO
        );
    }

    #[test]
    fn handback_starts_at_the_current_correction_and_keeps_tracking_live() {
        let mut state = Runtime::default();
        let graph = GraphState::FloorPickup;
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        state
            .update(native(100, graph, Vec3::ZERO), views, false)
            .unwrap();
        state
            .update(native(200, graph, Vec3::X), views, false)
            .unwrap();
        let now = Instant::now();
        let reason = Refusal::OrdinaryOrUnsupportedState;
        let start = state.release(reason, views, false, now).unwrap();
        assert!(start[1].w_axis.truncate().abs_diff_eq(Vec3::X, 0.0001));
        let moving = [TrackedView {
            world: Mat4::from_rotation_translation(Quat::from_rotation_y(0.3), Vec3::Y * 2.0),
            tracking_translation: Vec3::Y * 2.0,
        }; 2];
        let middle = state
            .release(reason, moving, false, now + HANDBACK_DURATION / 2)
            .unwrap();
        let expected = moving[1].world * Mat4::from_translation(Vec3::X * 0.5);
        assert!(middle[1].abs_diff_eq(expected, 0.0001));
        assert!(
            state
                .release(reason, moving, false, now + HANDBACK_DURATION)
                .is_none()
        );
        assert!(state.selected.is_none());
        assert!(state.collapse_normal);
        assert_eq!(state.graph_state, None);
    }

    #[test]
    fn unsupported_exit_and_active_handback_clear_on_discontinuity() {
        for during_handback in [false, true] {
            for change in 0..5 {
                let mut state = Runtime::default();
                let views = [TrackedView {
                    world: Mat4::IDENTITY,
                    tracking_translation: Vec3::ZERO,
                }; 2];
                let now = Instant::now();
                let epoch = 7;
                for (buffer, translation) in [(100, Vec3::ZERO), (200, Vec3::X)] {
                    let _ = state.capture(
                        Ok(native(buffer, GraphState::FloorPickup, translation)),
                        views,
                        false,
                        now,
                        epoch,
                    );
                }
                assert!(state.selected.is_some());
                let mut exit = native(300, GraphState::FloorPickup, Vec3::X);
                exit.state = Err(Refusal::OrdinaryOrUnsupportedState);
                exit.animation.samplers[0].weight = 0.0;
                exit.animation.playing[0].weight = 0.0;
                if during_handback {
                    assert!(state.capture(Ok(exit), views, false, now, epoch).is_some());
                    assert!(state.handback.is_some());
                    assert!(state.continuity.is_some());
                    exit = native(400, GraphState::FloorPickup, Vec3::X);
                    exit.state = Err(Refusal::OrdinaryOrUnsupportedState);
                    exit.animation.samplers[0].weight = 0.0;
                    exit.animation.playing[0].weight = 0.0;
                }
                let mut next_epoch = epoch;
                match change {
                    0 => exit.root.translation = Vec3::X * (MAX_ROOT_STEP + 1.0),
                    1 => exit.source.producer += 1,
                    2 => next_epoch += 1,
                    3 => exit.root.translation.x = f32::NAN,
                    _ => exit.character += 1,
                }
                assert!(
                    state
                        .capture(
                            Ok(exit),
                            views,
                            false,
                            now + HANDBACK_DURATION / 4,
                            next_epoch,
                        )
                        .is_none()
                );
                assert!(state.selected.is_none());
                assert!(state.handback.is_none());
                assert!(state.correction.is_none());
                assert!(state.continuity.is_none());
                assert!(state.collapse_normal);
            }
        }
    }

    #[test]
    fn handback_checks_each_new_root_and_native_read_failure() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        let now = Instant::now();
        for (buffer, translation) in [(100, Vec3::ZERO), (200, Vec3::X)] {
            let _ = state.capture(
                Ok(native(buffer, GraphState::FloorPickup, translation)),
                views,
                false,
                now,
                0,
            );
        }
        for (step, position) in [1.0, 6.0, 11.0].into_iter().enumerate() {
            let mut exit = native(300, GraphState::FloorPickup, Vec3::X);
            exit.state = Err(Refusal::IdleTailWeightNotBound);
            exit.animation.samplers[0].weight = 0.0;
            exit.animation.playing[0].weight = 0.0;
            exit.root.translation = Vec3::X * position;
            assert!(
                state
                    .capture(
                        Ok(exit),
                        views,
                        false,
                        now + Duration::from_millis(step as u64 * 10),
                        0,
                    )
                    .is_some()
            );
            assert_eq!(
                state.continuity.unwrap().root.translation,
                Vec3::X * position
            );
        }
        assert!(
            state
                .capture(
                    Err(Refusal::MissingChannel),
                    views,
                    false,
                    now + Duration::from_millis(40),
                    0,
                )
                .is_none()
        );
        assert!(state.selected.is_none());
        assert!(state.handback.is_none());
        assert!(state.continuity.is_none());
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
