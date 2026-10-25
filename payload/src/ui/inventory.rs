//! Source-backed inventory for migration checks, not a second implementation of the controls.
//!
//! IDs use a source path, enclosing function, primitive kind, normalized call fingerprint, and
//! duplicate occurrence. Whitespace, line movement, and unrelated insertions preserve an ID.
//! Migration must carry the old ID explicitly when moving or rewriting a call. Parameterized
//! loops remain call templates; expressions, source gates, and raw readout builders remain in
//! the included source evidence. The scanner is lexical, not a Rust parser or a runtime census.

use std::collections::{HashMap, HashSet};

pub(crate) use crate::ui::inventory::catalog::{FEATURES, FeatureId, FeatureSection, PageId};
use crate::ui::inventory::sources::{DEBUG_SOURCES, PAUSE_SOURCES};

mod catalog;
mod sources;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Debug,
    Pause,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SourceFile {
    pub path: &'static str,
    pub text: &'static str,
    pub surface: Surface,
    /// Candidates only; mixed-source controls require a per-control migration assignment.
    pub destinations: &'static [FeatureId],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ControlKind {
    Setting,
    Command,
    TextInput,
    Selection,
    View,
    Readout,
    Section,
    PauseItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ControlId(pub String);

#[derive(Clone, Debug)]
pub(crate) struct ExistingControl {
    pub id: ControlId,
    pub source: &'static str,
    pub function: String,
    pub line: usize,
    pub kind: ControlKind,
    /// Exact argument evidence retains setting targets, commands, and dynamic label expressions.
    pub arguments: String,
    pub surface: Surface,
    pub candidate_destinations: &'static [FeatureId],
}

#[derive(Clone, Debug)]
pub(crate) struct MigrationAssignment {
    pub control: ControlId,
    pub destination: FeatureId,
    pub section: FeatureSection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParityIssue {
    Missing(ControlId),
    Duplicate(ControlId),
    Unknown(ControlId),
    UnexpectedDestination(ControlId, FeatureId),
}

pub(crate) fn source_files() -> impl Iterator<Item = &'static SourceFile> {
    DEBUG_SOURCES.iter().chain(PAUSE_SOURCES)
}

pub(crate) fn existing_controls() -> Vec<ExistingControl> {
    source_files().flat_map(scan_source).collect()
}

/// No default migration assignment is inferred from a source file's first candidate.
pub(crate) fn migration_parity(
    inventory: &[ExistingControl],
    assignments: &[MigrationAssignment],
) -> Vec<ParityIssue> {
    let known: HashMap<_, _> = inventory
        .iter()
        .map(|control| (&control.id, control))
        .collect();
    let mut seen = HashSet::new();
    let mut issues = Vec::new();
    for assignment in assignments {
        if !seen.insert(&assignment.control) {
            issues.push(ParityIssue::Duplicate(assignment.control.clone()));
        }
        match known.get(&assignment.control) {
            None => issues.push(ParityIssue::Unknown(assignment.control.clone())),
            Some(control)
                if !control
                    .candidate_destinations
                    .contains(&assignment.destination) =>
            {
                issues.push(ParityIssue::UnexpectedDestination(
                    assignment.control.clone(),
                    assignment.destination,
                ));
            }
            Some(_) => {}
        }
    }
    for control in inventory {
        if !seen.contains(&control.id) {
            issues.push(ParityIssue::Missing(control.id.clone()));
        }
    }
    issues
}

fn scan_source(source: &SourceFile) -> Vec<ExistingControl> {
    let tokens = tokenize(source.text);
    let mut records = Vec::new();
    let mut occurrences = HashMap::new();
    let functions: Vec<_> = tokens
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| {
            if pair[0].text != "fn" || tokens.get(index + 2).is_none_or(|token| token.text != "(") {
                return None;
            }
            let body = (index + 2..tokens.len()).find(|&i| tokens[i].text == "{")?;
            let end = matching_end(&tokens, body)?;
            Some((body, end, pair[1].text))
        })
        .collect();
    for (index, token) in tokens.iter().enumerate() {
        let Some(kind) = primitive_kind(&tokens, index) else {
            continue;
        };
        let open = index + 1;
        if tokens
            .get(open)
            .is_none_or(|token| !matches!(token.text, "(" | "!"))
        {
            continue;
        }
        let open = if tokens[open].text == "!" {
            open + 1
        } else {
            open
        };
        let Some(end) = matching_end(&tokens, open) else {
            continue;
        };
        let function = functions
            .iter()
            .rev()
            .find(|(start, end, _)| *start < index && index < *end)
            .map_or("module", |(_, _, name)| *name);
        let normalized = tokens[index..=end]
            .iter()
            .map(|token| token.text)
            .collect::<Vec<_>>()
            .join(" ");
        let hash = fingerprint(normalized.as_bytes());
        let stem = format!("{}::{function}::{kind:?}::{hash:016x}", source.path);
        let occurrence = occurrences.entry(stem.clone()).or_insert(0_usize);
        *occurrence += 1;
        records.push(ExistingControl {
            id: ControlId(format!("{stem}::{}", *occurrence)),
            source: source.path,
            function: function.to_owned(),
            line: source.text[..token.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1,
            kind,
            arguments: source.text[tokens[open].start..tokens[end].end].to_owned(),
            surface: source.surface,
            candidate_destinations: source.destinations,
        });
    }
    records
}

fn primitive_kind(tokens: &[Token<'_>], index: usize) -> Option<ControlKind> {
    let name = tokens[index].text;
    let method = index > 0 && tokens[index - 1].text == ".";
    if method {
        return match name {
            "checkbox" | "radio_value" | "drag_angle" => Some(ControlKind::Setting),
            "button" | "small_button" | "link" | "hyperlink" | "hyperlink_to" => {
                Some(ControlKind::Command)
            }
            "text_edit_singleline" | "text_edit_multiline" => Some(ControlKind::TextInput),
            "selectable_value" | "selectable_label" | "radio" => Some(ControlKind::Selection),
            "image" | "image_button" => Some(ControlKind::View),
            "label" | "colored_label" | "monospace" | "code" | "weak" | "strong" | "text" => {
                Some(ControlKind::Readout)
            }
            "heading" | "collapsing" | "header" => Some(ControlKind::Section),
            "item" => Some(ControlKind::PauseItem),
            _ => None,
        };
    }
    if index >= 3 && tokens[index - 1].text == ":" && tokens[index - 2].text == ":" {
        return match (tokens[index - 3].text, name) {
            ("Button", "new") => Some(ControlKind::Command),
            ("Checkbox" | "Slider" | "DragValue", "new") => Some(ControlKind::Setting),
            ("TextEdit", "singleline" | "multiline") => Some(ControlKind::TextInput),
            ("CollapsingHeader", "new") => Some(ControlKind::Section),
            ("ComboBox", "from_id_salt" | "from_label") => Some(ControlKind::Selection),
            ("Image" | "Plot", "new") => Some(ControlKind::View),
            _ => None,
        };
    }
    match name {
        "patchbox" | "patch_slider" | "config_toggle" | "config_slider" | "config_choice" => {
            Some(ControlKind::Setting)
        }
        _ => None,
    }
}

fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[derive(Debug)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn tokenize(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
            continue;
        }
        let start = index;
        let raw_hashes = if bytes[index] == b'r' {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] == b'#' {
                end += 1;
            }
            (end < bytes.len() && bytes[end] == b'"').then_some(end - index - 1)
        } else {
            None
        };
        if let Some(hashes) = raw_hashes {
            index += hashes + 2;
            while index < bytes.len() {
                if bytes[index] == b'"'
                    && bytes
                        .get(index + 1..index + 1 + hashes)
                        .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
                {
                    index += 1 + hashes;
                    break;
                }
                index += 1;
            }
        } else if bytes[index] == b'"' {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == b'"' {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
        } else if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
        } else {
            index += text[index..]
                .chars()
                .next()
                .expect("remaining character")
                .len_utf8();
        }
        tokens.push(Token {
            text: &text[start..index],
            start,
            end: index,
        });
    }
    tokens
}

fn matching_end(tokens: &[Token<'_>], open: usize) -> Option<usize> {
    let close = match tokens.get(open)?.text {
        "(" => ")",
        "[" => "]",
        "{" => "}",
        _ => return None,
    };
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.text == tokens[open].text {
            depth += 1;
        }
        if token.text == close && depth > 0 {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "inventory/tests.rs"]
mod tests;
