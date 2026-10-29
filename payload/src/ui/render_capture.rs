//! The Tools page's render capture: the render trace dump with its frame count and per-eye
//! screenshots, and the live log filter. The trace's investigation options are in the render RE
//! window ([`crate::ui::diagnostics`]).

use std::sync::atomic::{AtomicI32, Ordering};

use crate::{config, debug::trace};

/// The frame count for the editable "Dump N frames" trace button, persisted across UI frames.
pub(crate) static TRACE_FRAME_COUNT: AtomicI32 = AtomicI32::new(60);

pub(crate) fn controls(ui: &mut egui::Ui) {
    let mut cfg = config::edit();
    // Deferred so the trace button's start() doesn't re-lock CONFIG (it snapshots the config for the
    // manifest) while this guard is held -- parking_lot is not reentrant, so that self-deadlocks.
    let mut start_trace: Option<i32> = None;

    // The trace dumps the next few frames' render events under whatever every option is set to
    // (writes next to the DLL). The long capture exists for periodic artifacts whose cadence exceeds
    // the short window (e.g. the one-frame exposure/shadow pulses of issue #10).
    ui.horizontal(|ui| {
        if ui.button("Dump render trace (4 frames)").clicked() {
            start_trace = Some(4);
        }
        let mut count = TRACE_FRAME_COUNT.load(Ordering::Relaxed);
        if ui.button(format!("Dump {count} frames")).clicked() {
            start_trace = Some(count);
        }
        if ui
            .add(egui::DragValue::new(&mut count).range(1..=600).speed(1))
            .changed()
        {
            TRACE_FRAME_COUNT.store(count, Ordering::Relaxed);
        }
    });
    ui.checkbox(
        &mut cfg.stereo.diagnose_rt_screenshots,
        "Dump per-eye frames into the trace (BackBufferLinear PNG per eye per frame)",
    );

    // Release CONFIG before start(), whose manifest snapshot re-locks it.
    drop(cfg);
    if let Some(frames) = start_trace {
        trace::TraceState::start(frames);
    }

    ui.separator();
    let mut edit = FILTER_EDIT.lock();
    let mut error = FILTER_ERROR.lock();
    ui.horizontal(|ui| {
        ui.label("Log filter");
        ui.text_edit_singleline(&mut *edit)
            .on_hover_text("RUST_LOG directive syntax, e.g. warn,vr=debug,coord_frame=debug.");
        if ui.button("Apply").clicked() {
            *error = crate::logging::set_filter(&edit).err();
        }
    });
}

pub(crate) fn status(ui: &mut egui::Ui) {
    let remaining = trace::active_frames();
    if remaining > 0 {
        ui.label(format!("tracing... {remaining} frames left"));
    } else if trace::TraceState::last_path().is_some() {
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
