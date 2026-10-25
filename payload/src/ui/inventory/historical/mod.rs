//! Frozen source-level obligations from the pre-migration UI.
//!
//! Token witnesses preserve setting aliases and attached command effects without coupling to
//! function names. Owners are reviewed assignments, not proof of production dispatch or rendering.
//! Runtime domains, helper effects, enabled gates, and widget activation need production-body tests.
//! Current witness v2 continues through nested widget arguments; pause-item suffixes retain v1.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::ui::inventory::{
    ControlKind, FEATURES, SourceFile, matching_end, primitive_kind, tokenize,
};

#[derive(Debug, Deserialize)]
pub(super) struct Baseline {
    pub schema: u32,
    pub commit: String,
    pub scanner: String,
    pub source_sha256: HashMap<String, String>,
    pub controls: Vec<Obligation>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Obligation {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub semantic: Witness,
}

#[derive(Debug, Deserialize)]
pub(super) struct Mapping {
    pub schema: u32,
    pub baseline_commit: String,
    pub scanner: String,
    pub consolidations: Vec<Consolidation>,
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Consolidation {
    pub id: String,
    pub old_ids: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct Assignment {
    pub old_id: String,
    pub path: String,
    pub kind: String,
    pub semantic: Witness,
    pub occurrence: usize,
    pub expected_matches: usize,
    pub owners: Vec<Owner>,
    pub route_evidence: Vec<String>,
    pub review: String,
    pub consolidation: Option<String>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash)]
pub(super) struct Witness {
    pub call: String,
    pub bindings: Vec<String>,
    pub effect: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash)]
pub(super) struct Owner {
    pub feature: String,
    pub section: Section,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Section {
    Controls,
    Status,
    Visualizations,
    Global,
    Pause,
    Parameterized,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Issue {
    MissingAssignment(String),
    UnknownId(String),
    DuplicateAssignment(String),
    InvalidReview(String),
    MissingTarget(String),
    TargetCount(String, usize, usize),
    UnapprovedConsolidation(String),
}

pub(super) fn fixtures() -> (Baseline, Mapping) {
    (
        serde_json::from_str(include_str!("baseline.json")).expect("historical UI baseline"),
        serde_json::from_str(include_str!("mapping.json")).expect("reviewed UI assignments"),
    )
}

pub(super) fn parity(baseline: &Baseline, mapping: &Mapping, sources: &[SourceFile]) -> Vec<Issue> {
    let obligations: HashMap<_, _> = baseline.controls.iter().map(|row| (&row.id, row)).collect();
    let mut issues = Vec::new();
    let mut assigned = HashSet::new();
    let mut targets: HashMap<_, Vec<&Assignment>> = HashMap::new();
    let current: HashMap<_, _> = sources
        .iter()
        .map(|source| (source.path, witnesses(source.text)))
        .collect();
    for row in &mapping.assignments {
        if !assigned.insert(&row.old_id) {
            issues.push(Issue::DuplicateAssignment(row.old_id.clone()));
        }
        let Some(old) = obligations.get(&row.old_id) else {
            issues.push(Issue::UnknownId(row.old_id.clone()));
            continue;
        };
        if row.review.is_empty()
            || row.owners.is_empty()
            || row.route_evidence.is_empty()
            || row.owners.iter().collect::<HashSet<_>>().len() != row.owners.len()
            || row.owners.iter().any(|owner| !valid_owner(owner))
            || !reviewed_kind(old, row, mapping)
            || old.semantic.call.is_empty()
            || !baseline.source_sha256.contains_key(&old.path)
            || row.occurrence == 0
            || row.occurrence > row.expected_matches
        {
            issues.push(Issue::InvalidReview(row.old_id.clone()));
        }
        let count = current.get(row.path.as_str()).map_or(0, |controls| {
            controls
                .iter()
                .filter(|(kind, semantic)| kind == &row.kind && semantic == &row.semantic)
                .count()
        });
        if count == 0 {
            issues.push(Issue::MissingTarget(row.old_id.clone()));
        } else if count != row.expected_matches {
            issues.push(Issue::TargetCount(
                row.old_id.clone(),
                row.expected_matches,
                count,
            ));
        }
        targets
            .entry((&row.path, &row.kind, &row.semantic, row.occurrence))
            .or_default()
            .push(row);
    }
    for old in &baseline.controls {
        if !assigned.contains(&old.id) {
            issues.push(Issue::MissingAssignment(old.id.clone()));
        }
    }
    let mut used_consolidations = HashSet::new();
    for rows in targets.values() {
        if rows.len() == 1 && rows[0].consolidation.is_none() {
            continue;
        }
        let approved = mapping.consolidations.iter().find(|group| {
            !group.reason.is_empty()
                && rows.len() == group.old_ids.len()
                && group.old_ids.iter().collect::<HashSet<_>>().len() == group.old_ids.len()
                && rows.iter().all(|row| {
                    row.consolidation.as_ref() == Some(&group.id)
                        && group.old_ids.contains(&row.old_id)
                        && row.owners == rows[0].owners
                })
        });
        if let Some(group) = approved {
            if !used_consolidations.insert(&group.id) {
                issues.push(Issue::UnapprovedConsolidation(group.id.clone()));
            }
        } else {
            issues.push(Issue::UnapprovedConsolidation(rows[0].old_id.clone()));
        }
    }
    for group in &mapping.consolidations {
        if !used_consolidations.contains(&group.id) {
            issues.push(Issue::UnapprovedConsolidation(group.id.clone()));
        }
    }
    issues
}

fn reviewed_kind(old: &Obligation, row: &Assignment, mapping: &Mapping) -> bool {
    if old.kind == row.kind {
        return true;
    }
    // The reviewed turn consolidation replaces selectable choices with radio settings.
    old.kind == "Selection"
        && row.kind == "Setting"
        && old.semantic.effect.is_empty()
        && row.semantic.effect.is_empty()
        && old
            .semantic
            .call
            .strip_prefix("selectable_value ")
            .zip(row.semantic.call.strip_prefix("radio_value "))
            .is_some_and(|(before, after)| before == after)
        && mapping.consolidations.iter().any(|group| {
            row.consolidation.as_ref() == Some(&group.id)
                && group.old_ids.contains(&old.id)
                && !group.reason.is_empty()
        })
}

fn valid_owner(owner: &Owner) -> bool {
    match (owner.feature.as_str(), owner.section) {
        ("GlobalChrome", Section::Global) | ("SharedHelper", Section::Parameterized) => true,
        (
            "PauseCalibration" | "PauseComfort" | "PauseControls" | "PauseDisplay",
            Section::Pause,
        ) => true,
        (_, Section::Controls | Section::Status | Section::Visualizations) => FEATURES
            .iter()
            .any(|feature| format!("{:?}", feature.id) == owner.feature),
        _ => false,
    }
}

fn witnesses(text: &str) -> Vec<(String, Witness)> {
    let tokens = tokenize(text);
    let mut controls = Vec::new();
    let functions: Vec<_> = (0..tokens.len().saturating_sub(2))
        .filter(|&i| tokens[i].text == "fn" && tokens[i + 2].text == "(")
        .filter_map(|i| {
            let start = (i + 2..tokens.len()).find(|&j| tokens[j].text == "{")?;
            Some((start, matching_end(&tokens, start)?))
        })
        .collect();
    for index in 0..tokens.len() {
        let Some(kind) = primitive_kind(&tokens, index) else {
            continue;
        };
        if !matches!(
            kind,
            ControlKind::Setting
                | ControlKind::Command
                | ControlKind::Selection
                | ControlKind::TextInput
                | ControlKind::PauseItem
        ) || tokens
            .get(index + 1)
            .is_none_or(|token| !matches!(token.text, "(" | "!"))
        {
            continue;
        }
        let open = index + 1 + usize::from(tokens[index + 1].text == "!");
        let Some(end) = matching_end(&tokens, open) else {
            continue;
        };
        let body = functions
            .iter()
            .rev()
            .find(|(start, stop)| *start < index && index < *stop)
            .map_or(0, |(start, _)| *start);
        let mut effect = Vec::new();
        let mut delimiters = Vec::new();
        let mut effect_end = None;
        let mut cursor = end + 1;
        while cursor < tokens.len() {
            match tokens[cursor].text {
                ";" | "}" if delimiters.is_empty() => break,
                "(" => delimiters.push(")"),
                "[" => delimiters.push("]"),
                ")" | "]" if delimiters.last() == Some(&tokens[cursor].text) => {
                    delimiters.pop();
                }
                "{" => {
                    let Some(stop) = matching_end(&tokens, cursor) else {
                        break;
                    };
                    effect_end = Some(stop);
                    // A block inside a builder argument can precede the attached click handler.
                    if kind == ControlKind::PauseItem || delimiters.is_empty() {
                        break;
                    }
                    cursor = stop;
                }
                _ => {}
            }
            cursor += 1;
        }
        if let Some(stop) = effect_end {
            effect = tokens[end + 1..=stop]
                .iter()
                .map(|token| token.text)
                .collect();
        }
        let mut names: HashSet<_> = tokens[index..=end].iter().map(|token| token.text).collect();
        names.extend(effect.iter().copied());
        let mut bindings = Vec::new();
        for _ in 0..8 {
            let previous = names.len();
            for i in body + 1..index {
                if tokens[i].text != "let" {
                    continue;
                }
                let j = i + 1 + usize::from(tokens[i + 1].text == "mut");
                if !names.contains(tokens[j].text) {
                    continue;
                }
                let Some(stop) = (j..index).find(|&k| tokens[k].text == ";") else {
                    continue;
                };
                let binding = tokens[i..=stop]
                    .iter()
                    .map(|token| token.text)
                    .collect::<Vec<_>>()
                    .join(" ");
                if !bindings.contains(&binding) {
                    bindings.push(binding);
                }
                names.extend(tokens[i..=stop].iter().map(|token| token.text));
            }
            if names.len() == previous {
                break;
            }
        }
        bindings.sort();
        controls.push((
            format!("{kind:?}"),
            Witness {
                call: tokens[index..=end]
                    .iter()
                    .map(|token| token.text)
                    .collect::<Vec<_>>()
                    .join(" "),
                bindings,
                effect: effect.join(" "),
            },
        ));
    }
    controls
}

#[cfg(test)]
mod tests;
