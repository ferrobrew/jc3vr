//! Single-pass stereo (experimental): the bring-up levers, the shader-rewrite census, and the
//! instanced eye-parity exposure readout.

use crate::{
    config,
    far_field::FarFieldMode,
    hooks::graphics_engine::shader,
    stereo::{
        config::set_single_pass_feature,
        single_pass::{self, Capability},
    },
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    // Single-pass stereo (experimental, off by default): render the G-buffer geometry once, emitting
    // both eyes via instancing + SV_ViewportArrayIndex routing, instead of the double-draw. The
    // pipeline is under construction; the census dry-run below is safe (no rendering change) and
    // reports how the vertex-shader rewriter fares against the game's real shader set.
    ui.collapsing("Single-pass stereo (experimental)", |ui| {
        // All-in/all-out toggle: flip the whole work-in-progress configuration together, so the one
        // button is the normal way to turn the feature on or off. The individual levers below stay for
        // bring-up. Reloads either way -- on to apply the patches, off to restore the pristine shaders.
        let enabled = cfg.stereo.single_pass.enabled;
        let toggle_label = if enabled {
            "⚡ Disable single-pass stereo"
        } else {
            "⚡ Enable single-pass stereo"
        };
        if ui
            .button(toggle_label)
            .on_hover_text(
                "Flips the whole feature -- single-pass, dual-eye, collapse, double-wide, the \
                 reprojected scene families, terrain, tree impostors, and the bark/foliage/occluder \
                 re-issue -- on or off together, along with native resolution. Clears the census \
                 dry-run and reloads the shaders.",
            )
            .clicked()
        {
            set_single_pass_feature(cfg, !enabled);
            shader::request_reload();
        }
        ui.separator();
        ui.checkbox(
            &mut cfg.stereo.single_pass.enabled,
            "Enable single-pass stereo (experimental)",
        )
        .on_hover_text(
            "Master switch. Renders the G-buffer once with stereo-rewritten vertex shaders. \
             Forced inert without the DXVK viewport-routing capability (below).",
        );
        ui.checkbox(
            &mut cfg.stereo.single_pass.patch_dryrun,
            "Census only (dry-run: patch + tally, do not substitute -- safe)",
        )
        .on_hover_text(
            "Runs the vertex-shader stereo rewrite on every shader at creation and counts the \
             outcomes, without changing rendering. Validates the rewriter against real shaders.",
        );
        ui.checkbox(
            &mut cfg.stereo.single_pass.dump_vs_name_census,
            "Dump the vertex-shader name census on the next shader reload",
        )
        .on_hover_text(
            "Records each vertex shader's name against its rewrite class and writes them to the \
             session directory when the shaders next reload. The census only sees shaders created \
             while it is on, so turn it on, visit the area whose families you want, and reload.",
        );
        ui.separator();
        ui.label("Enable top-to-bottom; all are needed together for a clean image:");
        ui.add_enabled_ui(cfg.stereo.single_pass.enabled, |ui| {
            ui.checkbox(
                &mut cfg.stereo.single_pass.dual_eye,
                "Dual-eye: distinct per-eye cb13 + eye-half viewports + instance doubling",
            )
            .on_hover_text(
                "Makes the eyes diverge. On its own (no double-wide, no collapse) each eye \
                 renders into half of a per-eye target -- squished; a bisection step.",
            );
            ui.add_enabled_ui(cfg.stereo.single_pass.dual_eye, |ui| {
                ui.checkbox(
                    &mut cfg.stereo.single_pass.double_wide,
                    "Double-wide render target (full per-eye resolution)",
                )
                .on_hover_text(
                    "Renders the scene targets at 2x per-eye width so each eye-half is full \
                     resolution instead of squished. Needs Collapse and native resolution on.",
                );
                let collapse_changed = ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.collapse,
                        "Collapse to a single game.Draw walk (the actual perf win; riskiest)",
                    )
                    .on_hover_text(
                        "One walk renders both eyes: centered camera, no between-eye restore, the \
                         back buffer split into the two eye textures. Without double-wide each eye \
                         is squished/half-filled; unpatched geometry and the HUD reach the left eye \
                         only. Turns the far-field Share mode off -- the two are exclusive. \
                         Reloads shaders, since some rewrites only apply while the collapse is on.",
                    )
                    .changed();
                if collapse_changed {
                    // Some shader rewrites are gated on the collapse being active -- the screen-space
                    // decal depth-UV bias is only correct against a double-wide target -- so a shader
                    // created under one setting is wrong under the other. Only the reload retracts or
                    // applies them.
                    shader::request_reload();
                }
                if collapse_changed
                    && cfg.stereo.single_pass.collapse
                    && cfg.far_field.mode == FarFieldMode::Share
                {
                    cfg.far_field.mode = FarFieldMode::Collect;
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.reproject,
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
                        &mut cfg.stereo.single_pass.reproject_camera_only,
                        "...including the ones claimed on a cb0[4] reference alone",
                    )
                    .on_hover_text(
                        "Extends the reprojection to allowlisted families the cb0 remap claims on a \
                         camera-position reference that is not their position path: generaljc3, \
                         landmark, layered and layeredblend all read cb0[4] for a LOD fade and build \
                         clip from a baked cb1. Without it they get viewport routing but no per-eye \
                         clip, so both eye halves are drawn from the collapsed centre viewpoint. \
                         Requires the reprojection above. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.terrain,
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
                        cfg.stereo.single_pass.terrain,
                        egui::Checkbox::new(
                            &mut cfg.stereo.single_pass.terrain_patch_per_eye,
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
                    .checkbox(
                        &mut cfg.stereo.single_pass.tree_impostors,
                        "Single-pass the tree impostors (far-distance billboards)",
                    )
                    .on_hover_text(
                        "Reprojects the treeimpostor* vertex shaders by the per-eye M_eye, like the \
                         scene families. They draw non-instanced with no GPU-indirect path, so this \
                         covers them completely. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(
                    &mut cfg.stereo.single_pass.tree_impostor_pixel_size_per_eye,
                    "Tree impostor pixel size per eye",
                )
                .on_hover_text(
                    "Stages the impostors' PixelSize at one eye's width under the double-wide \
                     collapse. Off, it is the width of both eyes, so each card's pixel footprint is \
                     twice its size and the derivative-scaled atlas offset moves the cards' cut-out \
                     silhouettes.",
                );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.tree_impostor_screen_door_per_eye,
                        "Tree impostor screen door per eye",
                    )
                    .on_hover_text(
                        "The impostors dissolve across their distance fade by a pattern hashed from \
                         their own projected position over the target size. Hashed from the \
                         eye-local pixel position instead, each eye keeps the same pixels of a fading \
                         card as an eye rendered on its own. Off, the pattern is the centred camera's \
                         over both eyes' width. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.static_lights_per_eye,
                        "Static light sprites per eye (distant town lights)",
                    )
                    .on_hover_text(
                        "Reprojects the lightsource* vertex shaders by M_eye, promotes their \
                         non-indexed draw to one two-instance draw routed to the eye halves, and \
                         stages the sprites' inverse screen width at one eye's width. Off, the \
                         sprites are drawn once across the whole double-wide target, at twice their \
                         distance from its centre and depth-tested at the wrong pixel. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(&mut cfg.stereo.single_pass.moon_per_eye, "Moon per eye")
                    .on_hover_text(
                        "Promotes the moon's non-indexed draw to one two-instance draw routed to the \
                         eye halves, with its far-plane depth kept through the reprojection. Off, \
                         the moon is drawn once across the whole double-wide target: missing from \
                         the left eye and twice as wide in the right. Needs the reprojection; \
                         applies at once.",
                    );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.water_plane_per_eye,
                        "Water reflection planes per eye",
                    )
                    .on_hover_text(
                        "Reprojects the watermask vertex shader by M_eye and promotes the water \
                         reflection planes' proxy quad to one two-instance draw routed to the eye \
                         halves. Off, the quad the screen-space reflections trace water against is \
                         drawn once across the whole double-wide target. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.clouds_per_eye, "Clouds per eye")
                    .on_hover_text(
                        "Reprojects the cloud vertex shaders by M_eye. Off, the clouds take their \
                         clip position from the centred render camera in both eyes, which is \
                         wrong for asymmetric or canted eyes. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.stars_per_eye, "Stars per eye")
                    .on_hover_text(
                        "Reprojects the stars' vertex shader by M_eye and promotes their \
                         non-indexed draw to one two-instance draw routed to the eye halves, \
                         keeping their far-plane depth. Off, the stars are drawn once across the \
                         whole double-wide target. Reloads shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.label("Render-block re-issue:");
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.bark,
                        "Bark (tree trunks/branches)",
                    )
                    .on_hover_text(
                        "Re-issues CRenderBlockBark's Draw/DrawZ once per eye with its baked cb1 \
                         view-projection reprojected by M_eye. Covers its plain/instanced/\
                         GPU-indirect draw kinds, and declines the cb0 remap on the vegetationbark* \
                         shaders (which read cb0[4] for shading, not position). Reloads shaders to \
                         apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.foliage, "Foliage (grass)")
                    .on_hover_text(
                        "Re-issues CRenderBlockFoliage's Draw/DrawZ once per eye with its baked cb2 \
                         view-projection reprojected by M_eye, and declines the cb0 remap on the \
                         vegetationfoliage* shaders (which read cb0[4] only as a wind-noise origin). \
                         Does not fix the separate forward-lighting black-grass issue. Reloads \
                         shaders to apply.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.checkbox(
                    &mut cfg.stereo.single_pass.vegetation_once,
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
                    crate::stereo::single_pass::vegetation_once::status()
                ));
                ui.checkbox(
                    &mut cfg.stereo.single_pass.occluder,
                    "Occluder (depth-prime boxes)",
                )
                .on_hover_text(
                    "Re-issues CRenderBlockOccluder's DrawZ once per eye with its baked cb1 \
                     view-projection reprojected by M_eye, priming each eye's depth with its own \
                     projection.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.instanced_per_eye,
                    "Already-instanced draws per eye (buildings, vegetation)",
                )
                .on_hover_text(
                    "Re-issues a DrawIndexedInstanced whose patched shader is bound once per eye, \
                     with both cb13 eye slots and both viewport slots pinned to that eye, so the \
                     game's own instance ids stop being read as an eye parity. Off, the batch is \
                     split alternately between the eyes -- the building flicker.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.hw_instanced_once,
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
                    crate::stereo::single_pass::hw_instanced::status()
                ));
                ui.checkbox(
                    &mut cfg.stereo.single_pass.indirect_per_eye,
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
                    &mut cfg.stereo.single_pass.vertex_globals_per_eye,
                    "Per-eye vertex globals for unpatched re-issues",
                )
                .on_hover_text(
                    "Binds each eye's copy of the vertex GlobalConstants (view-projections, camera \
                     position and axis) around the unpatched draws re-issued once per eye. Off, \
                     those draws read the centred render camera: no parallax, and misplaced under \
                     asymmetric or canted eyes. Applies at once.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.reconstruct_per_eye,
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
                    &mut cfg.stereo.single_pass.fullscreen_camera_per_eye,
                    "Fullscreen passes use each eye's camera position",
                )
                .on_hover_text(
                    "Stages each eye's own camera position, and the matching sun-shadow lookup \
                     correction, for every per-eye run of a split fullscreen pass. Off, both halves \
                     measure view vectors, fog distances, and shadow lookups from the centred camera.",
                );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.pixel_camera_per_eye,
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
                        &mut cfg.stereo.single_pass.local_lights_per_eye,
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
                let (rewritten, declined) = crate::stereo::single_pass::eye_camera::counts();
                ui.weak(format!(
                    "Pixel shaders reading each eye's camera: {rewritten} (declined {declined})"
                ));
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.screen_door_per_eye,
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
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.velocity_per_eye,
                        "Motion vectors per eye",
                    )
                    .on_hover_text(
                        "The velocity pass writes each pixel's motion since the last frame, which \
                         the motion blur and FSR read. Per eye, each eye's half measures it in that \
                         eye's own screen space, as an eye rendered on its own does. Off, every \
                         eye's motion carries a sideways ramp of half a pixel per pixel from its \
                         centre. Reloads shaders to apply the rewrite; turning it off takes effect \
                         at once.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                let (velocities, declined) = crate::stereo::single_pass::eye_camera::velocity_counts();
                ui.weak(format!(
                    "Velocity pixel shaders per eye: {velocities} (declined {declined})"
                ));
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.clip_origin_per_eye,
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
                    &mut cfg.stereo.single_pass.terrain_detail_eye_rows,
                    "Terrain detail from each eye's own rows",
                )
                .on_hover_text(
                    "Builds the terrain-detail draw's transform from each eye's own rows, as the \
                     engine builds it for a separately rendered eye. Off, the centre's transform is \
                     reprojected per eye, which rounds badly enough to move one eye's near ground \
                     by up to one part in ten thousand of its depth.",
                );
                let (rewritten, projected, declined) =
                    crate::stereo::single_pass::eye_camera::screen_door_counts();
                ui.weak(format!(
                    "Pixel shaders with a per-eye screen door: {rewritten}, from a projected \
                     position {projected} (declined {declined})"
                ));
                ui.checkbox(
                    &mut cfg.stereo.single_pass.nvwater_per_eye,
                    "WaveWorks water per eye (parallax)",
                )
                .on_hover_text(
                    "Gives the NvWater* blocks a per-eye view. Their screen-space reads were always \
                     fine; what is missing is parallax -- the vertex shader transforms from a baked \
                     matrix in its own constant buffer, so both eyes get the collapsed centre view. \
                     Flat water at the wrong depth looks right on the mirror and wrong in the headset.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.water_reflection_per_eye,
                    "...and look its reflection up per eye",
                )
                .on_hover_text(
                    "Reads the WaveWorks water's planar reflection by each eye's own screen \
                     coordinate. Off, each eye samples half of the one eye-sized reflection \
                     stretched across its view, shifting the water's colour differently in each \
                     eye. Applies immediately: the per-eye shaders are built with the originals.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.water_reflection_eye_size,
                    "...and render its reflection at one eye's size",
                )
                .on_hover_text(
                    "Sizes the water's planar reflection camera to one eye. Off, the engine sizes it \
                     to the double-wide target, so the reflection covers twice an eye's horizontal \
                     field and each eye sees it squeezed to half its width, which shows as a \
                     sparkle over far water. Applies immediately.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.water_camera_per_eye,
                    "...and shade it from each eye's position",
                )
                .on_hover_text(
                    "Moves the camera position the water's Fresnel, reflection, and specular terms \
                     are measured from to each eye for that eye's draw. Off, both eyes shade the \
                     water as seen from the point between them.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.ssdecal_geometry_per_eye,
                    "...and reproject the decal box geometry",
                )
                .on_hover_text(
                    "Sub-option of the decal fix: reprojects the decal box's baked transform per eye \
                     so its screen coverage has parallax too. The reconstruction fix alone stops the \
                     sliding; this adds depth to it.",
                );
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.ssdecal_per_eye,
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
                if ui
                    .checkbox(
                        &mut cfg.stereo.single_pass.composite_overlays_per_eye,
                        "Vignette and lens dirt per eye",
                    )
                    .on_hover_text(
                        "Maps the final composite's vignette and lens dirt over each eye. Off, both \
                         stretch across the whole double-wide frame, darkening each eye's outer edge. \
                         Reloads shaders to apply: it is a shader rewrite.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.glare_per_eye, "Glare per eye")
                    .on_hover_text(
                        "Keeps the bloom chain's blurs, downsamples, and upsamples, and the final \
                         composite's bloom lookups, inside each eye's half. Off, a bright sky or sun \
                         near one eye's inner edge bleeds across the seam into the other eye. Reloads \
                         shaders to apply the composite's half of it: that is a shader rewrite.",
                    )
                    .changed()
                {
                    shader::request_reload();
                }
                ui.weak(format!(
                    "Glare filter variants built: {}",
                    crate::stereo::single_pass::post_filters::registered_count()
                ));
                ui.checkbox(
                    &mut cfg.stereo.single_pass.foveation_per_eye,
                    "Foveation per eye",
                )
                .on_hover_text(
                    "Centres the foveation's full-rate region and its peripheral drop on each eye, \
                     sized for one eye. Off, one fovea sits on the seam, sized for the double-wide \
                     frame, so each eye drops pixels at its own centre of view.",
                );
                ui.checkbox(&mut cfg.stereo.single_pass.fsr_per_eye, "FSR per eye")
                    .on_hover_text(
                        "Resolves FSR once per eye over that eye's half, each with its own history, \
                         as an eye rendered on its own is. Off, one FSR pass covers both eyes: its \
                         history moves twice as far sideways as the eye did and reaches across the \
                         seam.",
                    );
                ui.weak(format!(
                    "FSR dispatches per eye this frame: {}",
                    crate::fsr::split_dispatch_count()
                ));
                if ui
                    .checkbox(&mut cfg.stereo.single_pass.ssao_per_eye, "SSAO per eye")
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
                        &mut cfg.stereo.single_pass.ao_volumes_per_eye,
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
                    .checkbox(&mut cfg.stereo.single_pass.ssr_per_eye, "SSR per eye")
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
                    &mut cfg.stereo.single_pass.subsurface_per_eye,
                    "Subsurface skin per eye",
                )
                .on_hover_text(
                    "Same reconstruction defect. This block rebuilds the inverse twice, once per blur \
                     axis, so both runs have to be masked.",
                );
                ui.checkbox(&mut cfg.stereo.single_pass.dof_per_eye, "Depth of field per eye")
                    .on_hover_text(
                        "The last consumer of the reconstruction basis. A post pass rather than a \
                         render block, so it may want the basis substituted rather than the pass \
                         re-issued.",
                    );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.collapse_viewport_follows_target,
                    "Eye viewports follow the bound target (clouds, smoke, spotlight volumetrics)",
                )
                .on_hover_text(
                    "Splits the viewport of whatever render target the engine currently has bound, instead of always splitting the scene's double-wide one. The low-resolution clouds, particles, and spot-light cones render into a shared quarter-resolution buffer (half per axis); handing those draws a full-scene viewport magnifies them 2x about the target's origin and crops them, which is also a 2x motion gain -- clouds and smoke sliding at twice the camera's rate. A no-op everywhere else, since the two viewports agree outside those passes.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.slot13_per_eye,
                    "Non-indexed geometry per eye (decals, roads, skidmarks)",
                )
                .on_hover_text(
                    "Re-issues a non-indexed Draw once per eye, with both viewport and cb13 eye slots pinned to that eye, for the passes known to submit geometry this way. Off, every slot-13 draw gets the whole double-wide viewport -- right for a fullscreen triangle, but it stretches decals and road layers 2x horizontally, which is a 2x motion gain and reads as them sliding over the world. Default off until the pass allowlist is confirmed: read the 'slot-13 by pass' census in the log to see which passes actually arrive here.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.non_indexed_geometry_once,
                    "Patched non-indexed geometry once for both eyes (skidmarks)",
                )
                .on_hover_text(
                    "In the same passes, a non-indexed Draw whose vertex shader is patched is promoted to one two-instance draw routed to the eye halves, instead of being re-issued once per eye. The shader picks its eye from the instance parity, so one submission covers both eyes. Applies at once.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.atmospheric_per_eye,
                    "Atmospheric scattering per eye (sun shadows, aerial perspective)",
                )
                .on_hover_text(
                    "The same split for the atmospheric-scattering pass, which reconstructs the \
                     whole screen -- sky included -- and ray-marches the sun cascade and aerial \
                     perspective over it. It is the second consumer of the reconstruction basis, so \
                     with only the deferred resolve split this pass paints the sliding error back \
                     over the fixed one. Needs the off-axis reconstruction override on.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.fog_volume_per_eye,
                    "Volumetric fog per eye",
                )
                .on_hover_text(
                    "The same split for the volumetric fog generation pass, which reconstructs world \
                     positions from depth into its half-resolution target and ray-marches the sun \
                     cascade towards them. It inverts the render context's view-projection itself, \
                     so each run is handed that eye's, mapped onto its half of the target.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.water_uv_per_eye,
                    "Water reflection UVs per eye (legacy non-WaveWorks water)",
                )
                .on_hover_text(
                    "Re-issues the Water*/WaterBox* blocks' Draw once per eye with their screen-UV \
                     matrix biased into that eye's half of the double-wide target. Those shaders \
                     sample reflection/refraction/depth through a projective TEXCOORD1 normalized \
                     over one eye's viewport while the buffers are double-wide, so each eye reads \
                     the whole two-eye image across its water -- a 2x stretch, and a 2x motion gain, \
                     which reads as the reflections sliding. Only affects the lower water-quality \
                     settings; the NvWater WaveWorks path samples by pixel coordinate and is fine.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.clustered_per_eye,
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
                    &mut cfg.stereo.single_pass.clustered_per_eye_light_view,
                    "...and assign its lights from each eye's position",
                )
                .on_hover_text(
                    "Folds the eye's world offset into the translation row of the light-assignment \
                     view matrix, so each eye's half is assigned from that eye rather than from the \
                     collapsed centre camera. Sub-option of the above, on its own flag because the \
                     difference may be below the 64-pixel tile granularity. Positional offset only; \
                     per-eye display canting is not applied.",
                );
                ui.checkbox(
                    &mut cfg.stereo.single_pass.uniform_viewport_slots,
                    "Uniform viewport slots outside the G-buffer",
                )
                .on_hover_text(
                    "Puts both viewport slots back to one region once the G-buffer range ends, so an \
                     instanced draw with a patched shader in a pass that is not eye-split (shadows, \
                     reflections, post) keeps its odd-numbered instances instead of routing them into \
                     the other eye's half. Slot 0 is never changed.",
                );
            });
        });

        let capability = single_pass::probe_if_needed();
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

/// Whether the config asks for the single-pass collapse, ignoring the runtime gates
/// ([`crate::stereo::single_pass::collapse_active`] also requires the device capability and a live
/// session). The UI needs the *intent*, so a mutually-exclusive option greys out as soon as the boxes
/// are ticked rather than only once the collapse is running.
pub(super) fn collapse_configured(cfg: &config::Config) -> bool {
    cfg.stereo.single_pass.enabled
        && cfg.stereo.single_pass.dual_eye
        && cfg.stereo.single_pass.collapse
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
