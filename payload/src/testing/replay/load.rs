//! Reading a recording for a replay, on a thread of its own, and checking it against this build.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

use crate::{
    controllers::frame::Hand,
    recorder::tracking::format::{self, Recording, SemanticSample},
    testing::replay::ReplayError,
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
    /// What differs between the recording and this build or its settings.
    pub(crate) warnings: Vec<String>,
}

/// What this build and its settings are, for [`check`].
#[derive(Debug, Copy, Clone)]
pub(crate) struct Expected {
    pub(crate) action_schema: u32,
    pub(crate) dominant_hand: Hand,
    pub(crate) world_scale: f32,
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
    let bytes = std::fs::read(path).map_err(|error| ReplayError::Read {
        path: path.to_owned(),
        error: Arc::new(error),
    })?;
    let recording = format::decode(&bytes).map_err(|error| ReplayError::Decode {
        path: path.to_owned(),
        error,
    })?;
    if recording.frames.is_empty() {
        return Err(ReplayError::Empty {
            path: path.to_owned(),
        });
    }
    let (semantic, warnings) = check(&recording, expected);
    let times = recording.frames.iter().map(|f| f.time_ns).collect();
    Ok(Loaded {
        path: path.to_owned(),
        recording,
        times,
        semantic,
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
    use glam::Vec2;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::{
        controllers::frame::ActionSetKind,
        recorder::tracking::format::{Frame, Header, SessionSample, StartMarker},
        vr::{
            input::semantic::{
                SemanticSet,
                frame::{ActionState, ActionValue, SetState},
            },
            stage::StageInfo,
        },
    };

    const EXPECTED: Expected = Expected {
        action_schema: 2,
        dominant_hand: Hand::Right,
        world_scale: 1.0,
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
            truncated: false,
            skipped_records: 0,
        }
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
}
