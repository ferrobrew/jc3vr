use std::{
    cell::Cell,
    fs::{self, File},
    io::{self, Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

use super::*;

#[test]
fn exhausted_first_address_rotates_to_later_success_on_next_bounded_retry() {
    let addresses = [
        "127.0.0.1:1".parse().unwrap(),
        "127.0.0.1:2".parse().unwrap(),
    ];
    let mut next = 0;
    let budget = Cell::new(CONNECT_TIMEOUT);
    let mut attempted = Vec::new();
    let result: Option<()> = connect_resolved(
        &addresses,
        &mut next,
        || budget.get(),
        |address, timeout| {
            attempted.push(*address);
            assert_eq!(timeout, CONNECT_TIMEOUT);
            budget.set(Duration::ZERO);
            None
        },
    );
    assert!(result.is_none());
    assert_eq!(attempted, [addresses[0]]);
    assert_eq!(next, 1);
    budget.set(CONNECT_TIMEOUT);
    let result = connect_resolved(
        &addresses,
        &mut next,
        || budget.get(),
        |address, _| {
            attempted.push(*address);
            Some("connected")
        },
    );
    assert_eq!(result, Some("connected"));
    assert_eq!(attempted, addresses);
    assert_eq!(next, 0);
    assert!(
        connect_resolved::<()>(
            &[],
            &mut next,
            || CONNECT_TIMEOUT,
            |_, _| panic!("no addresses")
        )
        .is_none()
    );
    assert!(
        connect_resolved::<()>(
            &addresses,
            &mut next,
            || Duration::ZERO,
            |_, _| panic!("expired budget")
        )
        .is_none()
    );
}

#[test]
fn pending_poll_retains_handle_and_completion_is_consumed_once() {
    let (release, wait) = mpsc::sync_channel(1);
    let mut writer = Writer::spawn_controlled(move |_| {
        let _ = wait.recv();
        Ok(())
    });
    writer.finish();
    assert!(writer.poll_completion().is_none());
    assert!(writer.handle.is_some());
    release.send(()).unwrap();
    assert!(await_completion(&mut writer).error.is_none());
    assert!(writer.handle.is_none());
    assert!(writer.poll_completion().is_none());
}

#[test]
fn finish_drains_queued_frames_with_original_stream_and_sidecar_order() {
    let folder = TempFolder::new();
    let path = folder.0.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let (entered, ready) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let mut writer = Writer::spawn_thread(move |receiver, pool, shared| {
        let mut session = WriterThread {
            target: "local test".into(),
            frame_rate: 72,
            sidecar: std::io::BufWriter::new(File::create(path.join("frames.jsonl")).unwrap()),
            folder: path,
            stream: Some(stream),
            next_address: 0,
            header_written: false,
            last_fresh: None,
            stream_frame: 0,
            dropped_before_connect: 0,
            pool,
            shared,
        };
        entered.send(()).unwrap();
        let _ = wait.recv();
        session.run(receiver);
    })
    .unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let metadata = |frame| FrameMetadata {
        frame,
        unix_ms: 1000 + i64::from(frame),
        game_run_time_us: Some(2000 + u64::from(frame)),
        eye: 1,
        width: 2,
        height: 2,
        views: None,
    };
    assert!(matches!(
        writer.send(Frame::Fresh(vec![1; 6], metadata(0))),
        SendOutcome::Queued
    ));
    assert!(matches!(
        writer.send(Frame::Duplicate(metadata(1))),
        SendOutcome::Queued
    ));
    assert!(matches!(
        writer.send(Frame::Fresh(vec![2; 6], metadata(2))),
        SendOutcome::Queued
    ));
    writer.finish();
    assert!(matches!(
        writer.send(Frame::Duplicate(metadata(3))),
        SendOutcome::Failed(_)
    ));
    assert!(writer.poll_completion().is_none());
    release.send(()).unwrap();
    assert!(await_completion(&mut writer).error.is_none());
    let bytes = reader.join().unwrap();
    let mut expected =
        b"YUV4MPEG2 W2 H2 F72:1 Ip A1:1 C420jpeg XYSCSS=420JPEG XCOLORRANGE=LIMITED\n".to_vec();
    for value in [1, 1, 2] {
        expected.extend_from_slice(b"FRAME\n");
        expected.extend_from_slice(&[value; 6]);
    }
    assert_eq!(bytes, expected);
    let sidecar = fs::read_to_string(folder.0.join("frames.jsonl")).unwrap();
    let lines: Vec<serde_json::Value> = sidecar
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line["stream_frame"], index);
        assert_eq!(line["capture_frame"], index);
        assert_eq!(line["duplicate"], index == 1);
        assert_eq!(line["unix_ms"], 1000 + index);
        assert_eq!(line["game_run_time_us"], 2000 + index);
        assert_eq!(line["eye"], 1);
    }
    let info: serde_json::Value =
        serde_json::from_slice(&fs::read(folder.0.join("stream.json")).unwrap()).unwrap();
    assert_eq!(info["width"], 2);
    assert_eq!(info["height"], 2);
    assert_eq!(info["frame_rate"], 72);
}

#[test]
fn network_write_checks_stop_and_frame_deadlines_between_partial_writes() {
    let shared = new_shared();
    let mut sink = StopAfterWrite {
        shared: &shared,
        writes: 0,
    };
    let error = write_network(
        &mut sink,
        &[1, 2],
        &shared,
        Instant::now() + FRAME_WRITE_BUDGET,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(sink.writes, 1);

    let shared = new_shared();
    let mut bytes = Vec::new();
    let error = write_network(&mut bytes, &[1], &shared, Instant::now()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(bytes.is_empty());
}

#[test]
fn network_write_preserves_partial_progress_and_reports_io_failure() {
    let shared = new_shared();
    let mut sink = PartialSink {
        bytes: Vec::new(),
        interrupted: false,
    };
    write_network(
        &mut sink,
        &[1, 2, 3],
        &shared,
        Instant::now() + FRAME_WRITE_BUDGET,
    )
    .unwrap();
    assert_eq!(sink.bytes, [1, 2, 3]);
    let mut sink = io::sink();
    *shared.finish_deadline.lock() = Some(Instant::now());
    assert_eq!(
        write_network(
            &mut sink,
            &[1],
            &shared,
            Instant::now() + FRAME_WRITE_BUDGET
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
    let shared = new_shared();
    assert_eq!(
        write_network(
            &mut BrokenSink,
            &[1],
            &shared,
            Instant::now() + FRAME_WRITE_BUDGET
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        write_network(
            &mut &mut [0u8; 0][..],
            &[1],
            &shared,
            Instant::now() + FRAME_WRITE_BUDGET
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(
        write_network(
            &mut TimedOutSink,
            &[1],
            &shared,
            Instant::now() + FRAME_WRITE_BUDGET
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::TimedOut
    );
}

fn new_shared() -> Shared {
    Shared {
        connected: AtomicBool::new(false),
        error: Mutex::new(None),
        finish_deadline: Mutex::new(None),
    }
}

fn await_completion(writer: &mut Writer) -> Completion {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(completion) = writer.poll_completion() {
            return completion;
        }
        assert!(Instant::now() < deadline, "video writer did not exit");
        std::thread::yield_now();
    }
}

struct StopAfterWrite<'a> {
    shared: &'a Shared,
    writes: usize,
}

impl Write for StopAfterWrite<'_> {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        *self.shared.finish_deadline.lock() = Some(Instant::now());
        Ok(1)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PartialSink {
    bytes: Vec<u8>,
    interrupted: bool,
}

impl Write for PartialSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.bytes.push(bytes[0]);
        Ok(1)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct BrokenSink;

impl Write for BrokenSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct TimedOutSink;

impl Write for TimedOutSink {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::TimedOut.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct TempFolder(PathBuf);

impl TempFolder {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jc3vr-video-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, AtomicOrdering::Relaxed)
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
