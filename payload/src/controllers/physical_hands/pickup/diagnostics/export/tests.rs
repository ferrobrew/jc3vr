use std::{
    num::NonZeroUsize,
    sync::atomic::{AtomicU64, Ordering},
};

use glam::{Quat, Vec3};
use serde_json::Value;

use super::*;
use crate::controllers::{
    bindings::controls::ButtonControl,
    grab::{
        GrabVolume, PickupKind,
        latch::GripLatch,
        resolve::{Lost, Reach as ResolvedReach},
        target::Tier,
    },
    physical_hands::{
        bodies::{BodyError, BodyState, Unclassified},
        doors::acquisition::PanelRefusal,
        grasp::{Reach, TargetKind},
        pickup::{
            HeldReport,
            diagnostics::{
                ConsumerResult, InputConsumer, RequestKey,
                scan::{Branch, ScanResult},
            },
        },
        rigid::{BodyPose, Velocities},
    },
};

struct TempOutput(PathBuf);

impl TempOutput {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir();
        for _ in 0..128 {
            let path = base.join(format!(
                "jc3vr-grip-export-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("test output directory {}: {error}", path.display()),
            }
        }
        panic!("test output directory: candidate names exhausted");
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove temporary export output");
    }
}

fn target() -> Reach {
    Reach {
        object: NonZeroUsize::new(8192).unwrap(),
        body: 17,
        point: Vec3::new(1.0, 2.0, 3.0),
        distance: 0.125,
        normal: Vec3::Y,
        kind: TargetKind::Prop { mass_kg: 5.0 },
    }
}

fn snapshot() -> Status {
    let reach = target();
    let candidate = Candidate {
        resolved: ResolvedReach {
            index: 7,
            volume: GrabVolume::Pickup(PickupKind::Prop),
            distance: f32::INFINITY,
            tier: Tier::Target,
        },
        observed: Some(reach),
        lost: Some(Lost::Farther(0.25)),
    };
    let mut scan = Scan::new();
    scan.ray_ran = true;
    scan.handles_ran = true;
    scan.ray_selected = Some(reach);
    scan.handle_selected = None;
    scan.selected = Some(reach);
    let outcomes = [
        ScanResult::Unclassified(Unclassified::Body(BodyError::Reowned)),
        ScanResult::PanelRefused(PanelRefusal::HandleRequired),
        ScanResult::Offered,
    ];
    for i in 0..SCAN_CAPACITY + 2 {
        scan.push(ScanEntry {
            branch: [
                Branch::Ray,
                Branch::Press,
                Branch::DoorPanel,
                Branch::DoorHandle,
            ][i % 4],
            object: Some(reach.object),
            body: Some(reach.body),
            door: None,
            point: Some(reach.point),
            distance: Some(reach.distance),
            mass_kg: Some(5.0),
            result: outcomes[i % outcomes.len()],
        });
    }
    let input = InputRecord {
        scan: Some(scan),
        sequence: u64::MAX - 1,
        observation: 11,
        episode: 3,
        grip: ButtonControl {
            down: true,
            pressed: true,
            released: false,
        },
        latch: GripLatch::Open,
        grab: Some(GrabVolume::Pickup(PickupKind::Prop)),
        chosen: Some(candidate),
        candidates: [Some(candidate); CANDIDATE_CAPACITY],
        candidates_omitted: 3,
        consumer: InputConsumer::Queued,
    };
    let mut left = HandEvidence::new();
    left.latest = Some(InputRecord {
        observation: 12,
        ..input
    });
    left.last_press = Some(input);
    left.evicted = 5;
    for (slot, consumer) in [
        ConsumerResult::BodyAccepted { joined: false },
        ConsumerResult::BodyRefused(BodyError::NotDynamic),
        ConsumerResult::Replaced,
        ConsumerResult::Cleared,
    ]
    .into_iter()
    .enumerate()
    {
        left.requests[slot] = Some(RequestRecord {
            key: RequestKey {
                sequence: input.sequence,
                episode: input.episode,
                request: 29 + slot as u64,
                target: reach,
            },
            input,
            consumer,
            drive: Some(DriveRecord {
                object: reach.object.get(),
                body: reach.body,
                before: BodyState {
                    pose: BodyPose {
                        centre: Vec3::X,
                        orientation: Quat::IDENTITY,
                    },
                    velocities: Velocities {
                        linear: Vec3::Y,
                        angular: Vec3::Z,
                    },
                    mass_kg: Some(5.0),
                    inverse_inertia: Vec3::ONE,
                },
                commanded: Some(Velocities {
                    linear: Vec3::Z,
                    angular: Vec3::X,
                }),
                result: Some(Err(BodyError::Reowned)),
                report: Some(HeldReport {
                    mass_kg: 5.0,
                    liftable: false,
                    shared: true,
                    centre: Vec3::X,
                    anchor: Vec3::Y,
                    target: Vec3::Z,
                    lag_m: 0.5,
                    load: 0.75,
                    twist: 0.25,
                    joined: false,
                }),
            }),
        });
    }
    Status {
        hands: [left, HandEvidence::new()],
    }
}

fn document() -> Document {
    Document::new(
        snapshot(),
        Provenance {
            build_stamp: "fixture-build-unverified-live".to_owned(),
            package_version: "fixture-version".to_owned(),
            process_id: 123,
            session_output_root: "fixture-session".to_owned(),
            export_local_timestamp: "fixture-artifact-time-only".to_owned(),
        },
    )
}

fn read_document(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn write_fixture(directory: &Path) -> Result<PathBuf, SaveError> {
    write_document(directory, &document(), &AtomicU64::new(0))
}

#[test]
fn real_file_preserves_exact_input_and_request_identities_and_bounded_facts() {
    let output = TempOutput::new();
    let path = write_fixture(&output.0).unwrap();
    let value = read_document(&path);
    let copied = snapshot();
    assert_eq!(value["schema_version"], SCHEMA_VERSION);
    assert_eq!(value["artifact"], "grip-acquisition-evidence");
    assert_eq!(
        value["provenance"]["build_stamp"],
        "fixture-build-unverified-live"
    );
    assert_eq!(
        value["provenance"]["session_output_root"],
        "fixture-session"
    );
    assert_eq!(
        value["provenance"]["export_local_timestamp"],
        "fixture-artifact-time-only"
    );
    assert_eq!(value["candidate_capacity_per_input"], CANDIDATE_CAPACITY);
    assert_eq!(value["scan_capacity_per_input"], SCAN_CAPACITY);
    assert_eq!(value["request_capacity_per_hand"], REQUEST_CAPACITY);
    let hand = &value["hands"][0];
    assert_eq!(hand["hand"], "left");
    assert_eq!(hand["evicted"], 5);
    assert_eq!(hand["latest"]["sequence"].as_u64(), Some(u64::MAX - 1));
    assert_eq!(hand["latest"]["observation"], 12);
    assert_eq!(hand["last_press"]["observation"], 11);
    assert_eq!(hand["last_press"]["episode"], 3);
    let input = &hand["last_press"];
    assert_eq!(input["grip"]["pressed"], true);
    assert_eq!(input["candidates_omitted"], 3);
    assert_eq!(
        input["candidates"].as_array().unwrap().len(),
        CANDIDATE_CAPACITY
    );
    assert_eq!(input["chosen"]["resolver_index"], 7);
    assert_eq!(input["chosen"]["distance_debug"], "inf");
    assert_eq!(
        input["chosen"]["observed_debug"],
        format!("{:#?}", target())
    );
    assert_eq!(
        input["chosen"]["lost_debug"],
        format!("{:#?}", Lost::Farther(0.25))
    );
    let scan = copied.hands[0].last_press.unwrap().scan.unwrap();
    assert_eq!(input["scan"]["omitted"], 2);
    assert_eq!(
        input["scan"]["entries"].as_array().unwrap().len(),
        SCAN_CAPACITY
    );
    assert_eq!(input["scan"]["ray_ran"], true);
    assert_eq!(input["scan"]["handles_ran"], true);
    assert_eq!(
        input["scan"]["ray_selected_debug"],
        format!("{:#?}", target())
    );
    assert!(input["scan"]["handle_selected_debug"].is_null());
    assert_eq!(input["scan"]["selected_debug"], format!("{:#?}", target()));
    for (index, entry) in scan.entries.into_iter().flatten().enumerate() {
        let exported = &input["scan"]["entries"][index];
        assert_eq!(exported["branch_debug"], format!("{:?}", entry.branch));
        assert_eq!(exported["object"], entry.object.unwrap().get());
        assert_eq!(exported["body"], entry.body.unwrap());
        assert_eq!(
            exported["point_debug"],
            format!("{:#?}", entry.point.unwrap())
        );
        assert_eq!(exported["result_debug"], format!("{:#?}", entry.result));
    }
    for (index, request) in copied.hands[0].requests.into_iter().flatten().enumerate() {
        let exported = &hand["requests"][index];
        assert_eq!(exported["request"], request.key.request);
        assert_eq!(exported["sequence"].as_u64(), Some(request.key.sequence));
        assert_eq!(exported["episode"], request.key.episode);
        assert_eq!(exported["input"]["observation"], request.input.observation);
        assert_eq!(
            exported["target_debug"],
            format!("{:#?}", request.key.target)
        );
        assert_eq!(
            exported["consumer_debug"],
            format!("{:#?}", request.consumer)
        );
        let drive = request.drive.unwrap();
        assert_eq!(exported["drive"]["object"], drive.object);
        assert_eq!(exported["drive"]["body"], drive.body);
        assert_eq!(
            exported["drive"]["before_debug"],
            format!("{:#?}", drive.before)
        );
        assert_eq!(
            exported["drive"]["commanded_debug"],
            format!("{:#?}", drive.commanded.unwrap())
        );
        assert_eq!(
            exported["drive"]["result_debug"],
            format!("{:#?}", drive.result.unwrap())
        );
        assert_eq!(
            exported["drive"]["report_debug"],
            format!("{:#?}", drive.report.unwrap())
        );
    }
    assert_eq!(value["hands"][1]["hand"], "right");
    assert!(value["hands"][1]["latest"].is_null());
    assert!(value["hands"][1]["last_press"].is_null());
    assert!(
        value["hands"][1]["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(Value::is_null)
    );
}

#[test]
fn repeated_saves_and_existing_names_never_overwrite() {
    let output = TempOutput::new();
    let occupied = evidence_path(&output.0, 0);
    fs::write(&occupied, b"existing-evidence").unwrap();
    let next_file = AtomicU64::new(0);
    let first = write_document(&output.0, &document(), &next_file).unwrap();
    let original = fs::read(&first).unwrap();
    let second = write_document(&output.0, &document(), &next_file).unwrap();
    assert_eq!(first, evidence_path(&output.0, 1));
    assert_eq!(second, evidence_path(&output.0, 2));
    assert_eq!(fs::read(&occupied).unwrap(), b"existing-evidence");
    assert_eq!(fs::read(&first).unwrap(), original);
    assert_eq!(read_document(&first), read_document(&second));
}

#[test]
fn collisions_stop_at_the_bound_without_touching_files() {
    let output = TempOutput::new();
    for ordinal in 0..NAME_ATTEMPTS as u64 {
        fs::write(evidence_path(&output.0, ordinal), b"reserved").unwrap();
    }
    let error = write_fixture(&output.0).unwrap_err();
    assert!(
        matches!(&error, SaveError::NamesExhausted { path, attempts }
        if path == &output.0 && *attempts == NAME_ATTEMPTS)
    );
    assert!(error.to_string().contains(&output.0.display().to_string()));
    assert!(!evidence_path(&output.0, NAME_ATTEMPTS as u64).exists());
    for ordinal in 0..NAME_ATTEMPTS as u64 {
        assert_eq!(
            fs::read(evidence_path(&output.0, ordinal)).unwrap(),
            b"reserved"
        );
    }
}

#[test]
fn directory_error_keeps_the_path_and_cause_and_creates_no_capture() {
    let output = TempOutput::new();
    let blocked = output.0.join("not-a-directory");
    fs::write(&blocked, b"keep").unwrap();
    let error = write_fixture(&blocked).unwrap_err();
    assert!(matches!(&error, SaveError::Directory { path, .. } if path == &blocked));
    assert!(error.source().is_some());
    assert!(error.to_string().starts_with("grip acquisition evidence:"));
    assert!(error.to_string().contains(&blocked.display().to_string()));
    assert_eq!(fs::read(blocked).unwrap(), b"keep");
    assert_eq!(fs::read_dir(&output.0).unwrap().count(), 1);
}

#[test]
fn sequential_saves_continue_beyond_one_collision_window() {
    let output = TempOutput::new();
    let next_file = AtomicU64::new(0);
    let document = document();
    for ordinal in 0..(NAME_ATTEMPTS + 1) as u64 {
        let path = write_document(&output.0, &document, &next_file).unwrap();
        assert_eq!(path, evidence_path(&output.0, ordinal));
    }
    assert_eq!(fs::read_dir(&output.0).unwrap().count(), NAME_ATTEMPTS + 1);
}

#[test]
fn create_error_keeps_the_candidate_path_and_original_cause() {
    let output = TempOutput::new();
    let path = evidence_path(&output.0, 0);
    fs::create_dir(&path).unwrap();
    let source = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_err();
    let original_kind = source.kind();
    let error = SaveError::Create {
        path: path.clone(),
        source,
    };
    assert!(error.to_string().contains(&path.display().to_string()));
    assert!(error.source().is_some());
    assert!(matches!(error, SaveError::Create { source, .. } if source.kind() == original_kind));
    assert!(path.is_dir());
}

#[test]
fn write_error_keeps_cleanup_failure_and_both_path_contexts() {
    let output = TempOutput::new();
    let path = evidence_path(&output.0, 0);
    let error = SaveError::Write {
        path: path.clone(),
        operation: "sync",
        source: io::Error::other("fixture storage error"),
        cleanup: Some(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture cleanup error",
        )),
    };
    let message = error.to_string();
    assert!(message.contains("sync"));
    assert!(message.contains("fixture storage error"));
    assert!(message.contains("fixture cleanup error"));
    assert_eq!(message.matches(&path.display().to_string()).count(), 2);
    assert!(error.source().is_some());
}

#[test]
fn request_only_copies_and_rejects_a_second_pending_snapshot_under_ui_guards() {
    let output = TempOutput::new();
    let requests = SaveRequests::new();
    let owner = Mutex::new(());
    let caller = Mutex::new(());
    let first = snapshot();
    let mut second = first;
    second.hands.swap(0, 1);
    {
        let _owner = owner.lock();
        let _caller = caller.lock();
        assert_eq!(requests.request(first), RequestResult::Queued);
        assert_eq!(requests.request(second), RequestResult::AlreadyPending);
        assert_eq!(*requests.pending.lock(), Some(first));
        assert_eq!(fs::read_dir(&output.0).unwrap().count(), 0);
    }
    let mut serviced = false;
    requests.service_with(|copied| {
        assert_eq!(copied, first);
        {
            let pending = requests
                .pending
                .try_lock()
                .expect("pending lock released before save");
            assert!(pending.is_none(), "request taken before save");
        }
        assert!(
            owner.try_lock().is_some(),
            "UI owner guard released before save"
        );
        assert!(
            caller.try_lock().is_some(),
            "UI caller guard released before save"
        );
        let document = Document::new(copied, document().provenance);
        write_document(&output.0, &document, &AtomicU64::new(0)).unwrap();
        serviced = true;
    });
    assert!(serviced);
    assert_eq!(fs::read_dir(&output.0).unwrap().count(), 1);
    requests.service_with(|_| panic!("an empty slot must not call the writer"));
}

#[test]
fn post_ui_service_runs_after_the_actual_layout_and_egui_owner_scopes() {
    let requests = SaveRequests::new();
    let caller = Mutex::new(());
    let egui_mutex;
    {
        let egui_state = crate::egui_impl::EguiState::get();
        egui_mutex = parking_lot::MutexGuard::mutex(&egui_state);
        let _caller = caller.lock();
        assert_eq!(requests.request(snapshot()), RequestResult::Queued);
    }
    requests.service_with(|_| {
        assert!(
            egui_mutex.try_lock().is_some(),
            "EguiState guard released before save"
        );
        assert!(caller.try_lock().is_some());
        assert!(requests.pending.try_lock().unwrap().is_none());
    });
    let ui_source = include_str!("../../../../../ui/mod.rs");
    assert!(ui_source.contains("let mut slot = layout::STATE.lock();"));
    assert!(!ui_source.contains("service_pending"));
    let source = include_str!("../../../../../lib.rs");
    assert!(source.contains("let mut egui_state_guard = EguiState::get();"));
    assert!(source.contains(".show(ctx, |ui| ui::egui_debug_window(ui, renderer));"));
    assert!(source.contains(concat!(
        "                });\n",
        "            }\n",
        "        }\n\n",
        "        // The UI callbacks and their layout guards, then egui_state_guard, end before disk I/O.\n",
        "        controllers::physical_hands::pickup::diagnostics::export::service_pending();",
    )), "the production service must remain outside both UI scopes");
}

#[test]
fn concurrent_creates_produce_distinct_complete_files() {
    let output = TempOutput::new();
    let paths = std::thread::scope(|scope| {
        let writers: Vec<_> = (0..8)
            .map(|_| {
                let directory = &output.0;
                scope.spawn(move || write_fixture(directory).unwrap())
            })
            .collect();
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .collect::<Vec<_>>()
    });
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), paths.len());
    for path in paths {
        assert_eq!(read_document(&path)["schema_version"], SCHEMA_VERSION);
    }
}
