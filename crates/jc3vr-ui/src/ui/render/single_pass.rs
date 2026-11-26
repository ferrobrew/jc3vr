//! Single-pass stereo: the whole-feature switch, the production switches, the shader-rewrite census,
//! and the instanced eye-parity exposure readout.

use std::sync::atomic::Ordering;

use jc3vr_core::config::engine::Parts;
use jc3vr_render::{
    hooks::{draw_count::EFFECT_DEPTH_COPIES_SKIPPED, graphics_engine::shader},
    stereo::single_pass,
};
use jc3vr_view::stereo::{
    config::{STEREO_CONFIG, set_single_pass_feature},
    layout::{self, Capability},
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut impl Parts) {
    // Single-pass stereo (on by default while a VR session runs): render the G-buffer geometry once,
    // emitting both eyes via instancing + SV_ViewportArrayIndex routing, instead of the double-draw.
    ui.collapsing("Single-pass stereo", |ui| {
        // All-in/all-out toggle: flip the whole configuration together, so the one button is the
        // normal way to switch between single-pass and multipass. The individual levers below stay
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
                 the reprojected scene families, terrain, tree impostors, and the bark/foliage/occluder \
                 re-issue -- on or off together, along with native resolution, and reloads the \
                 shaders.",
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
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.reproject,
                        "Reproject the no-cb0 scene families (NPCs, props, buildings, roads)",
                    )
                    .on_hover_text(
                        "Rewrites the baked-WVP scene shaders (characters, props, buildings, roads) \
                         to post-multiply their clip by the per-eye M_eye, instead of leaving them \
                         double-drawn. Reloads shaders to apply. Sky/UI/post are excluded.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.terrain,
                        "Single-pass the tessellated terrain (VS -> HS -> DS)",
                    )
                    .on_hover_text(
                        "Rides the eye index through the terrain tessellation pipeline: the vertex \
                         shader writes it on the free TEXCOORD3.z lane, the hull forwards it, and the \
                         domain reprojects its clip by the per-eye M_eye. Covers the DrawIndexed \
                         terrain passes; the GPU-indirect near passes stay double-drawn. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .add_enabled(
                        cfg.part_mut(&STEREO_CONFIG).single_pass.terrain,
                        egui::Checkbox::new(
                            &mut cfg.part_mut(&STEREO_CONFIG).single_pass.terrain_patch_per_eye,
                            "Terrain patches per eye",
                        ),
                    )
                    .on_hover_text(
                        "Reprojects the terrain patch system's tessellated and GPU-indirect \
                         pipelines, which cannot carry an eye index, by pinning cb13 to each eye in \
                         the per-eye re-issue their draws already take. Off, both eyes of those \
                         draws come from the centred camera. Adds no draws. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.part_mut(&STEREO_CONFIG).single_pass.clouds_per_eye, "Clouds per eye")
                    .on_hover_text(
                        "Reprojects the cloud vertex shaders by M_eye. Off, the clouds take their \
                         clip position from the centred render camera in both eyes, which is \
                         wrong for asymmetric or canted eyes. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.label("Render-block re-issue:");
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.vegetation_once,
                    "Bark and foliage once for both eyes",
                )
                .on_hover_text(
                    "Draws a bark or foliage block once instead of re-issuing it per eye: each \
                     draw binds a reprojected variant of its vertex shader, plain draws take two \
                     instances, GPU-indirect draws read a copy of their arguments with the instance \
                     counts doubled, and hardware-instanced draws are re-issued per eye unless \
                     their layout has a twin. Blocks without a variant keep the whole-block \
                     re-issue. Applies at once.",
                );
                ui.label(format!(
                    "Vegetation once: {}",
                    jc3vr_render::stereo::single_pass::vegetation_once::status()
                ));
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.instanced_per_eye,
                    "Already-instanced draws per eye (buildings, vegetation)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstanced whose patched shader is bound once per eye, \
                     with both cb13 eye slots and both viewport slots pinned to that eye, so the \
                     game's own instance ids stop being read as an eye parity. Off, the batch is \
                     split alternately between the eyes -- the building flicker.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.hw_instanced_once,
                    "Hardware-instanced buildings once for both eyes",
                )
                .on_hover_text(
                    "Draws a hardware-instanced building batch once, with its input layout swapped \
                     for a twin whose per-instance step rates are doubled and its instance count \
                     doubled, so each pair of instances shares one element and the instance parity \
                     picks the eye. Draws whose layout has no twin keep the per-eye re-issue. \
                     Applies at once.",
                );
                ui.label(format!(
                    "Hardware-instanced twins: {}",
                    jc3vr_render::stereo::single_pass::hw_instanced::status()
                ));
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.indirect_per_eye,
                    "GPU-indirect draws per eye (near terrain patches, foliage)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstancedIndirect / DrawInstancedIndirect once per eye \
                     with both viewport slots pinned to that eye's half. Nothing detoured these entry \
                     points, so the near tessellating terrain patches and the foliage inherited the \
                     full double-wide viewport and were stretched 2x horizontally -- which reads as \
                     them sliding across the screen at twice the camera's rate.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.reconstruct_per_eye,
                    "Deferred lighting resolve per eye (sun shadows)",
                )
                .on_hover_text(
                    "Runs the deferred clustered-lighting resolve twice, each run scissor-masked to \
                     one eye's half of the double-wide target and reconstructing depth with that \
                     eye's own basis. Off, the one fullscreen quad covers both halves with one eye's \
                     basis, and the error turns with the camera -- the sun shadows slide across the \
                     screen. Needs the off-axis reconstruction override on.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.fullscreen_camera_per_eye,
                    "Fullscreen passes use each eye's camera position",
                )
                .on_hover_text(
                    "Stages each eye's own camera position, and the matching sun-shadow lookup \
                     correction, for every per-eye run of a split fullscreen pass. Off, both halves \
                     measure view vectors, fog distances, and shadow lookups from the centred camera.",
                );
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.pixel_camera_per_eye,
                        "Geometry shades from each eye's camera position",
                    )
                    .on_hover_text(
                        "Every pixel shader that measures a view vector or a camera distance reads \
                         the camera position of the eye whose half its pixel lies in. Off, every \
                         instance-doubled draw shades both eyes from the point between them: cloth \
                         sheen, specular, Fresnel, and reflections differ from an eye rendered on its \
                         own. Reloads shaders to apply the rewrite; turning it off takes effect at \
                         once.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.local_lights_per_eye,
                        "Local lights measured from each eye",
                    )
                    .on_hover_text(
                        "In the same rewritten pixel shaders, measures every local point and spot \
                         light from the eye whose half the pixel lies in. The engine stores the \
                         lights relative to the centred camera, so off, the lights near the camera \
                         (a held device's glow) sit where the point between the eyes sees them. \
                         Reloads shaders to apply the rewrite; turning it off takes effect at once.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                let (rewritten, declined) = jc3vr_render::stereo::single_pass::eye_camera::counts();
                ui.weak(format!(
                    "Pixel shaders reading each eye's camera: {rewritten} (declined {declined})"
                ));
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.screen_door_per_eye,
                        "Screen-door fades laid out per eye",
                    )
                    .on_hover_text(
                        "The alpha-tested and fading materials (vegetation, props, buildings, the \
                         vehicle paint's stipple) and the volumetric fog dither by a pattern hashed \
                         from the pixel position. Laid out from each eye's own origin, every eye \
                         keeps the same pixels as an eye rendered on its own. Off, the right eye's \
                         pattern is shifted by the left eye's width, and a different set of pixels \
                         survives each fade. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                let (velocities, declined) = jc3vr_render::stereo::single_pass::eye_camera::velocity_counts();
                ui.weak(format!(
                    "Velocity pixel shaders per eye: {velocities} (declined {declined})"
                ));
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.clip_origin_per_eye,
                        "Road clip origin per eye",
                    )
                    .on_hover_text(
                        "The road surfaces and the road-biased models subtract the render camera's \
                         position, not the pass camera's, before projecting. Read per eye, each eye \
                         projects them from its own position. Off, both eyes project them from the \
                         centred camera, so the road's texture slides half the eye distance across \
                         its surface. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.terrain_cull_both_eyes,
                    "Terrain culled for both eyes",
                )
                .on_hover_text(
                    "Culls the tessellated terrain patches against a frustum bounding both eyes. Off, \
                     the hull programs cull them against the centred camera's frustum, which a canted \
                     eye sees past at its outer edge: distant hills go missing there.",
                );
                let (rewritten, projected, declined) =
                    jc3vr_render::stereo::single_pass::eye_camera::screen_door_counts();
                ui.weak(format!(
                    "Pixel shaders with a per-eye screen door: {rewritten}, from a projected \
                     position {projected} (declined {declined})"
                ));
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.nvwater_per_eye,
                    "WaveWorks water per eye (parallax)",
                )
                .on_hover_text(
                    "Gives the NvWater* blocks a per-eye view. Their screen-space reads were always \
                     fine; what is missing is parallax -- the vertex shader transforms from a baked \
                     matrix in its own constant buffer, so both eyes get the collapsed centre view. \
                     Flat water at the wrong depth looks right on the mirror and wrong in the headset.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.ssdecal_geometry_per_eye,
                    "...and reproject the decal box geometry",
                )
                .on_hover_text(
                    "Sub-option of the decal fix: reprojects the decal box's baked transform per eye \
                     so its screen coverage has parallax too. The reconstruction fix alone stops the \
                     sliding; this adds depth to it.",
                );
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.ssdecal_per_eye,
                        "Screen-space decals per eye",
                    )
                    .on_hover_text(
                        "Re-uploads each eye's reconstruction basis for the SSDecal block and biases \
                         its projective depth-fetch UV into that eye's half of the double-wide \
                         buffer. Off, the decal reconstructs its surface from the wrong part of the \
                         depth buffer and the error moves with the camera. Reloads shaders to apply: \
                         the depth-fetch bias is a shader rewrite, and until the reload the already \
                         rewritten permutations keep reading it.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.weak(format!(
                    "Glare filter variants built: {}",
                    jc3vr_render::stereo::single_pass::post_filters::registered_count()
                ));
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.foveation_per_eye,
                    "Foveation per eye",
                )
                .on_hover_text(
                    "Centres the foveation's full-rate region and its peripheral drop on each eye, \
                     sized for one eye. Off, one fovea sits on the seam, sized for the double-wide \
                     frame, so each eye drops pixels at its own centre of view.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.foveation_gbuffer,
                    "Foveation over the G-buffer",
                )
                .on_hover_text(
                    "Skips the foveation's dropped pixels in the terrain's apply passes, the road \
                     junctions, and the decals, in multipass too. Off, only the models, the \
                     vegetation, and the creatures skip them.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.foveation_lighting,
                    "Foveation over the lighting",
                )
                .on_hover_text(
                    "Skips the foveation's dropped pixels in the deferred lights, the sky, and the \
                     fog gradient, and fills them in right after, before the water and the \
                     transparents, in multipass too. Off, the fill-in runs at the start of the \
                     post effects.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.foveation_quads,
                    "Foveation drops whole quads",
                )
                .on_hover_text(
                    "Drops the foveation's peripheral pixels in 2 by 2 quads, so a dropped quad runs \
                     no pixel shader at all, in multipass too. Off, single pixels are dropped and \
                     nearly every quad still shades.",
                );
                ui.checkbox(&mut cfg.part_mut(&STEREO_CONFIG).single_pass.fsr_per_eye, "FSR per eye")
                    .on_hover_text(
                        "Resolves FSR once per eye over that eye's half, each with its own history, \
                         as an eye rendered on its own is. Off, one FSR pass covers both eyes: its \
                         history moves twice as far sideways as the eye did and reaches across the \
                         seam.",
                    );
                ui.weak(format!(
                    "FSR dispatches per eye this frame: {}",
                    jc3vr_render::fsr::split_dispatch_count()
                ));
                if ui
                    .checkbox(&mut cfg.part_mut(&STEREO_CONFIG).single_pass.ssao_per_eye, "SSAO per eye")
                    .on_hover_text(
                        "Computes ambient occlusion for each eye's half in the pass's one run: each \
                         eye's basis and view rotation, taps kept to the eye's half and scaled to it, \
                         and eye-local noise. Reloads shaders to apply: it is a shader rewrite.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.ssao_exact_sampling,
                        "SSAO exact sampling",
                    )
                    .on_hover_text(
                        "Computes the ambient occlusion's coordinates, texels, and blur and upsample \
                         weights in the shader in every mode, so an eye rendered on its own and each \
                         eye of the double-wide frame read the same texels with the same weights. \
                         Off, the rasterizer and the sampler round differently at the two widths, \
                         which shows as faint vertical stripes in the occlusion on the ground. Also \
                         changes multipass. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.part_mut(&STEREO_CONFIG).single_pass.ao_volumes_per_eye,
                        "Ambient-occlusion volumes per eye",
                    )
                    .on_hover_text(
                        "Draws the ambient-occlusion boxes and spheres once for both eyes: the \
                         volume is projected into each eye, and each pixel samples its own eye's \
                         depth and maps it through its own eye's camera. Off, each eye's half reads \
                         the depth of the wrong place through the centred camera, which in tunnels \
                         and under cover shows as an edge down the middle of each eye. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.part_mut(&STEREO_CONFIG).single_pass.ssr_per_eye, "SSR per eye")
                    .on_hover_text(
                        "Traces screen-space reflections for each eye's half in the pass's one run: \
                         each eye's view ray, view, projection, and history lookup, the march kept \
                         to the eye's half, and the blur's taps clamped to it. Reloads shaders to \
                         apply: it is a shader rewrite.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.collapse_viewport_follows_target,
                    "Eye viewports follow the bound target (clouds, smoke, spotlight volumetrics)",
                )
                .on_hover_text(
                    "Splits the viewport of whatever render target the engine currently has bound, instead of always splitting the scene's double-wide one. The low-resolution clouds, particles, and spot-light cones render into a shared quarter-resolution buffer (half per axis); handing those draws a full-scene viewport magnifies them 2x about the target's origin and crops them, which is also a 2x motion gain -- clouds and smoke sliding at twice the camera's rate. A no-op everywhere else, since the two viewports agree outside those passes.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.slot13_per_eye,
                    "Non-indexed geometry per eye (decals, roads, skidmarks)",
                )
                .on_hover_text(
                    "Re-issues a non-indexed Draw once per eye, with both viewport and cb13 eye slots pinned to that eye, for the passes known to submit geometry this way. Off, every slot-13 draw gets the whole double-wide viewport -- right for a fullscreen triangle, but it stretches decals and road layers 2x horizontally, which is a 2x motion gain and reads as them sliding over the world. Default off until the pass allowlist is confirmed: read the 'slot-13 by pass' census in the log to see which passes actually arrive here.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.clustered_per_eye,
                    "Clustered light grid per eye (local lights on forward materials)",
                )
                .on_hover_text(
                    "Builds the 64-pixel froxel light grid once per eye, each run masked to that \
                     eye's half of the tile grid with that eye's projection and tile bounds, and the \
                     second run's clear suppressed so the halves compose. Off, the grid is built with \
                     eye 0's projection against the double-wide tile count, so local lights land in \
                     the wrong tiles for both eyes -- on the ~20 forward-lit families that sample it \
                     (foliage, glass, particles, blended materials) as well as the deferred resolve. \
                     Needs the per-eye resolve and the off-axis tile-bounds fix on. Declines itself \
                     if the eye seam does not fall on a tile boundary.",
                );
                ui.checkbox(
                    &mut cfg.part_mut(&STEREO_CONFIG).single_pass.viewport_bind_cache,
                    "Skip viewport binds the slots already hold",
                )
                .on_hover_text(
                    "Records every bind of the two viewport slots, the engine's and the mod's, and \
                     skips a routed draw's bind when the slots already hold what it wants, so a \
                     pass's patched draws bind the eye split once rather than once each.",
                );
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
