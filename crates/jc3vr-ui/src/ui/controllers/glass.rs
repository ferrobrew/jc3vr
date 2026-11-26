//! Breaking glass: its switches and tuning, and what its strikes did.

use egui::Ui;
use jc3vr_gameplay::controllers::glass::{GlassConfig, GlassStatus, bullets, strike::Outcome};

pub(super) fn glass_settings(ui: &mut Ui, g: &mut GlassConfig) {
    ui.checkbox(&mut g.enabled, "Break glass by hand")
        .on_hover_text(
            "A palm or a held gun that strikes a window cracks it as a bullet does, from outside \
             or inside the car.",
        );
    ui.add_enabled_ui(g.enabled, |ui| {
        ui.checkbox(&mut g.weapons, "Held guns strike too");
        ui.checkbox(&mut g.shatter, "Bring the pane down")
            .on_hover_text(
                "A hard enough strike, or enough damage, hides the whole pane. The game has no \
                 broken-window state of its own; its glass only cracks over.",
            );
        ui.checkbox(&mut g.sound, "Impact sound and effect");
        ui.add_enabled(
            g.shatter,
            egui::Checkbox::new(
                &mut g.bullets_pass,
                "Bullets pass through a pane brought down",
            ),
        )
        .on_hover_text(
            "Hiding a pane leaves its glass shapes, so a bullet would still crack it, hit the car, \
             and maybe stop there. With this on it passes cleanly, as the game's own penetration \
             passes an object.",
        );
        ui.collapsing("Tuning", |ui| {
            let t = &mut g.tuning;
            ui.add(egui::Slider::new(&mut t.min_speed_mps, 0.5..=6.0).text("Slowest strike (m/s)"));
            ui.add(
                egui::Slider::new(&mut t.full_speed_mps, 1.0..=12.0).text("Full damage at (m/s)"),
            );
            ui.add(egui::Slider::new(&mut t.min_damage, 0.0..=1.0).text("Damage, slowest"));
            ui.add(egui::Slider::new(&mut t.max_damage, 0.0..=1.0).text("Damage, full"))
                .on_hover_text(
                    "A bullet adds a fixed strength at the point it hits; compare a shot.",
                );
            ui.add(
                egui::Slider::new(&mut t.shatter_speed_mps, 2.0..=20.0)
                    .text("Shatter at once at (m/s)"),
            );
            ui.add(egui::Slider::new(&mut t.shatter_total, 0.5..=10.0).text("Shatter at damage"));
            ui.add(egui::Slider::new(&mut t.cooldown_s, 0.0..=1.0).text("Cooldown (s)"));
            ui.add(egui::Slider::new(&mut t.palm_lead_m, 0.0..=0.2).text("Palm reach (m)"));
            ui.add(egui::Slider::new(&mut t.tip_lead_m, 0.0..=0.2).text("Muzzle reach (m)"));
            ui.add(egui::Slider::new(&mut t.pistol_reach_m, 0.0..=0.5).text("Pistol length (m)"));
            ui.add(egui::Slider::new(&mut t.max_gap_s, 0.02..=0.5).text("Longest sample gap (s)"));
        });
    });
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
