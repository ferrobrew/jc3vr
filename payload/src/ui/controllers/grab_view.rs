//! The grab view: its switch, what it drew, the palms' state, the candidates, and the refused
//! grabs.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::grab_view::{
        self, GrabViewStatus, draw::PRIMITIVE_BUDGET, model::CandidateKind,
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
        "{} target lines · {} rays · {} door handles · {} candidates · {} flashes · {} guns' zones · {} over budget · {} of {} primitives",
        drawn.target_lines,
        drawn.rays,
        drawn.handles,
        drawn.candidates,
        drawn.flashes,
        drawn.zones,
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
