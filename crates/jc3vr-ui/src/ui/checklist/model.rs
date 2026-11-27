//! The session checklist's checks and their results.
//!
//! The checks are the numbered list under the session notes' "Checks in the headset" heading, as
//! the build embedded it. Each result is saved at once to [`FILE_NAME`] in the configuration
//! directory, by the session's stamp and the check's number, with the start of the check's text so
//! a reader can match it, and logged, so a recording's log says when each check was marked.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use jc3vr_core::config::{paths, write};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// The results file's name, in the configuration directory.
pub(crate) const FILE_NAME: &str = "session_checklist.toml";

/// The session the embedded checks belong to, or empty without one.
pub(crate) const SESSION: &str = env!("jc3vr_checklist_session");

/// A check's result.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mark {
    #[default]
    Untested,
    Works,
    Broken,
}

impl Mark {
    pub(crate) const ALL: [Mark; 3] = [Mark::Untested, Mark::Works, Mark::Broken];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Mark::Untested => "Untested",
            Mark::Works => "Works",
            Mark::Broken => "Doesn't work",
        }
    }
}

/// One check of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Check {
    pub number: u32,
    /// The check's text, with the Markdown links reduced to their text.
    pub text: String,
}

/// The embedded session's checks.
pub(crate) fn checks() -> &'static [Check] {
    &CHECKS
}

/// The result of check `number`.
pub(crate) fn mark(number: u32) -> Mark {
    let mut state = STATE.lock();
    state
        .loaded()
        .session()
        .and_then(|session| session.checks.get(&number.to_string()))
        .map_or(Mark::Untested, |result| result.mark)
}

/// The note on check `number`.
pub(crate) fn note(number: u32) -> String {
    let mut state = STATE.lock();
    state
        .loaded()
        .session()
        .and_then(|session| session.checks.get(&number.to_string()))
        .map(|result| result.note.clone())
        .unwrap_or_default()
}

/// Mark check `check` as `mark` with the note `note`, save, and log it.
pub(crate) fn set(check: &Check, mark: Mark, note: &str) {
    let mut state = STATE.lock();
    let saved = state.set(check, mark, note);
    state.last = saved.err().map(|error| {
        tracing::warn!(target: "checklist", "{error}");
        error.to_string()
    });
}

/// How many checks carry each mark: untested, working, and broken.
pub(crate) fn counts() -> [usize; 3] {
    let mut counts = [0; 3];
    for check in checks() {
        let index = match mark(check.number) {
            Mark::Untested => 0,
            Mark::Works => 1,
            Mark::Broken => 2,
        };
        counts[index] += 1;
    }
    counts
}

/// The results file's path, and the last load or save failure.
pub(crate) fn file_status() -> (Option<PathBuf>, Option<String>, Recovery) {
    let mut state = STATE.lock();
    let last = state.loaded().last.clone();
    (state.path.clone(), last, state.recovery())
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum Recovery {
    Reload,
    RetrySave,
}

pub(crate) fn retry_save() {
    let mut state = STATE.lock();
    state.last = state.save().err().map(|error| {
        tracing::warn!(target: "checklist", "{error}");
        error.to_string()
    });
}

pub(crate) fn reload() {
    STATE.lock().reload();
}

/// The checks of the session notes `text`: the numbered lines under the checks' heading, up to the
/// next heading.
fn parse(text: &str) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            inside = heading.trim() == CHECKS_HEADING;
            continue;
        }
        if !inside {
            continue;
        }
        let Some((number, rest)) = line.split_once(". ") else {
            continue;
        };
        if let Ok(number) = number.trim().parse() {
            checks.push(Check {
                number,
                text: without_links(rest.trim()),
            });
        }
    }
    checks
}

/// `text` with each Markdown link `[label](target)` reduced to its label.
fn without_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            after[close + 2..]
                .find(')')
                .map(|end| (close, close + 2 + end))
        });
        match link {
            Some((close, end)) => {
                out.push_str(&rest[..open]);
                out.push_str(&after[..close]);
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The results file: each session's results by its stamp.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct ResultsFile {
    #[serde(default)]
    sessions: BTreeMap<String, SessionResults>,
}

/// One session's results by check number.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
struct SessionResults {
    #[serde(default)]
    checks: BTreeMap<String, CheckResult>,
}

/// One check's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CheckResult {
    mark: Mark,
    #[serde(default)]
    note: String,
    /// When it was marked, an RFC 3339 timestamp.
    at: String,
    /// The start of the check's text, to match it to the notes.
    text: String,
}

/// Why the results file could not be read or written.
#[derive(Debug)]
enum ChecklistError {
    NoDirectory,
    LoadBlocked(String),
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    Serialize(toml::ser::Error),
    Write(write::WriteError),
}

impl fmt::Display for ChecklistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDirectory => write!(
                f,
                "session checklist: there is no configuration directory to save to"
            ),
            Self::LoadBlocked(error) => write!(
                f,
                "session checklist: saving is disabled until the results are reloaded: {error}"
            ),
            Self::Read(path, error) => write!(
                f,
                "session checklist: could not read {}: {error}",
                path.display()
            ),
            Self::Parse(path, error) => write!(
                f,
                "session checklist: could not parse {}: {error}",
                path.display()
            ),
            Self::Serialize(error) => write!(f, "session checklist: {error}"),
            Self::Write(error) => write!(f, "session checklist: {error}"),
        }
    }
}

impl std::error::Error for ChecklistError {}

struct State {
    path: Option<PathBuf>,
    file: ResultsFile,
    load: LoadState,
    last: Option<String>,
}

enum LoadState {
    Pending,
    Ready,
    Failed(ChecklistError),
}

impl State {
    fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            file: ResultsFile::default(),
            load: LoadState::Pending,
            last: None,
        }
    }

    fn loaded(&mut self) -> &mut Self {
        if matches!(self.load, LoadState::Pending) {
            match load(self.path.as_deref()) {
                Ok(file) => {
                    self.file = file;
                    self.load = LoadState::Ready;
                }
                Err(error) => {
                    tracing::warn!(target: "checklist", "{error}");
                    self.last = Some(error.to_string());
                    self.load = LoadState::Failed(error);
                }
            }
        }
        self
    }

    fn reload(&mut self) {
        if matches!(self.load, LoadState::Failed(_)) {
            self.load = LoadState::Pending;
            self.last = None;
            self.loaded();
        }
    }

    fn recovery(&self) -> Recovery {
        match self.load {
            LoadState::Ready => Recovery::RetrySave,
            LoadState::Pending | LoadState::Failed(_) => Recovery::Reload,
        }
    }

    fn session(&self) -> Option<&SessionResults> {
        self.file.sessions.get(SESSION)
    }

    fn set(&mut self, check: &Check, mark: Mark, note: &str) -> Result<(), ChecklistError> {
        self.loaded().writable()?;
        let session = self.file.sessions.entry(SESSION.to_owned()).or_default();
        let key = check.number.to_string();
        if mark == Mark::Untested && note.is_empty() {
            session.checks.remove(&key);
        } else {
            session.checks.insert(
                key,
                CheckResult {
                    mark,
                    note: note.to_owned(),
                    at: jiff::Timestamp::now().to_string(),
                    text: check.text.chars().take(TEXT_KEPT).collect(),
                },
            );
        }
        self.save()?;
        tracing::info!(
            target: "checklist",
            session = SESSION,
            check = check.number,
            mark = mark.label(),
            note,
            "checklist: a check was marked"
        );
        Ok(())
    }

    fn writable(&self) -> Result<(), ChecklistError> {
        match &self.load {
            LoadState::Ready => Ok(()),
            LoadState::Failed(error) => Err(ChecklistError::LoadBlocked(error.to_string())),
            LoadState::Pending => Err(ChecklistError::LoadBlocked(
                "the results have not been loaded".to_owned(),
            )),
        }
    }

    fn save(&self) -> Result<(), ChecklistError> {
        self.writable()?;
        let path = self.path.as_deref().ok_or(ChecklistError::NoDirectory)?;
        let text = toml::to_string_pretty(&self.file).map_err(ChecklistError::Serialize)?;
        write::replace(path, text.as_bytes()).map_err(ChecklistError::Write)
    }
}

/// Read the results file; a missing file holds no results.
fn load(path: Option<&Path>) -> Result<ResultsFile, ChecklistError> {
    let path = path.ok_or(ChecklistError::NoDirectory)?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ResultsFile::default());
        }
        Err(error) => return Err(ChecklistError::Read(path.to_owned(), error)),
    };
    toml::from_str(&text).map_err(|error| ChecklistError::Parse(path.to_owned(), error))
}

static CHECKS: LazyLock<Vec<Check>> = LazyLock::new(|| parse(NOTES));

static STATE: LazyLock<Mutex<State>> =
    LazyLock::new(|| Mutex::new(State::new(paths::file(FILE_NAME))));

/// The embedded session notes.
const NOTES: &str = include_str!(concat!(env!("OUT_DIR"), "/next-session.md"));

/// The heading the checks sit under.
const CHECKS_HEADING: &str = "Checks in the headset";

/// How much of a check's text the results keep, characters.
const TEXT_KEPT: usize = 80;

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::ui::checklist::row_edit;

    struct Fixture {
        directory: PathBuf,
        state: State,
        check: Check,
    }

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "jc3vr-checklist-{}-{}-{}",
                std::process::id(),
                jiff::Timestamp::now().as_nanosecond(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            Self {
                state: State::new(Some(directory.join(FILE_NAME))),
                directory,
                check: Check {
                    number: 1,
                    text: "Test the control.".to_owned(),
                },
            }
        }

        fn path(&self) -> &Path {
            self.state.path.as_deref().unwrap()
        }

        fn result(&self) -> CheckResult {
            load(Some(self.path())).unwrap().sessions[SESSION].checks["1"].clone()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    #[test_case::test_case(b"not valid toml = [", "could not parse"; "malformed")]
    #[test_case::test_case(b"\xff\xfe", "could not read"; "unreadable text")]
    fn a_load_error_blocks_writes_until_explicit_recovery(bytes: &[u8], message: &str) {
        let mut fixture = Fixture::new();
        std::fs::write(fixture.path(), bytes).unwrap();
        fixture.state.loaded();
        let original_error = fixture.state.last.clone().unwrap();
        assert!(original_error.contains(message), "{original_error}");
        assert!(original_error.contains(&fixture.path().display().to_string()));
        for mark in [Mark::Works, Mark::Broken, Mark::Untested] {
            let error = fixture
                .state
                .set(&fixture.check, mark, "a note")
                .unwrap_err();
            assert!(error.to_string().contains(&original_error));
            assert!(fixture.state.save().is_err());
            assert_eq!(std::fs::read(fixture.path()).unwrap(), bytes);
            assert!(fixture.state.file.sessions.is_empty());
            assert_eq!(fixture.state.loaded().last.as_ref(), Some(&original_error));
        }
        std::fs::remove_file(fixture.path()).unwrap();
        assert!(fixture.state.set(&fixture.check, Mark::Works, "").is_err());
        assert!(!fixture.path().exists());
        fixture.state.reload();
        assert!(fixture.state.last.is_none());
        fixture
            .state
            .set(&fixture.check, Mark::Works, "recovered")
            .unwrap();
        assert_eq!(fixture.result().note, "recovered");
    }

    #[test]
    fn an_io_load_error_is_not_a_missing_file() {
        let mut fixture = Fixture::new();
        std::fs::create_dir(fixture.path()).unwrap();
        let kept = fixture.path().join("kept");
        std::fs::write(&kept, b"original bytes").unwrap();
        fixture.state.loaded();
        let error = fixture.state.last.clone().unwrap();
        assert!(error.contains("could not read"));
        assert!(fixture.state.set(&fixture.check, Mark::Works, "").is_err());
        assert_eq!(std::fs::read(kept).unwrap(), b"original bytes");
        assert!(fixture.path().is_dir());
    }

    #[test]
    fn a_failed_save_keeps_edits_for_retry_instead_of_reloading() {
        let mut fixture = Fixture::new();
        fixture
            .state
            .set(&fixture.check, Mark::Works, "saved")
            .unwrap();
        std::fs::remove_file(fixture.path()).unwrap();
        std::fs::create_dir(fixture.path()).unwrap();
        assert!(
            fixture
                .state
                .set(&fixture.check, Mark::Broken, "unsaved")
                .is_err()
        );
        assert_eq!(fixture.state.recovery(), Recovery::RetrySave);
        fixture.state.reload();
        let result = &fixture.state.session().unwrap().checks["1"];
        assert_eq!(result.mark, Mark::Broken);
        assert_eq!(result.note, "unsaved");
        std::fs::remove_dir(fixture.path()).unwrap();
        fixture.state.save().unwrap();
        assert_eq!(fixture.result().mark, Mark::Broken);
        assert_eq!(fixture.result().note, "unsaved");
    }

    #[test]
    fn a_missing_file_can_be_created_and_loaded_again() {
        let mut fixture = Fixture::new();
        fixture.state.loaded();
        assert!(fixture.state.last.is_none());
        fixture
            .state
            .set(&fixture.check, Mark::Broken, "new note")
            .unwrap();
        assert_eq!(fixture.result().mark, Mark::Broken);
        assert_eq!(fixture.result().note, "new note");
        let mut reloaded = State::new(Some(fixture.path().to_owned()));
        assert_eq!(
            reloaded.loaded().session().unwrap().checks["1"].note,
            "new note"
        );
    }

    #[test_case::test_case(Mark::Works; "works")]
    #[test_case::test_case(Mark::Untested; "untested")]
    fn selecting_a_mark_keeps_the_focused_edited_note_and_persists_both(selected: Mark) {
        let mut fixture = Fixture::new();
        fixture
            .state
            .set(&fixture.check, Mark::Broken, "original note")
            .unwrap();
        let context = egui::Context::default();
        let mut note = "original note".to_owned();
        let mut frame = |events| {
            let mark = fixture.state.session().unwrap().checks["1"].mark;
            let mut update = None;
            let output = context.run_ui(
                egui::RawInput {
                    events,
                    focused: true,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| update = row_edit(ui, &fixture.check, mark, &mut note),
            );
            if let Some(mark) = update {
                fixture.state.set(&fixture.check, mark, &note).unwrap();
            }
            output
        };
        frame(Vec::new());
        let output = frame(Vec::new());
        let text_position = |text: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(shape) if shape.galley.text() == text => {
                        Some(shape.visual_bounding_rect().center())
                    }
                    _ => None,
                })
                .unwrap()
        };
        let note_position = text_position("original note");
        let mark_position = text_position(selected.label());
        let pointer = |position, pressed| {
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        for pressed in [true, false] {
            frame(pointer(note_position, pressed));
        }
        assert!(context.memory(|memory| memory.focused().is_some()));
        frame(vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
            egui::Event::Text("latest note".to_owned()),
        ]);
        for pressed in [true, false] {
            frame(pointer(mark_position, pressed));
        }
        frame(Vec::new());
        assert_eq!(note, "latest note");
        assert_eq!(fixture.result().mark, selected);
        assert_eq!(fixture.result().note, "latest note");
        assert_eq!(fixture.state.session().unwrap().checks["1"].mark, selected);
    }

    #[test]
    fn the_checks_are_the_numbered_lines_under_their_heading() {
        let notes = "# After\n\n## Checks in the headset\n\n1. Walk [here](a.md#b) and back.\n\
                     2. Grip the `rail`.\n\n## Still open\n\n3. Not a check.\n";
        assert_eq!(
            parse(notes),
            vec![
                Check {
                    number: 1,
                    text: "Walk here and back.".to_owned()
                },
                Check {
                    number: 2,
                    text: "Grip the `rail`.".to_owned()
                },
            ]
        );
    }

    #[test]
    fn a_bracket_that_is_not_a_link_is_kept() {
        assert_eq!(without_links("a [b] c"), "a [b] c");
        assert_eq!(without_links("[x](y) and [z](w)"), "x and z");
    }

    #[test]
    fn the_embedded_notes_parse_into_numbered_checks() {
        // The build embeds the newest session's notes; each has at least one check.
        if !SESSION.is_empty() {
            assert!(!checks().is_empty());
            assert!(checks().windows(2).all(|w| w[0].number < w[1].number));
        }
    }
}
