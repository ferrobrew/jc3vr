//! Simulation-time body fitting from copied tracking and the solve gate's reference rig.
//!
//! A commit supplies the pose writer and the next locomotion task from one result. A fit that
//! misses its feasibility tolerance leaves the last accepted fit standing through [`hold`], which
//! fades it out only when rejection persists. Unavailable tracking and failed updates withdraw
//! inferred authority without changing the view or hand input.

mod hold;

use std::time::{Duration, Instant};

use glam::{Mat4, Quat, Vec2, Vec3};
use parking_lot::Mutex;

pub(crate) use crate::controllers::body::rig::runtime::hold::Phase;
#[cfg(test)]
use crate::controllers::{body::rig::estimate::MissingTerrain, frame::Pose};
use crate::controllers::{
    body::{
        BodyDimensions,
        procedural::rig::Rig,
        rig::{
            estimate::{
                Anatomy, EstimateError, Estimator, Quality, Snapshot, Support, SupportSample,
                Terrain, Transition, Update,
            },
            observation::{
                CompositionPair, CompositionSide, Epochs, Observation, RawPose, RawTracking,
                SampleStamp, StepStamp,
            },
            root::{owner::RootOwner, shift::CockpitBasis},
            runtime::hold::{Hold, Presentation},
            torso::{TorsoEstimate, wrap, yaw_of},
        },
    },
    frame::TrackedPose,
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
    /// Which fit the latest gate presented, if any.
    pub presented: Option<Phase>,
    /// The presented fit's share of the drawn pose.
    pub weight: f32,
}

/// The fit one gate presents to the pose writer.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct GateFit {
    pub snapshot: Snapshot,
    /// The character root the fit was made against. A held fit is realized against it, so it moves
    /// rigidly with the character while it stands in for rejected fits.
    pub root: Mat4,
    /// The fit's share of the drawn pose, `0..=1`; the animation has the rest.
    pub weight: f32,
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
    pub support: Option<SupportSample>,
    pub now: Instant,
    pub dt: f32,
}

/// Retain the frame's copied head and controller sample for the next simulation correction.
pub(crate) fn frame_tick(
    packet: Option<crate::controllers::body::rig::live_observation::RawPacket>,
) {
    *TRACKING.lock() = packet.map(Tracking::from);
}

impl From<crate::controllers::body::rig::live_observation::RawPacket> for Tracking {
    fn from(packet: crate::controllers::body::rig::live_observation::RawPacket) -> Self {
        Self {
            head: packet.head,
            grips: packet.grips,
            sequence: packet.sequence,
            display_time_ns: packet.display_time_ns,
            published_at: packet.published_at,
            focused: packet.focused,
        }
    }
}

/// Commit once at the local solve gate and publish exactly that result for the movement owner.
pub(crate) fn at_gate(input: GateInput<'_>, terrain: &mut dyn Terrain) -> Option<GateFit> {
    let tracking = *TRACKING.lock();
    STATE.lock().commit_terrain(tracking, input, terrain)
}

/// Withdraw the previous request when the animation or an anchored solve owns this update.
pub(crate) fn hand_back() {
    let mut state = STATE.lock();
    state.migrated_at = None;
    state.withdraw(Fallback::Owner);
}

/// Keep a rejected on-foot update on the migrated path while withdrawing its inferred output. A
/// fit that only missed its tolerance keeps the estimator's stream, so the next gate continues it
/// rather than reseeding from a neutral pose with the feet at rest.
pub(crate) fn gate_failed() {
    STATE.lock().gate_failed();
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
    let accepted = state.fresh(Instant::now())?.fit;
    yaw_of(
        accepted.view.inverse()
            * (Quat::from_rotation_y(accepted.snapshot.stance.yaw) * Vec3::NEG_Z),
    )
}

pub(crate) fn invalidate() {
    *TRACKING.lock() = None;
    STATE.lock().withdraw(Fallback::TrackingUnavailable);
}

pub(crate) fn status() -> FitStatus {
    let state = STATE.lock();
    let mut status = state.status;
    if state.hold.presented().is_some() && state.fresh(Instant::now()).is_none() {
        status.fallback = Some(Fallback::Stale);
    }
    status
}

/// Consume each committed request at most once, through the existing movement ledger.
pub(crate) fn root_velocity(now: Instant, dt: f32, current_root: Mat4, enabled: bool) -> Vec2 {
    STATE.lock().take_root(now, dt, current_root, enabled)
}

/// The same committed chest heading and landmarks used by the live procedural pose. A held fit
/// supplies them through the grace; once it fades out, the hands fall back to their own shoulders.
pub(crate) fn torso() -> Option<TorsoEstimate> {
    STATE.lock().torso(Instant::now())
}

const MAX_AGE: Duration = Duration::from_millis(250);
const MAX_GAP: Duration = Duration::from_millis(500);

/// The fit's torso, carried into the cockpit frame of the head it was fit to.
fn torso_of(accepted: &Accepted) -> Option<TorsoEstimate> {
    let Accepted {
        snapshot,
        view,
        head_world,
        head_cockpit,
        scale,
        ..
    } = *accepted;
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

#[derive(Debug, Copy, Clone)]
struct Tracking {
    head: TrackedPose,
    grips: [Option<TrackedPose>; 2],
    sequence: u64,
    display_time_ns: i64,
    published_at: Instant,
    focused: bool,
}

/// A fit that met its tolerance, with the frames it was made in.
#[derive(Debug, Copy, Clone)]
struct Accepted {
    snapshot: Snapshot,
    root: Mat4,
    view: Quat,
    head_world: Vec3,
    head_cockpit: Vec3,
    scale: f32,
}

impl Presentation<Accepted> {
    fn gate_fit(self) -> GateFit {
        GateFit {
            snapshot: self.fit.snapshot,
            root: self.fit.root,
            weight: self.weight,
        }
    }
}

#[derive(Default)]
struct Runtime {
    estimator: Estimator,
    anatomy: Option<Anatomy>,
    previous: Option<CompositionSide>,
    support: Option<SupportSample>,
    hold: Hold<Accepted>,
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
    support: None,
    hold: Hold::new(),
    last_gate: None,
    migrated_at: None,
    consumed: 0,
    status: FitStatus {
        step: 0,
        sample: None,
        quality: None,
        fallback: Some(Fallback::NotUpdated),
        evaluations: 0,
        presented: None,
        weight: 0.0,
    },
});

impl Runtime {
    fn withdraw(&mut self, reason: Fallback) {
        self.hold.forget();
        self.previous = None;
        self.support = None;
        self.status.fallback = Some(reason);
    }

    fn gate_failed(&mut self) {
        match self.status.fallback {
            Some(Fallback::Residual) => self.hold.forget(),
            reason => self.withdraw(reason.unwrap_or(Fallback::Owner)),
        }
    }

    #[cfg(test)]
    fn commit(&mut self, tracking: Option<Tracking>, input: GateInput<'_>) -> Option<GateFit> {
        self.commit_terrain(tracking, input, &mut MissingTerrain)
    }

    fn commit_terrain(
        &mut self,
        tracking: Option<Tracking>,
        input: GateInput<'_>,
        terrain: &mut dyn Terrain,
    ) -> Option<GateFit> {
        self.migrated_at = Some(input.now);
        self.status.step = self.status.step.wrapping_add(1);
        self.status.sample = None;
        self.status.quality = None;
        self.status.evaluations = 0;
        self.status.presented = None;
        self.status.weight = 0.0;
        let Some(tracking) = tracking else {
            self.withdraw(Fallback::TrackingUnavailable);
            return None;
        };
        self.status.sample = Some(tracking.sequence);
        let result = self.fit(tracking, &input, terrain);
        self.status.evaluations = self.estimator.last_work().evaluations;
        self.last_gate = Some(input.now);
        let presented = match result {
            Ok(snapshot) => {
                self.status.quality = Some(snapshot.quality);
                if snapshot.quality.feasible {
                    self.status.fallback = None;
                    let accepted = Accepted {
                        snapshot,
                        root: input.root,
                        view: input.view,
                        head_world: input.head_world,
                        head_cockpit: tracking.head.pose.position,
                        scale: input.world_scale,
                    };
                    Some(self.hold.accept(accepted, input.dt))
                } else {
                    // The estimator keeps the rejected fit as its warm start, which stays within
                    // the temporal envelope of the accepted one and keeps catching up with a
                    // fast duck; only the presentation holds the accepted fit.
                    self.status.fallback = Some(Fallback::Residual);
                    self.hold.reject(input.dt)
                }
            }
            Err(error) => {
                self.withdraw(Fallback::Rejected(error));
                None
            }
        };
        self.status.presented = presented.map(|presented| presented.phase);
        self.status.weight = presented.map_or(0.0, |presented| presented.weight);
        presented.map(Presentation::gate_fit)
    }

    fn fit(
        &mut self,
        tracking: Tracking,
        input: &GateInput<'_>,
        terrain: &mut dyn Terrain,
    ) -> Result<Snapshot, EstimateError> {
        if !input.previous_root.is_finite() {
            return Err(EstimateError::InvalidRoot);
        }
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
            self.previous.expect("a continuing fit has a side")
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
        let sample = input.support.filter(|sample| sample.at == input.now);
        let support = sample
            .filter(|sample| {
                crate::controllers::body::rig::estimate::support_transform_valid(sample.transform)
            })
            .map(|sample| {
                let last = self.support.filter(|last| {
                    !reseed
                        && last.identity == sample.identity
                        && last.generation == sample.generation
                        && last.at < sample.at
                });
                Support {
                    identity: sample.identity,
                    generation: sample.generation,
                    previous_step: previous.stamp,
                    current_step: current.stamp,
                    previous: last.map_or(sample.transform, |last| last.transform),
                    current: sample.transform,
                    half_extent: Vec2::splat(f32::MAX),
                }
            });
        let snapshot = self.estimator.update_terrain(
            Update {
                observation: &observation,
                now: input.now,
                max_age: MAX_AGE,
                dt: input.dt,
                anatomy: &anatomy,
                support,
                transition: if teleported {
                    Transition::Teleport
                } else if reseed {
                    Transition::Reseed
                } else {
                    Transition::Continue
                },
            },
            terrain,
        )?;
        self.anatomy = Some(anatomy);
        self.previous = Some(current);
        self.support = support.and(sample);
        Ok(snapshot)
    }

    /// The presented fit while the gate that presented it is recent.
    fn fresh(&self, now: Instant) -> Option<Presentation<Accepted>> {
        let at = self.last_gate?;
        if now < at || now.duration_since(at) > MAX_AGE {
            return None;
        }
        self.hold.presented()
    }

    fn torso(&self, now: Instant) -> Option<TorsoEstimate> {
        let presented = self.fresh(now)?;
        if presented.phase == Phase::FadingOut {
            return None;
        }
        torso_of(&presented.fit)
    }

    /// The chase toward the presented stance, once per gate. A held fit keeps the root following
    /// its stance, scaled by its fading weight.
    fn take_root(&mut self, now: Instant, dt: f32, root: Mat4, enabled: bool) -> Vec2 {
        let Some(presented) = self.fresh(now) else {
            return Vec2::ZERO;
        };
        let Accepted {
            snapshot,
            root: source_root,
            ..
        } = presented.fit;
        let step = self.status.step;
        if self.consumed == step {
            return Vec2::ZERO;
        }
        if !enabled {
            self.consumed = step;
            return Vec2::ZERO;
        }
        if !snapshot.root.permitted || !dt.is_finite() || dt <= 0.0 || !root.is_finite() {
            return Vec2::ZERO;
        }
        self.consumed = step;
        let completed = root.w_axis.truncate() - source_root.w_axis.truncate();
        let gap = snapshot.root.displacement - completed;
        (Vec2::new(gap.x, gap.z) / dt.max(0.08)).clamp_length_max(1.5) * presented.weight
    }
}

#[cfg(test)]
mod tests;
