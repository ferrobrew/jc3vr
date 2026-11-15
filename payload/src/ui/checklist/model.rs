//! The session checklist's checks and their results.
//!
//! The checks are the numbered list under the session notes' "Checks in the headset" heading, as
//! the build embedded it. Each result is saved at once to [`FILE_NAME`] in the configuration
//! directory, by the session's stamp and the check's number, with the start of the check's text so
//! a reader can match it, and logged, so a recording's log says when each check was marked.

use std::{collections::BTreeMap, fmt, path::PathBuf, sync::LazyLock};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::config::{paths, write};

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
    let mut guard = STATE.lock();
    let state = guard.loaded();
    let sessions = &mut state.file.sessions;
    let session = sessions.entry(SESSION.to_owned()).or_default();
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
    tracing::info!(
        target: "checklist",
        session = SESSION,
        check = check.number,
        mark = mark.label(),
        note,
        "checklist: a check was marked"
    );
    let saved = state.save();
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
pub(crate) fn file_status() -> (Option<PathBuf>, Option<String>) {
    let mut state = STATE.lock();
    let last = state.loaded().last.clone();
    (paths::file(FILE_NAME), last)
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
    file: ResultsFile,
    loaded: bool,
    last: Option<String>,
}

impl State {
    fn loaded(&mut self) -> &mut Self {
        if !self.loaded {
            self.loaded = true;
            match load() {
                Ok(file) => self.file = file,
                Err(error) => {
                    tracing::warn!(target: "checklist", "{error}");
                    self.last = Some(error.to_string());
                }
            }
        }
        self
    }

    fn session(&self) -> Option<&SessionResults> {
        self.file.sessions.get(SESSION)
    }

    fn save(&self) -> Result<(), ChecklistError> {
        let path = paths::file(FILE_NAME).ok_or(ChecklistError::NoDirectory)?;
        let text = toml::to_string_pretty(&self.file).map_err(ChecklistError::Serialize)?;
        write::replace(&path, text.as_bytes()).map_err(ChecklistError::Write)
    }
}

/// Read the results file; a missing file holds no results.
fn load() -> Result<ResultsFile, ChecklistError> {
    let path = paths::file(FILE_NAME).ok_or(ChecklistError::NoDirectory)?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ResultsFile::default());
        }
        Err(error) => return Err(ChecklistError::Read(path, error)),
    };
    toml::from_str(&text).map_err(|error| ChecklistError::Parse(path, error))
}

static CHECKS: LazyLock<Vec<Check>> = LazyLock::new(|| parse(NOTES));

static STATE: Mutex<State> = Mutex::new(State {
    file: ResultsFile {
        sessions: BTreeMap::new(),
    },
    loaded: false,
    last: None,
});

/// The embedded session notes.
const NOTES: &str = include_str!(concat!(env!("OUT_DIR"), "/next-session.md"));

/// The heading the checks sit under.
const CHECKS_HEADING: &str = "Checks in the headset";

/// How much of a check's text the results keep, characters.
const TEXT_KEPT: usize = 80;

#[cfg(test)]
mod tests {
    use super::*;

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
