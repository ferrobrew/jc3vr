//! The take's video, cut to the stretch and logged as a video asset with a frame reference per
//! video frame on the recording's timeline.
//!
//! Rerun reads video only from MP4, and a take is recorded to MKV, so the stretch is cut with
//! `ffmpeg` and encoded afresh as H.264 without B-frames, which every Rerun viewer decodes. Encoding
//! afresh cuts at the exact frame, where a stream copy would start at the keyframe before it. The
//! cut's frames are placed on the recording's clock through the sidecar: a video time is the
//! video's start on the wall clock plus that time, and the wall clock maps to the recording's.

use std::{
    fmt,
    path::{Path, PathBuf},
    process::Command,
};

use rerun::{AssetVideo, TimeColumn, VideoFrameReference, components::VideoTimestamp};

use crate::{
    rerun_export::{Context, TIMELINE, VideoSource, WALL_TIMELINE, clock::WallClock},
    rig_eval::VideoClock,
};

/// Why the video could not be logged.
#[derive(Debug)]
pub enum VideoError {
    /// The stretch's frames are not on the video.
    OutsideVideo,
    /// `ffmpeg` could not be started.
    Spawn(std::io::Error),
    /// `ffmpeg` failed.
    Ffmpeg {
        status: std::process::ExitStatus,
        stderr: String,
    },
    /// The cut could not be read back.
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    /// Rerun could not read the cut's frames.
    Frames(String),
    Log(rerun::RecordingStreamError),
    /// The no-wall-clock case, which the sidecar rules out.
    NoWallClock,
}

impl fmt::Display for VideoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "video: ")?;
        match self {
            VideoError::OutsideVideo => write!(f, "the stretch's frames are not on the video"),
            VideoError::Spawn(error) => {
                write!(f, "could not run ffmpeg, which cuts the video: {error}")
            }
            VideoError::Ffmpeg { status, stderr } => {
                write!(f, "ffmpeg failed ({status}): {}", stderr.trim())
            }
            VideoError::Read { path, error } => {
                write!(f, "could not read the cut at {}: {error}", path.display())
            }
            VideoError::Frames(error) => write!(f, "could not read the cut's frames: {error}"),
            VideoError::Log(error) => write!(f, "rerun: {error}"),
            VideoError::NoWallClock => write!(f, "the sidecar placed no frame on the wall clock"),
        }
    }
}

impl std::error::Error for VideoError {}

/// Cut `source` to the context's stretch, log it, and return how many frames were placed.
pub(crate) fn log(
    context: &Context<'_>,
    source: &VideoSource,
    out: &Path,
    clock: &VideoClock,
) -> Result<usize, VideoError> {
    let wall = context.wall.ok_or(VideoError::NoWallClock)?;
    let frames = context.frames;
    let (start_s, end_s) = frames
        .first()
        .zip(frames.last())
        .and_then(|(first, last)| {
            Some((
                clock.video_s(first.display_time_ns)?,
                clock.video_s(last.display_time_ns)?,
            ))
        })
        .ok_or(VideoError::OutsideVideo)?;
    let start_s = start_s.max(0.0);
    if end_s <= start_s {
        return Err(VideoError::OutsideVideo);
    }
    let cut = out.with_extension("cut.mp4");
    tracing::info!(
        from_s = %format_args!("{start_s:.2}"),
        to_s = %format_args!("{end_s:.2}"),
        "export-rerun: cutting the video with ffmpeg to {}",
        cut.display()
    );
    let result = encode(source, start_s, end_s - start_s, &cut)
        .and_then(|()| log_cut(context, &cut, start_s, clock.start_unix_ms(), wall));
    // The cut is copied into the recording, so it goes either way.
    let _ = std::fs::remove_file(&cut);
    result
}

/// Encode `duration_s` of `source` from `start_s` to `out` as H.264 in MP4, without B-frames, with
/// a keyframe every second so that scrubbing back decodes little, and without audio.
fn encode(
    source: &VideoSource,
    start_s: f64,
    duration_s: f64,
    out: &Path,
) -> Result<(), VideoError> {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-ss",
        ])
        .arg(format!("{start_s:.3}"))
        .arg("-i")
        .arg(&source.path)
        .arg("-t")
        .arg(format!("{duration_s:.3}"))
        .arg("-vf")
        .arg(format!("scale=-2:'min({},ih)'", source.height))
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "28",
            "-bf",
            "0",
            "-g",
            "60",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
        .arg(out)
        .output()
        .map_err(VideoError::Spawn)?;
    if !output.status.success() {
        return Err(VideoError::Ffmpeg {
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

fn log_cut(
    context: &Context<'_>,
    cut: &Path,
    start_s: f64,
    video_start_unix_ms: f64,
    wall: &WallClock,
) -> Result<usize, VideoError> {
    let asset = AssetVideo::from_file_path(cut).map_err(|error| VideoError::Read {
        path: cut.to_owned(),
        error,
    })?;
    let pts = asset
        .read_frame_timestamps_nanos()
        .map_err(|error| VideoError::Frames(error.to_string()))?;
    let (first, last) = (
        context.frames[0].time_ns,
        context.frames[context.frames.len() - 1].time_ns,
    );
    let mut timestamps = Vec::new();
    let mut times = Vec::new();
    let mut walls = Vec::new();
    for pts in pts {
        let unix_ms = video_start_unix_ms + (start_s + pts as f64 / 1.0e9) * 1000.0;
        let Some(time_ns) = wall.time_ns(unix_ms) else {
            continue;
        };
        if !(first..=last).contains(&time_ns) {
            continue;
        }
        timestamps.push(VideoTimestamp::from_nanos(pts));
        times.push(time_ns as i64);
        walls.push((unix_ms * 1.0e6) as i64);
    }
    let placed = timestamps.len();
    context
        .rec
        .log_static("video", &asset)
        .map_err(VideoError::Log)?;
    context
        .rec
        .send_columns(
            "video",
            [
                TimeColumn::new_duration_nanos(TIMELINE, times),
                TimeColumn::new_timestamp_nanos_since_epoch(WALL_TIMELINE, walls),
            ],
            VideoFrameReference::update_fields()
                .with_many_timestamp(timestamps)
                .columns_of_unit_batches()
                .map_err(|error| VideoError::Log(error.into()))?,
        )
        .map_err(VideoError::Log)?;
    Ok(placed)
}
