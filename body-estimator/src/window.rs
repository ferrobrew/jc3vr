//! A live window of the network's input, built frame by frame from tracking that arrives at
//! whatever rate the frames render, as the in-game estimator needs it.
//!
//! The offline path (`tracking-synth estimate-body`) has the whole recording and resamples it at
//! once; [`WindowBuilder`] does the same incrementally, keeping the last [`WINDOW_POSES`] poses:
//!
//! - The poses go into one fixed room frame. The caller's frame (the one its tracking is in) may
//!   turn, as a recenter turns it; [`WindowBuilder::turn`] folds each turn into the map from the
//!   caller's frame to the room frame, so the window never sees the turn as a turn of the body.
//! - A turn may also shift the caller's frame. The first frame after a turn takes the shift out by
//!   keeping the eyes' horizontal velocity continuous across it, as the offline path does.
//! - Each frame is resampled onto a grid at [`RATE_HZ`], positions linearly and orientations by
//!   slerp, the rate the network was trained at.
//! - A hand the caller says it cannot see is marked not visible in the frames it covers, so the
//!   network masks it; its pose holds the last one seen.
//!
//! A gap in the frames longer than [`MAX_GAP_S`] starts the window again: the poses before it no
//! longer describe the motion leading up to now.

use std::collections::VecDeque;

use glam::{DQuat, DVec3};

use crate::{
    features::{HandVisibility, SparseFrame, SparseWindow, WINDOW_FRAMES, WINDOW_POSES},
    tracking::{GripOffset, TrackedFrame},
};

/// The rate the window is resampled to, frames per second.
pub const RATE_HZ: f64 = 60.0;

/// A gap between frames longer than this starts the window again, seconds.
pub const MAX_GAP_S: f64 = 0.25;

/// One frame of tracking in the caller's frame, with what it could not see.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct LiveFrame {
    /// Seconds on the caller's clock.
    pub time_s: f64,
    /// The midpoint of the eyes, metres over the floor.
    pub eyes: DVec3,
    /// The head's orientation.
    pub head: DQuat,
    /// Each hand's OpenXR grip pose, left then right, when it is tracked.
    pub grips: [Option<(DVec3, DQuat)>; 2],
}

/// The live window.
#[derive(Debug, Clone)]
pub struct WindowBuilder {
    offset: GripOffset,
    /// The room frame from the caller's frame.
    room_from_caller: DQuat,
    /// What the shift removal adds to every room point.
    shift: DVec3,
    /// Whether the next frame is the first after a turn.
    turned: bool,
    /// The last two frames' eyes in the room frame before the shift removal, the latest last.
    eyes_history: [Option<DVec3>; 2],
    /// The last frame in the room frame, and the grips it last saw.
    last: Option<Sample>,
    seen_grips: [Option<(DVec3, DQuat)>; 2],
    /// The next grid time to sample, seconds on the caller's clock.
    next_grid_s: f64,
    /// The resampled poses and their hands' visibility, the oldest first.
    poses: VecDeque<(SparseFrame, [bool; 2])>,
    /// The caller's time of the latest resampled pose.
    latest_s: Option<f64>,
}

impl WindowBuilder {
    /// An empty window, with the wrist placed on the grip by `offset`.
    pub fn new(offset: GripOffset) -> Self {
        Self {
            offset,
            room_from_caller: DQuat::IDENTITY,
            shift: DVec3::ZERO,
            turned: false,
            eyes_history: [None, None],
            last: None,
            seen_grips: [None, None],
            next_grid_s: 0.0,
            poses: VecDeque::with_capacity(WINDOW_POSES),
            latest_s: None,
        }
    }

    /// Forget every pose, keeping the room frame: the next frame starts the window again.
    pub fn restart(&mut self) {
        self.eyes_history = [None, None];
        self.last = None;
        self.poses.clear();
        self.latest_s = None;
        self.turned = false;
    }

    /// The caller's frame turned by `turn` (new frame from old), as a recenter turns it.
    pub fn turn(&mut self, turn: DQuat) {
        if !turn.is_finite() {
            return;
        }
        self.room_from_caller = (self.room_from_caller * turn.inverse()).normalize();
        self.turned = true;
    }

    /// The room frame from the caller's frame: a heading in the room frame less this rotation's
    /// yaw is the heading in the caller's frame.
    pub fn room_from_caller(&self) -> DQuat {
        self.room_from_caller
    }

    /// Fold in one frame. A frame that is not later than the last one, or not finite, is ignored.
    pub fn push(&mut self, frame: &LiveFrame) {
        if !frame.time_s.is_finite() || !frame.eyes.is_finite() || !frame.head.is_finite() {
            return;
        }
        if let Some(last) = self.last {
            if frame.time_s <= last.time_s {
                return;
            }
            if frame.time_s - last.time_s > MAX_GAP_S {
                self.restart();
            }
        }
        let sample = self.place_in_room(frame);
        let Some(last) = self.last else {
            self.next_grid_s = frame.time_s;
            self.last = Some(sample);
            self.emit(&sample, &sample, 0.0, frame.time_s);
            self.next_grid_s = frame.time_s + 1.0 / RATE_HZ;
            return;
        };
        while self.next_grid_s <= frame.time_s {
            let span = sample.time_s - last.time_s;
            let alpha = ((self.next_grid_s - last.time_s) / span).clamp(0.0, 1.0);
            let at = self.next_grid_s;
            self.emit(&last, &sample, alpha, at);
            self.next_grid_s += 1.0 / RATE_HZ;
        }
        self.last = Some(sample);
    }

    /// The window of the latest [`WINDOW_POSES`] poses, once that many are held.
    pub fn window(&self) -> Option<SparseWindow> {
        if self.poses.len() < WINDOW_POSES {
            return None;
        }
        let poses: Vec<SparseFrame> = self.poses.iter().map(|(pose, _)| *pose).collect();
        let poses: &[SparseFrame; WINDOW_POSES] = poses.as_slice().try_into().ok()?;
        let mut visibility = HandVisibility::ALWAYS;
        // Frame `i` of the window is of pose `i + 1`.
        for (i, (_, seen)) in self.poses.iter().skip(1).enumerate().take(WINDOW_FRAMES) {
            visibility.left[i] = seen[0];
            visibility.right[i] = seen[1];
        }
        Some(SparseWindow::from_poses(poses).with_visibility(visibility))
    }

    /// The caller's time of the window's last pose, once there is one.
    pub fn latest_s(&self) -> Option<f64> {
        self.latest_s
    }

    /// How many poses are held, up to [`WINDOW_POSES`].
    pub fn len(&self) -> usize {
        self.poses.len()
    }

    /// Whether no pose is held.
    pub fn is_empty(&self) -> bool {
        self.poses.is_empty()
    }

    /// `frame` in the room frame, with the shift after a turn taken out.
    fn place_in_room(&mut self, frame: &LiveFrame) -> Sample {
        let turn = self.room_from_caller;
        let place = |p: DVec3| {
            let turned = turn * p;
            DVec3::new(turned.x, p.y, turned.z)
        };
        let eyes = place(frame.eyes);
        if std::mem::take(&mut self.turned)
            && let [Some(before), Some(last)] = self.eyes_history
        {
            let step = (eyes - last) - (last - before);
            self.shift -= DVec3::new(step.x, 0.0, step.z);
        }
        self.eyes_history = [self.eyes_history[1], Some(eyes)];
        let mut visible = [false; 2];
        for (side, grip) in frame.grips.iter().enumerate() {
            if let Some((position, orientation)) = grip
                && position.is_finite()
                && orientation.is_finite()
            {
                self.seen_grips[side] = Some((place(*position), turn * *orientation));
                visible[side] = true;
            }
        }
        let head = turn * frame.head;
        // A hand never seen stands at the head until it is: masked, it only feeds the rates of the
        // frame it first appears in.
        let grips = self.seen_grips.map(|grip| grip.unwrap_or((eyes, head)));
        Sample {
            time_s: frame.time_s,
            tracked: TrackedFrame {
                eyes: eyes + self.shift,
                head,
                grips: grips.map(|(position, orientation)| (position + self.shift, orientation)),
            },
            visible,
        }
    }

    /// Resample between `a` and `b` at `alpha` and keep the pose for time `at`.
    fn emit(&mut self, a: &Sample, b: &Sample, alpha: f64, at: f64) {
        let (a_t, b_t) = (&a.tracked, &b.tracked);
        let pose = |pa: (DVec3, DQuat), pb: (DVec3, DQuat)| {
            (pa.0.lerp(pb.0, alpha), pa.1.slerp(pb.1, alpha))
        };
        let (eyes, head) = pose((a_t.eyes, a_t.head), (b_t.eyes, b_t.head));
        let tracked = TrackedFrame {
            eyes,
            head,
            grips: [
                pose(a_t.grips[0], b_t.grips[0]),
                pose(a_t.grips[1], b_t.grips[1]),
            ],
        };
        let visible = [a.visible[0] && b.visible[0], a.visible[1] && b.visible[1]];
        if self.poses.len() == WINDOW_POSES {
            self.poses.pop_front();
        }
        self.poses
            .push_back((SparseFrame::from_tracking(&tracked, &self.offset), visible));
        self.latest_s = Some(at);
    }
}

/// One frame in the room frame.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Sample {
    time_s: f64,
    tracked: TrackedFrame,
    visible: [bool; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 90.0;

    fn frame(time_s: f64, x: f64, yaw: f64) -> LiveFrame {
        let head = DQuat::from_rotation_y(yaw);
        let eyes = DVec3::new(x, 1.7, 0.0);
        LiveFrame {
            time_s,
            eyes,
            head,
            grips: [
                Some((eyes + head * DVec3::new(-0.2, -0.5, -0.2), head)),
                Some((eyes + head * DVec3::new(0.2, -0.5, -0.2), head)),
            ],
        }
    }

    fn fill(builder: &mut WindowBuilder, from_s: f64, seconds: f64) -> f64 {
        let mut t = from_s;
        while t < from_s + seconds {
            builder.push(&frame(t, 0.0, 0.0));
            t += DT;
        }
        t
    }

    #[test]
    fn frames_at_any_rate_resample_to_sixty_hertz() {
        let mut builder = WindowBuilder::new(GripOffset::INDEX_BEST_GUESS);
        assert!(builder.window().is_none());
        fill(&mut builder, 10.0, 1.0);
        // A second at 90 Hz covers 60 or 61 grid points; a window needs 81, after 1.35 s.
        assert!((60..=61).contains(&builder.len()), "{}", builder.len());
        assert!(builder.window().is_none());
        fill(&mut builder, 11.0, 1.0);
        assert_eq!(builder.len(), WINDOW_POSES);
        let window = builder.window().unwrap();
        assert_eq!(window.visibility, HandVisibility::ALWAYS);
    }

    #[test]
    fn a_long_gap_starts_the_window_again() {
        let mut builder = WindowBuilder::new(GripOffset::INDEX_BEST_GUESS);
        let t = fill(&mut builder, 0.0, 2.0);
        assert_eq!(builder.len(), WINDOW_POSES);
        builder.push(&frame(t + MAX_GAP_S * 2.0, 0.0, 0.0));
        assert_eq!(builder.len(), 1);
    }

    #[test]
    fn a_lost_hand_is_masked_in_the_frames_it_covers() {
        let mut builder = WindowBuilder::new(GripOffset::INDEX_BEST_GUESS);
        let mut t = fill(&mut builder, 0.0, 2.0);
        for _ in 0..9 {
            let mut lost = frame(t, 0.0, 0.0);
            lost.grips[0] = None;
            builder.push(&lost);
            t += DT;
        }
        let window = builder.window().unwrap();
        assert!(!window.visibility.left[WINDOW_FRAMES - 1]);
        assert!(window.visibility.right[WINDOW_FRAMES - 1]);
        assert!(window.visibility.left[0]);
        // The hidden hand's features are the network's to mask; its pose held the last one seen.
        let held = window.features[WINDOW_FRAMES - 1];
        assert!(held.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn a_turn_of_the_callers_frame_leaves_the_window_unturned() {
        let mut turned = WindowBuilder::new(GripOffset::INDEX_BEST_GUESS);
        let mut still = WindowBuilder::new(GripOffset::INDEX_BEST_GUESS);
        let turn = DQuat::from_rotation_y(1.2);
        let mut t = 0.0;
        for i in 0..200 {
            // The player walks slowly along +X and turns; halfway, the caller's frame turns.
            let x = i as f64 * 0.002;
            let yaw = i as f64 * 0.004;
            let world = frame(t, x, yaw);
            still.push(&world);
            if i == 100 {
                turned.turn(turn);
            }
            let seen = if i >= 100 {
                LiveFrame {
                    eyes: turn * world.eyes + DVec3::new(0.3, 0.0, -0.4),
                    head: turn * world.head,
                    grips: world.grips.map(|grip| {
                        grip.map(|(p, q)| (turn * p + DVec3::new(0.3, 0.0, -0.4), turn * q))
                    }),
                    ..world
                }
            } else {
                world
            };
            turned.push(&seen);
            t += DT;
        }
        let (a, b) = (turned.window().unwrap(), still.window().unwrap());
        let worst = a
            .features
            .as_flattened()
            .iter()
            .zip(b.features.as_flattened())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        // The shift removal assumes the velocity across the turn is the one before it, which the
        // walk's constant speed makes exact up to the eyes' height, which never shifts.
        assert!(worst < 1e-4, "{worst}");
    }
}
