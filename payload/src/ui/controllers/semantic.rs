//! The semantic actions: the input self-check's results, and each action set's actions with their
//! active flag, per-hand state, and bound sources.

use egui::{RichText, Ui};

use crate::{
    controllers::frame::Hand,
    ui::controllers::yes_no,
    vr::input::{
        actions::SetId,
        layout::{BoolInput, FloatInput, HandsAction, Vec2Input},
        self_check,
        semantic::{
            SemanticSet,
            frame::{self as semantic_frame, ActionState, ActionValue},
        },
        sources::{self, ActionSources},
    },
};

/// Render the section. Showing it keeps the bound sources fresh (the VR thread refreshes them once
/// a second while they are viewed), so it is meant to be collapsed when not in use.
pub(super) fn semantic_readout(ui: &mut Ui) {
    sources::note_viewed();
    let check = self_check::status();
    let frame = semantic_frame::latest();
    let cache = sources::cached();

    ui.strong("Self-check");
    match &check.report {
        Some(report) => {
            ui.label(RichText::new(report).monospace());
        }
        None => {
            ui.label("Not run yet (it runs once a hand has an interaction profile).");
        }
    }
    for finding in &check.findings {
        ui.label(RichText::new(format!("finding: {finding}")).monospace());
    }
    if ui.button("Run the self-check again").clicked() {
        self_check::request_rerun();
    }

    ui.separator();
    match &frame {
        Some(f) => ui.label(format!(
            "Mode set {} · frame age {:.0} ms",
            f.action_set.name(),
            f.published_at.elapsed().as_secs_f32() * 1000.0
        )),
        None => ui.label("No semantic frame (no session, or the controllers are disabled)."),
    };
    for set in SetId::all() {
        let active = match set {
            SetId::Hands => true,
            SetId::Semantic(s) => frame.as_ref().and_then(|f| f.set(s)).is_some(),
        };
        let entry = cache.iter().find(|c| c.set == set);
        let refreshed = entry.map_or("never queried".to_owned(), |e| {
            format!(
                "sources {:.1} s old{}",
                e.refreshed_at.elapsed().as_secs_f32(),
                if e.active {
                    ""
                } else {
                    ", queried while inactive"
                }
            )
        });
        egui::CollapsingHeader::new(format!(
            "{} · active {} · {refreshed}",
            set.name(),
            yes_no(active)
        ))
        .id_salt(("semantic_set", set.name()))
        .default_open(active)
        .show(ui, |ui| {
            let names = action_names(set);
            let unbound = entry.map_or(0, |e| {
                e.actions
                    .iter()
                    .filter(|a| a.sources.as_ref().is_ok_and(|s| s.is_empty()))
                    .count()
            });
            ui.label(format!("{} actions, {unbound} unbound", names.len()));
            egui::Grid::new(("semantic_grid", set.name()))
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("action");
                    ui.strong("left");
                    ui.strong("right");
                    ui.strong("bound sources");
                    ui.end_row();
                    for (index, (name, kind)) in names.iter().enumerate() {
                        ui.label(format!("{name} ({kind})"));
                        for hand in Hand::BOTH {
                            let state = match set {
                                SetId::Semantic(s) => frame
                                    .as_ref()
                                    .and_then(|f| f.set(s))
                                    .map(|state| state.actions[index][hand.index()]),
                                SetId::Hands => None,
                            };
                            ui.label(state.map_or("–".to_owned(), state_label));
                        }
                        let sources =
                            entry.and_then(|e| e.actions.iter().find(|a| a.name == *name));
                        ui.label(sources.map_or("not queried".to_owned(), sources_label));
                        ui.end_row();
                    }
                });
        });
    }
}

/// The name and type of every action of `set`.
fn action_names(set: SetId) -> Vec<(&'static str, &'static str)> {
    match set {
        SetId::Hands => HandsAction::ALL
            .iter()
            .map(|a| (a.name(), "pose or output"))
            .chain(BoolInput::ALL.iter().map(|i| (i.name(), "bool")))
            .chain(FloatInput::ALL.iter().map(|i| (i.name(), "float")))
            .chain(Vec2Input::ALL.iter().map(|i| (i.name(), "vec2")))
            .collect(),
        SetId::Semantic(s) => semantic_names(s),
    }
}

fn semantic_names(set: SemanticSet) -> Vec<(&'static str, &'static str)> {
    set.actions()
        .iter()
        .map(|def| (def.name, def.kind.label()))
        .collect()
}

fn state_label(state: ActionState) -> String {
    if !state.active {
        return "inactive".to_owned();
    }
    match state.value {
        ActionValue::Bool(b) => if b { "on" } else { "off" }.to_owned(),
        ActionValue::Float(f) => format!("{f:.2}"),
        ActionValue::Vec2(v) => format!("({:+.2}, {:+.2})", v.x, v.y),
    }
}

fn sources_label(sources: &ActionSources) -> String {
    match &sources.sources {
        Ok(list) if list.is_empty() => "unbound".to_owned(),
        Ok(list) => list
            .iter()
            .map(|s| match &s.localized {
                Ok(name) => format!("{name} [{}]", s.path),
                Err(_) => s.path.clone(),
            })
            .collect::<Vec<_>>()
            .join("; "),
        Err(e) => format!("error: {e}"),
    }
}
