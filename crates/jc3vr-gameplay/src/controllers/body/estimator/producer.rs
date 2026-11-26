//! The estimator's game-thread half: the live window, fed once per rendered frame from the pose
//! composition ([`crate::controllers::roomscale::compose_cockpit`]), and the windows posted for the
//! render thread at the submit rate.
//!
//! The poses are the rig's room frame: the cockpit frame with its floor at zero, in the player's
//! metres, the eyes and the grips lifted together, and seated, the eyes at the standing height plus
//! the head's travel from the seated reference, as the rig reads them. A recenter turns and shifts
//! the cockpit frame; [`on_recenter`] hands the turn to the window, which keeps its own room frame
//! fixed and takes the shift out (`body_estimator::window`). A change of posture moves the eyes'
//! height by the remap's difference, so it starts the window again rather than feed the network a
//! jump.

use std::time::{Duration, Instant};

use body_estimator::{GripOffset, LiveFrame, SparseWindow, WindowBuilder};
use glam::{DQuat, Quat, Vec3};
use parking_lot::Mutex;

use crate::{
    controllers::{
        body::estimator::withdraw, config::CONTROLLERS_CONFIG,
        roomscale::posture::EffectivePosture, scheme,
    },
    vr,
};

/// One rendered frame's tracking, in the rig's room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Observation {
    pub now: Instant,
    /// The eyes, metres over the floor.
    pub eyes: Vec3,
    /// The head's cockpit orientation.
    pub head: Quat,
    /// Each tracked grip's pose, left then right.
    pub grips: [Option<(Vec3, Quat)>; 2],
    /// The posture whose head-height remap `eyes` is in.
    pub posture: EffectivePosture,
}

/// Fold one frame into the window, and post it for the GPU when the submit rate allows. Runs on
/// the game thread. With the estimator off, under the gamepad scheme, or without head tracking,
/// the window starts again and nothing is posted; off or under the gamepad scheme the published
/// estimate is withdrawn too.
pub fn observe(observation: &Observation) {
    let config = CONTROLLERS_CONFIG.lock().body.estimator.clone();
    let controllers = scheme::is_controllers();
    let mut state = STATE.lock();
    let producer = state.get_or_insert_with(Producer::new);
    if !config.enabled || !controllers {
        producer.stop();
        drop(state);
        *SUBMISSION.lock() = None;
        withdraw();
        return;
    }
    if !vr::head_tracked() {
        producer.stop();
        return;
    }
    let submit = producer.observe(observation, config.submit_hz);
    drop(state);
    if let Some(submission) = submit {
        *SUBMISSION.lock() = Some(submission);
    }
}

/// The cockpit frame turned by `turn` (new frame from old), as a recenter or a follow bank turns
/// it.
pub fn on_recenter(turn: Quat) {
    if let Some(producer) = STATE.lock().as_mut() {
        producer.builder.turn(turn.as_dquat());
    }
}

/// The room frame the estimate is in, from the cockpit frame.
pub(crate) fn room_from_cockpit() -> DQuat {
    STATE
        .lock()
        .as_ref()
        .map_or(DQuat::IDENTITY, |p| p.builder.room_from_caller())
}

/// The latest eyes the window took, in the rig's room frame, while recent.
pub(crate) fn eyes(now: Instant) -> Option<Vec3> {
    STATE
        .lock()
        .as_ref()
        .and_then(|p| p.eyes)
        .filter(|(_, at)| now.saturating_duration_since(*at) <= EYES_FRESH_FOR)
        .map(|(eyes, _)| eyes)
}

/// How many poses the window holds, for the debug UI.
pub(crate) fn window_len() -> usize {
    STATE.lock().as_ref().map_or(0, |p| p.builder.len())
}

/// Take the window posted for the GPU, if any. Runs on the render thread.
pub(crate) fn take_submission() -> Option<Submission> {
    SUBMISSION.lock().take()
}

/// A window posted for the GPU.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Submission {
    pub window: Box<SparseWindow>,
    /// When the window's last pose was taken.
    pub window_end: Instant,
    pub posture: EffectivePosture,
}

/// The game thread's window and its clock.
#[derive(Debug)]
pub(crate) struct Producer {
    pub(crate) builder: WindowBuilder,
    /// The instant the window's clock counts seconds from.
    epoch: Option<Instant>,
    posture: Option<EffectivePosture>,
    /// When the next window is due.
    next_submit: Option<Instant>,
    eyes: Option<(Vec3, Instant)>,
}

impl Producer {
    pub(crate) fn new() -> Self {
        Self {
            builder: WindowBuilder::new(GripOffset::INDEX_BEST_GUESS),
            epoch: None,
            posture: None,
            next_submit: None,
            eyes: None,
        }
    }

    /// Forget the window, keeping the room frame.
    pub(crate) fn stop(&mut self) {
        self.builder.restart();
        self.posture = None;
    }

    /// Fold `observation` in and return the window to post, at most `submit_hz` times a second.
    pub(crate) fn observe(
        &mut self,
        observation: &Observation,
        submit_hz: f32,
    ) -> Option<Submission> {
        let now = observation.now;
        if self.posture.is_some_and(|p| p != observation.posture) {
            self.builder.restart();
        }
        self.posture = Some(observation.posture);
        let epoch = *self.epoch.get_or_insert(now);
        let time_s = now.saturating_duration_since(epoch).as_secs_f64();
        self.builder.push(&LiveFrame {
            time_s,
            eyes: observation.eyes.as_dvec3(),
            head: observation.head.as_dquat().normalize(),
            grips: observation
                .grips
                .map(|grip| grip.map(|(p, q)| (p.as_dvec3(), q.as_dquat().normalize()))),
        });
        self.eyes = Some((observation.eyes, now));
        let interval = Duration::from_secs_f32(1.0 / submit_hz.clamp(1.0, 90.0));
        // A frame a little early for the due time takes it, and the next is due an interval after
        // the due time rather than the frame, so the rate does not fall to the frames' next
        // divisor; a due time an interval behind starts the schedule again.
        if self.next_submit.is_some_and(|due| now + SUBMIT_EARLY < due) {
            return None;
        }
        let window = self.builder.window()?;
        let latest_s = self.builder.latest_s()?;
        // The window's last pose is on the 60 Hz grid at or before this frame.
        let behind = Duration::try_from_secs_f64((time_s - latest_s).max(0.0)).unwrap_or_default();
        self.next_submit = Some(match self.next_submit {
            Some(due) if now.saturating_duration_since(due) < interval => due + interval,
            _ => now + interval,
        });
        Some(Submission {
            window: Box::new(window),
            window_end: now.checked_sub(behind).unwrap_or(now),
            posture: observation.posture,
        })
    }
}

/// How early a frame may take a submission's due time: a frame's jitter.
const SUBMIT_EARLY: Duration = Duration::from_millis(2);

/// The eyes stand for the window's position this long after the last frame.
const EYES_FRESH_FOR: Duration = Duration::from_millis(250);

static STATE: Mutex<Option<Producer>> = Mutex::new(None);

/// The latest window posted for the render thread and not yet taken.
static SUBMISSION: Mutex<Option<Submission>> = Mutex::new(None);
