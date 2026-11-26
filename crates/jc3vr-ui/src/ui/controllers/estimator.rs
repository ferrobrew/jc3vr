//! The learned body estimator: its switch, submit rate, and weights; the estimate's policy tuning in
//! the yaw; and the live readout of the executor and the estimate against the head and the rig.

use body_rig::math::angle::{heading, wrap};
use egui::Ui;
use jc3vr_body::controllers::body::{
    estimator, estimator::config::DEFAULT_WEIGHTS_FILE, rig::root::RootStatus,
};
use jc3vr_gameplay::controllers::config::ControllersConfig;
use jc3vr_view::headpose;

pub(super) fn estimator_readout(ui: &mut Ui, root: &RootStatus) {
    let executor = estimator::executor_status();
    ui.label(format!(
        "Estimator: {} · {} submitted · {} read · {} dropped · window {} poses · record {}",
        executor.stage,
        executor.submitted,
        executor.read,
        executor.dropped,
        estimator::producer::window_len(),
        executor
            .record
            .map_or_else(|| "–".to_owned(), |d| format!("{} µs", d.as_micros())),
    ));
    if let Some(path) = &executor.weights {
        ui.small(format!("Weights: {}", path.display()));
    }
    if let Some(error) = &executor.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    let Some(estimate) = estimator::latest() else {
        ui.label("No estimate.");
        return;
    };
    let head = headpose::xr::cockpit_pose().map(|pose| heading(pose.orientation));
    let relative = |yaw: f32| {
        head.map_or_else(
            || format!("{:+.1}°", yaw.to_degrees()),
            |head| {
                format!(
                    "{:+.1}° (head {:+.1}°)",
                    yaw.to_degrees(),
                    wrap(yaw - head).to_degrees()
                )
            },
        )
    };
    ui.label(format!(
        "Estimate: pelvis {} · chest {} · age {} ms{} · readback {} ms · {}",
        relative(estimate.pelvis_yaw),
        relative(estimate.chest_yaw),
        estimate.age.as_millis(),
        if estimate.stale() { " (stale)" } else { "" },
        estimate.readback.as_millis(),
        estimate.posture.label(),
    ));
    if let Some(rig) = root.frame.rig {
        let o = rig.output;
        ui.label(format!(
            "Rig: legs {} · chest {} · the estimate's say {:.2} · smoothed pelvis {}",
            relative(o.body_yaw),
            relative(o.chest_yaw),
            o.estimate_weight,
            o.estimate_pelvis_yaw.map_or_else(
                || "–".to_owned(),
                |yaw| format!("{:+.1}°", yaw.to_degrees())
            ),
        ));
    }
}

pub(super) fn estimator_settings(ui: &mut Ui, controllers: &mut ControllersConfig) {
    ui.collapsing("Learned body estimate", |ui| {
        let c = &mut controllers.body.estimator;
        ui.checkbox(&mut c.enabled, "Run the body estimator")
            .on_hover_text(
                "Run EgoPoser on the GPU under the controller scheme and let the grounded rig's \
                 legs and chest follow its estimate. Off, the yaw is the head's rules alone.",
            );
        let mut path = c.weights_path.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("Weights");
            if ui
                .text_edit_singleline(&mut path)
                .on_hover_text(format!(
                    "The safetensors file. Empty: {DEFAULT_WEIGHTS_FILE} next to the payload DLL."
                ))
                .changed()
            {
                c.weights_path = (!path.trim().is_empty()).then(|| path.trim().to_owned());
            }
        });
        ui.checkbox(&mut controllers.overlay.body_estimate, "Floor arrows")
            .on_hover_text(
                "Arrows under the eyes for the estimate's pelvis and chest, and the rig's legs and \
                 chest beside them.",
            );
        let e = &mut controllers.body.rig.yaw.estimate;
        ui.add(egui::Slider::new(&mut e.blend_s, 0.0..=3.0).text("Blend in and out (s)"));
        ui.add(egui::Slider::new(&mut e.smooth_s, 0.0..=1.5).text("Smoothing (s)"))
            .on_hover_text("The smoothing time of the spring that follows the estimate.");
        ui.add(egui::Slider::new(&mut e.deadband_deg, 0.0..=45.0).text("Deadband (°)"))
            .on_hover_text(
                "How far the smoothed estimate may be from the legs before it carries them.",
            );
        ui.add(egui::Slider::new(&mut e.slew_deg_s, 10.0..=720.0).text("Fastest turn (°/s)"));
        ui.add(egui::Slider::new(&mut e.centre_deg_s, 0.0..=120.0).text("Settle rate (°/s)"))
            .on_hover_text("How fast the legs settle onto a still estimate.");
        ui.add(egui::Slider::new(&mut e.still_deg_s, 0.0..=90.0).text("Still below (°/s)"));
        ui.add(egui::Slider::new(&mut e.still_s, 0.0..=5.0).text("Still for (s)"));
        ui.add(egui::Slider::new(&mut e.chest_weight, 0.0..=1.0).text("Pull on the chest"));
        ui.add(egui::Slider::new(&mut e.stick_blend_s, 0.0..=3.0).text("Stick hand-back (s)"))
            .on_hover_text("How long the stick's walk takes to hand the legs back to the head.");
        ui.add(egui::Slider::new(&mut e.fresh_s, 0.0..=2.0).text("Fresh for (s)"));
        ui.add(egui::Slider::new(&mut e.stale_s, 0.0..=5.0).text("Stale after (s)"))
            .on_hover_text("An estimate this old counts for nothing; it fades from the fresh age.");
    });
}
