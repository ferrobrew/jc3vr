//! The Tools page's render capture: the render trace over the next N frames, and the live log
//! filter. Every probe the trace records (and the brightness-step auto-capture) is toggled from the
//! render RE window ([`crate::ui::diagnostics`]).

use std::sync::atomic::Ordering;

use crate::debug::trace::{CAPTURE_FRAMES, TraceState, active_frames};

pub(crate) fn controls(ui: &mut egui::Ui) {
    // The trace records the next N frames' render events under whatever every option is set to.
    // Long captures exist for periodic artifacts whose cadence exceeds a short window (e.g. the
    // one-frame exposure/shadow pulses of issue #10).
    ui.horizontal(|ui| {
        let mut count = CAPTURE_FRAMES.load(Ordering::Relaxed);
        if ui.button(format!("Capture {count} frames")).clicked() {
            TraceState::start(count);
        }
        if ui
            .add(egui::DragValue::new(&mut count).range(1..=600).speed(1))
            .changed()
        {
            CAPTURE_FRAMES.store(count, Ordering::Relaxed);
        }
    });

    ui.separator();
    let mut edit = FILTER_EDIT.lock();
    let mut error = FILTER_ERROR.lock();
    ui.horizontal(|ui| {
        ui.label("Log filter");
        ui.text_edit_singleline(&mut *edit)
            .on_hover_text("RUST_LOG directive syntax, e.g. warn,vr=debug,stereo=debug.");
        if ui.button("Apply").clicked() {
            *error = crate::logging::set_filter(&edit).err();
        }
    });
}

pub(crate) fn status(ui: &mut egui::Ui) {
    let remaining = active_frames();
    if remaining > 0 {
        ui.label(format!("tracing... {remaining} frames left"));
    } else if TraceState::last_path().is_some() {
        ui.label("dumped");
    } else {
        ui.label("(writes to traces/<stamp>/)");
    }
    ui.label(match crate::logging::active_spec() {
        Some(spec) => format!("Active filter: {spec}"),
        None => "Active filter: (launch RUST_LOG, INFO floor)".to_string(),
    });
    if let Some(e) = FILTER_ERROR.lock().as_ref() {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
}

/// The in-progress log-filter text, kept across frames until applied.
static FILTER_EDIT: parking_lot::Mutex<String> = parking_lot::Mutex::new(String::new());
/// The last log-filter apply error, shown until the next successful apply.
static FILTER_ERROR: parking_lot::Mutex<Option<String>> = parking_lot::Mutex::new(None);
