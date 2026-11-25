use std::{
    fs,
    io::{BufRead, BufReader},
    net::TcpListener,
    path::PathBuf,
    sync::atomic::AtomicU64,
};

use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use super::*;
use crate::recorder::{
    Region,
    layout::{Arrangement, MIN_EYE_WIDTH, StreamLayout},
    protocol::{FRAME_TAG, VideoFormat, stream_header},
    stream::VideoFrame,
};

#[test]
fn sidecar_lines_carry_frames_markers_mirror_layouts_and_presents() {
    let mut out = Vec::new();
    write_record(&mut out, &Record::Frame(Box::new(frame(7)))).unwrap();
    write_record(
        &mut out,
        &Record::Marker {
            frame: 8,
            marker: marker(1),
        },
    )
    .unwrap();
    write_record(
        &mut out,
        &Record::Mirror {
            frame: 8,
            unix_ms: 1_700_000_000_123,
            mirror: MirrorFrame {
                view: MirrorView::Left,
                window: (1920, 1080),
                draws: [
                    Some(MirrorDraw {
                        eye: 0,
                        rect: [0.0, -420.0, 1920.0, 1920.0],
                    }),
                    None,
                ],
            },
        },
    )
    .unwrap();
    write_record(
        &mut out,
        &Record::Frame(Box::new(FrameRecord {
            views: None,
            ..frame(8)
        })),
    )
    .unwrap();
    write_record(
        &mut out,
        &Record::Present {
            frame: 9,
            unix_ms: 1_700_000_000_130,
            present: 41,
            stamped: true,
        },
    )
    .unwrap();
    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            json!({
                "frame": 7,
                "unix_ms": 1_700_000_000_007_i64,
                "game_run_time_us": 7_000,
                "predicted_display_time_ns": 70,
                "views": [
                    {"position": [0.0, 1.5, 0.0], "orientation": [0.0, 0.0, 0.0, 1.0],
                     "fov": [-1.0, 0.75, 0.5, -1.0]},
                    {"position": [0.0, 1.5, 0.0], "orientation": [0.0, 0.0, 0.0, 1.0],
                     "fov": [-1.0, 0.75, 0.5, -1.0]},
                ],
            }),
            json!({
                "frame": 8,
                "marker": {"index": 1, "stamp": "stamp-1", "unix_ms": 1, "game_run_time_us": null,
                           "label": "take"},
            }),
            json!({
                "frame": 8,
                "unix_ms": 1_700_000_000_123_i64,
                "mirror": {"view": "Left", "window": [1920, 1080],
                           "draws": [{"eye": 0, "rect": [0.0, -420.0, 1920.0, 1920.0]}]},
            }),
            json!({
                "frame": 8,
                "unix_ms": 1_700_000_000_008_i64,
                "game_run_time_us": 8_000,
                "predicted_display_time_ns": null,
                "views": null,
            }),
            json!({
                "frame": 9,
                "unix_ms": 1_700_000_000_130_i64,
                "present": 41,
                "stamped": true,
            }),
        ]
    );
}

#[test]
fn a_take_starts_the_host_writes_the_sidecar_and_stops_the_host() {
    let folder = TempFolder::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().to_string();
    let host = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let start = read_json(&mut reader);
        let mut stream = stream;
        stream
            .write_all(
                b"{\"type\":\"started\",\"file\":\"/s/video.mkv\",\"video_start_unix_ms\":12.5,\
                  \"region\":{\"x\":4,\"y\":31,\"width\":1280,\"height\":720},\"output\":\"DP-1\"}\n",
            )
            .unwrap();
        let stop = read_json(&mut reader);
        stream
            .write_all(b"{\"type\":\"stopped\",\"exit_code\":0}\n")
            .unwrap();
        (start, stop)
    });

    let mut take = Take::spawn(request(&folder, target)).unwrap();
    await_host(&take, |state| matches!(state, HostState::Capturing(_)));
    assert_eq!(
        take.host(),
        HostState::Capturing(Capture {
            file: "/s/video.mkv".to_owned(),
            region: Some(Region {
                x: 4,
                y: 31,
                width: 1280,
                height: 720,
            }),
            output: Some("DP-1".to_owned()),
            video_start_unix_ms: Some(12.5),
            encoder: None,
            audio: None,
        })
    );
    assert!(take.send(Record::Frame(Box::new(frame(0)))).is_ok());
    assert!(
        take.send(Record::Marker {
            frame: 1,
            marker: marker(1),
        })
        .is_ok()
    );
    assert!(take.send(Record::Frame(Box::new(frame(1)))).is_ok());
    take.finish();
    assert!(take.send(Record::Frame(Box::new(frame(2)))).is_err());
    let completion = await_completion(&mut take);
    assert_eq!(completion.error, None);
    assert_eq!(take.host(), HostState::Closed);
    assert_eq!(take.lines(), 3);

    let (start, stop) = host.join().unwrap();
    assert_eq!(
        start,
        json!({
            "protocol": "jc3vr-capture",
            "version": 1,
            "type": "start",
            "take": "2026-11-01_10-00-00_000",
            "folder": "/home/user/sessions/s/recordings/2026-11-01_10-00-00_000",
            "window": {"title": "Just Cause 3", "client_size": [1280, 720]},
        })
    );
    assert_eq!(stop, json!({"type": "stop"}));
    let sidecar = fs::read_to_string(folder.0.join("frames.jsonl")).unwrap();
    let frames: Vec<Value> = sidecar
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0]["frame"], 0);
    assert_eq!(frames[1]["marker"]["index"], 1);
    assert_eq!(frames[2]["frame"], 1);
}

#[test]
fn a_host_that_refuses_the_take_ends_the_video_with_its_message() {
    let folder = TempFolder::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().to_string();
    let host = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        read_json(&mut reader);
        stream
            .write_all(b"{\"type\":\"error\",\"message\":\"capture: the game window: none\"}\n")
            .unwrap();
    });
    let mut take = Take::spawn(request(&folder, target)).unwrap();
    await_host(&take, |state| *state == HostState::Closed);
    assert_eq!(
        take.error().as_deref(),
        Some("capture: the game window: none")
    );
    // The sidecar still takes records after the host refused the video.
    assert!(take.send(Record::Frame(Box::new(frame(0)))).is_ok());
    take.finish();
    let completion = await_completion(&mut take);
    assert_eq!(
        completion.error.as_deref(),
        Some("capture: the game window: none")
    );
    assert_eq!(take.lines(), 1);
    host.join().unwrap();
}

#[test]
fn a_streamed_take_connects_once_the_format_is_known_and_its_end_is_the_stop() {
    let folder = TempFolder::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().to_string();
    let format = VideoFormat {
        layout: StreamLayout::new(MIN_EYE_WIDTH, MIN_EYE_WIDTH, 0, Arrangement::SideBySide)
            .expect("a layout"),
        frame_rate: 30,
    };
    let frame_len = format.layout.frame_len();
    let host = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let start = read_json(&mut reader);
        let mut stream = stream;
        stream
            .write_all(
                b"{\"type\":\"started\",\"file\":\"C:\\\\s\\\\video.mkv\",\
                  \"encoder\":\"libopenh264\",\"audio\":\"none\"}\n",
            )
            .unwrap();
        // Everything after the start line is the stream, up to its end.
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let mut frames = 0;
        let mut tag = [0u8; FRAME_TAG.len()];
        let mut bytes = vec![0u8; frame_len];
        while reader.read_exact(&mut tag).is_ok() {
            reader.read_exact(&mut bytes).unwrap();
            frames += 1;
        }
        stream
            .write_all(b"{\"type\":\"stopped\",\"exit_code\":0}\n")
            .unwrap();
        (start, header, frames)
    });

    let mut take = Take::spawn(TakeRequest {
        stream: true,
        ..request(&folder, target)
    })
    .unwrap();
    // No format yet, so no connection.
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(take.host(), HostState::Waiting);
    let video = Arc::clone(take.video().expect("a streamed take has a video channel"));
    video.publish(format);
    await_host(&take, |state| matches!(state, HostState::Capturing(_)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !video.streaming() {
        assert!(Instant::now() < deadline, "the writer never streamed");
        std::thread::sleep(Duration::from_millis(5));
    }
    for i in 0..3 {
        assert!(
            video
                .send(VideoFrame {
                    repeats: u32::from(i > 0),
                    image: Some((vec![i as u8; frame_len], i)),
                })
                .is_ok()
        );
    }
    take.finish();
    let completion = await_completion(&mut take);
    assert_eq!(completion.error, None);

    let (start, header, frames) = host.join().unwrap();
    assert_eq!(start["version"], 2);
    assert_eq!(start["payload_folder"], folder.0.display().to_string());
    assert_eq!(start["video"]["width"], format.layout.width);
    assert_eq!(header, stream_header(&format));
    assert_eq!(frames, 5);
    let lines = fs::read_to_string(folder.0.join("frame_map.jsonl")).unwrap();
    assert_eq!(lines.lines().count(), 5);
}

#[test]
fn a_streamed_take_stopped_before_started_sends_eof_and_waits_for_stopped() {
    let folder = TempFolder::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = listener.local_addr().unwrap().to_string();
    let host = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(read_json(&mut reader)["version"], 2);
        let mut video = Vec::new();
        reader.read_to_end(&mut video).unwrap();
        assert!(video.is_empty());
        stream
            .write_all(b"{\"type\":\"started\",\"file\":\"video.mkv\"}\n")
            .unwrap();
        stream
            .write_all(b"{\"type\":\"stopped\",\"exit_code\":0}\n")
            .unwrap();
    });
    let mut take = Take::spawn(TakeRequest {
        stream: true,
        ..request(&folder, target)
    })
    .unwrap();
    take.video().unwrap().publish(VideoFormat {
        layout: StreamLayout::new(MIN_EYE_WIDTH, MIN_EYE_WIDTH, 0, Arrangement::SideBySide)
            .unwrap(),
        frame_rate: 30,
    });
    await_host(&take, |state| *state == HostState::Starting);
    take.finish();
    let completion = await_completion(&mut take);
    assert_eq!(
        completion.error.as_deref(),
        Some("recorder: no frame reached the capture host before the take ended")
    );
    assert_eq!(take.host(), HostState::Closed);
    host.join().unwrap();
}

#[test]
fn a_streamed_take_without_eye_views_says_why_it_has_no_video() {
    let folder = TempFolder::new();
    let target = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    let mut take = Take::spawn(TakeRequest {
        stream: true,
        ..request(&folder, target)
    })
    .unwrap();
    take.finish();
    let error = await_completion(&mut take).error.unwrap();
    assert!(error.contains("no eye views were captured"), "{error}");
}

#[test_case::test_case(true; "disconnected")]
#[test_case::test_case(false; "deadline expired")]
fn finalization_without_a_stop_acknowledgement_is_an_error(disconnect: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (peer, _) = listener.accept().unwrap();
    stream.set_nonblocking(true).unwrap();
    let mut peer = Some(peer);
    if disconnect {
        peer.take();
    }
    let shared = Shared {
        host: Mutex::new(HostState::Finishing),
        error: Mutex::new(None),
        lines: AtomicU32::new(0),
    };
    let mut confirmed = true;
    await_stopped(
        &mut stream,
        &mut LineBuffer::default(),
        &mut confirmed,
        Duration::from_millis(50),
        &shared,
    );
    let error = shared.error.lock().clone().unwrap();
    assert!(error.starts_with("capture host:"), "{error}");
    assert!(
        error.contains(if disconnect {
            "closed without confirming"
        } else {
            "not confirmed within"
        }),
        "{error}"
    );
    drop(peer);
}

#[test]
fn a_take_with_no_host_listening_says_it_has_no_video() {
    let folder = TempFolder::new();
    let target = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    let mut take = Take::spawn(request(&folder, target)).unwrap();
    assert_eq!(take.host(), HostState::Waiting);
    take.finish();
    let completion = await_completion(&mut take);
    let error = completion.error.unwrap();
    assert!(
        error.starts_with("capture host: no host listened at 127.0.0.1:"),
        "{error}"
    );
    assert!(error.ends_with("so the take has no video"), "{error}");
}

fn request(folder: &TempFolder, target: String) -> TakeRequest {
    TakeRequest {
        stamp: "2026-11-01_10-00-00_000".to_owned(),
        folder: folder.0.clone(),
        host_folder: Some("/home/user/sessions/s/recordings/2026-11-01_10-00-00_000".to_owned()),
        target,
        window: WindowHint {
            title: Some("Just Cause 3".to_owned()),
            client_size: Some([1280, 720]),
        },
        stream: false,
    }
}

fn frame(index: u32) -> FrameRecord {
    let eye = CaptureEyeView {
        position: [0.0, 1.5, 0.0],
        orientation: [0.0, 0.0, 0.0, 1.0],
        fov: [-1.0, 0.75, 0.5, -1.0],
    };
    FrameRecord {
        frame: index,
        unix_ms: 1_700_000_000_000 + i64::from(index),
        game_run_time_us: Some(u64::from(index) * 1_000),
        views: Some(CaptureViews {
            predicted_display_time_ns: i64::from(index) * 10,
            eyes: [eye, eye],
        }),
    }
}

fn marker(index: u32) -> Marker {
    Marker {
        index,
        stamp: format!("stamp-{index}"),
        unix_ms: i64::from(index),
        game_run_time_us: None,
        label: "take".to_owned(),
    }
}

fn read_json(reader: &mut impl BufRead) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn await_host(take: &Take, done: impl Fn(&HostState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(&take.host()) {
        assert!(Instant::now() < deadline, "host state: {:?}", take.host());
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn await_completion(take: &mut Take) -> Completion {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(completion) = take.poll_completion() {
            return completion;
        }
        assert!(Instant::now() < deadline, "the take thread did not exit");
        std::thread::sleep(Duration::from_millis(5));
    }
}

struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jc3vr-take-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
