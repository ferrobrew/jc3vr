//! Single-pass stereo: the whole-feature switch, the production switches, the shader-rewrite census,
//! and the instanced eye-parity exposure readout.

use std::sync::atomic::Ordering;

use jc3vr_core::config::engine::Parts;
use jc3vr_render::{
    hooks::{draw_count::EFFECT_DEPTH_COPIES_SKIPPED, graphics_engine::shader},
    stereo::single_pass,
};
use jc3vr_view::stereo::{
    config::{STEREO_CONFIG, SinglePassSwitch, set_single_pass_feature},
    layout::{self, Capability},
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut impl Parts) {
    // Single-pass stereo (on by default while a VR session runs): render the G-buffer geometry once,
    // emitting both eyes via instancing + SV_ViewportArrayIndex routing, instead of the double-draw.
    ui.collapsing("Single-pass stereo", |ui| {
        // All-in/all-out toggle: flip the whole configuration together, so the one button is the
        // normal way to switch between single-pass and multipass. The individual switches below stay
        // for bisection. Reloads either way -- on to apply the patches, off to restore the pristine
        // shaders.
        let enabled = cfg.part_mut(&STEREO_CONFIG).single_pass.enabled;
        let toggle_label = if enabled {
            "⚡ Disable single-pass stereo"
        } else {
            "⚡ Enable single-pass stereo"
        };
        if ui
            .button(toggle_label)
            .on_hover_text(
                "Flips the whole feature -- single-pass (the collapse into the double-wide target), \
                 the reprojected scene families, and terrain -- on or off together, along with \
                 native resolution, and reloads the shaders.",
            )
            .clicked()
        {
            set_single_pass_feature(cfg, !enabled);
            shader::request_reload();
        }
        ui.separator();
        ui.checkbox(
            &mut cfg.part_mut(&STEREO_CONFIG).single_pass.enabled,
            "Enable single-pass stereo",
        )
        .on_hover_text(
            "Master switch. Renders the G-buffer once with stereo-rewritten vertex shaders. \
             Forced inert without the DXVK viewport-routing capability (below).",
        );
        ui.separator();
        ui.add_enabled_ui(cfg.part_mut(&STEREO_CONFIG).single_pass.enabled, |ui| {
            for &switch in SinglePassSwitch::ALL {
                switch_row(ui, cfg, switch);
            }
        });

        let capability = layout::probe_if_needed();
        ui.label(match capability {
            Capability::Supported => "Viewport routing: supported ✓",
            Capability::Unsupported => "Viewport routing: UNSUPPORTED (single-pass will stay inert)",
            Capability::Unprobed => "Viewport routing: not yet probed (no device seen)",
        });

        let (patched, no_refs, deferred, errored) = (
            single_pass::patched_count(),
            single_pass::no_refs_count(),
            single_pass::deferred_count(),
            single_pass::errored_count(),
        );
        if patched + no_refs + deferred + errored == 0 {
            ui.label("Census: 0 shaders seen -- enable a mode above, then click Reload shaders.");
        } else {
            ui.label(format!(
                "Census: {patched} patched, {no_refs} no per-eye refs, {deferred} instance-id deferred, {errored} errored"
            ));
            if errored > 0 {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "errored > 0: a shader the offline corpus did not cover -- check the log",
                );
            }
        }
        let (velocities, declined) = single_pass::eye_camera::velocity_counts();
        ui.weak(format!(
            "Velocity pixel shaders per eye: {velocities} (declined {declined})"
        ));
        ui.weak(format!(
            "Glare filter variants built: {}",
            single_pass::post_filters::registered_count()
        ));
        let (hs_forwarded, ds_reprojected) = single_pass::terrain_counts();
        ui.label(format!(
            "Terrain: {hs_forwarded} hull forwarded, {ds_reprojected} domain reprojected"
        ));
        ui.label(format!(
            "Unread effect depth copies skipped: {}",
            EFFECT_DEPTH_COPIES_SKIPPED.load(Ordering::Relaxed)
        ));
        let s = single_pass::substitution_stats();
        ui.label(format!(
            "Recorded VS (doubled): {} | CreateVertexShader: pending {}, re-acquired [patched {}, already-cb13 {}, no-refs {}, err {}]",
            s.recorded_vs,
            s.cvs_pending,
            s.cvs_reacq_patched,
            s.cvs_reacq_cb13,
            s.cvs_reacq_no_refs,
            s.cvs_reacq_err,
        ));
        instanced_exposure_readout(ui);
        ui.horizontal(|ui| {
            if ui.button("Reload shaders").clicked() {
                shader::request_reload();
            }
            ui.label("re-runs the census over a fresh shader-creation pass");
        });
    });
}

/// One switch's row: its checkbox, greyed out while the switch it acts beneath is off, a shader
/// reload when the switch is a creation-time rewrite, and the switch's own readout.
fn switch_row(ui: &mut egui::Ui, cfg: &mut impl Parts, switch: SinglePassSwitch) {
    let config = &mut cfg.part_mut(&STEREO_CONFIG).single_pass;
    let available = switch
        .requires()
        .is_none_or(|required| required.get(config));
    let mut on = switch.get(config);
    let changed = ui
        .add_enabled(available, egui::Checkbox::new(&mut on, switch.label()))
        .on_hover_text(switch.hover())
        .changed();
    if changed {
        switch.set(config, on);
        if switch.rebuilds_shaders() {
            shader::request_reload();
        }
    }
    switch_readout(ui, switch);
}

/// The readout a switch's row carries beneath it, for the switches whose work is counted.
fn switch_readout(ui: &mut egui::Ui, switch: SinglePassSwitch) {
    match switch {
        SinglePassSwitch::VegetationOnce => {
            ui.label(format!(
                "Vegetation once: {}",
                single_pass::vegetation_once::status()
            ));
        }
        SinglePassSwitch::HwInstancedOnce => {
            ui.label(format!(
                "Hardware-instanced twins: {}",
                single_pass::hw_instanced::status()
            ));
        }
        SinglePassSwitch::LocalLightsPerEye => {
            let (rewritten, declined) = single_pass::eye_camera::counts();
            ui.weak(format!(
                "Pixel shaders reading each eye's camera: {rewritten} (declined {declined})"
            ));
        }
        SinglePassSwitch::ScreenDoorPerEye => {
            let (rewritten, projected, declined) = single_pass::eye_camera::screen_door_counts();
            ui.weak(format!(
                "Pixel shaders with a per-eye screen door: {rewritten}, from a projected position \
                 {projected} (declined {declined})"
            ));
        }
        SinglePassSwitch::FsrPerEye => {
            ui.weak(format!(
                "FSR dispatches per eye this frame: {}",
                jc3vr_render::fsr::split_dispatch_count()
            ));
        }
        _ => {}
    }
}

/// How much of the collapse's frame the already-instanced eye-parity case covers, and how much of that
/// the per-eye re-issue is handling. The case: an instanced draw's instance ids are the game's own, so
/// the patched shader's `SV_InstanceID & 1` reads them as an eye parity and the batch would be split
/// between the eyes (or, at one instance, land in the left eye alone). *Handled* draws were re-issued
/// once per eye; *exposed* ones still carry the artifact. The per-shader list says which families are
/// paying the extra submissions.
fn instanced_exposure_readout(ui: &mut egui::Ui) {
    let report = single_pass::instanced_exposure();
    if report.frames == 0 {
        ui.label("Instanced eye-parity: no single-pass geometry frames measured yet.");
        return;
    }
    let last = report.last_frame;
    ui.label(format!(
        "Instanced eye-parity: {} handled, {} exposed of {} DrawIndexedInstanced last frame | mean \
         over {} frames: {:.1} handled ({:.1} instances) + {:.1} exposed ({:.1} single-instance, \
         {:.1} split batches) of {:.1}, peak {} instances",
        last.handled,
        last.affected,
        last.total,
        report.frames,
        report.mean_handled,
        report.mean_handled_instances,
        report.mean_affected,
        report.mean_affected_single_instance,
        report.mean_affected_multi_instance,
        report.mean_total,
        report.peak_instances,
    ))
    .on_hover_text(
        "Draws where a patched vertex shader was bound inside the G-buffer range under the collapse, \
         excluding the mod's own promoted and per-eye re-issued draws. Handled = re-issued once per \
         eye with cb13 and the viewport pinned to that eye; exposed = still split by instance parity, \
         which is what the toggle above turns into handled.",
    );
    ui.label(format!(
        "By range: in-range {} patched + {} unpatched | out-of-range {} patched ({} instances) + {} \
         unpatched | mean out-of-range patched over {} frames: {:.1} draws, {:.1} instances",
        last.handled + last.affected,
        last.in_range_unpatched,
        last.out_of_range_patched,
        last.out_of_range_patched_instances,
        last.out_of_range_unpatched,
        report.frames,
        report.mean_out_of_range_patched,
        report.mean_out_of_range_patched_instances,
    ))
    .on_hover_text(
        "Every DrawIndexedInstanced of the frame, split by whether a patched vertex shader was bound \
         and whether the draw was inside the G-buffer range. Only the in-range patched draws are \
         eye-split; the out-of-range patched ones still write SV_ViewportArrayIndex from instance \
         parity, so they need both viewport slots to hold the same region.",
    );
    let offenders = single_pass::instanced_offenders(8);
    if offenders.is_empty() {
        return;
    }
    ui.collapsing("Instanced patched-shader draws by shader (sampled)", |ui| {
        ui.label(
            "Attributing a draw to a shader takes a lock, so it runs only on diagnostic frames. \
             These counts are a sample and are not comparable in absolute terms with the exhaustive \
             totals above -- read them for the ranking between shaders, not the magnitudes.",
        );
        for offender in offenders {
            let name = offender
                .name
                .unwrap_or_else(|| format!("<unnamed {:#x}>", offender.shader));
            ui.label(format!(
                "{name}: in-range {} draws / {} instances, out-of-range {} draws / {} instances",
                offender.draws,
                offender.instances,
                offender.out_of_range_draws,
                offender.out_of_range_instances
            ));
        }
    });
}
