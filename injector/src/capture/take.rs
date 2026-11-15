//! One streamed take: the start line, the ffmpeg that encodes the stream, and the pump that feeds
//! it.
//!
//! The take reads the payload's start line ([`crate::capture::protocol`]), creates the take's
//! folder, chooses the encoder ([`crate::capture::encoder`]), and starts ffmpeg reading the
//! `yuv4mpegpipe` stream from its standard input and writing `video.mkv` into the folder. It then
//! answers `started` and copies everything the payload sends into ffmpeg until the payload ends the
//! stream. Closing ffmpeg's input lets it drain its encoder and finalize the file, after which the
//! take answers `stopped` with ffmpeg's exit code.
//!
//! Matroska is written as it goes, so a take cut short by a crash is still playable up to the cut.
//! `stream.json` beside the video records the encoder, the stream's format as the payload sent it,
//! the wall-clock time the first frame arrived, and ffmpeg's exit.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ffmpeg_sidecar::{
    child::FfmpegChild,
    event::{FfmpegEvent, LogLevel},
};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::capture::{
    encoder::{EncoderCache, encoder_args, ffmpeg_command},
    protocol::{LINE_LIMIT, StreamStart, error_line, parse_start, started_line, stopped_line},
};

/// Serve one connection from the payload to its end.
pub(crate) fn run(stream: TcpStream, encoders: &EncoderCache) {
    let mut control = match stream.try_clone() {
        Ok(control) => control,
        Err(error) => {
            tracing::error!("capture host: could not share the connection: {error}");
            return;
        }
    };
    let _ = stream.set_read_timeout(Some(START_TIMEOUT));
    let mut reader = BufReader::with_capacity(PUMP_CHUNK, stream);
    let line = match read_line(&mut reader) {
        Ok(line) => line,
        Err(error) => {
            tracing::warn!("capture host: no start line: {error}");
            return;
        }
    };
    let start = match parse_start(&line) {
        Ok(start) => start,
        Err(error) => return refuse(&mut control, &error.to_string()),
    };
    tracing::info!(
        "capture host: take {} streams {}x{} at {} fps into {}",
        start.take.as_deref().unwrap_or("(unnamed)"),
        start.video.width,
        start.video.height,
        start.video.frame_rate,
        start.folder.display()
    );
    if let Err(error) = std::fs::create_dir_all(&start.folder) {
        return refuse(
            &mut control,
            &format!(
                "capture host: could not create {}: {error}",
                start.folder.display()
            ),
        );
    }
    let encoder = match encoders.choose(start.video) {
        Ok(encoder) => encoder,
        Err(error) => return refuse(&mut control, &format!("capture host: {error}")),
    };
    let (video, metadata_path) = take_paths(&start.folder);
    let args = ffmpeg_args(&start, &encoder, &video);
    let spawn_unix_ms = unix_ms();
    let mut child = match ffmpeg_command().args(&args).spawn() {
        Ok(child) => child,
        Err(error) => {
            return refuse(
                &mut control,
                &format!(
                    "capture host: could not start ffmpeg (run scripts/fetch_ffmpeg.sh to stage \
                     it beside the injector): {error}"
                ),
            );
        }
    };
    let Some(stdin) = child.take_stdin() else {
        let _ = child.kill();
        return refuse(&mut control, "capture host: ffmpeg has no standard input");
    };
    let log = FfmpegLog::spawn(&mut child);
    let mut metadata = json!({
        "format": STREAM_FORMAT,
        "version": STREAM_FORMAT_VERSION,
        "host": "injector",
        "take": start.take,
        "video": video.file_name().map(|name| name.to_string_lossy().into_owned()),
        "video_start_unix_ms": null,
        "spawn_unix_ms": spawn_unix_ms,
        "stop_unix_ms": null,
        "exit_code": null,
        "encoder": encoder,
        "audio": AUDIO_NONE,
        "stream": start.video_json,
        "command": args,
    });
    write_metadata(&metadata_path, &metadata);
    let file = video.display().to_string();
    if control
        .write_all(&started_line(&file, &encoder, AUDIO_NONE))
        .is_err()
    {
        tracing::warn!("capture host: the payload left before the take started");
    }
    tracing::info!("capture host: recording {file} with {encoder}");

    let _ = reader.get_ref().set_read_timeout(Some(STREAM_TIMEOUT));
    let pumped = pump(&mut reader, stdin, &log);
    metadata["video_start_unix_ms"] = json!(pumped.first_frame_unix_ms);
    match &pumped.end {
        StreamEnd::Finished => tracing::info!(
            "capture host: the stream ended after {} MB; finalizing",
            pumped.bytes / 1_000_000
        ),
        StreamEnd::Lost(error) => {
            tracing::warn!("capture host: the connection was lost: {error}; finalizing")
        }
        StreamEnd::EncoderFailed(error) => {
            let message = format!(
                "capture host: ffmpeg stopped taking the stream: {error}{}",
                log.last_error()
                    .map(|line| format!(" ({line})"))
                    .unwrap_or_default()
            );
            tracing::error!("{message}");
            let _ = control.write_all(&error_line(&message));
        }
    }
    let exit_code = finalize(&mut child);
    metadata["stop_unix_ms"] = json!(unix_ms());
    metadata["exit_code"] = json!(exit_code);
    write_metadata(&metadata_path, &metadata);
    if let Some(error) = log.last_error().filter(|_| exit_code != Some(0)) {
        tracing::error!("capture host: ffmpeg: {error}");
    }
    let _ = control.write_all(&stopped_line(exit_code));
    tracing::info!(
        "capture host: take finished, ffmpeg exited with {}",
        exit_code.map_or_else(|| "no code".to_owned(), |code| code.to_string())
    );
}

/// The metadata file's format name, shared with the relay's.
const STREAM_FORMAT: &str = "jc3vr-capture-stream";
/// The metadata format's version: 2 adds the streamed take's encoder and stream fields.
const STREAM_FORMAT_VERSION: u32 = 2;
/// The audio state reported when the take records none.
const AUDIO_NONE: &str = "none";
/// How long the payload has to send its start line after connecting.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the stream may go quiet before the take counts as lost. The payload repeats frames
/// rather than pausing, so a long silence means the game has stopped.
const STREAM_TIMEOUT: Duration = Duration::from_secs(60);
/// How long ffmpeg has to finalize the file after its input closes before it is killed.
const FINALIZE_TIMEOUT: Duration = Duration::from_secs(30);
/// The pump's read size: about a tenth of a side-by-side frame.
const PUMP_CHUNK: usize = 1 << 20;

/// Read one control line, without its newline.
fn read_line(reader: &mut impl BufRead) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(LINE_LIMIT as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if read == 0 {
        return Err(std::io::ErrorKind::UnexpectedEof.into());
    }
    if line.last() != Some(&b'\n') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the start line is too long or unterminated",
        ));
    }
    line.pop();
    Ok(line)
}

fn refuse(control: &mut TcpStream, message: &str) {
    tracing::error!("{message}");
    let _ = control.write_all(&error_line(message));
}

/// The take's video and metadata paths in `folder`: `video.mkv` and `stream.json`, or the first
/// free `video_N.mkv` and `stream_N.json` when an earlier connection used those.
pub(crate) fn take_paths(folder: &Path) -> (PathBuf, PathBuf) {
    (0..)
        .map(|n: u32| {
            let suffix = if n == 0 {
                String::new()
            } else {
                format!("_{n}")
            };
            (
                folder.join(format!("video{suffix}.mkv")),
                folder.join(format!("stream{suffix}.json")),
            )
        })
        .find(|(video, metadata)| !video.exists() && !metadata.exists())
        .expect("capture host: an unbounded range finds a free name")
}

/// ffmpeg's arguments for a take: the stream on standard input, encoded with `encoder` into a
/// Matroska file at `video`.
pub(crate) fn ffmpeg_args(start: &StreamStart, encoder: &str, video: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-nostats",
        "-y",
        "-thread_queue_size",
        "64",
        "-f",
        "yuv4mpegpipe",
        "-i",
        "-",
        "-map",
        "0:v",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.extend(encoder_args(encoder, start.video));
    args.extend(["-f".to_owned(), "matroska".to_owned()]);
    args.push(video.display().to_string());
    args
}

/// How the stream ended.
enum StreamEnd {
    /// The payload closed its half: the stop.
    Finished,
    /// The connection failed or went quiet.
    Lost(std::io::Error),
    /// ffmpeg stopped reading.
    EncoderFailed(std::io::Error),
}

struct Pumped {
    end: StreamEnd,
    bytes: u64,
    first_frame_unix_ms: Option<u64>,
}

/// Copy the stream into ffmpeg until it ends.
fn pump(reader: &mut BufReader<TcpStream>, mut stdin: impl Write, log: &FfmpegLog) -> Pumped {
    let mut buffer = vec![0u8; PUMP_CHUNK];
    let mut bytes = 0u64;
    let mut header_seen = false;
    let mut first_frame_unix_ms = None;
    let end = loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) => break StreamEnd::Finished,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => break StreamEnd::Lost(error),
        };
        let chunk = &buffer[..count];
        if first_frame_unix_ms.is_none() {
            // The header is one line; the first byte after it begins the first frame.
            let after_header = if header_seen {
                Some(0)
            } else {
                chunk.iter().position(|&b| b == b'\n').map(|end| end + 1)
            };
            header_seen |= after_header.is_some();
            if after_header.is_some_and(|start| start < count) {
                first_frame_unix_ms = Some(unix_ms());
            }
        }
        if let Err(error) = stdin.write_all(chunk) {
            break StreamEnd::EncoderFailed(error);
        }
        bytes += count as u64;
        if log.failed() {
            break StreamEnd::EncoderFailed(std::io::Error::other("ffmpeg reported a fatal error"));
        }
    };
    drop(stdin);
    Pumped {
        end,
        bytes,
        first_frame_unix_ms,
    }
}

/// Wait for ffmpeg to finalize the file, killing it after [`FINALIZE_TIMEOUT`]. Returns its exit
/// code, `None` when it was killed.
fn finalize(child: &mut FfmpegChild) -> Option<i32> {
    let deadline = Instant::now() + FINALIZE_TIMEOUT;
    loop {
        match child.as_inner_mut().try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) if Instant::now() >= deadline => {
                tracing::error!(
                    "capture host: ffmpeg did not finalize within {} s; killing it",
                    FINALIZE_TIMEOUT.as_secs()
                );
                let _ = child.kill();
                let _ = child.as_inner_mut().wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                tracing::error!("capture host: could not wait for ffmpeg: {error}");
                return None;
            }
        }
    }
}

/// ffmpeg's log, forwarded to tracing by a thread of its own, keeping the last error.
struct FfmpegLog {
    shared: Arc<LogShared>,
}

#[derive(Default)]
struct LogShared {
    last_error: Mutex<Option<String>>,
    fatal: std::sync::atomic::AtomicBool,
}

impl FfmpegLog {
    fn spawn(child: &mut FfmpegChild) -> Self {
        let shared = Arc::new(LogShared::default());
        match child.iter() {
            Ok(events) => {
                let thread_shared = Arc::clone(&shared);
                let spawned = std::thread::Builder::new()
                    .name("capture-ffmpeg-log".to_owned())
                    .spawn(move || {
                        for event in events {
                            thread_shared.note(event);
                        }
                    });
                if let Err(error) = spawned {
                    tracing::warn!("capture host: ffmpeg's log is not read: {error}");
                }
            }
            Err(error) => tracing::warn!("capture host: ffmpeg's log is not read: {error:#}"),
        }
        Self { shared }
    }

    fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().clone()
    }

    fn failed(&self) -> bool {
        self.shared.fatal.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl LogShared {
    fn note(&self, event: FfmpegEvent) {
        match event {
            FfmpegEvent::Log(LogLevel::Warning, line) => tracing::warn!("ffmpeg: {line}"),
            FfmpegEvent::Log(LogLevel::Error, line) | FfmpegEvent::Error(line) => {
                tracing::error!("ffmpeg: {line}");
                *self.last_error.lock() = Some(line);
            }
            FfmpegEvent::Log(LogLevel::Fatal, line) => {
                tracing::error!("ffmpeg: {line}");
                *self.last_error.lock() = Some(line);
                self.fatal.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

fn write_metadata(path: &Path, metadata: &Value) {
    let temporary = path.with_extension("json.tmp");
    let result = serde_json::to_vec_pretty(metadata)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&temporary, bytes))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = result {
        tracing::error!("capture host: could not write {}: {error}", path.display());
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::capture::protocol::VideoSpec;

    fn start() -> StreamStart {
        StreamStart {
            take: Some("t".to_owned()),
            folder: PathBuf::from("C:\\s\\recordings\\t"),
            video: VideoSpec {
                width: 3840,
                height: 2134,
                frame_rate: 30,
            },
            video_json: Value::Null,
        }
    }

    #[test]
    fn ffmpeg_reads_the_stream_from_standard_input_and_writes_matroska() {
        let args = ffmpeg_args(&start(), "h264_nvenc", Path::new("C:\\s\\video.mkv"));
        let input = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[input - 1], "yuv4mpegpipe");
        assert_eq!(args[input + 1], "-");
        assert_eq!(
            &args[args.len() - 3..],
            ["-f", "matroska", "C:\\s\\video.mkv"]
        );
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]));
    }

    #[test]
    fn a_folder_used_before_gets_numbered_files() {
        let folder =
            std::env::temp_dir().join(format!("jc3vr-capture-take-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (video, metadata) = take_paths(&folder);
        assert_eq!(video, folder.join("video.mkv"));
        assert_eq!(metadata, folder.join("stream.json"));
        std::fs::write(&metadata, b"{}").unwrap();
        assert_eq!(
            take_paths(&folder),
            (folder.join("video_1.mkv"), folder.join("stream_1.json"))
        );
        std::fs::remove_dir_all(&folder).unwrap();
    }

    /// A whole take through a real ffmpeg: run with `JC3VR_FFMPEG` naming an ffmpeg.exe, as
    /// `JC3VR_FFMPEG=... ./scripts/xwin_test.sh -p jc3vr_injector -- --ignored`.
    #[test]
    #[ignore = "needs an ffmpeg executable"]
    fn a_streamed_take_encodes_into_the_takes_folder() {
        let folder = std::env::temp_dir().join(format!("jc3vr-capture-e2e-{}", std::process::id()));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            run(stream, &EncoderCache::default());
        });
        let (width, height, frames) = (640u32, 360u32, 45u32);
        let mut payload = TcpStream::connect(address).unwrap();
        let start = json!({
            "protocol": "jc3vr-capture", "version": 2, "type": "start", "take": "e2e",
            "folder": null, "payload_folder": folder.display().to_string(),
            "window": {"title": null, "client_size": null},
            "video": {"format": "yuv4mpegpipe", "width": width, "height": height,
                      "frame_rate": 30, "pixel_format": "yuv420p", "color_space": "bt709",
                      "color_range": "limited", "layout": "side_by_side", "eyes": []},
        });
        payload.write_all(format!("{start}\n").as_bytes()).unwrap();
        let mut answers = BufReader::new(payload.try_clone().unwrap());
        let mut started = String::new();
        answers.read_line(&mut started).unwrap();
        let started: Value = serde_json::from_str(&started).unwrap();
        assert_eq!(started["type"], "started", "{started}");
        payload
            .write_all(format!("YUV4MPEG2 W{width} H{height} F30:1 Ip A1:1 C420jpeg\n").as_bytes())
            .unwrap();
        let frame_len = (width * height * 3 / 2) as usize;
        for i in 0..frames {
            payload.write_all(b"FRAME\n").unwrap();
            payload.write_all(&vec![(i * 5) as u8; frame_len]).unwrap();
        }
        payload.shutdown(std::net::Shutdown::Write).unwrap();
        let mut stopped = String::new();
        answers.read_line(&mut stopped).unwrap();
        let stopped: Value = serde_json::from_str(&stopped).unwrap();
        assert_eq!(stopped, json!({"type": "stopped", "exit_code": 0}));
        host.join().unwrap();
        let video = std::fs::metadata(folder.join("video.mkv")).unwrap();
        assert!(video.len() > 1000);
        let metadata: Value =
            serde_json::from_slice(&std::fs::read(folder.join("stream.json")).unwrap()).unwrap();
        assert_eq!(metadata["exit_code"], 0);
        assert!(metadata["video_start_unix_ms"].is_u64());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_start_line_is_read_without_its_newline_and_an_overlong_one_is_refused() {
        let mut reader = BufReader::new(&b"{\"type\":\"start\"}\nYUV4MPEG2"[..]);
        assert_eq!(read_line(&mut reader).unwrap(), b"{\"type\":\"start\"}");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"YUV4MPEG2");

        let long = vec![b'x'; LINE_LIMIT + 10];
        let mut reader = BufReader::new(&long[..]);
        assert!(read_line(&mut reader).is_err());
    }
}
