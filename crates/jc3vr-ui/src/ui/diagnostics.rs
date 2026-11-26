//! The render RE window: a separate egui window, opened from the Experimental page, holding the
//! tools that serve single-pass stereo and render reverse-engineering. These are the render-block
//! type registry, the exposure A/B, and the terrain and shadow diagnostics.

use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

use jc3gi::graphics_engine::{
    render_block::{
        RenderBlockTypeTerrain as BaseTerrain, RenderBlockTypeTerrainPatch as TerrainPatch,
    },
    render_engine::{RenderBlockTypeInstances, RenderBlockTypeRegistry},
};
use jc3vr_core::config::engine::{Parts, edit_parts};
use jc3vr_render::hooks::graphics_engine::config::EXPOSURE_CONFIG;
use jc3vr_view::stereo::config::STEREO_CONFIG;

/// The Experimental page's entry point: the button that opens the render RE window.
pub(crate) fn open_button(ui: &mut egui::Ui) {
    let open = OPEN.load(Ordering::Relaxed);
    if ui
        .add_enabled(!open, egui::Button::new("Open the render RE window"))
        .on_hover_text(
            "The render-block type registry, exposure A/B, and terrain and shadow diagnostics.",
        )
        .clicked()
    {
        OPEN.store(true, Ordering::Relaxed);
    }
}

/// Draws the render RE window while it is open. `editing` mirrors the main window's edit
/// boundary, so the window is read-only while gameplay input owns the controls.
pub(crate) fn window(ctx: &egui::Context, editing: bool) {
    let mut open = OPEN.load(Ordering::Relaxed);
    if !open {
        return;
    }
    egui::Window::new("Render RE")
        .open(&mut open)
        .default_size(egui::vec2(520.0, 640.0))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.add_enabled_ui(editing, |ui| {
                diagnostics_controls(ui);
            });
        });
    OPEN.store(open, Ordering::Relaxed);
}

/// Whether the render RE window is open. The Experimental page's button opens it and its close
/// button closes it; it is not persisted.
static OPEN: AtomicBool = AtomicBool::new(false);

fn diagnostics_controls(ui: &mut egui::Ui) {
    let mut cfg = edit_parts();
    ui.collapsing("Shadow cascades", |ui| {
            ui.checkbox(
                &mut cfg.part_mut(&STEREO_CONFIG).shadow_update_every_frame,
                "Update all shadow cascades every frame (defeats 2^L amortization; parity-flip probe)",
            )
            .on_hover_text(
                "Zeroes m_CascadeUpdateLevels so cascades 1-5 refresh every frame like cascade 0, instead \
                 of on the amortized schedule. Diagnostic for the #31 parity ping-pong: capture at native \
                 rate and check whether the ShadowState scale_blend.y 6<->1 flip flattens.",
            );
        });
    ui.separator();

    // Live-writes the engine's own CRenderBlockTypeTerrainPatch debug flags (not config, not saved) to
    // bisect which patch cull blackens tessellated cliff patches in VR: a patch culled only in the
    // color pass keeps its Z-prepass depth but writes no G-buffer, so deferred lighting resolves it to
    // flat black. Back-patch culling is the prime suspect -- it keys off the shared render camera's
    // forward vector, so it culls identically in both eyes and only past a facing threshold ("at
    // incidental angles"). Flags persist until the terrain type is recreated (level reload).
    ui.collapsing("Terrain patch culling (engine, live)", |ui| {
        match unsafe { TerrainPatch::get() } {
            Some(patch) => {
                // Sanity/discriminator: NoDraw stops every terrain-patch draw. If the cliffs (and the
                // black patches with them) vanish, the writes land and the black patches are this
                // render block. If the cliffs vanish but the black patches remain, the black is a
                // different render block or a deferred artifact, not the patch shader.
                let mut no_draw = patch.m_NoDraw;
                if ui
                    .checkbox(
                        &mut no_draw,
                        "No draw (stop ALL terrain-patch draws — sanity/discriminator test)",
                    )
                    .changed()
                {
                    patch.m_NoDraw = no_draw;
                }
                let mut show_material = patch.m_ShowMaterial;
                if ui
                    .checkbox(
                        &mut show_material,
                        "Show material (swaps the patch fragment program; does the black change?)",
                    )
                    .changed()
                {
                    patch.m_ShowMaterial = show_material;
                }
                let mut back = patch.m_EnableBackPatchCulling;
                if ui
                    .checkbox(
                        &mut back,
                        "Back-patch culling (shared view dir; prime suspect for both-eye black \
                         patches)",
                    )
                    .changed()
                {
                    patch.m_EnableBackPatchCulling = back;
                }
                let mut frustum = patch.m_EnableFrustumPatchCulling;
                if ui
                    .checkbox(
                        &mut frustum,
                        "Frustum patch culling (baked off-axis view-projection)",
                    )
                    .changed()
                {
                    patch.m_EnableFrustumPatchCulling = frustum;
                }
                let mut detail = patch.m_EnableCullByDetail;
                if ui.checkbox(&mut detail, "Cull by detail").changed() {
                    patch.m_EnableCullByDetail = detail;
                }
                let mut show = patch.m_ShowDebugCulling;
                if ui
                    .checkbox(
                        &mut show,
                        "Show debug culling (rasterize culled patches with CULLFACE_NONE)",
                    )
                    .changed()
                {
                    patch.m_ShowDebugCulling = show;
                }
                ui.add(
                    egui::Slider::new(&mut patch.m_BackPatchCullThreshold, -1.0..=1.0)
                        .text("Back-patch threshold")
                        .fixed_decimals(3),
                );
                ui.add(
                    egui::Slider::new(&mut patch.m_DebugMode, 0..=5)
                        .text("Debug mode (0 = normal shading)"),
                )
                .on_hover_text(
                    "Selects an engine debug fragment program (LOD colours, tessellation overlays); \
                     capped conservatively to stay in the shader array",
                );
            }
            None => {
                ui.label("Terrain patch render type not initialized yet.");
            }
        }
    });

    // The base VolumetricTerrain block (CRenderBlockTerrain) -- dormant in the game world (the live
    // terrain is the volumetric-patch system + the TerrainDetail rock skin), so these engine flags are
    // inert there; kept for other worlds. Live engine flags, not saved; reset on level reload.
    ui.collapsing("Base terrain culling (walls; engine, live)", |ui| {
        match unsafe { BaseTerrain::get() } {
            Some(terrain) => {
                let mut back = terrain.m_EnableBackPatchCulling;
                if ui
                    .checkbox(
                        &mut back,
                        "Back-patch culling (shared view dir; PRIME suspect for black wall tiles)",
                    )
                    .changed()
                {
                    terrain.m_EnableBackPatchCulling = back;
                }
                let mut frustum = terrain.m_EnableFrustumPatchCulling;
                if ui
                    .checkbox(
                        &mut frustum,
                        "Frustum patch culling (baked off-axis view-projection)",
                    )
                    .changed()
                {
                    terrain.m_EnableFrustumPatchCulling = frustum;
                }
                let mut detail = terrain.m_EnableCullByDetail;
                if ui.checkbox(&mut detail, "Cull by detail").changed() {
                    terrain.m_EnableCullByDetail = detail;
                }
                let mut show = terrain.m_ShowDebugCulling;
                if ui
                    .checkbox(
                        &mut show,
                        "Show debug culling (rasterize culled patches with CULLFACE_NONE)",
                    )
                    .changed()
                {
                    terrain.m_ShowDebugCulling = show;
                }
                ui.add(
                    egui::Slider::new(&mut terrain.m_BackPatchCullThreshold, -1.0..=1.0)
                        .text("Back-patch threshold")
                        .fixed_decimals(3),
                );
                ui.separator();
                ui.label(
                    "Tessellation factors -- if steep/grazing tiles collapse to zero tess (holes) \
                     under the wide VR FOV, raising Edge / lowering MinSpacing should refill them:",
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorEdge, 0.0..=64.0)
                        .text("Edge")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorInner, 0.0..=64.0)
                        .text("Inner")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorMinSpacing, 0.1..=64.0)
                        .text("MinSpacing (smaller = more tess)")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorSphere, 0.0..=64.0)
                        .text("Sphere")
                        .fixed_decimals(2),
                );
                ui.add(
                    egui::Slider::new(&mut terrain.m_TessellationFactorNormalDiff, 0.0..=64.0)
                        .text("NormalDiff")
                        .fixed_decimals(2),
                );
            }
            None => {
                ui.label("Base terrain render type not initialized yet.");
            }
        }
    });

    // The engine's render-block-type registry: every registered type by name, with its engine-native
    // enable flag (CRenderPass::DoDraw skips disabled types in every pass). The definitive bisect for
    // "which render block draws this surface": disable types one at a time until the surface vanishes.
    ui.collapsing(
        "Render block types (engine registry; disable to bisect)",
        |ui| {
            show_render_block_type_registry(ui);
            show_unregistered_render_block_types(ui);
        },
    );

    ui.collapsing("Exposure A/B (pin the applied exposure)", |ui| {
        ui.checkbox(
            &mut cfg.part_mut(&EXPOSURE_CONFIG).force,
            "Force exposure (pin after the engine's Update)",
        );
        ui.add(
            egui::Slider::new(&mut cfg.part_mut(&EXPOSURE_CONFIG).forced_value, 0.0..=0.5)
                .text("Forced exposure (~0.11 = non-stereo daylight)"),
        );
        ui.label(
            "A/B: enable in both stereo and non-stereo at the same value. Same brightness => the \
             darkening was the exposure loop; stereo still darker => a render path.",
        );
    });
}

/// A vtable-compatible `IRenderBlockType::IsEnabled` override that always reports disabled.
/// `CRenderPass::DoDraw` calls the virtual per type run, so pointing a type's vtable slot here
/// stops its draws in every pass.
unsafe extern "system" fn render_block_type_always_disabled(
    _this: *mut ::core::ffi::c_void,
) -> bool {
    false
}

/// The `IsEnabled` vtable slots currently patched to
/// [`render_block_type_always_disabled`], keyed by slot address.
static DISABLED_TYPE_SLOTS: parking_lot::Mutex<BTreeSet<usize>> =
    parking_lot::Mutex::new(BTreeSet::new());

/// Render the render-block types the registry cannot reach.
///
/// The registry list above is not "every type". A render block whose `InitType` stores its type in a
/// global and returns *without* calling `AddType` never enters the registry, so it does not get a row
/// -- while `CRenderPass::DoDraw` still dispatches its `IsEnabled` through the vtable exactly as it
/// does for a registered type. Particles are the case in practice: unticking every row in the registry
/// list leaves smoke and every other particle effect still drawing, which reads as a broken kill
/// switch rather than a missing entry, and quietly invalidates any bisect run through that list.
///
/// The set is computed rather than listed: every render-block type enters
/// [`RenderBlockTypeInstances`] when it is constructed, so the instance table minus the registry *is*
/// the unreachable set, and a type that stops (or starts) registering shows up here on its own. Each
/// row uses the same vtable patch, the same bookkeeping, and the same restore path as the registry
/// rows.
fn show_unregistered_render_block_types(ui: &mut egui::Ui) {
    ui.separator();

    // SAFETY: the instance table is static engine storage within the loaded module image, and its
    // slots are live type objects or null. The vtable slot write is an aligned qword store through
    // the patcher while the render thread may read it -- acceptable for a diagnostic toggle, as for
    // the registry rows.
    unsafe {
        let Some(reg) = RenderBlockTypeRegistry::get() else {
            ui.label("registry not reachable; cannot tell registered types from unregistered ones");
            return;
        };
        let entries = reg.as_slice();
        if entries.len() > 256 {
            ui.label(format!(
                "registry not initialized or invalid ({} entries); cannot tell registered types \
                 from unregistered ones",
                entries.len()
            ));
            return;
        }
        let registered: BTreeSet<usize> =
            entries.iter().map(|entry| entry.m_Type as usize).collect();

        // The table is not compacted -- a destroyed type leaves a null hole with live slots after
        // it -- so every slot is visited and nulls are skipped rather than treated as the end.
        let mut rows: Vec<(&str, usize)> = RenderBlockTypeInstances::as_slice()
            .iter()
            .filter_map(|ptr| {
                let ty = ptr.as_mut()?;
                if registered.contains(&(*ptr as usize)) {
                    return None;
                }
                let name = ty.get_type_name_str().unwrap_or("(unnamed)");
                // The patch target: the address of the vtable's `IsEnabled` entry, with the field
                // offset taken from the generated vftable type.
                let slot = (&raw const (*ty.vftable()).IsEnabled) as usize;
                Some((name, slot))
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        // Deduplicate by slot address: two instances of the same unregistered type share a vtable
        // and thus the same `IsEnabled` slot, so without this a duplicate checkbox appears per
        // instance.
        rows.dedup_by(|a, b| a.1 == b.1);

        if rows.is_empty() {
            ui.label(
                "No unregistered types: every live render-block type is in the registry list \
                 above, so unticking all of it reaches every type",
            );
            return;
        }
        ui.label(format!(
            "{} type(s) the registry does not list (their InitType never calls AddType, so the \
             list above cannot reach them -- but their draws are gated identically). Computed as \
             the engine's live-instance table minus the registry.",
            rows.len()
        ));

        let mut disabled_slots = DISABLED_TYPE_SLOTS.lock();
        for (name, slot) in rows {
            let mut enabled = !disabled_slots.contains(&slot);
            if ui
                .checkbox(&mut enabled, format!("{name} (unregistered)"))
                .changed()
                && let Some(mut patcher) = jc3vr_core::hooks::patcher()
            {
                if enabled {
                    patcher.unpatch(slot);
                    disabled_slots.remove(&slot);
                } else {
                    let stub = render_block_type_always_disabled as *const () as usize;
                    patcher.patch(slot, &stub.to_le_bytes());
                    disabled_slots.insert(slot);
                }
            }
        }
    }
}

/// Render the engine render-block-type registry: one row per registered type, name from the type's
/// own `GetTypeName`, and a checkbox that patches the type's `IsEnabled` vtable slot to a
/// return-false stub (restored on re-tick or uninject). The engine's own
/// `Enable`/`Disable` compile to no-ops and `IsEnabled` to `return true`, but `CRenderPass::DoDraw` still
/// dispatches `IsEnabled` through the vtable, so the slot patch is the working kill switch.
fn show_render_block_type_registry(ui: &mut egui::Ui) {
    // SAFETY: the registry is static engine storage; entries are live type singletons registered at
    // startup and only removed at shutdown. The vtable slot write is an aligned qword store through
    // the patcher while the render thread may read it -- acceptable for a diagnostic toggle.
    unsafe {
        let Some(reg) = RenderBlockTypeRegistry::get() else {
            ui.label("registry not reachable");
            return;
        };
        let entries = reg.as_slice();
        if entries.is_empty() || entries.len() > 256 {
            ui.label(format!(
                "registry not initialized or invalid ({} entries)",
                entries.len()
            ));
            return;
        }
        ui.label(format!(
            "{} registered types; unticking disables the type's draws in every pass \
             (vtable IsEnabled patch; auto-reverts on uninject)",
            entries.len()
        ));
        let mut disabled_slots = DISABLED_TYPE_SLOTS.lock();
        // Sort by type name so the list is stable and scannable (the registry's own order is
        // registration order, which shuffles as the engine loads).
        let mut rows: Vec<(&str, usize, u32)> = entries
            .iter()
            .filter_map(|entry| {
                let ty = entry.m_Type.as_mut()?;
                let name = ty.get_type_name_str().unwrap_or("(unnamed)");
                // The patch target: the address of the vtable's `IsEnabled` entry, with the field
                // offset taken from the generated vftable type.
                let slot = (&raw const (*ty.vftable()).IsEnabled) as usize;
                Some((name, slot, entry.m_Hash))
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        egui::ScrollArea::vertical()
            .id_salt("render_block_type_registry")
            .max_height(240.0)
            .show(ui, |ui| {
                for (name, slot, hash) in rows {
                    let mut enabled = !disabled_slots.contains(&slot);
                    if ui
                        .checkbox(&mut enabled, format!("{name} ({hash:#010x})"))
                        .changed()
                    {
                        let Some(mut patcher) = jc3vr_core::hooks::patcher() else {
                            continue;
                        };
                        if enabled {
                            patcher.unpatch(slot);
                            disabled_slots.remove(&slot);
                        } else {
                            let stub = render_block_type_always_disabled as *const () as usize;
                            patcher.patch(slot, &stub.to_le_bytes());
                            disabled_slots.insert(slot);
                        }
                    }
                }
            });
    }
}
