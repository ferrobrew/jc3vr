//! Reading a recording for a replay, on a thread of its own, and checking it against this build.

use std::{
    fs::File,
    io::{Error as IoError, ErrorKind as IoErrorKind, Read},
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

use crate::{
    controllers::frame::Hand,
    recorder::tracking::format::{self, Recording, SemanticSample},
    testing::replay::{ReplayError, StartMode, validate},
};

/// A recording read and checked, ready to replay.
#[derive(Debug)]
pub(crate) struct Loaded {
    pub(crate) path: PathBuf,
    pub(crate) recording: Recording,
    /// Each frame's time, for the [`Cursor`](crate::testing::replay::cursor::Cursor).
    pub(crate) times: Vec<u64>,
    /// Whether the recorded semantic states index this build's actions, so they can be published.
    pub(crate) semantic: bool,
    /// Ordered history exists and indexes this build's semantic schema.
    pub(crate) input_available: bool,
    /// Group/Policy dependencies indexed on the loader thread, never during frame staging.
    pub(crate) input_replay: tracking_format::input::replay::PreparedInput,
    /// What differs between the recording and this build or its settings.
    pub(crate) warnings: Vec<String>,
}

/// What this build and its settings are, for [`check`].
#[derive(Debug, Copy, Clone)]
pub(crate) struct Expected {
    pub(crate) action_schema: u32,
    pub(crate) dominant_hand: Hand,
    pub(crate) world_scale: f32,
    pub(crate) start: StartMode,
}

/// Read the recording at `path` on a new thread.
pub(crate) fn spawn(
    path: PathBuf,
    expected: Expected,
) -> Result<JoinHandle<Result<Loaded, ReplayError>>, ReplayError> {
    std::thread::Builder::new()
        .name("jc3vr-replay-loader".to_owned())
        .spawn(move || load(&path, expected))
        .map_err(|error| ReplayError::LoaderThread(Arc::new(error)))
}

/// Read and check the recording at `path`.
fn load(path: &Path, expected: Expected) -> Result<Loaded, ReplayError> {
    let file = File::open(path).map_err(|error| ReplayError::Read {
        path: path.to_owned(),
        error: Arc::new(error),
    })?;
    let bytes = read_bounded(
        file,
        tracking_format::DecodeLimits::default().max_file_bytes,
    )
    .map_err(|error| ReplayError::Read {
        path: path.to_owned(),
        error: Arc::new(IoError::new(error.io_kind(), error)),
    })?;
    let recording = format::decode(&bytes).map_err(|error| ReplayError::Decode {
        path: path.to_owned(),
        error,
    })?;
    prepare(path, recording, expected)
}

#[derive(Debug)]
enum ReadRecordingError {
    Limit { max_bytes: u64 },
    AddressSpace { max_bytes: u64 },
    Allocation { required_bytes: usize },
    Io(IoError),
}

impl ReadRecordingError {
    fn io_kind(&self) -> IoErrorKind {
        match self {
            Self::Limit { .. } => IoErrorKind::InvalidData,
            Self::AddressSpace { .. } => IoErrorKind::InvalidInput,
            Self::Allocation { .. } => IoErrorKind::OutOfMemory,
            Self::Io(error) => error.kind(),
        }
    }
}

impl std::fmt::Display for ReadRecordingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "recording read: ")?;
        match self {
            Self::Limit { max_bytes } => write!(f, "the file exceeds the {max_bytes}-byte limit"),
            Self::AddressSpace { max_bytes } => write!(
                f,
                "the {max_bytes}-byte limit and sentinel do not fit the platform address space"
            ),
            Self::Allocation { required_bytes } => write!(
                f,
                "the {required_bytes}-byte input buffer cannot be allocated"
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ReadRecordingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

fn read_bounded(reader: impl Read, max_bytes: u64) -> Result<Vec<u8>, ReadRecordingError> {
    let sentinel = read_sentinel(max_bytes, usize::MAX as u64)?;
    let max =
        usize::try_from(max_bytes).map_err(|_| ReadRecordingError::AddressSpace { max_bytes })?;
    let mut limited = reader.take(sentinel);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = match limited.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(count) => count,
            Err(error) if error.kind() == IoErrorKind::Interrupted => continue,
            Err(error) => return Err(ReadRecordingError::Io(error)),
        };
        let required = bytes
            .len()
            .checked_add(count)
            .ok_or(ReadRecordingError::AddressSpace { max_bytes })?;
        if required > max {
            return Err(ReadRecordingError::Limit { max_bytes });
        }
        if required > bytes.capacity() {
            let target = required.max(bytes.capacity().saturating_mul(2)).min(max);
            bytes.try_reserve_exact(target - bytes.len()).map_err(|_| {
                ReadRecordingError::Allocation {
                    required_bytes: target,
                }
            })?;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
}

fn read_sentinel(max_bytes: u64, address_limit: u64) -> Result<u64, ReadRecordingError> {
    max_bytes
        .checked_add(1)
        .filter(|sentinel| *sentinel <= address_limit)
        .ok_or(ReadRecordingError::AddressSpace { max_bytes })
}

fn prepare(path: &Path, recording: Recording, expected: Expected) -> Result<Loaded, ReplayError> {
    if recording.frames.is_empty() {
        return Err(ReplayError::Empty {
            path: path.to_owned(),
        });
    }
    if recording
        .frames
        .windows(2)
        .any(|pair| pair[0].time_ns > pair[1].time_ns)
        || recording
            .input
            .windows(2)
            .any(|pair| pair[0].capture_time_ns > pair[1].capture_time_ns)
    {
        return Err(ReplayError::Decode {
            path: path.to_owned(),
            error: format::DecodeError {
                offset: 0,
                kind: tracking_format::DecodeErrorKind::InvalidValue {
                    field: "recording capture time order",
                    value: 0,
                },
            },
        });
    }
    validate::recording(path, &recording, expected.world_scale)?;
    if expected.start == StartMode::Marker {
        validate::transform(path, "start.transform", &recording.header.start.transform)?;
    }
    let (semantic, mut warnings) = check(&recording, expected);
    let input_available = recording.version >= 2
        && !recording.input.is_empty()
        && semantic
        && recording.input.iter().all(|entry| match &entry.record {
            tracking_format::input::InputRecord::Group(group) => {
                group.action_schema == expected.action_schema
            }
            tracking_format::input::InputRecord::Discontinuity { .. }
            | tracking_format::input::InputRecord::Policy(_) => true,
        });
    if !input_available {
        warnings.push("the recording has no compatible ordered input history; snapshots replay without ordered input evidence".to_owned());
    }
    let times = recording.frames.iter().map(|f| f.time_ns).collect();
    let input_replay = tracking_format::input::replay::PreparedInput::new(&recording.input);
    if input_available && input_replay.max_unit_records() > crate::testing::replay::INPUT_BUDGET {
        return Err(ReplayError::WorkLimit {
            path: path.to_owned(),
            frame: None,
            field: "input dependency unit",
            count: input_replay.max_unit_records(),
            max: crate::testing::replay::INPUT_BUDGET,
        });
    }
    if let Some((index, frame)) = recording
        .frames
        .iter()
        .enumerate()
        .find(|(_, frame)| frame.recenters.len() > crate::testing::replay::RECENTER_BUDGET)
    {
        return Err(ReplayError::WorkLimit {
            path: path.to_owned(),
            frame: Some(index),
            field: "recenters",
            count: frame.recenters.len(),
            max: crate::testing::replay::RECENTER_BUDGET,
        });
    }
    Ok(Loaded {
        path: path.to_owned(),
        recording,
        times,
        semantic,
        input_available,
        input_replay,
        warnings,
    })
}

/// Whether the recording's semantic states can be published by this build, and what else differs
/// from it that a replay should say. A schema of another version, or a set recorded with another
/// number of actions, means the action indices do not line up, so the semantic states are left out
/// and the bindings see none; the raw inputs, which the gestures, the grabs, and the pointers read,
/// still replay.
pub(crate) fn check(recording: &Recording, expected: Expected) -> (bool, Vec<String>) {
    let mut warnings = Vec::new();
    let header = &recording.header;
    let mut semantic = header.action_schema == expected.action_schema;
    if !semantic {
        warnings.push(format!(
            "the recording's action schema is version {}, this build's is {}; the semantic \
             actions are not replayed",
            header.action_schema, expected.action_schema
        ));
    } else if let Some(index) = recording
        .frames
        .iter()
        .position(|f| f.semantic.as_ref().is_some_and(|s| !matches_schema(s)))
    {
        semantic = false;
        warnings.push(format!(
            "frame {index}'s semantic actions do not match this build's action sets; the semantic \
             actions are not replayed"
        ));
    }
    if header.dominant_hand != expected.dominant_hand {
        warnings.push(format!(
            "the recording was made with the {} hand dominant, and the {} hand is now",
            header.dominant_hand.label(),
            expected.dominant_hand.label()
        ));
    }
    if (header.world_scale - expected.world_scale).abs() > 1e-4 {
        warnings.push(format!(
            "the recording was made at world scale {}, and the world scale is now {}",
            header.world_scale, expected.world_scale
        ));
    }
    if recording.truncated {
        warnings.push("the recording's last frame was cut short and is left out".to_owned());
    }
    if recording.skipped_records > 0 {
        warnings.push(format!(
            "{} records of a kind this build does not know were skipped",
            recording.skipped_records
        ));
    }
    (semantic, warnings)
}

/// Whether both of a semantic sample's sets hold as many actions as this build defines.
fn matches_schema(sample: &SemanticSample) -> bool {
    [&sample.global, &sample.mode]
        .iter()
        .all(|state| state.actions.len() == state.set.actions().len())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use glam::{Mat4, Quat, Vec2, Vec3};
    use pretty_assertions::assert_eq;
    use tracking_synth::{scripts, synth::Options};

    use super::*;
    use crate::{
        controllers::{
            fingers::{config::FingerSource, curl},
            frame::{
                ActionSetKind, HAND_JOINT_COUNT, HandFrame, HandInputs, HandJoint,
                InteractionProfile, Pose,
            },
        },
        recorder::tracking::format::{
            ControllerSample, EyeSample, Frame, HeadSample, Header, RecenterEvent, SessionSample,
            StartMarker,
        },
        vr::{
            RecenterCause, RecenterKind,
            input::semantic::{
                SCHEMA_VERSION, SemanticSet,
                frame::{ActionState, ActionValue, SetState},
                vocabulary::OnFootAction,
            },
            stage::StageInfo,
        },
    };

    struct InjectedRead {
        bytes: Vec<u8>,
        position: usize,
        max_step: usize,
        fail_at: Option<usize>,
        interrupt_once: bool,
        grow_at: Option<(usize, Vec<u8>)>,
    }

    impl Read for InjectedRead {
        fn read(&mut self, output: &mut [u8]) -> Result<usize, IoError> {
            if self.interrupt_once {
                self.interrupt_once = false;
                return Err(IoErrorKind::Interrupted.into());
            }
            if let Some((at, _)) = &self.grow_at
                && self.position >= *at
            {
                let (_, extra) = self.grow_at.take().unwrap();
                self.bytes.extend(extra);
            }
            if self.fail_at.is_some_and(|at| self.position >= at) {
                return Err(IoError::new(
                    IoErrorKind::PermissionDenied,
                    "injected partial read failure",
                ));
            }
            let remaining = self.bytes.len() - self.position;
            let before_error = self.fail_at.map_or(usize::MAX, |at| at - self.position);
            let count = output
                .len()
                .min(self.max_step)
                .min(remaining)
                .min(before_error);
            output[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
            self.position += count;
            Ok(count)
        }
    }

    fn reader(bytes: &[u8], max_step: usize) -> InjectedRead {
        InjectedRead {
            bytes: bytes.to_vec(),
            position: 0,
            max_step,
            fail_at: None,
            interrupt_once: false,
            grow_at: None,
        }
    }

    #[test]
    fn bounded_read_accepts_exact_limit_and_rejects_only_one_sentinel_byte() {
        for max in [0, 1, 16384, 16385] {
            let bytes = vec![42; max];
            let mut exact = reader(&bytes, usize::MAX);
            assert_eq!(read_bounded(&mut exact, max as u64).unwrap(), bytes);
            assert_eq!(exact.position, max);
            let mut extra = reader(&vec![42; max + 100], usize::MAX);
            assert!(
                matches!(read_bounded(&mut extra, max as u64), Err(ReadRecordingError::Limit { max_bytes }) if max_bytes == max as u64)
            );
            assert_eq!(extra.position, max + 1);
        }
    }

    #[test]
    fn bounded_read_handles_short_reads_interruption_and_late_growth() {
        let mut short = reader(b"short reads", 1);
        short.interrupt_once = true;
        assert_eq!(read_bounded(&mut short, 11).unwrap(), b"short reads");
        let mut growing = reader(b"four", 2);
        growing.grow_at = Some((4, b" additional bytes".to_vec()));
        assert!(matches!(
            read_bounded(&mut growing, 4),
            Err(ReadRecordingError::Limit { max_bytes: 4 })
        ));
        assert_eq!(growing.position, 5);
        assert!(
            read_bounded(Cursor::new(b""), 1024 * 1024 * 1024)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn bounded_read_propagates_partial_io_failure_without_partial_recording() {
        let mut failing = reader(b"partial data", 2);
        failing.fail_at = Some(4);
        let error = read_bounded(&mut failing, 100).unwrap_err();
        assert!(
            matches!(&error, ReadRecordingError::Io(error) if error.kind() == IoErrorKind::PermissionDenied)
        );
        assert_eq!(failing.position, 4);
        let wrapped = IoError::new(error.io_kind(), error);
        assert!(matches!(
            wrapped
                .get_ref()
                .unwrap()
                .downcast_ref::<ReadRecordingError>(),
            Some(ReadRecordingError::Io(_))
        ));
    }

    #[test]
    fn bounded_read_checks_sentinel_and_platform_address_space() {
        assert_eq!(read_sentinel(9, 10).unwrap(), 10);
        assert!(matches!(
            read_sentinel(10, 10),
            Err(ReadRecordingError::AddressSpace { max_bytes: 10 })
        ));
        assert!(matches!(
            read_sentinel(u64::MAX, u64::MAX),
            Err(ReadRecordingError::AddressSpace { .. })
        ));
        assert!(matches!(
            read_sentinel(u64::from(u32::MAX), u64::from(u32::MAX)),
            Err(ReadRecordingError::AddressSpace { .. })
        ));
        assert!(matches!(
            read_bounded(Cursor::new(b""), u64::MAX),
            Err(ReadRecordingError::AddressSpace { .. })
        ));
    }

    const EXPECTED: Expected = Expected {
        action_schema: 2,
        dominant_hand: Hand::Right,
        world_scale: 1.0,
        start: StartMode::InPlace,
    };

    fn set(set: SemanticSet, count: usize) -> SetState {
        let rest = ActionState {
            value: ActionValue::Vec2(Vec2::ZERO),
            active: false,
            down: false,
        };
        SetState {
            set,
            actions: vec![[rest; 2]; count],
        }
    }

    fn recording(global_actions: usize) -> Recording {
        Recording {
            header: Header {
                build: String::new(),
                recorded_at: String::new(),
                action_schema: 2,
                start: StartMarker {
                    transform: [0.0; 16],
                    bookmark: None,
                },
                dominant_hand: Hand::Right,
                world_scale: 1.0,
                time_scale: 1.0,
            },
            frames: vec![Frame {
                time_ns: 0,
                display_time_ns: 0,
                session: SessionSample {
                    state: None,
                    should_render: true,
                    head_tracked: true,
                },
                head: None,
                stage: StageInfo::default(),
                controllers: None,
                semantic: Some(SemanticSample {
                    action_set: ActionSetKind::OnFoot,
                    global: set(SemanticSet::Global, global_actions),
                    mode: set(SemanticSet::OnFoot, SemanticSet::OnFoot.actions().len()),
                }),
                recenters: Vec::new(),
            }],
            input: Vec::new(),
            version: 1,
            truncated: false,
            skipped_records: 0,
        }
    }

    fn wire_load(recording: Recording) -> Result<Loaded, ReplayError> {
        wire_load_mode(recording, StartMode::InPlace)
    }

    fn wire_load_mode(recording: Recording, start: StartMode) -> Result<Loaded, ReplayError> {
        let mut bytes = format::encode_input_preamble(&recording.header);
        for entry in &recording.input {
            bytes.extend(tracking_format::encode_input(entry).unwrap());
        }
        for frame in &recording.frames {
            bytes.extend(format::encode_frame(frame));
        }
        prepare(
            Path::new("fixture.tracking"),
            format::decode(&bytes).unwrap(),
            Expected { start, ..EXPECTED },
        )
    }

    #[test]
    fn numeric_admission_follows_wire_decode_and_component_validity() {
        let base = || recording(SemanticSet::Global.actions().len());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut sample = base();
            sample.header.start.transform = Mat4::IDENTITY.to_cols_array();
            sample.header.start.transform[12] = bad;
            assert!(matches!(
                wire_load_mode(sample.clone(), StartMode::Marker),
                Err(ReplayError::InvalidSample { frame: None, .. })
            ));
            assert!(wire_load_mode(sample.clone(), StartMode::InPlace).is_ok());
            assert!(wire_load_mode(sample, StartMode::Bookmark).is_ok());
            let mut sample = base();
            sample.frames[0].head = Some(HeadSample {
                eyes: [EyeSample {
                    pose: Pose::IDENTITY,
                    fov: [-0.5, 0.5, 0.5, -0.5],
                }; 2],
            });
            sample.frames[0].head.as_mut().unwrap().eyes[1].fov[0] = bad;
            assert!(
                matches!(wire_load(sample), Err(ReplayError::InvalidSample { frame: Some(0), field, .. }) if field == "head.eyes[1].fov")
            );
            let mut sample = base();
            let mut hand = HandFrame::default();
            hand.grip.pose.position = Vec3::splat(bad);
            hand.grip.pose.orientation = Quat::from_xyzw(bad, 0.0, 0.0, 0.0);
            hand.joints = Some([HandJoint::default(); HAND_JOINT_COUNT]);
            hand.joints.as_mut().unwrap()[0].pose.position = Vec3::splat(bad);
            sample.frames[0].controllers = Some(ControllerSample {
                focused: true,
                action_set: ActionSetKind::OnFoot,
                hands: [hand, HandFrame::default()],
            });
            assert!(wire_load(sample.clone()).is_ok());
            sample.frames[0].controllers.as_mut().unwrap().hands[0]
                .grip
                .position_valid = true;
            assert!(matches!(
                wire_load(sample.clone()),
                Err(ReplayError::InvalidSample { frame: Some(0), .. })
            ));
            sample.frames[0].controllers.as_mut().unwrap().hands[0]
                .grip
                .position_valid = false;
            sample.frames[0].controllers.as_mut().unwrap().hands[0]
                .grip
                .orientation_valid = true;
            assert!(wire_load(sample).is_err());
        }
        let mut sample = base();
        assert!(wire_load_mode(sample.clone(), StartMode::Marker).is_err());
        sample.frames[0].recenters.push(RecenterEvent {
            kind: RecenterKind::Full,
            cause: RecenterCause::Manual,
            turn: Quat::from_xyzw(0.0, 0.0, 0.0, 0.0),
        });
        assert!(
            matches!(wire_load(sample), Err(ReplayError::InvalidSample { frame: Some(0), field, .. }) if field == "recenters[0].turn")
        );
    }

    #[test]
    fn active_raw_inputs_are_admitted_by_domain_before_finger_estimation() {
        type InputScalar = fn(&mut HandInputs) -> &mut f32;
        let fields: [(&str, InputScalar); 8] = [
            ("trigger", |i| &mut i.trigger),
            ("squeeze", |i| &mut i.squeeze),
            ("squeeze_force", |i| &mut i.squeeze_force),
            ("trackpad_force", |i| &mut i.trackpad_force),
            ("thumbstick", |i| &mut i.thumbstick.x),
            ("thumbstick", |i| &mut i.thumbstick.y),
            ("trackpad", |i| &mut i.trackpad.x),
            ("trackpad", |i| &mut i.trackpad.y),
        ];
        for (name, field) in fields {
            for touch in [false, true] {
                for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.01, -1.01] {
                    let mut sample = recording(SemanticSet::Global.actions().len());
                    let mut hand = HandFrame {
                        active: true,
                        ..Default::default()
                    };
                    hand.inputs.trigger_touch = touch;
                    hand.inputs.trackpad_touch = touch;
                    hand.inputs.thumbstick_touch = touch;
                    *field(&mut hand.inputs) = bad;
                    sample.frames[0].controllers = Some(ControllerSample {
                        focused: true,
                        action_set: ActionSetKind::OnFoot,
                        hands: [hand, HandFrame::default()],
                    });
                    assert!(
                        matches!(wire_load(sample.clone()), Err(ReplayError::InvalidSample { frame: Some(0), field, .. }) if field == format!("controllers.hands[0].inputs.{name}"))
                    );
                    sample.frames[0].controllers.as_mut().unwrap().hands[0].active = false;
                    assert!(wire_load(sample).is_ok());
                }
            }
        }
        for profile in [
            InteractionProfile::Other,
            InteractionProfile::ValveIndex,
            InteractionProfile::OculusTouch,
        ] {
            for value in [0.0, 0.5, 1.0] {
                let mut sample = recording(SemanticSet::Global.actions().len());
                let hand = HandFrame {
                    active: true,
                    profile,
                    inputs: HandInputs {
                        trigger: value,
                        trigger_touch: true,
                        squeeze: value,
                        squeeze_force: value,
                        trackpad_force: value,
                        thumbstick: Vec2::new(-value, value),
                        trackpad: Vec2::new(value, -value),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                sample.frames[0].controllers = Some(ControllerSample {
                    focused: true,
                    action_set: ActionSetKind::OnFoot,
                    hands: [hand, HandFrame::default()],
                });
                let loaded = wire_load(sample).unwrap();
                let hand = &loaded.recording.frames[0]
                    .controllers
                    .as_ref()
                    .unwrap()
                    .hands[0];
                assert!(hand.joints.is_none());
                for source in [FingerSource::Auto, FingerSource::Estimate] {
                    let (_, curls) = curl::select(source, Hand::Left, hand).unwrap();
                    assert!(
                        curls
                            .curl
                            .into_iter()
                            .chain(curls.splay)
                            .all(f32::is_finite)
                    );
                }
            }
        }
    }

    #[test]
    fn cockpit_positions_bound_actual_midpoint_offsets_and_scaled_composition() {
        let head = |positions: [Vec3; 2]| HeadSample {
            eyes: positions.map(|position| EyeSample {
                pose: Pose {
                    position,
                    orientation: Quat::from_rotation_y(0.3),
                },
                fov: [-0.5, 0.5, 0.5, -0.5],
            }),
        };
        let mut sample = recording(SemanticSet::Global.actions().len());
        let huge = Vec3::splat(3e38);
        assert!(!(0.5 * (huge + huge)).is_finite());
        sample.frames[0].head = Some(head([huge; 2]));
        assert!(wire_load(sample).is_err());
        let max = validate::MAX_COCKPIT_POSITION_M;
        for positions in [
            [Vec3::splat(max - 0.1), Vec3::splat(max)],
            [Vec3::splat(-max), Vec3::splat(max)],
        ] {
            let mut sample = recording(SemanticSet::Global.actions().len());
            sample.frames[0].head = Some(head(positions));
            let loaded = wire_load(sample).unwrap();
            let eyes = loaded.recording.frames[0].head.unwrap().eyes;
            let center = 0.5 * (eyes[0].pose.position + eyes[1].pose.position);
            let orientation = eyes[0]
                .pose
                .orientation
                .slerp(eyes[1].pose.orientation, 0.5);
            for scale in [0.01, 1.0, 100.0] {
                let body = Quat::from_euler(glam::EulerRot::YXZ, 1.2, -0.7, 0.8);
                let world = crate::headpose::xr::compose(
                    center,
                    orientation,
                    body,
                    Vec3::new(10000.0, 200.0, -3000.0),
                    scale,
                );
                assert!(world.position.is_finite() && world.orientation.is_finite());
                for eye in eyes {
                    let offset = body * ((eye.pose.position - center) * scale);
                    assert!(offset.is_finite() && (world.position + offset).is_finite());
                }
            }
        }
        let mut sample = recording(SemanticSet::Global.actions().len());
        sample.header.start.transform = Mat4::from_translation(huge).to_cols_array();
        assert!(wire_load_mode(sample, StartMode::Marker).is_ok());
    }

    #[test]
    fn finite_degenerate_views_and_recenter_work_are_rejected_after_decode() {
        let base = || recording(SemanticSet::Global.actions().len());
        for fov in [[0.0; 4], [-2.0, 0.5, 0.5, -0.5], [0.5, -0.5, 0.5, -0.5]] {
            let mut sample = base();
            sample.frames[0].head = Some(HeadSample {
                eyes: [EyeSample {
                    pose: Pose::IDENTITY,
                    fov,
                }; 2],
            });
            assert!(wire_load(sample).is_err());
        }
        let mut sample = base();
        let mut pose = Pose::IDENTITY;
        pose.orientation = Quat::from_xyzw(0.0, 0.0, 0.0, 0.0);
        sample.frames[0].head = Some(HeadSample {
            eyes: [EyeSample {
                pose,
                fov: [-0.5, 0.5, 0.5, -0.5],
            }; 2],
        });
        assert!(wire_load(sample).is_err());
        let mut sample = base();
        sample.frames[0].recenters = vec![
            RecenterEvent {
                kind: RecenterKind::Full,
                cause: RecenterCause::Manual,
                turn: Quat::IDENTITY
            };
            crate::testing::replay::RECENTER_BUDGET + 1
        ];
        assert!(matches!(
            wire_load(sample),
            Err(ReplayError::WorkLimit {
                frame: Some(0),
                field: "recenters",
                ..
            })
        ));
    }

    #[test]
    fn loader_rejects_backward_frame_and_input_capture_times_after_wire_decode() {
        let mut frames = recording(SemanticSet::Global.actions().len());
        frames.frames[0].time_ns = 20;
        let mut second = frames.frames[0].clone();
        second.time_ns = 10;
        frames.frames.push(second);
        assert!(matches!(wire_load(frames), Err(ReplayError::Decode { .. })));
        let mut input = recording(SemanticSet::Global.actions().len());
        input.input = [20, 10]
            .map(|capture_time_ns| tracking_format::input::CapturedInput {
                capture_time_ns,
                frame_sequence: None,
                record: tracking_format::input::InputRecord::Discontinuity {
                    reason: tracking_format::input::DiscontinuityReason::Unavailable,
                    discarded: 0,
                },
            })
            .to_vec();
        assert!(matches!(wire_load(input), Err(ReplayError::Decode { .. })));
    }

    #[test]
    fn incompatible_group_schema_keeps_snapshots_but_marks_history_unavailable() {
        let mut recording = recording(SemanticSet::Global.actions().len());
        recording.input.push(tracking_format::input::CapturedInput {
            capture_time_ns: 0,
            frame_sequence: Some(1),
            record: tracking_format::input::InputRecord::Group(
                tracking_format::input::InputGroup {
                    stamp: tracking_format::input::InputStamp {
                        epoch: 1,
                        clock: tracking_format::input::ClockDomain::HostMonotonic,
                        time_ns: 1,
                        sync: 1,
                        sequence: 1,
                    },
                    order: tracking_format::input::GroupOrder::UnorderedSync,
                    source_generation: 1,
                    binding_generation: 1,
                    action_schema: EXPECTED.action_schema + 1,
                    context: 0,
                    focused: true,
                    samples: Vec::new(),
                },
            ),
        });
        let loaded = wire_load(recording).unwrap();
        assert!(loaded.semantic);
        assert!(!loaded.input_available);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.contains("no compatible ordered input history"))
        );
    }

    #[test]
    fn a_recording_of_this_schema_replays_its_semantic_actions() {
        let recording = recording(SemanticSet::Global.actions().len());
        assert_eq!(check(&recording, EXPECTED), (true, Vec::new()));
    }

    #[test]
    fn another_schema_leaves_the_semantic_actions_out() {
        let recording = recording(SemanticSet::Global.actions().len());
        let (semantic, warnings) = check(
            &recording,
            Expected {
                action_schema: 3,
                ..EXPECTED
            },
        );
        assert!(!semantic);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn a_set_of_another_size_leaves_the_semantic_actions_out() {
        let recording = recording(SemanticSet::Global.actions().len() + 1);
        let (semantic, warnings) = check(&recording, EXPECTED);
        assert!(!semantic);
        assert!(warnings[0].starts_with("frame 0's semantic actions"));
    }

    #[test]
    fn changed_settings_and_a_cut_tail_are_reported() {
        let mut recording = recording(SemanticSet::Global.actions().len());
        recording.truncated = true;
        recording.skipped_records = 2;
        let (semantic, warnings) = check(
            &recording,
            Expected {
                dominant_hand: Hand::Left,
                world_scale: 1.5,
                ..EXPECTED
            },
        );
        assert!(semantic);
        assert_eq!(warnings.len(), 4);
    }

    #[test]
    fn the_synthetic_recordings_replay_their_semantic_actions() {
        let expected = Expected {
            action_schema: SCHEMA_VERSION,
            ..EXPECTED
        };
        for script in scripts::all() {
            let bytes = tracking_synth::generate(&script, Options::default())
                .expect("the script generates");
            let recording = format::decode(&bytes).expect("the recording decodes");
            assert_eq!(
                check(&recording, expected),
                (true, Vec::new()),
                "{}",
                script.name
            );
        }
    }

    #[test]
    fn the_synthetic_left_stick_drives_this_build_s_move_action() {
        let script = scripts::named("stick_walk").expect("a built-in script");
        let bytes =
            tracking_synth::generate(&script, Options::default()).expect("the script generates");
        let recording = format::decode(&bytes).expect("the recording decodes");
        // Three seconds in, the left stick is pushed fully forward.
        let semantic = recording.frames[270]
            .semantic
            .as_ref()
            .expect("the semantic actions are recorded");
        assert_eq!(semantic.mode.set, SemanticSet::OnFoot);
        let movement = semantic.mode.actions[OnFootAction::Move.id().index()];
        assert_eq!(
            movement[Hand::Left.index()].value,
            ActionValue::Vec2(Vec2::Y)
        );
        assert_eq!(
            semantic.mode.actions[OnFootAction::Turn.id().index()][Hand::Right.index()].value,
            ActionValue::Float(0.0)
        );
    }
}
