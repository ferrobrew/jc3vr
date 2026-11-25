//! The capture host's half of the control protocol, version 2: a take whose frames the payload
//! streams itself.
//!
//! The payload opens with one line of UTF-8 JSON, at most [`LINE_LIMIT`] bytes:
//!
//! ```text
//! {"protocol": "jc3vr-capture", "version": 2, "type": "start", "take": "<stamp>",
//!  "folder": "/host/path" | null, "payload_folder": "C:\\...\\recordings\\<stamp>",
//!  "window": {...},
//!  "video": {"format": "yuv4mpegpipe", "width": 3840, "height": 2134, "frame_rate": 30,
//!            "pixel_format": "yuv420p", "color_space": "bt709", "color_range": "limited",
//!            "layout": "side_by_side", "eyes": [{"eye": 0, "x": 0, "y": 0, ...}, ...]}}
//! ```
//!
//! The injector runs in the game's own Windows environment, so it writes into `payload_folder` and
//! ignores `folder`, which names the same place for a host outside it. It answers with one of
//!
//! ```text
//! {"type": "started", "file": "<path>", "encoder": "<ffmpeg encoder>", "audio": "<state>"}
//! {"type": "error", "message": "..."}
//! ```
//!
//! and closes after an error. After `started` everything the payload sends is the `yuv4mpegpipe`
//! stream, and the end of the stream (the payload closing its writing half) ends the take. The host
//! may send `{"type": "audio", "state": "..."}` when its audio capture changes during the take, and
//! answers `{"type": "stopped", "exit_code": <int | null>}` once the file is finalized. A version 1
//! start, which asks the host to record the mirror window, is refused: only the relay does that.

use std::{fmt, path::PathBuf};

use serde_json::{Value, json};

/// The protocol name a start line must carry.
pub(crate) const PROTOCOL: &str = "jc3vr-capture";
/// The version of a streamed take.
pub(crate) const STREAM_VERSION: u64 = 2;
/// The longest control line accepted.
pub(crate) const LINE_LIMIT: usize = 64 * 1024;

/// A streamed take's start.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StreamStart {
    pub(crate) take: Option<String>,
    /// The take's folder as the payload names it.
    pub(crate) folder: PathBuf,
    pub(crate) video: VideoSpec,
    /// The start's `video` object as sent, for the take's metadata.
    pub(crate) video_json: Value,
}

/// The stream a take sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VideoSpec {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) frame_rate: u32,
}

/// A start line the host cannot serve.
#[derive(Debug, PartialEq)]
pub(crate) enum StartError {
    /// The line is not a JSON object.
    NotJson(String),
    /// The first message is not a start.
    NotStart,
    /// The start names another protocol.
    WrongProtocol(String),
    /// The start asks for a mirror recording (version 1).
    MirrorTake,
    /// The start is for a version this host does not speak.
    UnknownVersion(String),
    /// A field is missing or out of range.
    Invalid(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(error) => {
                write!(f, "capture host: the start is not a JSON object: {error}")
            }
            Self::NotStart => write!(f, "capture host: the first message must be a start"),
            Self::WrongProtocol(name) => write!(
                f,
                "capture host: the start names protocol {name}, not {PROTOCOL:?}"
            ),
            Self::MirrorTake => write!(
                f,
                "capture host: the injector records only the in-process stream (protocol version \
                 2); turn the in-process capture on in the Video feature, or run \
                 scripts/capture_host.sh to record the mirror window"
            ),
            Self::UnknownVersion(version) => write!(
                f,
                "capture host: the start is for protocol version {version}; the injector speaks \
                 version {STREAM_VERSION}"
            ),
            Self::Invalid(reason) => write!(f, "capture host: the start is invalid: {reason}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Parse the payload's first line, without its newline.
pub(crate) fn parse_start(line: &[u8]) -> Result<StreamStart, StartError> {
    let message: Value =
        serde_json::from_slice(line).map_err(|error| StartError::NotJson(error.to_string()))?;
    let Value::Object(message) = message else {
        return Err(StartError::NotJson("not an object".to_owned()));
    };
    if message.get("type").and_then(Value::as_str) != Some("start") {
        return Err(StartError::NotStart);
    }
    match message.get("protocol") {
        Some(Value::String(name)) if name == PROTOCOL => {}
        other => {
            return Err(StartError::WrongProtocol(
                other.map_or_else(|| "none".to_owned(), Value::to_string),
            ));
        }
    }
    match message.get("version").and_then(Value::as_u64) {
        Some(STREAM_VERSION) => {}
        Some(1) => return Err(StartError::MirrorTake),
        _ => {
            return Err(StartError::UnknownVersion(
                message
                    .get("version")
                    .map_or_else(|| "none".to_owned(), Value::to_string),
            ));
        }
    }
    let take = match message.get("take") {
        None | Some(Value::Null) => None,
        Some(Value::String(take)) if is_plain_name(take) => Some(take.clone()),
        Some(other) => {
            return Err(StartError::Invalid(format!(
                "the take is not a plain name: {other}"
            )));
        }
    };
    let folder = match message.get("payload_folder") {
        Some(Value::String(folder)) if is_absolute_windows_path(folder) => PathBuf::from(folder),
        other => {
            return Err(StartError::Invalid(format!(
                "`payload_folder` is not an absolute path: {}",
                other.map_or_else(|| "none".to_owned(), Value::to_string)
            )));
        }
    };
    let video_json = message
        .get("video")
        .cloned()
        .ok_or_else(|| StartError::Invalid("the start has no `video`".to_owned()))?;
    let video = parse_video(&video_json)?;
    Ok(StreamStart {
        take,
        folder,
        video,
        video_json,
    })
}

/// The `started` answer.
pub(crate) fn started_line(file: &str, encoder: &str, audio: &str) -> Vec<u8> {
    line(&json!({"type": "started", "file": file, "encoder": encoder, "audio": audio}))
}

/// The `audio` update.
pub(crate) fn audio_line(state: &str) -> Vec<u8> {
    line(&json!({"type": "audio", "state": state}))
}

/// The `error` answer, after which the host closes.
pub(crate) fn error_line(message: &str) -> Vec<u8> {
    line(&json!({"type": "error", "message": message}))
}

/// The `stopped` answer, once the file is finalized.
pub(crate) fn stopped_line(exit_code: Option<i32>) -> Vec<u8> {
    line(&json!({"type": "stopped", "exit_code": exit_code}))
}

/// The largest frame dimension accepted: H.264's encoders top out at 4096 wide, and a frame larger
/// than this is a malformed start rather than a real stream.
const MAX_DIMENSION: u64 = 8192;
const MAX_FRAME_RATE: u64 = 240;

fn parse_video(video: &Value) -> Result<VideoSpec, StartError> {
    let text = |key: &str| video.get(key).and_then(Value::as_str);
    if text("format") != Some("yuv4mpegpipe") {
        return Err(StartError::Invalid(format!(
            "the stream format is {:?}, not yuv4mpegpipe",
            text("format")
        )));
    }
    if text("pixel_format") != Some("yuv420p") {
        return Err(StartError::Invalid(format!(
            "the pixel format is {:?}, not yuv420p",
            text("pixel_format")
        )));
    }
    let number = |key: &str, max: u64| -> Result<u32, StartError> {
        match video.get(key).and_then(Value::as_u64) {
            Some(value) if (1..=max).contains(&value) => Ok(value as u32),
            _ => Err(StartError::Invalid(format!(
                "`video.{key}` must be an integer from 1 to {max}"
            ))),
        }
    };
    let spec = VideoSpec {
        width: number("width", MAX_DIMENSION)?,
        height: number("height", MAX_DIMENSION)?,
        frame_rate: number("frame_rate", MAX_FRAME_RATE)?,
    };
    if !spec.width.is_multiple_of(2) || !spec.height.is_multiple_of(2) {
        return Err(StartError::Invalid(format!(
            "4:2:0 needs even dimensions, not {}x{}",
            spec.width, spec.height
        )));
    }
    Ok(spec)
}

fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

/// A drive-rooted (`C:\...`) or UNC (`\\server\...`) path.
fn is_absolute_windows_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    (drive || path.starts_with("\\\\")) && !path.contains('\0')
}

fn line(message: &Value) -> Vec<u8> {
    let mut bytes = message.to_string().into_bytes();
    bytes.push(b'\n');
    bytes
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn start(version: u64, video: Value) -> Vec<u8> {
        json!({
            "protocol": "jc3vr-capture",
            "version": version,
            "type": "start",
            "take": "2026-11-15_10-00-00_000",
            "folder": "/home/user/s/recordings/2026-11-15_10-00-00_000",
            "payload_folder": "Z:\\home\\user\\s\\recordings\\2026-11-15_10-00-00_000",
            "window": {"title": "Just Cause 3", "client_size": [1920, 1080]},
            "video": video,
        })
        .to_string()
        .into_bytes()
    }

    fn video() -> Value {
        json!({
            "format": "yuv4mpegpipe",
            "width": 3840,
            "height": 2134,
            "frame_rate": 30,
            "pixel_format": "yuv420p",
            "color_space": "bt709",
            "color_range": "limited",
            "layout": "side_by_side",
            "eyes": [],
        })
    }

    #[test]
    fn a_streamed_start_names_the_payloads_folder_and_the_stream() {
        let start = parse_start(&start(2, video())).unwrap();
        assert_eq!(start.take.as_deref(), Some("2026-11-15_10-00-00_000"));
        assert_eq!(
            start.folder,
            PathBuf::from("Z:\\home\\user\\s\\recordings\\2026-11-15_10-00-00_000")
        );
        assert_eq!(
            start.video,
            VideoSpec {
                width: 3840,
                height: 2134,
                frame_rate: 30
            }
        );
        assert_eq!(start.video_json, video());
    }

    #[test]
    fn a_mirror_take_is_refused_with_the_way_to_record_one() {
        let error = parse_start(&start(1, Value::Null)).unwrap_err();
        assert_eq!(error, StartError::MirrorTake);
        assert!(error.to_string().contains("scripts/capture_host.sh"));
    }

    #[test]
    fn malformed_starts_are_refused_with_the_reason() {
        let mut odd = video();
        odd["width"] = json!(3841);
        let mut huge = video();
        huge["frame_rate"] = json!(1000);
        let mut nv12 = video();
        nv12["pixel_format"] = json!("nv12");
        for (line, expected) in [
            (b"not json".to_vec(), "not a JSON object"),
            (br#"{"type":"stop"}"#.to_vec(), "must be a start"),
            (start(3, video()), "protocol version 3"),
            (start(2, odd), "even dimensions"),
            (start(2, huge), "video.frame_rate"),
            (start(2, nv12), "not yuv420p"),
        ] {
            let error = parse_start(&line).unwrap_err().to_string();
            assert!(error.starts_with("capture host: "), "{error}");
            assert!(error.contains(expected), "{error}");
        }
        let mut relative: Value = serde_json::from_slice(&start(2, video())).unwrap();
        relative["payload_folder"] = json!("recordings\\take");
        let error = parse_start(relative.to_string().as_bytes()).unwrap_err();
        assert!(
            error.to_string().contains("not an absolute path"),
            "{error}"
        );
    }

    #[test]
    fn answers_are_single_json_lines() {
        for line in [
            started_line("C:\\s\\video.mkv", "h264_nvenc", "none"),
            audio_line("lost"),
            error_line("capture host: no"),
            stopped_line(Some(0)),
            stopped_line(None),
        ] {
            assert_eq!(line.last(), Some(&b'\n'));
            assert_eq!(line.iter().filter(|&&b| b == b'\n').count(), 1);
            let _: Value = serde_json::from_slice(&line).unwrap();
        }
        let value: Value = serde_json::from_slice(&stopped_line(None)).unwrap();
        assert_eq!(value, json!({"type": "stopped", "exit_code": null}));
    }
}
