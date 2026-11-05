//! `export-rerun`: a recording, or a stretch of it, written as a Rerun recording (`.rrd`) to scrub
//! in the Rerun viewer, with the take's video beside it. See `docs/mod/testing/rerun.md`.
//!
//! Everything is logged on the `recording` timeline (seconds on the recording's clock) and, when the
//! take's video sidecar or a take marker anchors it, on the `wall_clock` timeline.
//!
//! - [`registration`] places the cockpit frame in the world: from the recorded view where the
//!   recording holds it (from version 8), and by an estimate otherwise.
//! - [`view`] turns a recorded view into a placement and logs the camera.
//! - [`clock`] maps the recording's clock to the wall clock and the video's clock.
//! - [`scene`] logs the frames: the tracked poses, the offline rig, and their scalars.
//! - [`character`] logs the controller states: Rico, the aim, the grapple, the vehicle, and their
//!   scalars.
//! - [`pose`] logs Rico's recorded skeleton, and [`recorded_rig`] the rig the game ran.
//! - [`bodies`] logs the physics bodies near Rico, what the hands hold, and the forces applied.
//! - [`events`] logs the other events as text log entries, state lanes, and 3D markers.
//! - [`video`] cuts the take's video to the stretch and logs it as a video asset.
//! - [`blueprint`] is the default layout.

mod blueprint;
mod bodies;
mod character;
mod clock;
mod events;
mod pose;
mod recorded_rig;
pub mod registration;
mod scene;
#[cfg(test)]
mod tests;
mod video;
mod view;

use std::{
    fmt,
    path::{Path, PathBuf},
};

use glam::Vec3;
use rerun::{RecordingStream, RecordingStreamBuilder, TextDocument, ViewCoordinates};
use tracking_format::{
    Frame, Recording,
    event::{ControllerState, EventKind},
};

use crate::{
    dump::Range,
    rerun_export::{
        clock::WallClock,
        registration::{Anchor, Correspondence, Fit, Placement},
    },
    rig_eval::{self, EvalOptions, EvalRow, VideoClock},
};

/// What [`export`] writes.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    /// The stretch to export.
    pub range: Range,
    /// The take's video, to cut to the stretch and log beside it.
    pub video: Option<VideoSource>,
    /// The body and the rig's tuning the grounded rig is evaluated with.
    pub eval: EvalOptions,
    /// The recording's path, named in the export's notes.
    pub source: PathBuf,
}

/// The take's video and the height its cut is scaled to.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoSource {
    pub path: PathBuf,
    /// The cut's height in pixels, at most the video's own.
    pub height: u32,
}

/// The cut's height without `--video-height`, pixels: enough to read the game's HUD and the debug
/// UI, at about a fifth of the source's size.
pub const DEFAULT_VIDEO_HEIGHT: u32 = 720;

/// What [`export`] wrote.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExportSummary {
    pub frames: usize,
    pub events: usize,
    /// Rows of the offline rig evaluation inside the stretch.
    pub rig_rows: usize,
    /// Video frames placed on the timeline.
    pub video_frames: usize,
    /// Whether the export has a wall-clock timeline.
    pub wall_clock: bool,
    /// The median distance between a registered grip's wrist and the recorded world wrist, metres.
    pub median_wrist_error_m: Option<f32>,
}

/// Why [`export`] could not write the recording.
#[derive(Debug)]
pub enum ExportError {
    /// The range holds no frames.
    EmptyRange,
    /// A video was given without the sidecar that places it on the recording's clock.
    NoVideoClock,
    /// Rerun refused to open the output or to log.
    Rerun(rerun::RecordingStreamError),
    /// The output could not be flushed to disk.
    Flush(rerun::sink::SinkFlushError),
    Video(video::VideoError),
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "export-rerun: ")?;
        match self {
            ExportError::EmptyRange => write!(f, "the range holds no frames"),
            ExportError::NoVideoClock => write!(
                f,
                "a video needs the take's `frames.jsonl` and `stream.json` beside the recording, \
                 which place it on the recording's clock"
            ),
            ExportError::Rerun(error) => write!(f, "rerun: {error}"),
            ExportError::Flush(error) => write!(f, "rerun: could not write the file: {error}"),
            ExportError::Video(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<rerun::RecordingStreamError> for ExportError {
    fn from(error: rerun::RecordingStreamError) -> Self {
        ExportError::Rerun(error)
    }
}

impl From<video::VideoError> for ExportError {
    fn from(error: video::VideoError) -> Self {
        ExportError::Video(error)
    }
}

/// Write `recording`'s stretch to `out` as a Rerun recording. `sidecar` is the take's video clock,
/// when the sidecar sits beside the recording.
pub fn export(
    recording: &Recording,
    sidecar: Option<&VideoClock>,
    options: &ExportOptions,
    out: &Path,
) -> Result<ExportSummary, ExportError> {
    let (from, to) = options
        .range
        .resolve(&recording.frames)
        .ok_or(ExportError::EmptyRange)?;
    let first = recording.frames.partition_point(|f| f.time_ns < from);
    let last = recording.frames.partition_point(|f| f.time_ns <= to);
    let frames = &recording.frames[first..last];
    if frames.is_empty() {
        return Err(ExportError::EmptyRange);
    }
    if options.video.is_some() && sidecar.is_none() {
        return Err(ExportError::NoVideoClock);
    }

    let wall = WallClock::new(recording, frames, sidecar);
    let states = FrameStates::pair(recording, frames, (from, to));
    let correspondences: Vec<Correspondence> = frames
        .iter()
        .zip(&states.per_frame)
        .scan(0u32, |segment, (frame, state)| {
            if !frame.recenters.is_empty() {
                *segment += 1;
            }
            Some(scene::correspondence(
                frame,
                state.map(|i| states.all[i].1),
                recording.header.world_scale,
                options.eval.body.eye_height,
                *segment,
            ))
        })
        .collect();
    let views: Vec<Option<Placement>> = states
        .per_frame
        .iter()
        .map(|state| {
            let view = states.all[(*state)?].1.view.as_ref()?;
            view::placement(view)
        })
        .collect();
    let fit = registration::place(&correspondences, &views, REGISTRATION_WINDOW);
    let origin = world_origin(recording, &states, &fit.placements);
    // The rig needs the recording's history before the stretch, so it runs over the whole of it.
    let (rows, _) = rig_eval::evaluate(recording, &options.eval, None);
    let rows: Vec<EvalRow> = rows
        .into_iter()
        .filter(|row| (from..=to).contains(&seconds_to_ns(row.time_s)))
        .collect();

    let rec = RecordingStreamBuilder::new(APPLICATION_ID)
        .recording_name(format!(
            "{} {:.1}-{:.1} s",
            recording.header.recorded_at,
            frames[0].time_ns as f64 / 1.0e9,
            frames[frames.len() - 1].time_ns as f64 / 1.0e9,
        ))
        .save(out)?;
    blueprint::send(&rec, options.video.is_some())?;
    rec.log_static("world", &ViewCoordinates::RIGHT_HAND_Y_UP())?;
    rec.log_static(
        "about",
        &TextDocument::from_markdown(about(recording, options, origin, &fit, wall.is_some())),
    )?;

    let context = Context {
        rec: &rec,
        recording,
        frames,
        states: &states,
        fit: &fit,
        wall: wall.as_ref(),
        origin,
        eye_height: options.eval.body.eye_height,
    };
    scene::log_frames(&context, &rows)?;
    character::log(&context)?;
    view::log(&context)?;
    pose::log(&context, (from, to))?;
    recorded_rig::log(&context, (from, to))?;
    let events = events::log(&context, (from, to))? + bodies::log(&context, (from, to))?;
    let video_frames = match (&options.video, sidecar) {
        (Some(source), Some(clock)) => video::log(&context, source, out, clock)?,
        _ => 0,
    };
    rec.flush_blocking().map_err(ExportError::Flush)?;

    let mut errors: Vec<f32> = fit.wrist_errors.iter().flatten().copied().collect();
    errors.sort_by(f32::total_cmp);
    Ok(ExportSummary {
        frames: frames.len(),
        events,
        rig_rows: rows.len(),
        video_frames,
        wall_clock: wall.is_some(),
        median_wrist_error_m: errors.get(errors.len() / 2).copied(),
    })
}

/// What the loggers share.
pub(crate) struct Context<'a> {
    pub rec: &'a RecordingStream,
    pub recording: &'a Recording,
    /// The stretch's frames.
    pub frames: &'a [Frame],
    pub states: &'a FrameStates<'a>,
    pub fit: &'a Fit,
    pub wall: Option<&'a WallClock>,
    /// The world point subtracted from every world position.
    pub origin: Vec3,
    pub eye_height: f32,
}

impl Context<'_> {
    /// Log what follows at a recording time.
    pub fn set_time(&self, time_ns: u64) {
        self.rec.set_duration_secs(TIMELINE, time_ns as f64 / 1.0e9);
        match self.wall.and_then(|wall| wall.unix_ns(time_ns)) {
            Some(ns) => self.rec.set_timestamp_nanos_since_epoch(WALL_TIMELINE, ns),
            None => self.rec.disable_timeline(WALL_TIMELINE),
        }
    }

    /// A world position as the export logs it.
    pub fn world(&self, position: Vec3) -> [f32; 3] {
        (position - self.origin).to_array()
    }

    /// The index of the stretch's frame at or before a recording time.
    pub fn frame_at(&self, time_ns: u64) -> Option<usize> {
        self.frames
            .partition_point(|f| f.time_ns <= time_ns)
            .checked_sub(1)
    }
}

/// A recorded name as one part of an entity path: anything but letters, digits, and underscores
/// becomes an underscore, so that a name with spaces, dots, or slashes stays one part.
pub(crate) fn path_part(name: &str) -> String {
    let part: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if part.is_empty() {
        "_".to_owned()
    } else {
        part
    }
}

/// The recording's own clock, seconds.
pub(crate) const TIMELINE: &str = "recording";

/// The wall clock, from the take's sidecar or a take marker.
pub(crate) const WALL_TIMELINE: &str = "wall_clock";

/// The controller states in a stretch, and the one each frame's game frame read.
pub(crate) struct FrameStates<'a> {
    /// `(time_ns, state)`, in time order.
    pub all: Vec<(u64, &'a ControllerState)>,
    /// For each frame, the index in [`all`](Self::all) of the state read after it: the first one
    /// after the frame and before the next.
    pub per_frame: Vec<Option<usize>>,
}

impl<'a> FrameStates<'a> {
    fn pair(recording: &'a Recording, frames: &[Frame], (from, to): (u64, u64)) -> Self {
        let all: Vec<(u64, &ControllerState)> = recording
            .events
            .iter()
            .filter(|event| (from..=to).contains(&event.time_ns))
            .filter_map(|event| match &event.kind {
                EventKind::ControllerState(state) => Some((event.time_ns, state.as_ref())),
                _ => None,
            })
            .collect();
        let per_frame = frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let next = frames.get(index + 1).map_or(u64::MAX, |f| f.time_ns);
                let at = all.partition_point(|(t, _)| *t <= frame.time_ns);
                (at < all.len() && all[at].0 < next).then_some(at)
            })
            .collect();
        Self { all, per_frame }
    }

    /// The latest state at or before a recording time.
    pub fn at(&self, time_ns: u64) -> Option<&'a ControllerState> {
        let index = self.all.partition_point(|(t, _)| *t <= time_ns);
        Some(self.all[index.checked_sub(1)?].1)
    }
}

/// The application the recordings belong to in the viewer.
const APPLICATION_ID: &str = "jc3vr_tracking";

/// How many frames either side of a frame the heading estimate pools: about a quarter of a second
/// at the headset's rate.
const REGISTRATION_WINDOW: usize = 12;

/// The world point subtracted from every world position, so that the viewer works near zero: the
/// root's first position in the stretch, else the start marker's, rounded to the metre.
fn world_origin(
    recording: &Recording,
    states: &FrameStates<'_>,
    placements: &[Option<Placement>],
) -> glam::Vec3 {
    let root = states
        .all
        .iter()
        .find_map(|(_, state)| state.root.as_ref().map(|root| root.position))
        .or_else(|| placements.iter().flatten().next().map(|p| p.translation))
        .unwrap_or_else(|| recording.header.start.position());
    root.round()
}

fn about(
    recording: &Recording,
    options: &ExportOptions,
    origin: glam::Vec3,
    fit: &Fit,
    wall_clock: bool,
) -> String {
    let header = &recording.header;
    let markers: Vec<String> = recording
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::TakeMarker(marker) => Some(format!(
                "- #{} at {:.2} s{}",
                marker.index,
                event.time_ns as f64 / 1.0e9,
                if marker.label.is_empty() {
                    String::new()
                } else {
                    format!(": {}", marker.label)
                }
            )),
            _ => None,
        })
        .collect();
    format!(
        "# {recorded}\n\n\
         Exported from `{source}` (format version {version}{truncated}), recorded by build \
         `{build}`, {hand:?} hand dominant, world scale {scale}, time scale {time_scale}.\n\n\
         The `world` entity is the game's world, Y up, less the origin ({ox}, {oy}, {oz}). \
         `world/rico` is the root's world position from the root's tick. `world/cockpit` is the \
         cockpit frame placed in the world: {placed} The drawn wrists sit off the grips by left \
         {ol:?} m and right {or:?} m in the grips' frames, as estimated from the placements \
         (`states/cockpit_placement` names each frame's source). \
         `world/cockpit/room` is the room frame, the cockpit with its floor at zero, which the \
         grounded rig (evaluated offline with the payload's solver, eye height {eye:.2} m) works \
         in. Velocity arrows show a quarter-second's travel.\n\n\
         Timelines: `recording` (seconds on the recording's clock){wall}.\n\n\
         Take markers:\n\n{markers}\n",
        recorded = header.recorded_at,
        source = options.source.display(),
        version = recording.version,
        truncated = if recording.truncated {
            ", cut short"
        } else {
            ""
        },
        build = header.build,
        hand = header.dominant_hand,
        scale = header.world_scale,
        time_scale = header.time_scale,
        ox = origin.x,
        oy = origin.y,
        oz = origin.z,
        placed = placement_note(fit),
        ol = fit.offsets[0].to_array(),
        or = fit.offsets[1].to_array(),
        eye = options.eval.body.eye_height,
        wall = if wall_clock { " and `wall_clock`" } else { "" },
        markers = if markers.is_empty() {
            "none".to_owned()
        } else {
            markers.join("\n")
        },
    )
}

/// What placed the stretch's frames, for the notes.
fn placement_note(fit: &Fit) -> String {
    let count = |anchor: Anchor| {
        fit.placements
            .iter()
            .flatten()
            .filter(|p| p.anchor == anchor)
            .count()
    };
    let viewed = count(Anchor::View);
    let total = fit.placements.len();
    if viewed == total {
        "every frame by the recorded view.".to_owned()
    } else {
        format!(
            "{viewed} of {total} frames by the recorded view; the rest by an estimate fitted from \
             the drawn wrists' world positions against the grips ({} from the root tick, {} from \
             the wrists alone, {} holding the last placement), the recording not holding the \
             view for them.",
            count(Anchor::Root),
            count(Anchor::Wrists),
            count(Anchor::Held),
        )
    }
}

fn seconds_to_ns(seconds: f64) -> u64 {
    (seconds * 1.0e9).round().max(0.0) as u64
}
