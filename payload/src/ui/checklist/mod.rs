//! The session checklist: the newest session's headset checks, each marked untested, working, or
//! not working, for the player to work through in the game ([`model`]).

mod model;

use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, Ordering},
};

use egui::{RichText, Ui};
use parking_lot::Mutex;

use crate::ui::checklist::model::{Check, Mark};

/// The checklist's feature body.
pub(crate) fn feature(ui: &mut Ui) {
    let checks = model::checks();
    if checks.is_empty() {
        ui.label("No session checklist was embedded in this build.");
        return;
    }
    let [untested, works, broken] = model::counts();
    ui.label(format!(
        "Session {}: {works} work, {broken} don't, {untested} untested of {}",
        model::SESSION,
        checks.len()
    ));
    let (path, last) = model::file_status();
    if let Some(path) = path {
        ui.label(format!("Results: {}", path.display()));
    }
    if let Some(last) = last {
        ui.colored_label(egui::Color32::LIGHT_RED, last);
    }
    let mut hide_finished = HIDE_FINISHED.load(Ordering::Relaxed);
    if ui
        .checkbox(&mut hide_finished, "Hide the checks already marked")
        .changed()
    {
        HIDE_FINISHED.store(hide_finished, Ordering::Relaxed);
    }
    ui.separator();
    for check in checks {
        let mark = model::mark(check.number);
        if hide_finished && mark != Mark::Untested {
            continue;
        }
        row(ui, check, mark);
    }
}

/// One check: its number, the three marks, its text, and a note once it is marked not working.
fn row(ui: &mut Ui, check: &Check, mark: Mark) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}.", check.number)).strong());
            for candidate in Mark::ALL {
                let text = RichText::new(candidate.label()).color(match candidate {
                    Mark::Untested => ui.visuals().text_color(),
                    Mark::Works => egui::Color32::LIGHT_GREEN,
                    Mark::Broken => egui::Color32::LIGHT_RED,
                });
                if ui.selectable_label(mark == candidate, text).clicked() && mark != candidate {
                    model::set(check, candidate, &model::note(check.number));
                }
            }
        });
        ui.label(&check.text);
        if mark == Mark::Broken {
            let mut notes = NOTES.lock();
            let note = notes
                .entry(check.number)
                .or_insert_with(|| model::note(check.number));
            let response = ui.add(
                egui::TextEdit::singleline(note)
                    .hint_text("What went wrong (optional)")
                    .desired_width(f32::INFINITY),
            );
            if response.lost_focus() && *note != model::note(check.number) {
                model::set(check, mark, note);
            }
        }
    });
}

/// Whether the checks already marked are hidden.
static HIDE_FINISHED: AtomicBool = AtomicBool::new(false);

/// The notes being edited, by check number.
static NOTES: Mutex<BTreeMap<u32, String>> = Mutex::new(BTreeMap::new());
