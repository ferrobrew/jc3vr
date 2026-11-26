//! Breaking glass: the bullets' switch, and what its strikes did.

use egui::Ui;
use jc3vr_physical::controllers::glass::{GlassConfig, GlassStatus, bullets, strike::Outcome};

pub(super) fn glass_settings(ui: &mut Ui, g: &mut GlassConfig) {
    ui.checkbox(
        &mut g.bullets_pass,
        "Bullets pass through a pane brought down",
    )
    .on_hover_text(
        "Hiding a pane leaves its glass shapes, so a bullet would still crack it, hit the car, and \
         maybe stop there. With this on it passes cleanly, as the game's own penetration passes an \
         object.",
    );
}

pub(super) fn glass_readout(ui: &mut Ui, status: &GlassStatus) {
    if let Some(idle) = status.idle {
        ui.label(format!("Glass: standing down ({}).", idle.label()));
    }
    ui.label(format!(
        "Glass: {} sweeps, {} met glass ({} named no pane), {} strikes, {} panes down, {} \
         refused (model draws more than glass).",
        status.sweeps,
        status.glass_hits,
        status.no_pane,
        status.strikes,
        status.shatters,
        status.shared_models
    ));
    ui.label(format!(
        "Glass: {} bullets passed through panes brought down; intact glass stops a bullet {}.",
        bullets::passes(),
        bullets::collide_factor().map_or("(not yet read)".to_owned(), |f| format!(
            "{:.0}% of the time",
            f * 100.0
        ))
    ));
    if let Some(last) = status.last {
        let outcome = match last.outcome {
            Outcome::Damage => "cracked".to_owned(),
            Outcome::Shatter(cause) if last.hidden => format!("brought down by {}", cause.label()),
            Outcome::Shatter(cause) => format!("would shatter ({}) but stays", cause.label()),
        };
        ui.label(format!(
            "Last strike: {} at {:.1} m/s, damage {:.2}, pane total {:.2}, {outcome}.",
            last.striker.label(),
            last.speed_mps,
            last.damage,
            last.total
        ));
    }
    if let Some(error) = status.last_error {
        ui.label(format!("Last error: {error}"));
    }
}
