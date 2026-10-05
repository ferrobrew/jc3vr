//! The render side of the world markers: on eye 0, choose the layout for the displayed snapshot
//! and place each marker; on every eye, crop the markers out of the HUD texture into their slots,
//! cut them out of the panel, and draw each slot as a billboard at its marker's world point.
//!
//! The UI renders into the HUD texture once per eye draw, and the panel is drawn and the texture
//! cleared after each, so the crops and the cut are redone per eye from that eye's HUD render. The
//! placement (and the billboards' world corners) is computed once on eye 0 and shared, like the
//! panel's. Everything here runs on the render thread with the engine context mutex held.

use std::{collections::HashMap, time::Instant};

use glam::{Mat4, Quat, Vec3, Vec4};
use jc3gi::{
    graphics_engine::{device::Device, graphics_engine::GraphicsEngine, texture::Texture},
    ui::ui_manager::UIManager,
};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{D3D11_BOX, ID3D11DeviceContext};

use crate::{
    config::Config,
    hud::{
        config::{CropExtents, WorldMarkersConfig},
        quad::HudQuad,
        target::HudTarget,
        world_markers::{
            DistanceSource, MarkerStatus, STATUS, Status,
            depth::{DepthPoint, DepthProbe, RESULT_TTL_S, Timed},
            layout::{
                Billboard, Crop, Sight, StageFrame, billboard_corners, crop, placement_distance,
                radians_per_texel, slot_size, smooth_distance,
            },
            layout_for, raycast,
        },
    },
};

/// The eye-0 inputs for placing the frame's markers.
pub(crate) struct FrameInputs<'a> {
    /// The head (cyclopean eye) pose the panel was placed from.
    pub head: Vec3,
    pub head_rotation: Quat,
    /// The panel's height over its distance: the angular size the HUD texture is shown at.
    pub panel_height_over_distance: f32,
    /// The redirected HUD texture, or `None` while the redirect is not applied.
    pub hud: Option<&'a HudTarget>,
    pub graphics_engine: Option<&'a GraphicsEngine>,
}

/// Place this frame's markers (eye 0). Clears the plan when the prototype is off or nothing can be
/// placed, so the eye passes draw nothing.
pub(crate) fn prepare_frame(
    context: &ID3D11DeviceContext,
    device: &Device,
    inputs: &FrameInputs<'_>,
) {
    let (cfg, hud_cfg) = Config::lock_query(|c| (c.hud.world_markers, c.hud));
    let mut renderer = RENDERER.lock();
    let active = cfg.enabled
        && hud_cfg.redirect
        && hud_cfg.quad
        && crate::hud::current_mode() == crate::hud::HudMode::Hud
        && !crate::director::cinematic::active();
    let Some(hud) = inputs.hud.filter(|_| active) else {
        if let Some(renderer) = renderer.as_mut() {
            renderer.plan.clear();
        }
        raycast::request(Vec::new());
        *STATUS.lock() = None;
        return;
    };
    if renderer.is_none() {
        match Renderer::new(device) {
            Ok(built) => *renderer = Some(built),
            Err(e) => {
                log_once(&e);
                return;
            }
        }
    }
    let Some(renderer) = renderer.as_mut() else {
        return;
    };
    renderer.plan_frame(context, device, inputs, hud, &cfg);
}

/// Drop the frame's plan, so the eye passes draw nothing (eye 0, when no panel pose exists).
pub(crate) fn clear_frame() {
    if let Some(renderer) = RENDERER.lock().as_mut() {
        renderer.plan.clear();
    }
    *STATUS.lock() = None;
}

/// Crop this eye's markers out of the HUD texture into their slots and, when configured, cut them
/// out of the panel. Call on every eye after the UI rendered into the HUD texture and before the
/// panel draws.
pub(crate) fn prepare_eye(context: &ID3D11DeviceContext, hud: Option<&HudTarget>) {
    let mut renderer = RENDERER.lock();
    let (Some(renderer), Some(hud)) = (renderer.as_mut(), hud) else {
        return;
    };
    let hide = Config::lock_query(|c| c.hud.world_markers.hide_from_panel);
    renderer.copy_crops(context, hud, hide);
}

/// Draw this eye's marker billboards over `target`, farthest first. Call on every eye after
/// [`prepare_eye`].
pub(crate) fn draw_billboards(context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
    let renderer = RENDERER.lock();
    let Some(renderer) = renderer.as_ref() else {
        return;
    };
    for planned in &renderer.plan {
        let Some(slot) = renderer.slot(planned.slot) else {
            continue;
        };
        renderer
            .quad
            .draw(context, device, target, slot.color_srv(), &planned.corners);
    }
}

/// The render-side state, created on the first active frame.
static RENDERER: Mutex<Option<Renderer>> = Mutex::new(None);

/// The slot textures, the cut source, the depth probe, the per-marker placement state, and the
/// frame's plan.
struct Renderer {
    quad: HudQuad,
    depth: Option<DepthProbe>,
    /// The slots for unlabelled markers, all [`slot_size`](Renderer::slot_size).
    slots: Vec<HudTarget>,
    slot_size: (u32, u32),
    /// The slot for the labelled marker, wider on its left.
    label_slot: Option<HudTarget>,
    label_slot_size: (u32, u32),
    /// A transparent-black texture at least as large as any slot, copied over the markers' rects
    /// to cut them out of the panel.
    blank: Option<HudTarget>,
    /// Whether the blank texture still needs its clear.
    blank_dirty: bool,
    /// The latest depth result per marker.
    depth_results: HashMap<u32, Timed>,
    /// The smoothed drawn distance per marker, and when it was last updated.
    distances: HashMap<u32, (f32, Instant)>,
    plan: Vec<Planned>,
}

/// One marker placed for the frame.
struct Planned {
    slot: SlotId,
    crop: Crop,
    corners: [Vec4; 4],
    distance: f32,
}

/// Which slot a marker is cropped into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotId {
    Plain(usize),
    Label,
}

impl Renderer {
    fn new(device: &Device) -> anyhow::Result<Self> {
        Ok(Self {
            quad: HudQuad::new(device)?,
            depth: None,
            slots: Vec::new(),
            slot_size: (0, 0),
            label_slot: None,
            label_slot_size: (0, 0),
            blank: None,
            blank_dirty: false,
            depth_results: HashMap::new(),
            distances: HashMap::new(),
            plan: Vec::new(),
        })
    }

    fn slot(&self, id: SlotId) -> Option<&HudTarget> {
        match id {
            SlotId::Plain(index) => self.slots.get(index),
            SlotId::Label => self.label_slot.as_ref(),
        }
    }

    /// Choose the layout, size the slots, place every marker, sample the depth, and publish the
    /// status.
    fn plan_frame(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        inputs: &FrameInputs<'_>,
        hud: &HudTarget,
        cfg: &WorldMarkersConfig,
    ) {
        self.plan.clear();
        // SAFETY: the UI manager and its movie are live singletons; the snapshot ids are plain
        // integers the render side reads.
        let displayed = unsafe {
            UIManager::get()
                .and_then(|m| m.m_Movie.as_ref())
                .map_or(0, |movie| movie.RenderContext.SnapshotFrameIds[2])
        };
        let Some((layout, matched)) = layout_for(displayed) else {
            self.publish_status(Status {
                displayed_snapshot: displayed,
                ..Status::default()
            });
            return;
        };
        let texture = hud.size();
        let Some(frame) = StageFrame::new(layout.stage, texture.0 as f32 / texture.1.max(1) as f32)
        else {
            return;
        };
        let scale = frame.texels_per_unit(texture.0);
        let label_extents = CropExtents {
            left: cfg.crop.left + cfg.label_extra_left.max(0.0),
            ..cfg.crop
        };
        if let Err(e) = self.ensure_slots(
            device,
            slot_size(&cfg.crop, scale),
            slot_size(&label_extents, scale),
            layout.markers.len(),
        ) {
            log_once(&e);
            return;
        }

        let per_texel = radians_per_texel(inputs.panel_height_over_distance, texture.1);
        let now = Instant::now();
        let mut depth_points = Vec::with_capacity(layout.markers.len());
        let mut ray_requests = Vec::new();
        let mut statuses = Vec::with_capacity(layout.markers.len());
        let mut next_plain = 0;
        raycast::expire(RESULT_TTL_S * 4.0);
        for marker in &layout.markers {
            let labelled = marker.id != 0 && marker.id == layout.labelled_id;
            let (slot, size, extents) = if labelled {
                (SlotId::Label, self.label_slot_size, &label_extents)
            } else {
                let slot = SlotId::Plain(next_plain);
                next_plain += 1;
                (slot, self.slot_size, &cfg.crop)
            };
            let Some(crop) = crop(&frame, texture, size, marker.origin, marker.offset, extents)
            else {
                continue;
            };
            let offset = marker.world - inputs.head;
            let true_distance = offset.length();
            let Some(direction) = offset.try_normalize() else {
                continue;
            };
            depth_points.push(DepthPoint {
                id: marker.id,
                world: marker.world,
                true_distance,
            });
            let (sight, source) = self.sight(marker.id, cfg, now);
            if source != DistanceSource::Depth && cfg.surface_placement {
                ray_requests.push((marker.id, marker.world));
            }
            let target = if cfg.surface_placement {
                placement_distance(true_distance, sight, cfg.surface_pull_m, MIN_DISTANCE_M)
            } else {
                true_distance
            };
            let distance = self.smoothed(marker.id, target, cfg.depth_halflife_s, now);
            let drawn = distance.min(cfg.max_distance_m.max(MIN_DISTANCE_M));
            let billboard = Billboard {
                head: inputs.head,
                head_rotation: inputs.head_rotation,
                direction,
                distance: drawn,
            };
            let Some(corners) = billboard_corners(&billboard, size, crop.anchor, per_texel) else {
                continue;
            };
            let moved = matches!(sight, Sight::Surface(_)) && target < true_distance;
            statuses.push(MarkerStatus {
                id: marker.id,
                world: marker.world,
                origin: marker.origin,
                rect: (
                    crop.source.x0,
                    crop.source.y0,
                    crop.source.x1,
                    crop.source.y1,
                ),
                true_distance,
                drawn_distance: drawn,
                source: if moved { source } else { DistanceSource::Point },
                in_cell: marker.in_cell,
            });
            self.plan.push(Planned {
                slot,
                crop,
                corners,
                distance: drawn,
            });
        }
        self.plan.sort_by(|a, b| {
            b.distance
                .partial_cmp(&a.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.distances
            .retain(|_, (_, at)| now.duration_since(*at).as_secs_f32() < FORGET_S);
        self.depth_results
            .retain(|_, timed| now.duration_since(timed.at).as_secs_f32() < FORGET_S);

        if cfg.surface_placement {
            self.sample_depth(context, device, inputs, &depth_points, now);
        }
        raycast::request(if cfg.ray_fallback {
            ray_requests
        } else {
            Vec::new()
        });

        self.publish_status(Status {
            layout_snapshot: Some(layout.snapshot),
            displayed_snapshot: displayed,
            layout_matched: matched,
            skipped: layout.skipped,
            labelled_id: layout.labelled_id,
            stage: layout.stage,
            texture,
            markers: statuses,
        });
    }

    /// The freshest line-of-sight result for a marker: the depth probe's, else the raycast's, else
    /// clear.
    fn sight(&self, id: u32, cfg: &WorldMarkersConfig, now: Instant) -> (Sight, DistanceSource) {
        let fresh = |timed: &Timed| now.duration_since(timed.at).as_secs_f32() < RESULT_TTL_S;
        if let Some(timed) = self.depth_results.get(&id).filter(|t| fresh(t)) {
            return (timed.sight, DistanceSource::Depth);
        }
        if cfg.ray_fallback
            && let Some(timed) = raycast::result(id).filter(|t| fresh(t))
        {
            return (timed.sight, DistanceSource::Ray);
        }
        (Sight::Clear, DistanceSource::Point)
    }

    /// Ease a marker's drawn distance toward `target`.
    fn smoothed(&mut self, id: u32, target: f32, halflife: f32, now: Instant) -> f32 {
        let previous = self.distances.get(&id).copied();
        let dt = previous.map_or(0.0, |(_, at)| now.duration_since(at).as_secs_f32().min(0.1));
        let distance = smooth_distance(previous.map(|(d, _)| d), target, dt, halflife);
        self.distances.insert(id, (distance, now));
        distance
    }

    /// Dispatch the frame's depth samples and fold in the completed ones.
    fn sample_depth(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        inputs: &FrameInputs<'_>,
        points: &[DepthPoint],
        now: Instant,
    ) {
        let Some(graphics_engine) = inputs.graphics_engine else {
            return;
        };
        if self.depth.is_none() {
            match DepthProbe::new(device) {
                Ok(probe) => self.depth = Some(probe),
                Err(e) => {
                    log_once(&e);
                    return;
                }
            }
        }
        let Some(probe) = self.depth.as_mut() else {
            return;
        };
        // Under the collapsed single walk the depth buffer is double-wide: sample eye 0's half
        // through eye 0's own view-projection, as the panel draw does.
        let (vp, viewport) = match crate::stereo::single_pass::collapse_ui_eye_override() {
            Some((viewport, vp)) => (vp, Some(viewport)),
            None => match crate::hud::quad::fetch_view_projection() {
                Some(vp) => (vp, None),
                None => return,
            },
        };
        let results = probe.sample(context, graphics_engine, Mat4::from(vp), viewport, points);
        for (id, sight) in results {
            self.depth_results.insert(id, Timed { sight, at: now });
        }
    }

    /// Size the slots for this frame: recreate them on a size change, and grow the plain slots to
    /// `count`.
    fn ensure_slots(
        &mut self,
        device: &Device,
        plain: (u32, u32),
        label: (u32, u32),
        count: usize,
    ) -> anyhow::Result<()> {
        if self.slot_size != plain {
            self.slots.clear();
            self.slot_size = plain;
        }
        while self.slots.len() < count.min(crate::hud::world_markers::MAX_MARKERS) {
            self.slots.push(HudTarget::new(device, plain.0, plain.1)?);
        }
        if self.label_slot_size != label || self.label_slot.is_none() {
            self.label_slot = Some(HudTarget::new(device, label.0, label.1)?);
            self.label_slot_size = label;
        }
        let blank_size = (plain.0.max(label.0), plain.1.max(label.1));
        if self.blank.as_ref().map(HudTarget::size) != Some(blank_size) {
            self.blank = None;
            let blank = HudTarget::new(device, blank_size.0, blank_size.1)?;
            self.blank = Some(blank);
            self.blank_dirty = true;
        }
        Ok(())
    }

    /// Copy every planned marker's crop into its slot, then (with `hide`) blank the crops in the
    /// HUD texture. All crops are taken before any blanking, so overlapping markers each keep the
    /// other's pixels as the panel showed them.
    fn copy_crops(&mut self, context: &ID3D11DeviceContext, hud: &HudTarget, hide: bool) {
        if self.plan.is_empty() {
            return;
        }
        // SAFETY: every texture is ours (the HUD target, the slots, the blank), live, and of the
        // same format; the context is the engine's immediate context under its mutex.
        unsafe {
            if self.blank_dirty
                && let Some(blank) = self.blank.as_ref()
            {
                context.ClearRenderTargetView(blank.color_rtv(), &[0.0; 4]);
                self.blank_dirty = false;
            }
            for planned in &self.plan {
                let Some(slot) = self.slot(planned.slot) else {
                    continue;
                };
                context.ClearRenderTargetView(slot.color_rtv(), &[0.0; 4]);
                let source = planned.crop.source;
                context.CopySubresourceRegion(
                    slot.color_texture(),
                    0,
                    planned.crop.slot_offset.0,
                    planned.crop.slot_offset.1,
                    0,
                    hud.color_texture(),
                    0,
                    Some(&D3D11_BOX {
                        left: source.x0,
                        top: source.y0,
                        front: 0,
                        right: source.x1,
                        bottom: source.y1,
                        back: 1,
                    }),
                );
            }
            if !hide {
                return;
            }
            let Some(blank) = self.blank.as_ref() else {
                return;
            };
            for planned in &self.plan {
                let source = planned.crop.source;
                context.CopySubresourceRegion(
                    hud.color_texture(),
                    0,
                    source.x0,
                    source.y0,
                    0,
                    blank.color_texture(),
                    0,
                    Some(&D3D11_BOX {
                        left: 0,
                        top: 0,
                        front: 0,
                        right: source.width(),
                        bottom: source.height(),
                        back: 1,
                    }),
                );
            }
        }
    }

    fn publish_status(&self, status: Status) {
        *STATUS.lock() = Some(status);
    }
}

/// The nearest a marker is drawn, in metres.
const MIN_DISTANCE_M: f32 = 0.5;

/// How long a marker's placement state outlives its last sighting, in seconds.
const FORGET_S: f32 = 2.0;

/// Log a resource failure once per session; the next frame retries regardless.
fn log_once(error: &anyhow::Error) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::error!("hud world markers: {error:#}");
    }
}
