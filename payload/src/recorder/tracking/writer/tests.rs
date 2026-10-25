use tracking_format::{Header, StartMarker, model::Hand};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    None,
    Write(usize),
    PartialWrite,
    Flush,
    Sync,
    Panic(usize),
}

struct TestSink {
    bytes: Arc<Mutex<Vec<u8>>>,
    fault: Fault,
    writes: usize,
}

impl Write for TestSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.writes += 1;
        assert_ne!(
            self.fault,
            Fault::Panic(self.writes),
            "injected writer panic"
        );
        if self.fault == Fault::Write(self.writes)
            || (self.fault == Fault::PartialWrite && self.writes > 1)
        {
            return Err(std::io::Error::other("injected write"));
        }
        if self.fault == Fault::PartialWrite {
            self.bytes.lock().push(bytes[0]);
            return Ok(1);
        }
        self.bytes.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if self.fault == Fault::Flush {
            Err(std::io::Error::other("injected flush"))
        } else {
            Ok(())
        }
    }
}

impl Sink for TestSink {
    fn sync(&mut self) -> std::io::Result<()> {
        if self.fault == Fault::Sync {
            Err(std::io::Error::other("injected sync"))
        } else {
            Ok(())
        }
    }
}

fn preamble() -> Vec<u8> {
    tracking_format::encode_input_preamble(&Header {
        build: "fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 4,
        start: StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    })
}

fn writer() -> (Writer, Receiver<Vec<u8>>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        Writer {
            sender: Some(sender),
            handle: None,
            pending_loss: None,
            shared: Arc::new(Shared::new(0)),
        },
        receiver,
    )
}

fn batch(time: u64, groups: u64) -> CaptureBatch {
    CaptureBatch {
        bytes: Vec::new(),
        capture_time_ns: time,
        input_groups: groups,
    }
}

fn input_batch(time: u64) -> CaptureBatch {
    let record = CapturedInput {
        capture_time_ns: time,
        frame_sequence: Some(time),
        record: InputRecord::Discontinuity {
            reason: DiscontinuityReason::Unavailable,
            discarded: 0,
        },
    };
    let frame = crate::recorder::tracking::format::Frame {
        time_ns: time,
        display_time_ns: 90,
        session: crate::recorder::tracking::format::SessionSample {
            state: None,
            should_render: true,
            head_tracked: false,
        },
        head: None,
        stage: crate::vr::stage::StageInfo::default(),
        controllers: None,
        semantic: None,
        recenters: Vec::new(),
    };
    crate::recorder::tracking::encode_capture(&frame, &[record]).unwrap()
}

#[test]
fn actual_drain_retains_atomic_batches_and_final_loss() {
    let (mut writer, receiver) = writer();
    assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
    assert!(matches!(writer.send(batch(20, 4)), SendOutcome::Full));
    writer.finish();
    let bytes = Arc::new(Mutex::new(preamble()));
    let sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::None,
        writes: 0,
    };
    run(sink, &receiver, &writer.shared).unwrap();
    let decoded = tracking_format::decode(&bytes.lock()).unwrap();
    assert_eq!(decoded.frames.len(), 1);
    assert_eq!(decoded.frames[0].time_ns, 10);
    assert_eq!(decoded.input.len(), 2);
    assert_eq!(decoded.input[0].frame_sequence, Some(10));
    assert_eq!(
        decoded.input[1].record,
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: 4
        }
    );
    assert_eq!(
        writer.bytes_written(),
        (bytes.lock().len() - preamble().len()) as u64
    );
    assert!(writer.shared.final_loss.lock().is_none());
}

#[test]
fn full_queue_recovery_prefixes_loss_once_and_drains_the_real_batches() {
    let (mut writer, receiver) = writer();
    assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
    assert!(matches!(writer.send(batch(20, 2)), SendOutcome::Full));
    assert!(matches!(writer.send(batch(30, 3)), SendOutcome::Full));
    let first = receiver.recv().unwrap();
    assert!(matches!(writer.send(input_batch(40)), SendOutcome::Queued));
    writer.finish();
    let bytes = Arc::new(Mutex::new(preamble()));
    let mut sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::None,
        writes: 0,
    };
    write_record(&mut sink, &first, &writer.shared).unwrap();
    run(sink, &receiver, &writer.shared).unwrap();
    let decoded = tracking_format::decode(&bytes.lock()).unwrap();
    assert_eq!(
        decoded
            .frames
            .iter()
            .map(|frame| frame.time_ns)
            .collect::<Vec<_>>(),
        [10, 40]
    );
    assert_eq!(decoded.input.len(), 3);
    assert_eq!(decoded.input[1].capture_time_ns, 20);
    assert_eq!(
        decoded.input[1].record,
        InputRecord::Discontinuity {
            reason: DiscontinuityReason::Overflow,
            discarded: 5
        }
    );
    assert_eq!(decoded.input[2].frame_sequence, Some(40));
    assert!(writer.pending_loss.is_none());
}

#[test]
fn stopped_writer_reports_accepted_write_marker_flush_and_sync_failures() {
    for (fault, expected, written) in [
        (Fault::Write(1), "record write", 0),
        (Fault::Write(2), "final input loss write", 1),
        (Fault::Flush, "flush", 2),
        (Fault::Sync, "sync", 2),
    ] {
        let (mut writer, receiver) = writer();
        assert!(matches!(
            writer.send(CaptureBatch {
                bytes: vec![7],
                ..batch(10, 0)
            }),
            SendOutcome::Queued
        ));
        assert!(matches!(writer.send(batch(20, 4)), SendOutcome::Full));
        writer.finish();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = TestSink {
            bytes: Arc::clone(&bytes),
            fault,
            writes: 0,
        };
        let error = run(sink, &receiver, &writer.shared).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(writer.error().as_deref(), Some(error.as_str()));
        let expected_bytes = match written {
            0 => 0,
            1 => 1,
            _ => {
                1 + encode_loss(Loss {
                    time_ns: 20,
                    groups: 4,
                })
                .len()
            }
        };
        assert_eq!(writer.bytes_written(), expected_bytes as u64);
        assert_eq!(bytes.lock().len(), expected_bytes);
        crate::recorder::tracking::assert_active_failure_transition(completion(
            Ok(Err(error)),
            writer.bytes_written(),
        ));
    }
}

#[test]
fn partial_accepted_write_is_counted_before_its_error() {
    let (mut writer, receiver) = writer();
    assert!(matches!(
        writer.send(CaptureBatch {
            bytes: vec![1, 2, 3],
            ..batch(10, 0)
        }),
        SendOutcome::Queued
    ));
    writer.finish();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::PartialWrite,
        writes: 0,
    };
    assert!(
        run(sink, &receiver, &writer.shared)
            .unwrap_err()
            .contains("record write")
    );
    assert_eq!(writer.bytes_written(), 1);
    assert_eq!(*bytes.lock(), [1]);
}

#[test]
fn active_sink_panic_preserves_exact_cause_and_bytes_through_fail_stop_reap() {
    let (mut writer, receiver) = writer();
    writer.finish();
    let shared = Arc::clone(&writer.shared);
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::Panic(2),
        writes: 0,
    };
    let (sender, panic_receiver) = mpsc::sync_channel(2);
    sender.send(vec![1, 2, 3]).unwrap();
    sender.send(vec![4]).unwrap();
    drop(sender);
    let handle = std::thread::spawn(move || run(sink, &panic_receiver, &shared));
    let completion = completion(handle.join(), writer.bytes_written());
    drop(receiver);
    assert_eq!(writer.error(), None);
    assert_eq!(
        completion.error.as_deref(),
        Some("tracking writer: the writer thread panicked")
    );
    assert_eq!(completion.bytes, 3);
    assert_eq!(*bytes.lock(), [1, 2, 3]);
    crate::recorder::tracking::assert_active_failure_transition(completion);
}
