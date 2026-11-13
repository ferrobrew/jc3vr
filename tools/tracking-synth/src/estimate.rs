//! A learned body estimate over a recording: EgoPoser (`body-estimator`) run on the recording's raw
//! head and grip poses, one 80-frame window per frame at 60 Hz, so its estimate can be compared
//! with the body rig's and with what the game drew.
//!
//! The poses are brought into one fixed room frame first, as the network needs a world that does
//! not jump:
//!
//! - Each recenter's turn is undone, so the room frame is the cockpit frame before the first one.
//!   A recenter's horizontal shift is taken out by keeping the eyes' velocity continuous across it.
//! - Heights are over the STAGE floor: each frame's eye height over the floor is its STAGE head
//!   height, and the frame's other points move with it.
//!
//! Then the frames are resampled to 60 Hz (positions linearly, orientations by slerp), the rate the
//! network was trained at, and each window ends at one resampled frame. The windows are independent,
//! so they are spread over the machine's cores.

use std::{
    io::{self, Write},
    num::NonZeroUsize,
    thread,
    time::{Duration, Instant},
};

use body_estimator::{
    EgoPoser, EgoPoserOutput, GripOffset, SparseFrame, SparseWindow, TrackedFrame, WINDOW_POSES,
};
use glam::{DQuat, DVec3, Quat, Vec3};
use tracking_format::Frame;

/// The rate the windows are resampled to, frames per second.
pub const RATE_HZ: f64 = 60.0;

/// How long before the stretch the resampling starts, seconds without `--lead`: enough for the
/// first estimate's window and more.
pub const DEFAULT_LEAD_S: f64 = 10.0;

/// One estimate: the body at the end of the window ending at `time_s`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct EstimateRow {
    /// Seconds on the recording's clock.
    pub time_s: f64,
    pub output: EgoPoserOutput,
}

/// What an estimate run did.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct EstimateSummary {
    pub inferences: usize,
    /// The time spent in the network's forward passes alone.
    pub inference: Duration,
}

/// Run `model` over the recording's `frames` from `from_s` to `to_s` (seconds on the recording's
/// clock), with the resampling starting `lead_s` earlier.
pub fn estimate(
    frames: &[Frame],
    model: &EgoPoser,
    (from_s, to_s): (f64, f64),
    lead_s: f64,
) -> (Vec<EstimateRow>, EstimateSummary) {
    let room = room_frames(frames);
    let samples = resample(&room, from_s - lead_s, to_s);
    let poses: Vec<SparseFrame> = samples
        .iter()
        .map(|(_, frame)| SparseFrame::from_tracking(frame, &GripOffset::INDEX_BEST_GUESS))
        .collect();
    let ends: Vec<usize> = (WINDOW_POSES - 1..poses.len())
        .filter(|&end| samples[end].0 >= from_s)
        .collect();
    let workers = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let chunk = ends.len().div_ceil(workers).max(1);
    let parts: Vec<(Vec<EstimateRow>, Duration)> = thread::scope(|scope| {
        let handles: Vec<_> = ends
            .chunks(chunk)
            .map(|ends| {
                let (poses, samples) = (&poses, &samples);
                scope.spawn(move || {
                    let mut spent = Duration::ZERO;
                    let rows = ends
                        .iter()
                        .map(|&end| {
                            let window: &[SparseFrame; WINDOW_POSES] = poses
                                [end + 1 - WINDOW_POSES..=end]
                                .try_into()
                                .expect("the slice spans one window");
                            let window = SparseWindow::from_poses(window);
                            let started = Instant::now();
                            let output = model.infer(&window);
                            spent += started.elapsed();
                            EstimateRow {
                                time_s: samples[end].0,
                                output,
                            }
                        })
                        .collect();
                    (rows, spent)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("an estimate worker panicked"))
            .collect()
    });
    let mut rows = Vec::with_capacity(ends.len());
    let mut summary = EstimateSummary::default();
    for (part, spent) in parts {
        summary.inferences += part.len();
        summary.inference += spent;
        rows.extend(part);
    }
    (rows, summary)
}

/// Write `rows` as CSV: the time, the root orientation (6D), and the 21 joints' rotations (6D
/// each).
pub fn write_csv(rows: &[EstimateRow], out: &mut impl Write) -> io::Result<()> {
    write!(out, "time_s")?;
    for i in 0..6 {
        write!(out, ",root_orient_{i}")?;
    }
    for i in 0..126 {
        write!(out, ",pose_body_{i}")?;
    }
    writeln!(out)?;
    for row in rows {
        write!(out, "{:.6}", row.time_s)?;
        for value in row.output.root_orient.iter().chain(&row.output.pose_body) {
            write!(out, ",{value:e}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// One recorded frame's tracking in the room frame.
#[derive(Debug, Copy, Clone)]
struct RoomFrame {
    time_s: f64,
    tracked: TrackedFrame,
}

/// One frame's tracking as recorded, in the cockpit frame, with the floor's offset from it.
#[derive(Debug, Copy, Clone)]
struct CockpitFrame {
    eyes: DVec3,
    head: DQuat,
    /// What lifts a cockpit point to its height over the STAGE floor.
    floor: f64,
    grips: [(DVec3, DQuat); 2],
}

/// Every frame in the room frame. A frame without the head, the stage height, or the controllers
/// keeps the last frame's.
fn room_frames(frames: &[Frame]) -> Vec<RoomFrame> {
    let mut turn = DQuat::IDENTITY;
    let mut last: Option<CockpitFrame> = None;
    let mut room = Vec::with_capacity(frames.len());
    let mut recentered = Vec::new();
    for frame in frames {
        for recenter in &frame.recenters {
            // A recenter's turn takes the old cockpit frame to the new: the room frame is the
            // cockpit frame turned back by every one so far.
            turn *= dquat(recenter.turn).inverse();
        }
        if !frame.recenters.is_empty() {
            recentered.push(room.len());
        }
        let head = frame.head.as_ref().map(|head| {
            let [left, right] = head.eyes.map(|eye| eye.pose);
            let eyes = (dvec3(left.position) + dvec3(right.position)) * 0.5;
            let orientation = dquat(left.orientation).slerp(dquat(right.orientation), 0.5);
            (eyes, orientation)
        });
        let floor = match (frame.stage.head_height, head) {
            (Some(height), Some((eyes, _))) if height.is_finite() => {
                Some(f64::from(height) - eyes.y)
            }
            _ => None,
        };
        let grips = frame.controllers.as_ref().map(|controllers| {
            controllers.hands.each_ref().map(|hand| {
                (
                    dvec3(hand.grip.pose.position),
                    dquat(hand.grip.pose.orientation),
                )
            })
        });
        let previous = last;
        let (Some((eyes, orientation)), Some(floor), Some(grips)) = (
            head.or(previous.map(|p| (p.eyes, p.head))),
            floor.or(previous.map(|p| p.floor)),
            grips.or(previous.map(|p| p.grips)),
        ) else {
            continue;
        };
        last = Some(CockpitFrame {
            eyes,
            head: orientation,
            floor,
            grips,
        });
        let place = |p: DVec3| {
            let turned = turn * p;
            DVec3::new(turned.x, p.y + floor, turned.z)
        };
        room.push(RoomFrame {
            time_s: frame.time_ns as f64 / 1.0e9,
            tracked: TrackedFrame {
                eyes: place(eyes),
                head: turn * orientation,
                grips: grips.map(|(position, orientation)| (place(position), turn * orientation)),
            },
        });
    }
    remove_recenter_shifts(&mut room, &recentered);
    room
}

/// Take out each recenter's horizontal shift: at a recentered frame, the eyes' step beyond the
/// step before it is the shift, and it is subtracted from that frame on.
fn remove_recenter_shifts(room: &mut [RoomFrame], recentered: &[usize]) {
    let mut offset = DVec3::ZERO;
    let mut next = recentered.iter().copied().filter(|&i| i >= 2).peekable();
    let original: Vec<DVec3> = room.iter().map(|frame| frame.tracked.eyes).collect();
    for (i, frame) in room.iter_mut().enumerate() {
        if next.peek() == Some(&i) {
            next.next();
            let step = (original[i] - original[i - 1]) - (original[i - 1] - original[i - 2]);
            offset -= DVec3::new(step.x, 0.0, step.z);
        }
        frame.tracked.eyes += offset;
        for grip in &mut frame.tracked.grips {
            grip.0 += offset;
        }
    }
}

/// The room frames resampled at `RATE_HZ` from `start_s` up to `end_s`: positions interpolated
/// linearly and orientations by slerp between the frames either side, held at the ends.
fn resample(room: &[RoomFrame], start_s: f64, end_s: f64) -> Vec<(f64, TrackedFrame)> {
    let mut samples = Vec::new();
    if room.is_empty() {
        return samples;
    }
    let mut j = 0;
    for k in 0.. {
        let t = start_s + k as f64 / RATE_HZ;
        if t >= end_s {
            break;
        }
        while j + 1 < room.len() && room[j + 1].time_s <= t {
            j += 1;
        }
        let (a, b) = (&room[j], &room[(j + 1).min(room.len() - 1)]);
        let span = b.time_s - a.time_s;
        let alpha = if span > 0.0 {
            ((t - a.time_s) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (&a.tracked, &b.tracked);
        let pose = |pa: (DVec3, DQuat), pb: (DVec3, DQuat)| {
            (pa.0.lerp(pb.0, alpha), pa.1.slerp(pb.1, alpha))
        };
        let (eyes, head) = pose((a.eyes, a.head), (b.eyes, b.head));
        samples.push((
            t,
            TrackedFrame {
                eyes,
                head,
                grips: [pose(a.grips[0], b.grips[0]), pose(a.grips[1], b.grips[1])],
            },
        ));
    }
    samples
}

fn dvec3(v: Vec3) -> DVec3 {
    v.as_dvec3()
}

fn dquat(q: Quat) -> DQuat {
    q.as_dquat().normalize()
}
