//! Simulation-time body fitting from copied tracking and the solve gate's reference rig.
//!
//! A commit supplies the pose writer and the next locomotion task from one result. Unavailable
//! tracking and rejected fits withdraw inferred authority without changing the view or hand input.

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec2, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    body::{
        BodyDimensions,
        procedural::rig::Rig,
        rig::{
            estimate::{Anatomy, EstimateError, Estimator, Quality, Snapshot, Transition, Update},
            observation::{
                CompositionPair, CompositionSide, Epochs, Observation, RawPose, RawTracking,
                SampleStamp, StepStamp,
            },
            root::{owner::RootOwner, shift::CockpitBasis},
            torso::{TorsoEstimate, wrap, yaw_of},
        },
    },
    frame::{ControllerFrame, Hand, Pose, TrackedPose},
};

/// Why inferred body authority is absent on the latest simulation update.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum Fallback {
    #[default]
    NotUpdated,
    TrackingUnavailable,
    Owner,
    Rejected(EstimateError),
    Residual,
    Realization,
    Stale,
}

/// The fit's actual work and residuals, including unsuccessful updates.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub(crate) struct FitStatus {
    pub step: u64,
    pub sample: Option<u64>,
    pub quality: Option<Quality>,
    pub fallback: Option<Fallback>,
    pub evaluations: usize,
}

/// Copied simulation inputs. Neither the adapter nor the estimator holds an engine pointer.
pub(crate) struct GateInput<'a> {
    pub rig: &'a Rig,
    pub player: BodyDimensions,
    pub previous_root: Mat4,
    pub root: Mat4,
    pub head_world: Vec3,
    pub view: Quat,
    pub world_scale: f32,
    pub origin: u64,
    pub now: Instant,
    pub dt: f32,
}

/// Retain the frame's copied head and controller sample for the next simulation correction.
pub(crate) fn frame_tick(head: Pose, frame: Option<&ControllerFrame>) {
    let packet = frame.map(|frame| Tracking {
        head: TrackedPose {
            pose: head,
            position_valid: head.position.is_finite(),
            orientation_valid: head.orientation.is_finite()
                && (head.orientation.length_squared() - 1.0).abs() < 1.0e-3,
            ..TrackedPose::default()
        },
        grips: Hand::BOTH.map(|hand| {
            let hand = frame.hand(hand);
            hand.active.then_some(hand.grip)
        }),
        sequence: frame.sequence,
        display_time_ns: frame.display_time_ns,
        published_at: frame.published_at,
        focused: frame.focused,
    });
    *TRACKING.lock() = packet;
}

/// Commit once at the local solve gate and publish exactly that result for the movement owner.
pub(crate) fn at_gate(input: GateInput<'_>) -> Option<Snapshot> {
    let tracking = *TRACKING.lock();
    STATE.lock().commit(tracking, input)
}

/// Withdraw the previous request when the animation or an anchored solve owns this update.
pub(crate) fn hand_back() {
    let mut state = STATE.lock();
    state.migrated_at = None;
    state.withdraw(Fallback::Owner);
}

/// Keep a rejected on-foot update on the migrated path while withdrawing its inferred output.
pub(crate) fn gate_failed() {
    let mut state = STATE.lock();
    let reason = state.status.fallback.unwrap_or(Fallback::Owner);
    state.withdraw(reason);
}

pub(crate) fn realization_failed() {
    STATE.lock().withdraw(Fallback::Realization);
}

/// Whether the latest gate excludes independent pre-solve torso and pelvis targets.
pub(crate) fn migrated_path() -> bool {
    STATE
        .lock()
        .migrated_at
        .is_some_and(|at| at.elapsed() <= MAX_AGE)
}

/// The fitted stance heading in the cockpit frame, for the existing orientation executor.
pub(crate) fn follow_yaw() -> Option<f32> {
    let state = STATE.lock();
    let (snapshot, _, view, ..) = state.fresh(Instant::now())?;
    yaw_of(view.inverse() * (Quat::from_rotation_y(snapshot.stance.yaw) * Vec3::NEG_Z))
}

pub(crate) fn invalidate() {
    *TRACKING.lock() = None;
    STATE.lock().withdraw(Fallback::TrackingUnavailable);
}

pub(crate) fn status() -> FitStatus {
    let state = STATE.lock();
    let mut status = state.status;
    if state.accepted.is_some() && state.fresh(Instant::now()).is_none() {
        status.fallback = Some(Fallback::Stale);
    }
    status
}

/// Consume each committed request at most once, through the existing movement ledger.
pub(crate) fn root_velocity(now: Instant, dt: f32, current_root: Mat4) -> Vec2 {
    STATE.lock().take_root(now, dt, current_root)
}

/// The same committed chest heading and landmarks used by the live procedural pose.
pub(crate) fn torso() -> Option<TorsoEstimate> {
    let state = STATE.lock();
    let (snapshot, _, view, head_world, head_cockpit, scale) = state.fresh(Instant::now())?;
    let cockpit = |point| head_cockpit + view.inverse() * (point - head_world) / scale;
    let yaw =
        yaw_of(view.inverse() * (Quat::from_rotation_y(snapshot.body.chest_yaw) * Vec3::NEG_Z))?;
    Some(TorsoEstimate {
        yaw,
        head_yaw: wrap(yaw + snapshot.body.neck_twist),
        neck_twist: snapshot.body.neck_twist,
        hands_yaw: None,
        hands_weight: 0.0,
        neck: cockpit(snapshot.body.neck),
        chest: cockpit(snapshot.body.chest),
        pelvis: cockpit(snapshot.body.pelvis),
        shoulders: snapshot.player_shoulders.map(cockpit),
        hips: snapshot.body.hips.map(cockpit),
    })
}

const MAX_AGE: Duration = Duration::from_millis(250);
const MAX_GAP: Duration = Duration::from_millis(500);

#[derive(Debug, Copy, Clone)]
struct Tracking {
    head: TrackedPose,
    grips: [Option<TrackedPose>; 2],
    sequence: u64,
    display_time_ns: i64,
    published_at: Instant,
    focused: bool,
}

type Accepted = (Snapshot, Instant, Quat, Vec3, Vec3, f32);

#[derive(Default)]
struct Runtime {
    estimator: Estimator,
    anatomy: Option<Anatomy>,
    previous: Option<CompositionSide>,
    accepted: Option<Accepted>,
    last_gate: Option<Instant>,
    migrated_at: Option<Instant>,
    consumed: u64,
    status: FitStatus,
}

static TRACKING: Mutex<Option<Tracking>> = Mutex::new(None);
static STATE: Mutex<Runtime> = Mutex::new(Runtime {
    estimator: Estimator::empty(),
    anatomy: None,
    previous: None,
    accepted: None,
    last_gate: None,
    migrated_at: None,
    consumed: 0,
    status: FitStatus {
        step: 0,
        sample: None,
        quality: None,
        fallback: Some(Fallback::NotUpdated),
        evaluations: 0,
    },
});

impl Runtime {
    fn withdraw(&mut self, reason: Fallback) {
        self.accepted = None;
        self.previous = None;
        self.status.fallback = Some(reason);
    }

    fn commit(&mut self, tracking: Option<Tracking>, input: GateInput<'_>) -> Option<Snapshot> {
        self.migrated_at = Some(input.now);
        self.status.step = self.status.step.wrapping_add(1);
        self.status.sample = None;
        self.status.quality = None;
        self.status.evaluations = 0;
        let Some(tracking) = tracking else {
            self.withdraw(Fallback::TrackingUnavailable);
            return None;
        };
        self.status.sample = Some(tracking.sequence);
        let result = self.fit(tracking, &input);
        self.status.evaluations = self.estimator.last_work().evaluations;
        self.last_gate = Some(input.now);
        match result {
            Ok(snapshot) => {
                self.status.quality = Some(snapshot.quality);
                if snapshot.quality.feasible {
                    self.status.fallback = None;
                    self.accepted = Some((
                        snapshot,
                        input.now,
                        input.view,
                        input.head_world,
                        tracking.head.pose.position,
                        input.world_scale,
                    ));
                    Some(snapshot)
                } else {
                    self.accepted = None;
                    self.status.fallback = Some(Fallback::Residual);
                    None
                }
            }
            Err(error) => {
                self.withdraw(Fallback::Rejected(error));
                None
            }
        }
    }

    fn fit(
        &mut self,
        tracking: Tracking,
        input: &GateInput<'_>,
    ) -> Result<Snapshot, EstimateError> {
        let calibration = 1;
        let anatomy = Anatomy::from_rig(input.player, input.rig, calibration)?;
        let epochs = Epochs {
            origin: input.origin,
            calibration,
            world_scale: u64::from(input.world_scale.to_bits()),
            authority: 1,
        };
        let teleported = self
            .previous
            .is_some_and(|last| last.stamp.epochs.origin != epochs.origin);
        let reseed = self.previous.is_none()
            || self.anatomy != Some(anatomy)
            || self
                .previous
                .is_some_and(|last| last.stamp.epochs != epochs)
            || self
                .last_gate
                .is_none_or(|last| input.now.saturating_duration_since(last) > MAX_GAP);
        // Frame tracking is relative to the recentered standing head, whereas the fit's player
        // proportions use floor-relative points. This translation leaves composed world targets intact.
        let floor_offset = Vec3::Y * input.player.eye_height;
        let shift_pose = |mut tracked: TrackedPose| {
            tracked.pose.position += floor_offset;
            tracked
        };
        let head = shift_pose(tracking.head);
        let grips = tracking.grips.map(|grip| grip.map(shift_pose));
        let stamp = SampleStamp {
            sequence: tracking.sequence,
            display_time_ns: tracking.display_time_ns,
            published_at: tracking.published_at,
            epochs,
        };
        let current = CompositionSide {
            stamp: StepStamp {
                step: self.status.step,
                epochs,
            },
            root: input.root,
            anchor_world: input.head_world - input.view * (head.pose.position * input.world_scale),
            basis: CockpitBasis {
                body: input.view,
                world_scale: input.world_scale,
            },
            shift_cockpit: Vec3::ZERO,
            owner: RootOwner::Player,
            native_rig: None,
        };
        let previous = if reseed {
            current
        } else {
            CompositionSide {
                root: input.previous_root,
                ..self.previous.expect("a continuing fit has a side")
            }
        };
        let observation = Observation::new(
            RawTracking {
                head: RawPose {
                    stamp,
                    tracked: head,
                },
                grips: grips.map(|grip| grip.map(|tracked| RawPose { stamp, tracked })),
                focused: tracking.focused,
            },
            CompositionPair::new(previous, current)?,
        )?;
        let snapshot = self.estimator.update(Update {
            observation: &observation,
            now: input.now,
            max_age: MAX_AGE,
            dt: input.dt,
            anatomy: &anatomy,
            support: None,
            transition: if teleported {
                Transition::Teleport
            } else if reseed {
                Transition::Reseed
            } else {
                Transition::Continue
            },
        })?;
        self.anatomy = Some(anatomy);
        self.previous = Some(current);
        Ok(snapshot)
    }

    fn fresh(&self, now: Instant) -> Option<Accepted> {
        self.accepted
            .filter(|(_, at, ..)| now >= *at && now.duration_since(*at) <= MAX_AGE)
    }

    fn take_root(&mut self, now: Instant, dt: f32, root: Mat4) -> Vec2 {
        let Some((snapshot, ..)) = self.fresh(now) else {
            return Vec2::ZERO;
        };
        if self.consumed == snapshot.provenance.current.step
            || !snapshot.root.permitted
            || !dt.is_finite()
            || dt <= 0.0
            || !root.is_finite()
        {
            return Vec2::ZERO;
        }
        self.consumed = snapshot.provenance.current.step;
        let source_root = self.previous.expect("an accepted fit has its root").root;
        let completed = root.w_axis.truncate() - source_root.w_axis.truncate();
        let gap = snapshot.root.displacement - completed;
        (Vec2::new(gap.x, gap.z) / dt.max(0.08)).clamp_length_max(1.5)
    }
}

#[cfg(test)]
mod tests;
