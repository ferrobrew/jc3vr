//! The render side of the world markers: on eye 0, choose the layout for the displayed snapshot
//! and place each marker; crop the markers out of the HUD texture into their slots, cut them out of
//! the panel, and draw each slot as a billboard at its marker's world point for each eye.
//!
//! Ordinary two-pass rendering crops and cuts on every dispatch from that dispatch's UI render,
//! then clears the HUD texture after the panel draws. Collapse crops and cuts only on eye 0; both
//! eyes reuse the slots and the cut panel, and the HUD texture is cleared after eye 1. The placement
//! (and the billboards' world corners) is computed once on eye 0 and shared, like the panel's.
//! Everything here runs on the render thread with the engine context mutex held.

use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

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
        log_once::LogOnce,
        quad::HudQuad,
        target::HudTarget,
        world_markers::{
            CROP, DistanceSource, LABEL_CROP, MarkerStatus, STATUS, Status,
            depth::{DepthPoint, DepthProbe, RESULT_TTL_S, Timed},
            layout::{
                Billboard, Crop, Sight, StageFrame, billboard_corners, crop, placement_distance,
                radians_per_texel, slot_size, smooth_distance, surface_sight,
            },
            layout_for, raycast,
        },
        world_quads_active,
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
    let _timing = crate::frame_timing::scope(crate::frame_timing::Slot::WorldMarkers);
    let cfg = Config::lock_query(|c| c.hud);
    let mut renderer = RENDERER.lock();
    let active = cfg.world_markers.enabled && world_quads_active(&cfg);
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
                FAILURES.error(&e);
                return;
            }
        }
    }
    let Some(renderer) = renderer.as_mut() else {
        return;
    };
    renderer.plan_frame(context, device, inputs, hud);
}

/// Drop the frame's plan, so the eye passes draw nothing (eye 0, when no panel pose exists).
pub(crate) fn clear_frame() {
    if let Some(renderer) = RENDERER.lock().as_mut() {
        renderer.plan.clear();
    }
    *STATUS.lock() = None;
}

/// Crop the markers out of the HUD texture into their slots and cut them out of the panel after the
/// UI renders and before the panel draws. Call on every dispatch in ordinary two-pass rendering,
/// but only on eye 0 under collapse; eye 1 reuses the slots and the cut panel.
pub(crate) fn prepare_eye(context: &ID3D11DeviceContext, hud: Option<&HudTarget>) {
    let mut renderer = RENDERER.lock();
    let (Some(renderer), Some(hud)) = (renderer.as_mut(), hud) else {
        return;
    };
    renderer.copy_crops(context, hud);
}

/// Draw this eye's marker billboards over `target`, farthest first. Call on every eye after
/// [`prepare_eye`] prepares the slots for the dispatch, or for both eyes under collapse.
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
    /// The markers placed on a surface on their last frame, for the placement's hysteresis.
    on_surface: HashSet<u32>,
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
            on_surface: HashSet::new(),
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
        if let Err(e) = self.ensure_slots(
            device,
            slot_size(&CROP, scale),
            slot_size(&LABEL_CROP, scale),
            layout.markers.len(),
        ) {
            FAILURES.error(&e);
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
                (SlotId::Label, self.label_slot_size, &LABEL_CROP)
            } else {
                let slot = SlotId::Plain(next_plain);
                next_plain += 1;
                (slot, self.slot_size, &CROP)
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
            let (sight, source) = self.sight(marker.id, now);
            if source != DistanceSource::Depth {
                ray_requests.push((marker.id, marker.world));
            }
            let held = self.on_surface.contains(&marker.id);
            let sight = surface_sight(sight, true_distance, SURFACE_MIN_M, held);
            if matches!(sight, Sight::Surface(_)) {
                self.on_surface.insert(marker.id);
            } else {
                self.on_surface.remove(&marker.id);
            }
            let target = placement_distance(true_distance, sight, SURFACE_PULL_M, MIN_DISTANCE_M);
            let distance = self.smoothed(marker.id, target, DEPTH_HALFLIFE_S, now);
            let drawn = distance.min(MAX_DISTANCE_M);
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
        let distances = &self.distances;
        self.on_surface.retain(|id| distances.contains_key(id));
        self.depth_results
            .retain(|_, timed| now.duration_since(timed.at).as_secs_f32() < FORGET_S);

        self.sample_depth(context, device, inputs, &depth_points, now);
        raycast::request(ray_requests);

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
    fn sight(&self, id: u32, now: Instant) -> (Sight, DistanceSource) {
        let fresh = |timed: &Timed| now.duration_since(timed.at).as_secs_f32() < RESULT_TTL_S;
        if let Some(timed) = self.depth_results.get(&id).filter(|t| fresh(t)) {
            return (timed.sight, DistanceSource::Depth);
        }
        if let Some(timed) = raycast::result(id).filter(|t| fresh(t)) {
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
                    FAILURES.error(&e);
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
    fn copy_crops(&mut self, context: &ID3D11DeviceContext, hud: &HudTarget) {
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

/// Surfaces nearer the eye than this are ignored for the placement, in metres: the player's own
/// gun and hands, and foliage brushing the head, would otherwise pull every marker behind them
/// onto them.
const SURFACE_MIN_M: f32 = 1.2;

/// How far a surface-placed marker is pulled toward the eye from the surface, in metres, so it is
/// not coplanar with it.
const SURFACE_PULL_M: f32 = 0.3;

/// The halflife of the placement distance's smoothing, in seconds, so a flickering occluder does
/// not make the marker pop.
const DEPTH_HALFLIFE_S: f32 = 0.15;

/// The farthest a billboard is placed, in metres. A farther marker is drawn at this distance along
/// its direction, where its stereo disparity is already indistinguishable from infinity.
const MAX_DISTANCE_M: f32 = 300.0;

/// How long a marker's placement state outlives its last sighting, in seconds.
const FORGET_S: f32 = 2.0;

static FAILURES: LogOnce = LogOnce::new("hud world markers");
