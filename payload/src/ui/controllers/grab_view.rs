//! The grab view: its switch, what it drew, the palms' state, the candidates, the refused grabs,
//! and its settings.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::grab_view::{
        self, GrabViewConfig, GrabViewStatus, draw::PRIMITIVE_BUDGET, model::CandidateKind,
    },
};

pub(super) fn grab_view_readout(ui: &mut Ui, status: &GrabViewStatus) {
    let label = if status.enabled {
        "Hide the grab view"
    } else {
        "Show the grab view"
    };
    if ui.button(label).clicked() {
        grab_view::toggle();
    }
    if !status.enabled {
        return;
    }
    for hand in Hand::BOTH {
        ui.label(format!(
            "Palm, {} hand: {}",
            hand.label(),
            status.palms[hand.index()].map_or("no reading", |p| p.label())
        ));
    }
    let drawn = &status.drawn;
    ui.label(format!(
        "{} target lines · {} rays · {} door handles · {} candidates · {} flashes · {} over budget · {} of {} primitives",
        drawn.target_lines,
        drawn.rays,
        drawn.handles,
        drawn.candidates,
        drawn.flashes,
        drawn.dropped,
        status.primitives,
        PRIMITIVE_BUDGET
    ));
    let kinds = CandidateKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let count = status.candidates[kind.index()];
            (count > 0).then(|| format!("{count} {}", kind.label()))
        })
        .collect::<Vec<_>>();
    ui.label(format!(
        "Candidates: {} · {} reads · last read {} µs",
        if kinds.is_empty() {
            "none".to_owned()
        } else {
            kinds.join(" · ")
        },
        status.candidate_reads,
        status.candidate_read_us
    ));
    match &status.last_refusal {
        Some((hand, why)) => {
            ui.label(format!(
                "Refused grabs: {} · last, {} hand: {why}",
                status.refusals,
                hand.label()
            ));
        }
        None => {
            ui.label("Refused grabs: none");
        }
    }
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}

pub(super) fn grab_view_settings(ui: &mut Ui, v: &mut GrabViewConfig) {
    ui.checkbox(&mut v.enabled, "Draw what the grip and the reach saw");
    ui.add_enabled_ui(v.enabled, |ui| {
        ui.checkbox(&mut v.rays, "Reach rays");
        ui.checkbox(&mut v.targets, "Lines to the grip targets");
        ui.add(egui::Slider::new(&mut v.radius_m, 0.1..=10.0).text("Target radius (m)"));
        ui.checkbox(&mut v.wrist, "Palm state on the wrists");
        ui.checkbox(&mut v.refusals, "Refused grabs");
        ui.checkbox(&mut v.candidates, "Candidates near the hands");
        ui.add(
            egui::Slider::new(&mut v.candidate_radius_m, 0.1..=10.0).text("Candidate radius (m)"),
        );
        ui.add(egui::Slider::new(&mut v.max_candidates, 1..=64).text("Most candidates"));
    });
}
