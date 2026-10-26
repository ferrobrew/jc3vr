use tracking_format::{Header, StartMarker, model::Hand};

use super::*;
use crate::{
    controllers::frame::{ActionSetKind, HandFrame, HandJoint},
    recorder::tracking::{
        encode_capture,
        format::{ControllerSample, Frame, SessionSample},
    },
    vr::stage::StageInfo,
};

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

fn header() -> Header {
    Header {
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
    }
}

fn preamble() -> Vec<u8> {
    tracking_format::encode_latest_chunked_preamble(&header())
}

fn writer() -> (Writer, Receiver<Vec<u8>>) {
    let (sender, receiver) = mpsc::sync_channel(1);
    (
        Writer {
            sender: Some(sender),
            handle: None,
            pending_loss: None,
            shared: Arc::new(Shared::new(0, RecordingFormat::ChunkedV4)),
        },
        receiver,
    )
}

fn batch(time: u64, groups: u64) -> CaptureBatch {
    CaptureBatch {
        bytes: Vec::new(),
        capture_time_ns: time,
        input_records: groups,
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
    let frame = Frame {
        time_ns: time,
        display_time_ns: 90,
        session: SessionSample {
            state: None,
            should_render: true,
            head_tracked: false,
        },
        head: None,
        stage: StageInfo::default(),
        controllers: Some(ControllerSample {
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: std::array::from_fn(|_| HandFrame {
                joints: Some(std::array::from_fn(|i| HandJoint {
                    position_valid: i & 1 != 0,
                    orientation_valid: i & 2 != 0,
                    position_tracked: i & 4 != 0,
                    orientation_tracked: i & 8 != 0,
                    ..HandJoint::default()
                })),
                ..HandFrame::default()
            }),
        }),
        semantic: None,
        recenters: Vec::new(),
    };
    encode_capture(&frame, &[record]).unwrap()
}

#[test]
fn format_defaults_to_chunked_v4_and_false_selects_uncompressed_v4() {
    assert_eq!(RecordingFormat::default(), RecordingFormat::ChunkedV4);
    assert_eq!(
        RecordingFormat::from_compression_setting(false),
        RecordingFormat::UncompressedV4
    );
    assert_eq!(
        RecordingFormat::from_compression_setting(true),
        RecordingFormat::ChunkedV4
    );
    let payload_header = crate::recorder::tracking::format::Header {
        build: "fixture".into(),
        recorded_at: "fixture".into(),
        action_schema: 4,
        start: crate::recorder::tracking::format::StartMarker {
            transform: [0.0; 16],
            bookmark: None,
        },
        dominant_hand: crate::controllers::frame::Hand::Right,
        world_scale: 1.0,
        time_scale: 1.0,
    };
    for (format, version) in [
        (RecordingFormat::UncompressedV4, 4),
        (RecordingFormat::ChunkedV4, 4),
    ] {
        let bytes = format.preamble(&payload_header);
        assert_eq!(tracking_format::decode(&bytes).unwrap().version, version);
    }
}

#[test]
fn both_formats_preserve_batches_final_loss_and_preamble_counters() {
    for (format, version) in [
        (RecordingFormat::UncompressedV4, 4),
        (RecordingFormat::ChunkedV4, 4),
    ] {
        let (mut writer, receiver) = writer();
        let preamble = match format {
            RecordingFormat::UncompressedV4 => tracking_format::encode_latest_preamble(&header()),
            RecordingFormat::ChunkedV4 => preamble(),
        };
        let preamble_len = preamble.len();
        writer.shared = Arc::new(Shared::new(preamble_len as u64, format));
        let first = input_batch(10).bytes;
        assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
        assert!(matches!(writer.send(batch(20, 7)), SendOutcome::Full));
        writer.finish();
        let bytes = Arc::new(Mutex::new(preamble.clone()));
        let sink = TestSink {
            bytes: Arc::clone(&bytes),
            fault: Fault::None,
            writes: 0,
        };
        run(sink, &receiver, &writer.shared).unwrap();
        let final_loss = encode_loss(Loss {
            time_ns: 20,
            groups: 7,
        });
        let mut expected = preamble;
        match format {
            RecordingFormat::UncompressedV4 => {
                expected.extend(&first);
                expected.extend(&final_loss);
            }
            RecordingFormat::ChunkedV4 => {
                expected.extend(tracking_format::encode_chunk(&first).unwrap());
                expected.extend(tracking_format::encode_chunk(&final_loss).unwrap());
            }
        }
        assert_eq!(*bytes.lock(), expected);
        assert_eq!(writer.bytes_written(), expected.len() as u64);
        assert_eq!(
            writer.logical_bytes_written(),
            (preamble_len + first.len() + final_loss.len()) as u64
        );
        let decoded = tracking_format::decode(&bytes.lock()).unwrap();
        assert_eq!(decoded.version, version);
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!(decoded.input.len(), 2);
        assert_eq!(
            decoded.input[1].record,
            InputRecord::Discontinuity {
                reason: DiscontinuityReason::Overflow,
                discarded: 7
            }
        );
    }
}

#[test]
fn both_formats_run_through_the_started_writer_thread() {
    for (format, version) in [
        (RecordingFormat::UncompressedV4, 4),
        (RecordingFormat::ChunkedV4, 4),
    ] {
        let preamble = match format {
            RecordingFormat::UncompressedV4 => tracking_format::encode_latest_preamble(&header()),
            RecordingFormat::ChunkedV4 => preamble(),
        };
        let bytes = Arc::new(Mutex::new(preamble.clone()));
        let sink = TestSink {
            bytes: Arc::clone(&bytes),
            fault: Fault::None,
            writes: 0,
        };
        let mut writer = Writer::spawn_sink(sink, preamble.len() as u64, format).unwrap();
        let original = input_batch(10).bytes;
        assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
        writer.finish();
        let owner = writer.handle.take().unwrap();
        let Owner::Thread(handle) = owner else {
            panic!("expected the real writer thread");
        };
        let result = completion(handle.join(), writer.bytes_written());
        assert!(result.error.is_none(), "{:?}", result.error);
        let decoded = tracking_format::decode(&bytes.lock()).unwrap();
        assert_eq!(decoded.version, version);
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!(decoded.input.len(), 1);
        let payload = format::decode(&bytes.lock()).unwrap();
        assert_eq!(
            encode_capture(&payload.frames[0], &payload.input)
                .unwrap()
                .bytes,
            original
        );
        assert_eq!(result.bytes, bytes.lock().len() as u64);
        assert_eq!(
            writer.logical_bytes_written(),
            (preamble.len() + original.len()) as u64
        );
    }
}

#[test]
fn both_formats_count_partial_storage_without_completing_logical_batch() {
    for format in [RecordingFormat::UncompressedV4, RecordingFormat::ChunkedV4] {
        let (mut writer, receiver) = writer();
        writer.shared = Arc::new(Shared::new(0, format));
        let first = input_batch(10).bytes;
        assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
        writer.finish();
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = TestSink {
            bytes: Arc::clone(&bytes),
            fault: Fault::PartialWrite,
            writes: 0,
        };
        assert!(run(sink, &receiver, &writer.shared).is_err());
        assert_eq!(writer.bytes_written(), 1);
        assert_eq!(writer.logical_bytes_written(), 0);
        let expected = if format == RecordingFormat::UncompressedV4 {
            first[0]
        } else {
            tracking_format::TAG_CHUNK
        };
        assert_eq!(*bytes.lock(), [expected]);
    }
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
    assert_eq!(
        writer.logical_bytes_written(),
        (input_batch(10).bytes.len()
            + encode_loss(Loss {
                time_ns: 20,
                groups: 4
            })
            .len()) as u64
    );
}

#[test]
fn policy_only_and_mixed_writer_loss_counts_actual_records() {
    for mixed in [false, true] {
        let policy = tracking_format::input::policy::PolicyEvaluation {
            version: tracking_format::input::policy::POLICY_A_VERSION,
            stamp: tracking_format::input::InputStamp {
                epoch: 1,
                clock: tracking_format::input::ClockDomain::Synthetic,
                time_ns: 1,
                sync: 1,
                sequence: 1,
            },
            action_schema: 4,
            timing: None,
            context: 0,
            gameplay: false,
            outcome: tracking_format::input::policy::PolicyOutcome::Cancelled,
            neutral: false,
            singles: [tracking_format::input::policy::AcceptedSingle::None; 3],
            single_down: [false; 3],
            withheld: [true; 3],
            command: tracking_format::input::policy::AcceptedCommand::None,
            raw: [0; 3],
            direct: Default::default(),
            claims: 0,
            commands: 0,
        };
        let mut records = vec![InputRecord::Policy(policy)];
        if mixed {
            records.push(InputRecord::Group(tracking_format::input::InputGroup {
                stamp: policy.stamp,
                order: tracking_format::input::GroupOrder::UnorderedSync,
                source_generation: 1,
                binding_generation: 1,
                action_schema: 4,
                context: 0,
                focused: true,
                samples: Vec::new(),
            }));
        }
        let encoded = records
            .iter()
            .flat_map(|record| {
                tracking_format::encode_input(&CapturedInput {
                    capture_time_ns: 20,
                    frame_sequence: None,
                    record: record.clone(),
                })
                .unwrap()
            })
            .collect::<Vec<_>>();
        let count = records.len() as u64;
        let (mut writer, receiver) = writer();
        assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
        assert!(matches!(
            writer.send(CaptureBatch {
                bytes: encoded,
                capture_time_ns: 20,
                input_records: count
            }),
            SendOutcome::Full
        ));
        writer.finish();
        let bytes = Arc::new(Mutex::new(preamble()));
        run(
            TestSink {
                bytes: Arc::clone(&bytes),
                fault: Fault::None,
                writes: 0,
            },
            &receiver,
            &writer.shared,
        )
        .unwrap();
        let decoded = tracking_format::decode(&bytes.lock()).unwrap();
        assert!(
            matches!(decoded.input.last().map(|entry| &entry.record), Some(InputRecord::Discontinuity { discarded, .. }) if *discarded == count)
        );
    }
}

#[test]
fn actual_writer_chunks_recover_atomic_frame_input_boundaries_at_every_cut() {
    let (mut writer, receiver) = writer();
    assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
    let first = receiver.recv().unwrap();
    assert!(matches!(writer.send(input_batch(20)), SendOutcome::Queued));
    writer.finish();
    let bytes = Arc::new(Mutex::new(preamble()));
    let mut sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::None,
        writes: 0,
    };
    write_batch(&mut sink, &first, &writer.shared).unwrap();
    let boundary = bytes.lock().len();
    run(sink, &receiver, &writer.shared).unwrap();
    let complete = bytes.lock().clone();
    for end in preamble().len()..=complete.len() {
        let decoded = tracking_format::decode(&complete[..end]).unwrap();
        let count = usize::from(end >= boundary) + usize::from(end == complete.len());
        assert_eq!(decoded.frames.len(), count, "cut {end}");
        assert_eq!(decoded.input.len(), count, "cut {end}");
        assert_eq!(
            decoded.truncated,
            end != preamble().len() && end != boundary && end != complete.len()
        );
    }
    assert_eq!(
        writer.logical_bytes_written(),
        (first.len() + input_batch(20).bytes.len()) as u64
    );
    assert_eq!(
        writer.bytes_written(),
        (complete.len() - preamble().len()) as u64
    );
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
    write_batch(&mut sink, &first, &writer.shared).unwrap();
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
fn writer_codec_failure_keeps_prior_complete_chunks_and_exact_error() {
    let (mut writer, receiver) = writer();
    assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
    let first = receiver.recv().unwrap();
    assert!(matches!(
        writer.send(CaptureBatch {
            bytes: vec![1],
            ..batch(20, 0)
        }),
        SendOutcome::Queued
    ));
    writer.finish();
    let bytes = Arc::new(Mutex::new(preamble()));
    let mut sink = TestSink {
        bytes: Arc::clone(&bytes),
        fault: Fault::None,
        writes: 0,
    };
    write_batch(&mut sink, &first, &writer.shared).unwrap();
    let accepted = writer.bytes_written();
    let error = run(sink, &receiver, &writer.shared).unwrap_err();
    assert!(error.contains("record write"));
    assert_eq!(writer.error().as_deref(), Some(error.as_str()));
    assert_eq!(writer.bytes_written(), accepted);
    assert_eq!(writer.logical_bytes_written(), first.len() as u64);
    let decoded = tracking_format::decode(&bytes.lock()).unwrap();
    assert_eq!(decoded.frames.len(), 1);
    assert_eq!(decoded.input.len(), 1);
    assert!(!decoded.truncated);
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
        assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
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
            1 => tracking_format::encode_chunk(&input_batch(10).bytes)
                .unwrap()
                .len(),
            _ => {
                tracking_format::encode_chunk(&input_batch(10).bytes)
                    .unwrap()
                    .len()
                    + tracking_format::encode_chunk(&encode_loss(Loss {
                        time_ns: 20,
                        groups: 4,
                    }))
                    .unwrap()
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
    assert!(matches!(writer.send(input_batch(10)), SendOutcome::Queued));
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
    assert_eq!(writer.logical_bytes_written(), 0);
    assert_eq!(*bytes.lock(), [tracking_format::TAG_CHUNK]);
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
    let first = input_batch(10).bytes;
    let expected = tracking_format::encode_chunk(&first).unwrap();
    sender.send(first).unwrap();
    sender.send(input_batch(20).bytes).unwrap();
    drop(sender);
    let handle = std::thread::spawn(move || run(sink, &panic_receiver, &shared));
    let completion = completion(handle.join(), writer.bytes_written());
    drop(receiver);
    assert_eq!(writer.error(), None);
    assert_eq!(
        completion.error.as_deref(),
        Some("tracking writer: the writer thread panicked")
    );
    assert_eq!(completion.bytes, expected.len() as u64);
    assert_eq!(*bytes.lock(), expected);
    crate::recorder::tracking::assert_active_failure_transition(completion);
}
