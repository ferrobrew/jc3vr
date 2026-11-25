//! Choosing the H.264 encoder for a stream: a hardware encoder where ffmpeg has one that works on
//! this machine, a software one otherwise.
//!
//! ffmpeg lists the encoders it was built with, not the ones the machine can run: an NVENC build
//! lists `h264_nvenc` on an AMD card, and under Wine, where ffmpeg.exe reaches no GPU encoder, it
//! still lists all of them. So each listed candidate is probed with a three-frame encode at the
//! stream's own size (the hardware encoders have size limits), all at once to keep the take's start
//! short, and the first in [`CANDIDATES`] order that succeeds is chosen. The choice is cached per
//! stream size for the rest of the host's run.
//!
//! The LGPL ffmpeg build the fetch script stages has no libx264, which is GPL, so its software
//! fallback is Cisco's OpenH264; an ffmpeg build with libx264 put in its place is used as it is.
//! `JC3VR_CAPTURE_ENCODER` names one encoder to use instead of the list.

use std::{
    collections::HashMap,
    process::ExitStatus,
    sync::mpsc,
    time::{Duration, Instant},
};

use ffmpeg_sidecar::command::FfmpegCommand;
use parking_lot::Mutex;

use crate::capture::protocol::VideoSpec;

/// The encoders tried, best first.
pub(crate) const CANDIDATES: &[&str] = &[
    "h264_nvenc",
    "h264_amf",
    "h264_qsv",
    "h264_mf",
    "libx264",
    "libopenh264",
];

/// The environment variable that names one encoder to use.
pub(crate) const ENCODER_VARIABLE: &str = "JC3VR_CAPTURE_ENCODER";
/// The environment variable that names the ffmpeg executable to run.
pub(crate) const FFMPEG_VARIABLE: &str = "JC3VR_FFMPEG";

/// No encoder could encode the stream.
#[derive(Debug)]
pub(crate) enum EncoderError {
    /// ffmpeg could not be run to list its encoders.
    Unavailable(String),
    /// None of the candidates ffmpeg lists encoded the probe; each failure's reason.
    NoneWorked(Vec<(String, String)>),
}

impl std::fmt::Display for EncoderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "encoder: could not run ffmpeg: {reason}"),
            Self::NoneWorked(failures) if failures.is_empty() => write!(
                f,
                "encoder: ffmpeg lists none of the H.264 encoders the capture host uses ({})",
                CANDIDATES.join(", ")
            ),
            Self::NoneWorked(failures) => {
                write!(f, "encoder: no H.264 encoder works here:")?;
                for (name, reason) in failures {
                    write!(f, " {name}: {reason};")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for EncoderError {}

/// The encoders chosen so far, by stream size and rate.
#[derive(Default)]
pub(crate) struct EncoderCache {
    chosen: Mutex<HashMap<VideoSpec, String>>,
}

impl EncoderCache {
    /// The encoder for `video`, probing on the first stream of its size.
    pub(crate) fn choose(&self, video: VideoSpec) -> Result<String, EncoderError> {
        if let Some(name) = self.chosen.lock().get(&video) {
            return Ok(name.clone());
        }
        let requested = std::env::var(ENCODER_VARIABLE).ok();
        let listed = list_encoders()?;
        let candidates = candidates(&listed, requested.as_deref());
        tracing::info!(
            "capture host: probing {} for a {}x{} stream",
            candidates.join(", "),
            video.width,
            video.height
        );
        let results = probe_all(&candidates, video);
        let mut failures = Vec::new();
        for (name, result) in candidates.iter().zip(results) {
            match result {
                Ok(()) => {
                    tracing::info!("capture host: encoding with {name}");
                    self.chosen.lock().insert(video, name.clone());
                    return Ok(name.clone());
                }
                Err(reason) => {
                    tracing::info!("capture host: {name} cannot encode here: {reason}");
                    failures.push((name.clone(), reason));
                }
            }
        }
        Err(EncoderError::NoneWorked(failures))
    }
}

/// An ffmpeg command: the executable `JC3VR_FFMPEG` names, else the `ffmpeg.exe` beside the
/// injector, else the one on `PATH`.
pub(crate) fn ffmpeg_command() -> FfmpegCommand {
    match std::env::var_os(FFMPEG_VARIABLE) {
        Some(path) if !path.is_empty() => FfmpegCommand::new_with_path(path),
        _ => FfmpegCommand::new(),
    }
}

/// The encoder names in the output of `ffmpeg -encoders`.
pub(crate) fn parse_encoders(output: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut past_legend = false;
    for line in output.lines() {
        let line = line.trim();
        // The legend ends with a line of dashes; the table follows.
        if !past_legend {
            past_legend = line.starts_with("---");
            continue;
        }
        let mut fields = line.split_whitespace();
        if let (Some(flags), Some(name)) = (fields.next(), fields.next())
            && flags.len() == 6
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// The candidates to probe, in order: the requested encoder alone when one is named (whether or
/// not ffmpeg lists it, so a misspelling fails with ffmpeg's own reason), otherwise each of
/// [`CANDIDATES`] that ffmpeg lists.
pub(crate) fn candidates(listed: &[String], requested: Option<&str>) -> Vec<String> {
    if let Some(requested) = requested.map(str::trim).filter(|r| !r.is_empty()) {
        return vec![requested.to_owned()];
    }
    CANDIDATES
        .iter()
        .filter(|name| listed.iter().any(|l| l == *name))
        .map(|name| (*name).to_owned())
        .collect()
}

/// The target bitrate in bits a second: about 0.12 bits a pixel, which keeps fast motion clean in
/// H.264. A side-by-side 3840x2134 stream at 30 fps comes to about 30 Mbps.
pub(crate) fn bitrate(video: VideoSpec) -> u64 {
    let pixels_per_second =
        u64::from(video.width) * u64::from(video.height) * u64::from(video.frame_rate);
    pixels_per_second * 12 / 100
}

/// The output arguments for `encoder` at `video`'s size and rate: the codec, its rate control at
/// [`bitrate`], a keyframe every two seconds, and BT.709 limited-range colour tags.
pub(crate) fn encoder_args(encoder: &str, video: VideoSpec) -> Vec<String> {
    let rate = bitrate(video);
    let peak = rate * 3 / 2;
    let buffer = rate * 2;
    let mut args: Vec<String> = vec!["-c:v".into(), encoder.into()];
    let codec_args: &[&str] = match encoder {
        "h264_nvenc" => &["-preset", "p5", "-tune", "hq", "-rc", "vbr"],
        "h264_amf" => &[
            "-usage",
            "transcoding",
            "-quality",
            "balanced",
            "-rc",
            "vbr_peak",
        ],
        "h264_qsv" => &["-preset", "medium"],
        "h264_mf" => &["-rate_control", "pc_vbr", "-scenario", "camera_record"],
        "libx264" => &["-preset", "veryfast"],
        _ => &[],
    };
    args.extend(codec_args.iter().map(|a| (*a).to_owned()));
    args.extend([
        "-b:v".into(),
        rate.to_string(),
        "-maxrate".into(),
        peak.to_string(),
        "-bufsize".into(),
        buffer.to_string(),
        "-g".into(),
        (video.frame_rate * 2).to_string(),
        "-pix_fmt".into(),
        pixel_format(encoder).into(),
        "-color_range".into(),
        "tv".into(),
        "-colorspace".into(),
        "bt709".into(),
        "-color_primaries".into(),
        "bt709".into(),
        "-color_trc".into(),
        "bt709".into(),
    ]);
    args
}

/// How long a probe may take before it counts as failed. A hardware encoder's first open loads its
/// driver, which can take a few seconds.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The pixel format an encoder takes: the Intel and Media Foundation encoders want NV12, which
/// ffmpeg converts the stream's planar 4:2:0 to; the others take it as it is.
fn pixel_format(encoder: &str) -> &'static str {
    match encoder {
        "h264_qsv" | "h264_mf" => "nv12",
        _ => "yuv420p",
    }
}

fn list_encoders() -> Result<Vec<String>, EncoderError> {
    let mut command = ffmpeg_command();
    command.hide_banner().arg("-encoders");
    let output = run(&mut command, PROBE_TIMEOUT).map_err(EncoderError::Unavailable)?;
    if !output.status.success() {
        return Err(EncoderError::Unavailable(format!(
            "ffmpeg -encoders exited with {}: {}",
            output.status,
            last_line(&output.stderr)
        )));
    }
    Ok(parse_encoders(&output.stdout))
}

/// Probe every candidate at once, returning each one's result in order.
fn probe_all(candidates: &[String], video: VideoSpec) -> Vec<Result<(), String>> {
    std::thread::scope(|scope| {
        let probes: Vec<_> = candidates
            .iter()
            .map(|name| scope.spawn(move || probe(name, video)))
            .collect();
        probes
            .into_iter()
            .map(|probe| {
                probe
                    .join()
                    .unwrap_or_else(|_| Err("the probe thread panicked".to_owned()))
            })
            .collect()
    })
}

/// Encode three black frames of `video`'s size with `encoder`, discarding the output.
fn probe(encoder: &str, video: VideoSpec) -> Result<(), String> {
    let mut command = ffmpeg_command();
    command.hide_banner().args([
        "-nostdin",
        "-f",
        "lavfi",
        "-i",
        &format!(
            "color=black:s={}x{}:r={}",
            video.width, video.height, video.frame_rate
        ),
        "-frames:v",
        "3",
    ]);
    command.args(encoder_args(encoder, video));
    command.args(["-f", "null", "-"]);
    let output = run(&mut command, PROBE_TIMEOUT)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(last_line(&output.stderr))
    }
}

struct Output {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

/// Run `command` to completion within `timeout`, collecting its output.
fn run(command: &mut FfmpegCommand, timeout: Duration) -> Result<Output, String> {
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    drop(child.take_stdin());
    let stdout = child.take_stdout().map(read_in_background);
    let stderr = child.take_stderr().map(read_in_background);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.as_inner_mut().try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.as_inner_mut().wait();
                return Err(format!("timed out after {} s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    };
    let collect = |reader: Option<mpsc::Receiver<String>>| {
        reader
            .and_then(|r| r.recv_timeout(Duration::from_secs(2)).ok())
            .unwrap_or_default()
    };
    Ok(Output {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
    })
}

fn read_in_background(mut pipe: impl std::io::Read + Send + 'static) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let mut bytes = Vec::new();
        if pipe.read_to_end(&mut bytes).is_ok() {
            text = String::from_utf8_lossy(&bytes).into_owned();
        }
        let _ = sender.send(text);
    });
    receiver
}

/// The last non-empty line of ffmpeg's log, which names the failure.
fn last_line(log: &str) -> String {
    log.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no output")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const ENCODERS: &str = "Encoders:
 V..... = Video
 A..... = Audio
 ------
 V....D libopenh264          OpenH264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D h264_amf             AMD AMF H.264 Encoder (codec h264)
 V....D h264_mf              H264 via MediaFoundation (codec h264)
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 V..... h264_qsv             H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (Intel Quick Sync Video acceleration) (codec h264)
 A....D aac                  AAC (Advanced Audio Coding)
";

    fn side_by_side() -> VideoSpec {
        VideoSpec {
            width: 3840,
            height: 2134,
            frame_rate: 30,
        }
    }

    #[test]
    fn the_encoder_table_is_read_past_its_legend() {
        assert_eq!(
            parse_encoders(ENCODERS),
            [
                "libopenh264",
                "h264_amf",
                "h264_mf",
                "h264_nvenc",
                "h264_qsv",
                "aac"
            ]
        );
    }

    #[test]
    fn listed_candidates_are_tried_hardware_first() {
        let listed = parse_encoders(ENCODERS);
        assert_eq!(
            candidates(&listed, None),
            [
                "h264_nvenc",
                "h264_amf",
                "h264_qsv",
                "h264_mf",
                "libopenh264"
            ]
        );
        assert_eq!(candidates(&listed, Some(" libx264 ")), ["libx264"]);
        assert_eq!(candidates(&listed, Some("")), candidates(&listed, None));
    }

    #[test]
    fn the_bitrate_follows_the_pixel_rate() {
        assert_eq!(bitrate(side_by_side()), 29_500_416);
    }

    #[test]
    fn encoder_arguments_carry_rate_control_keyframes_and_colour_tags() {
        let args = encoder_args("h264_qsv", side_by_side());
        let pairs: Vec<(&str, &str)> = args
            .chunks(2)
            .map(|pair| (pair[0].as_str(), pair[1].as_str()))
            .collect();
        for expected in [
            ("-c:v", "h264_qsv"),
            ("-b:v", "29500416"),
            ("-g", "60"),
            ("-pix_fmt", "nv12"),
            ("-colorspace", "bt709"),
            ("-color_range", "tv"),
        ] {
            assert!(pairs.contains(&expected), "{expected:?} in {pairs:?}");
        }
        assert!(
            encoder_args("libopenh264", side_by_side())
                .windows(2)
                .any(|pair| pair == ["-pix_fmt", "yuv420p"])
        );
    }
}
