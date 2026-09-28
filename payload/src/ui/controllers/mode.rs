//! The mode detector and the bindings' injection.

use egui::Ui;

use crate::{
    controllers::{
        bindings::{Idle, TickReport, hold::Phase, inject::Outcome},
        mode::{Detection, ModeSnapshot},
    },
    ui::controllers::yes_no,
};

pub(super) fn mode_readout(
    ui: &mut Ui,
    detection: Detection,
    snapshot: Option<&ModeSnapshot>,
    report: &TickReport,
) {
    ui.label(format!(
        "Mode: {:?}{}{}",
        detection.mode,
        detection
            .traversal
            .map_or(String::new(), |t| format!(" · traversal {t:?}")),
        detection
            .held
            .map_or(String::new(), |h| format!(" · held ({h:?})"))
    ));
    if let Some(s) = snapshot {
        ui.label(format!(
            "UI signals: clock stopped {}, screen focus {}, mod panel {} · teleporting {}",
            yes_no(s.ui.clock_stopped),
            yes_no(s.ui.screen_focus),
            yes_no(s.ui.mod_panel),
            yes_no(s.teleporting)
        ));
        ui.label(match &s.character {
            Some(c) => format!(
                "State words {:016x} {:016x} {:016x} · vehicle {:?} · wire {} · reeled {}",
                c.state_bits[0],
                c.state_bits[1],
                c.state_bits[2],
                c.vehicle,
                yes_no(c.grapple_wire),
                yes_no(c.reeled_in)
            ),
            None => "Character: not readable".to_owned(),
        });
    }
    ui.separator();
    match report.idle {
        Some(idle) => {
            ui.label(format!("Injection idle: {}", idle_label(idle)));
        }
        None => {
            ui.label(format!(
                "Injecting · body turn {:+.2} · {} writes last tick",
                report.body_turn,
                report.writes.len()
            ));
        }
    }
    egui::Grid::new("controller_writes")
        .striped(true)
        .show(ui, |ui| {
            for w in &report.writes {
                ui.label(format!("{:?}", w.map));
                ui.label(format!("{:?}", w.action));
                ui.label(format!("{:.2}", w.value));
                ui.label(match w.phase {
                    Phase::Value => "value".to_owned(),
                    Phase::Clicked => "clicked".to_owned(),
                    Phase::Held { seconds } => format!("held {seconds:.2} s"),
                    Phase::Released { seconds } => format!("released after {seconds:.2} s"),
                });
                ui.label(match w.outcome {
                    Outcome::Applied => "applied",
                    Outcome::KeptPhysicalPress => "kept a physical press",
                    Outcome::Unresolved => "unresolved",
                });
                ui.end_row();
            }
        });
}

fn idle_label(idle: Idle) -> &'static str {
    match idle {
        Idle::Disabled => "the bindings are off",
        Idle::GamepadScheme => "the gamepad scheme is in effect",
        Idle::NoControllers => "no active controller",
        Idle::ModPanel => "the mod panel is up",
    }
}
