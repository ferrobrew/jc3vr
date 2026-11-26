use std::{
    io::{BufRead, BufReader, Read},
    net::TcpListener,
};

use pretty_assertions::assert_eq;
use serde_json::{Value, json};

use super::*;
use crate::recorder::layout::{Arrangement, MIN_EYE_WIDTH, StreamLayout};

/// A sidecar the test can read back after the writer has finished with it.
#[derive(Clone, Default)]
struct SharedSink(Arc<Mutex<Vec<u8>>>);

impl Write for SharedSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedSink {
    fn lines(&self) -> Vec<Value> {
        String::from_utf8(self.0.lock().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn small_format() -> VideoFormat {
    VideoFormat {
        layout: StreamLayout::new(MIN_EYE_WIDTH, MIN_EYE_WIDTH, 0, Arrangement::SideBySide)
            .expect("a layout"),
        frame_rate: 30,
    }
}

fn image(format: &VideoFormat, fill: u8) -> Vec<u8> {
    vec![fill; format.layout.frame_len()]
}

/// Connect a loopback pair: the writer's end, non-blocking as the take's thread leaves it, and the
/// host's end.
fn connection() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client.set_nonblocking(true).unwrap();
    let (host, _) = listener.accept().unwrap();
    (client, host)
}

/// Read the stream the host receives: its header line, then each frame's first byte.
fn read_stream(host: TcpStream, frame_len: usize) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(host);
    let mut header = String::new();
    reader.read_line(&mut header).unwrap();
    let mut fills = Vec::new();
    let mut tag = [0u8; FRAME_TAG.len()];
    let mut frame = vec![0u8; frame_len];
    while reader.read_exact(&mut tag).is_ok() {
        assert_eq!(&tag, FRAME_TAG);
        reader.read_exact(&mut frame).unwrap();
        assert!(frame.iter().all(|&b| b == frame[0]));
        fills.push(frame[0]);
    }
    (header, fills)
}

fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn repeats_resend_the_previous_image_and_the_sidecar_names_each_frames_source() {
    let format = small_format();
    let sink = SharedSink::default();
    let mut writer = Writer::spawn_with(sink.clone()).unwrap();
    let channel = Arc::clone(writer.channel());
    channel.publish(format);
    let (client, host) = connection();
    let frame_len = format.layout.frame_len();
    let reader = std::thread::spawn(move || read_stream(host, frame_len));
    writer.connect(client);
    wait_until(|| channel.streaming());

    // Nothing to repeat yet: the stream starts at the first fresh image.
    for frame in [
        VideoFrame {
            repeats: 2,
            image: None,
        },
        VideoFrame {
            repeats: 0,
            image: Some((image(&format, 10), 3)),
        },
        VideoFrame {
            repeats: 2,
            image: None,
        },
        VideoFrame {
            repeats: 1,
            image: Some((image(&format, 20), 8)),
        },
    ] {
        assert!(channel.send(frame).is_ok());
        wait_until(|| channel.stats().queued.load(Ordering::Relaxed) == 0);
    }
    assert!(writer.finish(Duration::from_secs(5)));
    assert_eq!(writer.error(), None);

    let (header, fills) = reader.join().unwrap();
    assert_eq!(header, stream_header(&format));
    assert_eq!(fills, [10, 10, 10, 10, 20]);
    assert_eq!(
        sink.lines(),
        [
            json!({"stream_frame": 0, "frame": 3, "repeat": false}),
            json!({"stream_frame": 1, "frame": 3, "repeat": true}),
            json!({"stream_frame": 2, "frame": 3, "repeat": true}),
            json!({"stream_frame": 3, "frame": 3, "repeat": true}),
            json!({"stream_frame": 4, "frame": 8, "repeat": false}),
        ]
    );
    let stats = channel.stats();
    assert_eq!(stats.written.load(Ordering::Relaxed), 5);
    assert_eq!(stats.repeated.load(Ordering::Relaxed), 3);
    assert!(writer.wrote_frames());
}

#[test]
fn frames_before_the_connection_are_dropped_and_counted() {
    let format = small_format();
    let mut writer = Writer::spawn_with(SharedSink::default()).unwrap();
    let channel = Arc::clone(writer.channel());
    channel.publish(format);
    assert!(!channel.streaming());
    assert!(
        channel
            .send(VideoFrame {
                repeats: 0,
                image: Some((image(&format, 1), 0)),
            })
            .is_ok()
    );
    wait_until(|| channel.stats().dropped.load(Ordering::Relaxed) == 1);
    assert!(writer.finish(Duration::from_secs(5)));
    assert!(!writer.wrote_frames());
    assert!(matches!(
        channel.send(VideoFrame {
            repeats: 0,
            image: None
        }),
        Err(QueueError::Closed)
    ));
}

#[test]
fn a_full_queue_hands_the_frame_back() {
    let format = small_format();
    let mut writer = Writer::spawn_with(SharedSink::default()).unwrap();
    let channel = Arc::clone(writer.channel());
    channel.publish(format);
    // The writer is not connected, so it drains and drops; flood faster than that until refused.
    let mut refused = None;
    for i in 0..10_000u32 {
        match channel.send(VideoFrame {
            repeats: 0,
            image: Some((image(&format, 0), i)),
        }) {
            Ok(()) => {}
            Err(QueueError::Full(frame)) => {
                refused = Some(frame);
                break;
            }
            Err(QueueError::Closed) => panic!("the queue closed"),
        }
    }
    let frame = refused.expect("the queue fills");
    assert_eq!(
        frame.image.map(|(buffer, _)| buffer.len()),
        Some(format.layout.frame_len())
    );
    assert!(writer.finish(Duration::from_secs(5)));
}

#[test]
fn a_host_that_closes_ends_the_stream_with_an_error() {
    let format = small_format();
    let mut writer = Writer::spawn_with(SharedSink::default()).unwrap();
    let channel = Arc::clone(writer.channel());
    channel.publish(format);
    let (client, host) = connection();
    drop(host);
    writer.connect(client);
    wait_until(|| channel.streaming());
    for i in 0..64 {
        let _ = channel.send(VideoFrame {
            repeats: 0,
            image: Some((image(&format, 0), i)),
        });
        if writer.error().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    wait_until(|| writer.error().is_some());
    assert!(
        writer
            .error()
            .unwrap()
            .starts_with("capture host: could not send"),
        "{:?}",
        writer.error()
    );
    assert!(writer.finish(Duration::from_secs(5)));
}
