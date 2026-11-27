//! `scan-view`: a take's video scanned for flashes (a frame or two unlike the frames on both sides
//! of it, which are alike: the camera dipping into Rico's body, or any other one-frame glitch), each
//! flash cross-checked against the recording's camera and Rico's animated head. See
//! `docs/mod/testing/rerun.md` ("Scanning for flashes").
//!
//! The video is decoded by `ffmpeg` to small thumbnails, one frame at a time, so a whole take scans
//! in a few minutes without holding it in memory.

use std::{
    fmt,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use glam::Vec3;
use tracking_format::{
    Recording,
    event::{EventKind, PoseStage},
};

use crate::rig_eval::VideoClock;

/// What [`scan`] is asked to do.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanOptions {
    /// The take's video.
    pub video: PathBuf,
    /// The video's frame rate, frames per second.
    pub fps: f64,
    /// The flash score an event must pass, the mean absolute RGB difference of the thumbnails,
    /// `0..=255`.
    pub threshold: f32,
}

impl ScanOptions {
    /// The defaults for `video`: the capture's 60 fps and a threshold that passes the camera
    /// dipping into the body and few ordinary cuts.
    pub fn new(video: PathBuf) -> Self {
        Self {
            video,
            fps: 60.0,
            threshold: 25.0,
        }
    }
}

/// One flash, with the camera's place against Rico's head around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Flash {
    /// Seconds into the video.
    pub video_s: f64,
    /// Seconds on the recording's clock (Rerun's `recording` timeline), when the video maps to it.
    pub recording_s: Option<f64>,
    pub score: f32,
    /// How many frames the flash lasts, one or two.
    pub frames: u8,
    /// The camera's place against the head around the flash, when the recording holds both.
    pub camera: Option<CameraNear>,
}

/// The camera against Rico's animated head around a flash.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraNear {
    /// The camera's lead over the head along the head's motion, metres, the least within a tenth
    /// of a second: negative is behind the head, inside or behind the back. `None` while the head
    /// moves slower than [`MIN_SPEED_MPS`], where its motion gives no direction to measure along.
    pub lead_m: Option<f32>,
    /// The camera's greatest distance from the head within the same window, metres.
    pub distance_m: f32,
    /// The head's speed, metres per second.
    pub speed_mps: f32,
    /// The mode detector's result at the flash.
    pub mode: String,
}

/// The scan's result.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    pub frames: usize,
    pub flashes: Vec<Flash>,
    pub threshold: f32,
}

/// The lead below which a flash counts as the camera behind the head, metres.
pub const BEHIND_M: f32 = -0.12;

/// The head speed below which the lead is not measured, metres per second.
pub const MIN_SPEED_MPS: f32 = 1.0;

impl CameraNear {
    /// Whether the camera was more than [`BEHIND_M`] behind the head.
    pub fn behind(&self) -> bool {
        self.lead_m.is_some_and(|lead| lead < BEHIND_M)
    }
}

/// Why a scan failed.
#[derive(Debug)]
pub enum ScanError {
    /// `ffmpeg` could not be started.
    Spawn(io::Error),
    /// Reading `ffmpeg`'s output failed.
    Read(io::Error),
    /// `ffmpeg` failed.
    Ffmpeg(String),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::Spawn(error) => write!(f, "scan-view: could not run ffmpeg: {error}"),
            ScanError::Read(error) => write!(f, "scan-view: reading ffmpeg's frames: {error}"),
            ScanError::Ffmpeg(stderr) => write!(f, "scan-view: ffmpeg failed: {}", stderr.trim()),
        }
    }
}

impl std::error::Error for ScanError {}

/// Scan `options.video` for flashes and cross-check them against `recording`, placing the video
/// on the recording's clock through `clock` (the take's video sidecar) when there is one.
pub fn scan(
    recording: &Recording,
    clock: Option<&VideoClock>,
    options: &ScanOptions,
) -> Result<Scan, ScanError> {
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&options.video)
        .args([
            "-vf",
            &format!("scale={THUMB_W}:{THUMB_H}:flags=area"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ScanError::Spawn)?;
    let mut stdout = child.stdout.take().expect("the child's stdout is piped");
    let mut detector = FlashDetector::new();
    let mut scores = Vec::new();
    let mut thumb = vec![0u8; THUMB_BYTES];
    loop {
        match stdout.read_exact(&mut thumb) {
            Ok(()) => scores.extend(detector.push(&thumb)),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(ScanError::Read(error)),
        }
    }
    let output = child.wait_with_output().map_err(ScanError::Read)?;
    if !output.status.success() {
        return Err(ScanError::Ffmpeg(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let frames = detector.frames();
    let timeline = Timeline::new(recording, clock);
    let camera = CameraSeries::new(recording);
    let flashes = events(&scores, options.threshold, MERGE_FRAMES)
        .into_iter()
        .map(|score| {
            let video_s = score.frame as f64 / options.fps;
            let recording_s = timeline.recording_s(video_s);
            Flash {
                video_s,
                recording_s,
                score: score.score,
                frames: score.frames,
                camera: recording_s.and_then(|time| camera.near(time)),
            }
        })
        .collect();
    Ok(Scan {
        frames,
        flashes,
        threshold: options.threshold,
    })
}

/// Write `scan` as a Markdown report: the counts by mode and the flashes, each with its times and
/// the camera's place against the head.
pub fn write_report(scan: &Scan, source: &Path, out: &mut impl Write) -> io::Result<()> {
    writeln!(out, "# View scan of {}\n", source.display())?;
    let behind = scan
        .flashes
        .iter()
        .filter(|flash| flash.camera.as_ref().is_some_and(CameraNear::behind))
        .count();
    writeln!(
        out,
        "{} frames scanned; {} flashes scored over {}; {} with the camera more than {:.2} m behind \
         Rico's animated head along his motion.\n",
        scan.frames,
        scan.flashes.len(),
        scan.threshold,
        behind,
        -BEHIND_M
    )?;
    let mut modes: Vec<(String, usize, usize)> = Vec::new();
    for flash in &scan.flashes {
        let mode = flash
            .camera
            .as_ref()
            .map_or("unknown", |camera| camera.mode.as_str());
        let is_behind = flash.camera.as_ref().is_some_and(CameraNear::behind);
        match modes.iter_mut().find(|(name, _, _)| name == mode) {
            Some(entry) => {
                entry.1 += 1;
                entry.2 += usize::from(is_behind);
            }
            None => modes.push((mode.to_owned(), 1, usize::from(is_behind))),
        }
    }
    modes.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    writeln!(out, "| Mode | Flashes | Camera behind the head |")?;
    writeln!(out, "|---|---|---|")?;
    for (mode, count, behind) in &modes {
        writeln!(out, "| {mode} | {count} | {behind} |")?;
    }
    writeln!(
        out,
        "\n| Recording s | Video s | Score | Frames | Camera lead m | Distance m | Speed m/s | Mode |"
    )?;
    writeln!(out, "|---|---|---|---|---|---|---|---|")?;
    for flash in &scan.flashes {
        let recording = flash
            .recording_s
            .map_or_else(|| "-".to_owned(), |s| format!("{s:.2}"));
        let (lead, distance, speed, mode) = flash.camera.as_ref().map_or_else(
            || {
                (
                    "-".to_owned(),
                    "-".to_owned(),
                    "-".to_owned(),
                    "-".to_owned(),
                )
            },
            |c| {
                (
                    c.lead_m
                        .map_or_else(|| "-".to_owned(), |lead| format!("{lead:+.2}")),
                    format!("{:.2}", c.distance_m),
                    format!("{:.1}", c.speed_mps),
                    c.mode.clone(),
                )
            },
        );
        writeln!(
            out,
            "| {recording} | {:.2} | {:.1} | {} | {lead} | {distance} | {speed} | {mode} |",
            flash.video_s, flash.score, flash.frames
        )?;
    }
    Ok(())
}

const THUMB_W: usize = 96;
const THUMB_H: usize = 54;
const THUMB_BYTES: usize = THUMB_W * THUMB_H * 3;

/// Flashes closer together than this, frames, are one.
const MERGE_FRAMES: usize = 6;

/// One frame's flash score.
#[derive(Debug, Copy, Clone, PartialEq)]
struct FrameScore {
    frame: usize,
    score: f32,
    frames: u8,
}

/// The flash score of each frame, from the frames on either side: for a one-frame flash, how much
/// less alike the frame is to its neighbours than they are to each other; for a two-frame flash,
/// the same of the pair against the frames either side of it. The better of the two.
struct FlashDetector {
    /// The last four thumbnails, oldest first.
    window: Vec<Vec<u8>>,
    frames: usize,
}

impl FlashDetector {
    fn new() -> Self {
        Self {
            window: Vec::with_capacity(4),
            frames: 0,
        }
    }

    /// Take the next frame's thumbnail and score the frame two before it, once it has neighbours
    /// on both sides.
    fn push(&mut self, thumb: &[u8]) -> Option<FrameScore> {
        if self.window.len() == 4 {
            self.window.remove(0);
        }
        self.window.push(thumb.to_vec());
        self.frames += 1;
        let [before, frame, after, next] =
            <&[Vec<u8>; 4]>::try_from(self.window.as_slice()).ok()?;
        let one =
            difference(frame, before).min(difference(frame, after)) - difference(before, after);
        let two = difference(frame, before).min(difference(after, next)) - difference(before, next);
        Some(FrameScore {
            frame: self.frames - 3,
            score: one.max(two),
            frames: if one >= two { 1 } else { 2 },
        })
    }

    fn frames(&self) -> usize {
        self.frames
    }
}

/// The mean absolute difference of two thumbnails.
fn difference(a: &[u8], b: &[u8]) -> f32 {
    let total: u64 = a
        .iter()
        .zip(b)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    total as f32 / a.len().max(1) as f32
}

/// The frames scoring over `threshold`, the best of each run within `merge` frames of another.
fn events(scores: &[FrameScore], threshold: f32, merge: usize) -> Vec<FrameScore> {
    let mut out: Vec<FrameScore> = Vec::new();
    for score in scores.iter().filter(|score| score.score > threshold) {
        match out.last_mut() {
            Some(last) if score.frame - last.frame < merge => {
                if score.score > last.score {
                    *last = *score;
                }
            }
            _ => out.push(*score),
        }
    }
    out
}

/// The map from the video's clock to the recording's: through the sidecar's stamp for each frame,
/// or, without a sidecar, the two clocks taken as one.
struct Timeline {
    /// `(video_s, recording_s)`, in video order.
    points: Vec<(f64, f64)>,
    identity: bool,
}

impl Timeline {
    fn new(recording: &Recording, clock: Option<&VideoClock>) -> Self {
        let mut points: Vec<(f64, f64)> = clock
            .map(|clock| {
                recording
                    .frames
                    .iter()
                    .filter_map(|frame| {
                        Some((
                            clock.video_s(frame.display_time_ns)?,
                            frame.time_ns as f64 / 1.0e9,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let identity = points.is_empty();
        Self { points, identity }
    }

    fn recording_s(&self, video_s: f64) -> Option<f64> {
        if self.identity {
            return Some(video_s);
        }
        let index = self.points.partition_point(|p| p.0 < video_s);
        if index == 0 || index == self.points.len() {
            return None;
        }
        let ((v0, r0), (v1, r1)) = (self.points[index - 1], self.points[index]);
        if v1 <= v0 {
            return Some(r1);
        }
        Some(r0 + (r1 - r0) * (video_s - v0) / (v1 - v0))
    }
}

/// The camera and Rico's animated head, as the recording holds them. The camera is its current tick
/// pose (`CameraState::current`), placed from the same tick as the latest pre-IK sample; the
/// rendered camera trails it by part of a tick, a metre or more at a flight's speed.
struct CameraSeries {
    /// `(recording_s, camera, mode)`, in time order.
    cameras: Vec<(f64, Vec3, String)>,
    /// `(recording_s, head)`, the pre-IK head bone in the world, one per pass, in time order.
    heads: Vec<(f64, Vec3)>,
}

impl CameraSeries {
    fn new(recording: &Recording) -> Self {
        let mut cameras = Vec::new();
        let mut heads: Vec<(f64, Vec3)> = Vec::new();
        for event in &recording.events {
            let time = event.time_ns as f64 / 1.0e9;
            match &event.kind {
                EventKind::ControllerState(state) => {
                    if let Some(camera) = state.view.as_ref().and_then(|view| view.camera.as_ref())
                    {
                        cameras.push((time, camera.current.position, state.mode.to_string()));
                    }
                }
                EventKind::Pose(sample) if sample.stage == PoseStage::PreIk => {
                    let Some(Some(head)) = sample.bones.get(HEAD_BONE) else {
                        continue;
                    };
                    let world =
                        sample.character.position + sample.character.orientation * head.position;
                    if heads.last().is_none_or(|last| last.0 < time) {
                        heads.push((time, world));
                    }
                }
                _ => {}
            }
        }
        Self { cameras, heads }
    }

    /// The camera against the head within a tenth of a second of `time`.
    fn near(&self, time: f64) -> Option<CameraNear> {
        let from = self.cameras.partition_point(|c| c.0 < time - NEAR_S);
        let to = self.cameras.partition_point(|c| c.0 <= time + NEAR_S);
        let mut lead: Option<f32> = None;
        let mut distance: f32 = 0.0;
        let mut speed = 0.0;
        for (camera_time, camera, _) in &self.cameras[from..to] {
            let index = self.heads.partition_point(|h| h.0 <= *camera_time);
            if index < 2 {
                continue;
            }
            let (t1, head) = self.heads[index - 1];
            let (t0, previous) = self.heads[index - 2];
            let velocity = (head - previous) / ((t1 - t0).max(1.0e-3) as f32);
            let offset = *camera - head;
            speed = velocity.length();
            if speed >= MIN_SPEED_MPS
                && let Some(direction) = velocity.try_normalize()
            {
                let along = offset.dot(direction);
                lead = Some(lead.map_or(along, |lead: f32| lead.min(along)));
            }
            distance = distance.max(offset.length());
        }
        let mode = self.cameras.get(from).filter(|_| from < to)?;
        Some(CameraNear {
            lead_m: lead,
            distance_m: distance,
            speed_mps: speed,
            mode: mode.2.clone(),
        })
    }
}

/// The head's index in the body bone set (`tracking_format::event::BODY_BONES`).
const HEAD_BONE: usize = 6;

/// How far either side of a flash the camera is read, seconds.
const NEAR_S: f64 = 0.1;

#[cfg(test)]
mod tests {
    use super::*;

    fn thumb(value: u8) -> Vec<u8> {
        vec![value; THUMB_BYTES]
    }

    #[test]
    fn a_one_frame_flash_scores_and_a_cut_does_not() {
        let mut detector = FlashDetector::new();
        let scores: Vec<_> = [10, 10, 10, 200, 10, 10, 10, 90, 90, 90]
            .into_iter()
            .filter_map(|value| detector.push(&thumb(value)))
            .collect();
        let flash = scores.iter().find(|s| s.frame == 3).unwrap();
        assert!(flash.score > 150.0, "{flash:?}");
        assert_eq!(flash.frames, 1);
        // A cut to a new scene and staying there is no flash.
        assert!(
            scores
                .iter()
                .filter(|s| s.frame >= 6)
                .all(|s| s.score <= 0.0)
        );
    }

    #[test]
    fn a_two_frame_flash_scores_as_two() {
        let mut detector = FlashDetector::new();
        let scores: Vec<_> = [10, 10, 200, 200, 10, 10]
            .into_iter()
            .filter_map(|value| detector.push(&thumb(value)))
            .collect();
        let flash = scores.iter().find(|s| s.frame == 2).unwrap();
        assert!(flash.score > 150.0);
        assert_eq!(flash.frames, 2);
    }

    #[test]
    fn nearby_scores_merge_into_the_best() {
        let score = |frame, score| FrameScore {
            frame,
            score,
            frames: 1,
        };
        let merged = events(
            &[
                score(10, 30.0),
                score(12, 50.0),
                score(40, 26.0),
                score(41, 5.0),
            ],
            25.0,
            6,
        );
        assert_eq!(merged, vec![score(12, 50.0), score(40, 26.0)]);
    }
}
