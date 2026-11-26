//! Owned named-case history and one camera result shared by the tree and render hooks.

use std::time::{Duration, Instant};

use body_rig::smooth::smoothstep;
use glam::{Mat4, Quat, Vec3};
use jc3gi::character::character::Character;
use parking_lot::Mutex;

use crate::{
    headpose::teleport,
    hooks::camera::authored::{
        Phase, Refusal, STATUS, Status,
        composition::{Trs, compose_view, deltas},
        evidence::Clip,
        native::{NativeSample, Source, State, read, trs},
    },
    tuning::VIEW,
};

/// The ordinary camera and the tracked-only displacement on one side of the pose pair.
#[derive(Clone, Copy)]
pub(crate) struct TrackedView {
    pub world: Mat4,
    pub tracking_translation: Vec3,
}

/// Capture at the native camera-tree read phase. No pointer escapes this call.
pub(crate) unsafe fn capture(character: &Character, views: [TrackedView; 2]) -> Option<[Mat4; 2]> {
    // Under the headset the view already follows the animated head frame (`headpose::animation`),
    // so the authored camera runs only under the flatscreen head source.
    if crate::headpose::source() == crate::headpose::Source::Vr {
        reset(Refusal::HeadsetSource);
        return None;
    }
    let now = Instant::now();
    let native = unsafe { read(character) };
    STATE.lock().capture(native, views, now, teleport::epoch())
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

/// The root and the marker's model-space transform from one finalized pose.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    root: Trs,
    local: Trs,
}

#[derive(Default)]
struct Runtime {
    character: u64,
    source: Option<Source>,
    graph_state: Option<State>,
    finalized: u64,
    observed: Option<NativeSample>,
    clips: Vec<Clip>,
    clip_source: u64,
    outgoing: bool,
    run: u64,
    /// The marker's transform at the run's entry.
    baseline: Option<Trs>,
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
    duration: Duration,
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
            let result = self.update(native, views);
            self.continuity = Some(continuity);
            result
        });
        match result {
            Ok(result) => result,
            Err(reason)
                if self.continuity.is_some()
                    && matches!(
                        reason,
                        Refusal::UnsupportedState | Refusal::NativeTailComplete
                    ) =>
            {
                self.release(reason, views, now)
            }
            Err(reason) => {
                self.clear();
                *STATUS.lock() = Status::NormalTracked { reason };
                None
            }
        }
    }

    fn validate_continuity(&self, current: Continuity) -> Result<(), Refusal> {
        current.root.matrix()?;
        if let Some(previous) = self.continuity {
            if previous.character != current.character {
                return Err(Refusal::CharacterChanged);
            }
            if previous.source != current.source {
                return Err(Refusal::SourceChanged);
            }
            if previous.teleport_epoch != current.teleport_epoch
                || previous.root.translation.distance(current.root.translation)
                    > VIEW.get().handback.max_root_step_m
            {
                return Err(Refusal::Teleport);
            }
        }
        Ok(())
    }

    fn update(
        &mut self,
        native: NativeSample,
        views: [TrackedView; 2],
    ) -> Result<Option<[Mat4; 2]>, Refusal> {
        native.animation.validate()?;
        // A pending handoff retains admitted clips; coherence compares the unchanged native input.
        if self.previous.is_some()
            && self.finalized == native.finalized
            && self.observed.as_ref() != Some(&native)
        {
            return Err(Refusal::IncoherentAnimation);
        }
        let outgoing = matches!(native.state, Err(Refusal::UnsupportedState));
        let mut graph_state = if outgoing {
            self.graph_state.ok_or(native.state.unwrap_err())?
        } else {
            native.state?
        };
        let mut clips = if outgoing {
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
        let handoff_contributes = !outgoing
            && self.character == native.character
            && self.source == Some(native.source)
            && self
                .graph_state
                .is_some_and(|previous| previous != graph_state && previous.continues(graph_state))
            && self.clips.iter().any(|previous| {
                native
                    .animation
                    .samplers
                    .iter()
                    .any(|clip| clip.source == previous.source && clip.contributes())
            });
        if !clips.iter().copied().any(Clip::contributes) {
            if !handoff_contributes {
                return Err(if outgoing {
                    Refusal::NativeTailComplete
                } else {
                    Refusal::MissingChannel
                });
            }
            graph_state = self.graph_state.ok_or(Refusal::MissingState)?;
            clips = self
                .clips
                .iter()
                .filter_map(|previous| {
                    native
                        .animation
                        .samplers
                        .iter()
                        .find(|clip| clip.source == previous.source)
                        .copied()
                })
                .collect();
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
            && previous.root.translation.distance(native.root.translation)
                > VIEW.get().handback.max_root_step_m
        {
            return Err(Refusal::Teleport);
        }
        if self.previous.is_none() || self.finalized != native.finalized {
            let sample = Sample {
                root: native.root,
                local: native.local,
            };
            let baseline = match self.baseline {
                Some(baseline) => baseline,
                None => {
                    let root = native.root.matrix()?;
                    self.eye_offset = root.inverse().transform_point3(
                        views[1].world.w_axis.truncate() - views[1].tracking_translation,
                    );
                    self.baseline = Some(native.local);
                    native.local
                }
            };
            // The first sample of a run only records the baseline; normal tracking stays active
            // until the second forms the first pair.
            let phase = match self.previous {
                None => {
                    self.pair = None;
                    self.deltas = None;
                    Phase::Entry
                }
                Some(previous) => {
                    self.deltas = Some(deltas(baseline, [previous.local, sample.local])?);
                    self.pair = Some([previous, sample]);
                    if outgoing { Phase::Tail } else { Phase::Live }
                }
            };
            self.previous = Some(sample);
            self.finalized = native.finalized;
            self.observed = Some(native);
            *STATUS.lock() = Status::Following { phase };
        }
        let (Some(pair), Some(deltas)) = (self.pair, self.deltas) else {
            self.selected = None;
            return Ok(None);
        };
        let mut result = [Mat4::IDENTITY; 2];
        for side in 0..2 {
            result[side] =
                compose_tracked(pair[side].root, self.eye_offset, deltas[side], views[side])?;
        }
        self.correction = Some(trs(views[1].world.inverse() * result[1])?);
        self.handback = None;
        self.collapse_normal = false;
        self.selected = Some(result);
        *STATUS.lock() = Status::Following {
            phase: if outgoing { Phase::Tail } else { Phase::Live },
        };
        Ok(self.selected)
    }

    fn release(
        &mut self,
        reason: Refusal,
        views: [TrackedView; 2],
        now: Instant,
    ) -> Option<[Mat4; 2]> {
        if self.handback.is_none() {
            let handback = self.correction.map(|correction| Handback {
                correction,
                started: now,
                duration: handback_duration(correction),
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
                / handback.duration.as_secs_f32())
            .min(1.0);
        if remaining <= 0.0 {
            self.clear();
            *STATUS.lock() = Status::NormalTracked { reason };
            return None;
        }
        let amount = smoothstep(remaining);
        let correction = Trs {
            translation: handback.correction.translation * amount,
            rotation: Quat::IDENTITY.slerp(handback.correction.rotation, amount),
            scale: Vec3::ONE.lerp(handback.correction.scale, amount),
        };
        let result = correction.matrix().ok().and_then(|correction| {
            let result = views.map(|view| view.world * correction);
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
        *STATUS.lock() = Status::Handback { reason };
        self.selected
    }

    fn clear(&mut self) {
        self.collapse_normal |= self.selected.is_some();
        self.run = self.run.wrapping_add(1);
        self.character = 0;
        self.source = None;
        self.graph_state = None;
        self.finalized = 0;
        self.observed = None;
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

/// The handback eases out the correction at a bounded average rate, so a large residual (an
/// interrupted inversion can leave up to a half-turn of roll) returns over proportionally longer
/// than a small one, and no correction returns faster than the minimum.
fn handback_duration(correction: Trs) -> Duration {
    let angle = correction
        .rotation
        .normalize()
        .angle_between(Quat::IDENTITY);
    let distance = correction.translation.length();
    let tuning = &VIEW.get().handback;
    if !angle.is_finite() || !distance.is_finite() {
        return Duration::from_secs_f32(tuning.max_s);
    }
    let seconds = (angle / tuning.rotation_rate_rad_per_s)
        .max(distance / tuning.translation_rate_m_per_s)
        .clamp(tuning.min_s, tuning.max_s);
    Duration::from_secs_f32(seconds)
}

/// Freeze the animated eye offset, not the tracked displacement or the character root.
fn compose_tracked(
    root: Trs,
    eye_offset: Vec3,
    delta: Mat4,
    view: TrackedView,
) -> Result<Mat4, Refusal> {
    let root_matrix = root.matrix()?;
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
    compose_view(base, delta, head)
}

static STATE: Mutex<Runtime> = Mutex::new(Runtime {
    character: 0,
    source: None,
    graph_state: None,
    finalized: 0,
    observed: None,
    clips: Vec::new(),
    clip_source: 0,
    outgoing: false,
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
    mod adapter;
    mod grapple;

    use crate::hooks::camera::authored::evidence::{Animation, ClipSource};

    fn handback_min() -> Duration {
        Duration::from_secs_f32(VIEW.get().handback.min_s)
    }

    fn handback_max() -> Duration {
        Duration::from_secs_f32(VIEW.get().handback.max_s)
    }

    fn native(buffer: u64, state: State, translation: Vec3) -> NativeSample {
        let graph_serial: u64 = match state {
            State::PreUpsideDown => 1,
            State::UpsideDown => 2,
            State::StuntIdle => 3,
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
    fn repeated_reads_of_one_serial_reuse_the_owned_pair() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        assert!(
            state
                .update(native(100, State::PreUpsideDown, Vec3::ZERO), views)
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .update(native(100, State::PreUpsideDown, Vec3::ZERO), views)
                .unwrap()
                .is_none()
        );
        let pair = state
            .update(native(200, State::UpsideDown, Vec3::X), views)
            .unwrap()
            .unwrap();
        assert!(pair[0].w_axis.truncate().abs_diff_eq(Vec3::ZERO, 0.0001));
        assert!(pair[1].w_axis.truncate().abs_diff_eq(Vec3::X, 0.0001));
        let repeated = state
            .update(native(200, State::UpsideDown, Vec3::X), views)
            .unwrap()
            .unwrap();
        assert_eq!(pair, repeated);
    }

    #[test]
    fn leaving_a_continuing_state_rebases_even_on_the_same_buffer() {
        let mut state = Runtime::default();
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        let first = State::UpsideDown;
        state.update(native(100, first, Vec3::ZERO), views).unwrap();
        state.update(native(200, first, Vec3::X), views).unwrap();
        for graph in [State::PreUpsideDown, State::StuntIdle] {
            let previous_run = state.run;
            assert!(
                state
                    .update(native(200, graph, Vec3::Y * 3.0), views)
                    .unwrap()
                    .is_none()
            );
            assert!(state.run > previous_run);
            assert_eq!(state.baseline.unwrap().translation, Vec3::Y * 3.0);
            let pair = state
                .update(native(300, graph, Vec3::Y * 4.0), views)
                .unwrap()
                .unwrap();
            assert!(pair[0].w_axis.truncate().abs_diff_eq(Vec3::ZERO, 0.0001));
            assert!(pair[1].w_axis.truncate().abs_diff_eq(Vec3::Y, 0.0001));
        }
    }

    #[test]
    fn handback_duration_scales_with_the_correction() {
        let rotated = |angle: f32| Trs {
            rotation: Quat::from_rotation_z(angle),
            ..Trs::IDENTITY
        };
        assert_eq!(handback_duration(Trs::IDENTITY), handback_min());
        let quarter = handback_duration(rotated(std::f32::consts::FRAC_PI_2));
        let half = handback_duration(rotated(std::f32::consts::PI));
        assert!((quarter.as_secs_f32() - 0.75).abs() < 0.001);
        assert!(half > quarter);
        assert_eq!(
            handback_duration(rotated(3.0 * std::f32::consts::PI / 2.0)),
            quarter
        );
        assert!(half <= handback_max());
        let far = Trs {
            translation: Vec3::X * 10.0,
            ..Trs::IDENTITY
        };
        assert_eq!(handback_duration(far), handback_max());
        let nan = Trs {
            translation: Vec3::NAN,
            ..Trs::IDENTITY
        };
        assert_eq!(handback_duration(nan), handback_max());
    }

    #[test]
    fn handback_starts_at_the_current_correction_and_keeps_tracking_live() {
        let mut state = Runtime::default();
        let graph = State::StuntIdle;
        let views = [TrackedView {
            world: Mat4::IDENTITY,
            tracking_translation: Vec3::ZERO,
        }; 2];
        state.update(native(100, graph, Vec3::ZERO), views).unwrap();
        state.update(native(200, graph, Vec3::X), views).unwrap();
        let now = Instant::now();
        let reason = Refusal::UnsupportedState;
        let start = state.release(reason, views, now).unwrap();
        assert!(start[1].w_axis.truncate().abs_diff_eq(Vec3::X, 0.0001));
        let duration = state.handback.unwrap().duration;
        assert_eq!(duration, Duration::from_secs(1));
        let moving = [TrackedView {
            world: Mat4::from_rotation_translation(Quat::from_rotation_y(0.3), Vec3::Y * 2.0),
            tracking_translation: Vec3::Y * 2.0,
        }; 2];
        let middle = state.release(reason, moving, now + duration / 2).unwrap();
        let expected = moving[1].world * Mat4::from_translation(Vec3::X * 0.5);
        assert!(middle[1].abs_diff_eq(expected, 0.0001));
        assert!(state.release(reason, moving, now + duration).is_none());
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
                        Ok(native(buffer, State::StuntIdle, translation)),
                        views,
                        now,
                        epoch,
                    );
                }
                assert!(state.selected.is_some());
                let mut exit = native(300, State::StuntIdle, Vec3::X);
                exit.state = Err(Refusal::UnsupportedState);
                exit.animation.samplers[0].weight = 0.0;
                exit.animation.playing[0].weight = 0.0;
                if during_handback {
                    assert!(state.capture(Ok(exit), views, now, epoch).is_some());
                    assert!(state.handback.is_some());
                    assert!(state.continuity.is_some());
                    exit = native(400, State::StuntIdle, Vec3::X);
                    exit.state = Err(Refusal::UnsupportedState);
                    exit.animation.samplers[0].weight = 0.0;
                    exit.animation.playing[0].weight = 0.0;
                }
                let mut next_epoch = epoch;
                match change {
                    0 => {
                        exit.root.translation =
                            Vec3::X * (VIEW.get().handback.max_root_step_m + 1.0)
                    }
                    1 => exit.source.producer += 1,
                    2 => next_epoch += 1,
                    3 => exit.root.translation.x = f32::NAN,
                    _ => exit.character += 1,
                }
                assert!(
                    state
                        .capture(Ok(exit), views, now + handback_min() / 4, next_epoch,)
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
                Ok(native(buffer, State::StuntIdle, translation)),
                views,
                now,
                0,
            );
        }
        for (step, position) in [1.0, 6.0, 11.0].into_iter().enumerate() {
            let mut exit = native(300, State::StuntIdle, Vec3::X);
            exit.state = Err(Refusal::UnsupportedState);
            exit.animation.samplers[0].weight = 0.0;
            exit.animation.playing[0].weight = 0.0;
            exit.root.translation = Vec3::X * position;
            assert!(
                state
                    .capture(
                        Ok(exit),
                        views,
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
