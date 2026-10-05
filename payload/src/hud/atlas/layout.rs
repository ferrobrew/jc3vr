//! The atlas's pure geometry: the bands the stage leaves free in the HUD texture, the cells packed
//! into them, the marker cells' assignment across captures, where a moved clip goes in its parent's
//! space, and which texels a cell covers.

use glam::Vec2;

use crate::hud::{
    config::CropExtents,
    world_markers::layout::{StageFrame, TexelRect},
};

/// A rectangle in stage coordinates.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct StageRect {
    pub min: Vec2,
    pub size: Vec2,
}

impl StageRect {
    pub fn max(&self) -> Vec2 {
        self.min + self.size
    }
}

/// One part's cell: the rectangle its content fills, where its clip's origin goes, and the scale
/// it is captured at, all in stage coordinates.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Cell {
    pub rect: StageRect,
    /// The clip's origin, in whole stage pixels.
    pub origin: Vec2,
    pub scale: f32,
}

/// A part to place: its extents around its clip's origin (in the clip's own pixels) and the
/// largest scale it is captured at.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PartRequest {
    pub extents: CropExtents,
    pub max_scale: f32,
}

/// What a capture asks the atlas to hold.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct LayoutRequest {
    pub stage: Vec2,
    pub frame: StageFrame,
    /// The empty margin around every cell, stage pixels.
    pub gutter: f32,
    pub ammo: Option<PartRequest>,
    pub heat: Option<PartRequest>,
    /// The weapon selection cross.
    pub cross: Option<PartRequest>,
    /// The tether readout.
    pub tether: Option<PartRequest>,
    /// The labelled marker's extents (its label sits to the left of its icon).
    pub label: Option<CropExtents>,
    /// Every other marker's extents.
    pub marker: Option<CropExtents>,
    /// The most marker cells to place.
    pub max_markers: usize,
}

/// The cells of one capture. Fixed for a given request, so consecutive captures agree.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AtlasLayout {
    /// The stage size the layout was made for.
    pub stage: Vec2,
    /// The bands above and below the stage.
    pub bands: [StageRect; 2],
    pub ammo: Option<Cell>,
    pub heat: Option<Cell>,
    pub cross: Option<Cell>,
    pub tether: Option<Cell>,
    pub label: Option<Cell>,
    pub markers: Vec<Cell>,
}

/// The bands the show-all scaling exposes above and below the stage, or `None` when the visible
/// frame does not extend past the stage vertically (by less than a pixel).
pub(crate) fn bands(stage: Vec2, frame: &StageFrame) -> Option<[StageRect; 2]> {
    let band = frame.vertical_band();
    if band < 1.0 {
        return None;
    }
    let x = frame.origin.x;
    let width = frame.size.x;
    Some([
        StageRect {
            min: Vec2::new(x, -band),
            size: Vec2::new(width, band),
        },
        StageRect {
            min: Vec2::new(x, stage.y),
            size: Vec2::new(width, band),
        },
    ])
}

/// Pack the request's parts into the bands: the ammunition and heat readouts, the selection cross,
/// and the tether readout first, then the label cell, then as many marker cells as fit, up to the
/// request's cap. A part that fits nowhere gets no cell. `None` without bands.
pub(crate) fn layout(request: &LayoutRequest) -> Option<AtlasLayout> {
    let bands = bands(request.stage, &request.frame)?;
    let gutter = request.gutter.max(0.0);
    let mut packer = Packer::new(bands, gutter);
    let ammo = request.ammo.and_then(|part| place_part(&mut packer, &part));
    let heat = request.heat.and_then(|part| place_part(&mut packer, &part));
    let cross = request
        .cross
        .and_then(|part| place_part(&mut packer, &part));
    let tether = request
        .tether
        .and_then(|part| place_part(&mut packer, &part));
    let label = request
        .label
        .and_then(|extents| place_marker(&mut packer, extents));
    let mut markers = Vec::new();
    if let Some(extents) = request.marker {
        while markers.len() < request.max_markers {
            let Some(cell) = place_marker(&mut packer, extents) else {
                break;
            };
            markers.push(cell);
        }
    }
    Some(AtlasLayout {
        stage: request.stage,
        bands,
        ammo,
        heat,
        cross,
        tether,
        label,
        markers,
    })
}

/// The marker cells' assignment, kept across captures so a marker keeps its cell while it stays
/// visible: the render side may pair a snapshot with a neighbouring capture's layout for a frame,
/// and a stable cell keeps that harmless.
#[derive(Debug, Default)]
pub(crate) struct MarkerSlots {
    /// Each marker's cell, as `(id, cell index)`; there are only a few dozen at most.
    by_id: Vec<(u32, usize)>,
}

impl MarkerSlots {
    pub const fn new() -> Self {
        Self { by_id: Vec::new() }
    }

    /// Assign a cell index under `capacity` to each of `ids`, in order: a marker keeps its previous
    /// cell, and a new one takes the lowest free cell. Markers past the capacity (and repeats of
    /// an id) get none. Markers no longer present release their cells.
    pub fn assign(&mut self, ids: &[u32], capacity: usize) -> Vec<Option<usize>> {
        self.by_id
            .retain(|(id, slot)| *slot < capacity && ids.contains(id));
        let mut used = vec![false; capacity];
        for (_, slot) in &self.by_id {
            used[*slot] = true;
        }
        let mut seen = Vec::with_capacity(ids.len());
        let mut result = Vec::with_capacity(ids.len());
        for &id in ids {
            if seen.contains(&id) {
                result.push(None);
                continue;
            }
            seen.push(id);
            let kept = self
                .by_id
                .iter()
                .find(|(kept, _)| *kept == id)
                .map(|(_, slot)| *slot);
            let slot = kept.or_else(|| {
                let free = used.iter().position(|u| !u)?;
                used[free] = true;
                self.by_id.push((id, free));
                Some(free)
            });
            result.push(slot);
        }
        result
    }

    pub fn clear(&mut self) {
        self.by_id.clear();
    }
}

/// A display object's 2D placement in its parent: its position and scale (rotation is not used by
/// any part the atlas moves).
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Placement {
    pub position: Vec2,
    pub scale: Vec2,
}

/// The placement in a parent placed at `parent` (in stage coordinates) that puts a clip's origin at
/// the stage point `origin` at the uniform stage scale `scale`. `None` for a degenerate parent
/// scale.
pub(crate) fn local_placement(parent: Placement, origin: Vec2, scale: f32) -> Option<Placement> {
    if parent.scale.x.abs() < f32::EPSILON || parent.scale.y.abs() < f32::EPSILON {
        return None;
    }
    Some(Placement {
        position: (origin - parent.position) / parent.scale,
        scale: Vec2::splat(scale) / parent.scale,
    })
}

/// A clip's extents around its origin from its edges' signed offsets from it, in the order the HUD
/// keeps the selection cross's (bottom, top, left, right), grown by `margin` on every side, or
/// `None` for an empty or unmeasured extent.
pub(crate) fn extents_from_edges(
    [bottom, top, left, right]: [f32; 4],
    margin: f32,
) -> Option<CropExtents> {
    let finite = [bottom, top, left, right].iter().all(|e| e.is_finite());
    if !finite || right <= left || bottom <= top {
        return None;
    }
    let margin = margin.max(0.0);
    Some(CropExtents {
        left: margin - left,
        right: right + margin,
        up: margin - top,
        down: bottom + margin,
    })
}

/// The texels a stage rectangle covers in a texture of `texture` texels, shrunk to whole texels
/// inside it, or `None` when nothing of it is inside the texture.
pub(crate) fn texel_rect(
    frame: &StageFrame,
    texture: (u32, u32),
    rect: &StageRect,
) -> Option<TexelRect> {
    let size = Vec2::new(texture.0 as f32, texture.1 as f32);
    let min = (frame.to_uv(rect.min) * size).ceil();
    let max = (frame.to_uv(rect.max()) * size).floor();
    let clamp = |v: f32, limit: u32| (v.max(0.0) as u32).min(limit);
    let texels = TexelRect {
        x0: clamp(min.x, texture.0),
        y0: clamp(min.y, texture.1),
        x1: clamp(max.x, texture.0),
        y1: clamp(max.y, texture.1),
    };
    (texels.x1 > texels.x0 && texels.y1 > texels.y0).then_some(texels)
}

/// The texels of the bands, grown to whole texels outward from the stage so that no band texel is
/// left over, and never reaching into the stage.
pub(crate) fn band_texels(
    frame: &StageFrame,
    texture: (u32, u32),
    stage: Vec2,
) -> Option<[TexelRect; 2]> {
    if frame.vertical_band() < 1.0 {
        return None;
    }
    let height = texture.1 as f32;
    let top_end = (frame.to_uv(Vec2::ZERO).y * height).floor().max(0.0) as u32;
    let bottom_start = ((frame.to_uv(stage).y * height).ceil() as u32).min(texture.1);
    let rows = |y0: u32, y1: u32| TexelRect {
        x0: 0,
        y0,
        x1: texture.0,
        y1,
    };
    let [top, bottom] = [rows(0, top_end), rows(bottom_start, texture.1)];
    (top.height() > 0 && bottom.height() > 0).then_some([top, bottom])
}

/// Place a part at the largest scale up to its own that fits a band.
fn place_part(packer: &mut Packer, part: &PartRequest) -> Option<Cell> {
    let raw = extents_size(&part.extents);
    if raw.x <= 0.0 || raw.y <= 0.0 {
        return None;
    }
    let room = packer.largest_room().floor();
    let scale = part.max_scale.min(room.x / raw.x).min(room.y / raw.y);
    if scale.is_nan() || scale <= 0.0 {
        return None;
    }
    let size = (raw * scale).ceil().min(room);
    let min = packer.place(size)?;
    Some(Cell {
        rect: StageRect { min, size },
        origin: (min + Vec2::new(part.extents.left, part.extents.up) * scale).round(),
        scale,
    })
}

/// Place a marker at its own size: its crop keeps the size it has on the panel, so a marker too
/// large for a band gets no cell rather than a smaller one.
fn place_marker(packer: &mut Packer, extents: CropExtents) -> Option<Cell> {
    let room = packer.largest_room();
    let raw = extents_size(&extents).ceil();
    if raw.x > room.x || raw.y > room.y {
        return None;
    }
    place_part(
        packer,
        &PartRequest {
            extents,
            max_scale: 1.0,
        },
    )
}

/// The size of an extents rectangle.
fn extents_size(extents: &CropExtents) -> Vec2 {
    Vec2::new(
        (extents.left + extents.right).max(0.0),
        (extents.up + extents.down).max(0.0),
    )
}

/// Shelf packing in the two bands: each band holds rows of cells left to right, a row as tall as
/// its first cell, rows stacked downward while they fit. Every placed size is surrounded by the
/// gutter.
struct Packer {
    bands: [StageRect; 2],
    gutter: f32,
    shelves: [Vec<Shelf>; 2],
}

/// One row of cells.
struct Shelf {
    /// The row's top edge (gutter included).
    y: f32,
    height: f32,
    /// The next free x (gutter included).
    x: f32,
}

impl Packer {
    fn new(bands: [StageRect; 2], gutter: f32) -> Self {
        Self {
            bands,
            gutter,
            shelves: [Vec::new(), Vec::new()],
        }
    }

    /// The largest content size a cell can have in an empty band.
    fn largest_room(&self) -> Vec2 {
        self.bands
            .iter()
            .map(|band| band.size - Vec2::splat(2.0 * self.gutter))
            .fold(Vec2::ZERO, Vec2::max)
    }

    /// Place a cell of `size` (content only) and return its content's top-left, or `None` when no
    /// band has room.
    fn place(&mut self, size: Vec2) -> Option<Vec2> {
        let outer = size + Vec2::splat(2.0 * self.gutter);
        for (band, shelves) in self.bands.iter().zip(self.shelves.iter_mut()) {
            let right = band.max().x;
            if let Some(shelf) = shelves
                .iter_mut()
                .find(|shelf| shelf.height >= outer.y && shelf.x + outer.x <= right)
            {
                let at = Vec2::new(shelf.x, shelf.y);
                shelf.x += outer.x;
                return Some((at + Vec2::splat(self.gutter)).ceil());
            }
            let next_y = shelves.last().map_or(band.min.y, |s| s.y + s.height);
            if next_y + outer.y <= band.max().y && band.min.x + outer.x <= right {
                shelves.push(Shelf {
                    y: next_y,
                    height: outer.y,
                    x: band.min.x + outer.x,
                });
                return Some((Vec2::new(band.min.x, next_y) + Vec2::splat(self.gutter)).ceil());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Vec2 = Vec2::new(1280.0, 720.0);

    fn frame(aspect: f32) -> StageFrame {
        StageFrame::new(STAGE, aspect).unwrap()
    }

    fn marker() -> CropExtents {
        CropExtents {
            left: 40.0,
            right: 90.0,
            up: 44.0,
            down: 44.0,
        }
    }

    fn request(aspect: f32) -> LayoutRequest {
        LayoutRequest {
            stage: STAGE,
            frame: frame(aspect),
            gutter: 6.0,
            ammo: Some(PartRequest {
                extents: CropExtents {
                    left: 25.0,
                    right: 285.0,
                    up: 90.0,
                    down: 175.0,
                },
                max_scale: 1.0,
            }),
            heat: Some(PartRequest {
                extents: CropExtents {
                    left: 140.0,
                    right: 170.0,
                    up: 110.0,
                    down: 30.0,
                },
                max_scale: 1.0,
            }),
            cross: Some(PartRequest {
                extents: CropExtents {
                    left: 160.0,
                    right: 160.0,
                    up: 120.0,
                    down: 120.0,
                },
                max_scale: 1.0,
            }),
            tether: Some(PartRequest {
                extents: CropExtents {
                    left: 60.0,
                    right: 60.0,
                    up: 25.0,
                    down: 30.0,
                },
                max_scale: 1.0,
            }),
            label: Some(CropExtents {
                left: 360.0,
                ..marker()
            }),
            marker: Some(marker()),
            max_markers: 24,
        }
    }

    fn overlaps(a: &StageRect, b: &StageRect) -> bool {
        a.min.x < b.max().x && b.min.x < a.max().x && a.min.y < b.max().y && b.min.y < a.max().y
    }

    fn inside(rect: &StageRect, band: &StageRect) -> bool {
        rect.min.x >= band.min.x
            && rect.min.y >= band.min.y
            && rect.max().x <= band.max().x
            && rect.max().y <= band.max().y
    }

    fn all_cells(layout: &AtlasLayout) -> Vec<Cell> {
        layout
            .ammo
            .iter()
            .chain(layout.heat.iter())
            .chain(layout.cross.iter())
            .chain(layout.tether.iter())
            .chain(layout.label.iter())
            .chain(layout.markers.iter())
            .copied()
            .collect()
    }

    #[test]
    fn bands_frame_the_stage() {
        let [top, bottom] = bands(STAGE, &frame(1.3)).unwrap();
        let band = (1280.0 / 1.3 - 720.0) * 0.5;
        assert!((top.size.y - band).abs() < 1e-3);
        assert!((top.max().y).abs() < 1e-3, "the top band ends at the stage");
        assert_eq!(bottom.min.y, 720.0);
        assert_eq!(top.size.x, 1280.0);
        // A frame no taller than the stage leaves no bands.
        assert_eq!(bands(STAGE, &frame(16.0 / 9.0)), None);
    }

    /// Every cell sits inside a band, clear of the others by the gutter, and away from the stage.
    #[test]
    fn cells_are_disjoint_and_inside_the_bands() {
        let layout = layout(&request(1.3)).unwrap();
        let cells = all_cells(&layout);
        assert!(layout.ammo.is_some() && layout.heat.is_some() && layout.label.is_some());
        assert!(layout.cross.is_some() && layout.tether.is_some());
        assert!(!layout.markers.is_empty());
        for (i, a) in cells.iter().enumerate() {
            assert!(
                layout.bands.iter().any(|band| inside(&a.rect, band)),
                "{a:?} is outside the bands"
            );
            assert!(!overlaps(
                &a.rect,
                &StageRect {
                    min: Vec2::ZERO,
                    size: STAGE
                }
            ));
            for b in &cells[i + 1..] {
                let grown = StageRect {
                    min: a.rect.min - Vec2::splat(6.0),
                    size: a.rect.size + Vec2::splat(12.0),
                };
                assert!(!overlaps(&grown, &b.rect), "{a:?} and {b:?} touch");
            }
        }
    }

    /// A part taller than a band is scaled down to fit it; markers are never scaled.
    #[test]
    fn tall_parts_are_scaled_to_fit() {
        let layout = layout(&request(1.3)).unwrap();
        let ammo = layout.ammo.unwrap();
        let band = layout.bands[0].size.y;
        assert!(ammo.scale < 1.0);
        assert!(ammo.rect.size.y <= band - 12.0 + 1.0, "{ammo:?}");
        assert!(layout.markers.iter().all(|m| m.scale == 1.0));
        assert_eq!(layout.label.unwrap().scale, 1.0);
    }

    /// The clip's origin sits its scaled extents inside the cell, on a whole pixel.
    #[test]
    fn origins_follow_the_extents() {
        let layout = layout(&request(1.3)).unwrap();
        let marker = layout.markers[0];
        assert_eq!(marker.origin, marker.rect.min + Vec2::new(40.0, 44.0));
        let ammo = layout.ammo.unwrap();
        let expected = ammo.rect.min + Vec2::new(25.0, 90.0) * ammo.scale;
        assert!((ammo.origin - expected).length() <= 0.71);
        assert_eq!(ammo.origin, ammo.origin.round());
    }

    #[test]
    fn markers_stop_at_the_cap_and_at_the_room() {
        let mut capped = request(1.3);
        capped.max_markers = 3;
        assert_eq!(layout(&capped).unwrap().markers.len(), 3);
        let roomy = layout(&request(1.3)).unwrap();
        assert!(
            roomy.markers.len() < 24,
            "the bands hold fewer than the cap"
        );
    }

    /// A taller texture has taller bands, which stack more rows of markers.
    #[test]
    fn taller_bands_hold_more_rows() {
        let short = layout(&request(1.3)).unwrap().markers.len();
        let tall = layout(&request(1.0)).unwrap().markers.len();
        assert!(tall > short, "{tall} <= {short}");
    }

    #[test]
    fn nothing_fits_without_bands() {
        assert_eq!(layout(&request(16.0 / 9.0)), None);
    }

    #[test]
    fn marker_slots_are_stable() {
        let mut slots = MarkerSlots::new();
        assert_eq!(slots.assign(&[7, 9, 3], 4), vec![Some(0), Some(1), Some(2)]);
        // 9 leaves; 7 and 3 keep their cells, and the newcomer takes the freed one.
        assert_eq!(
            slots.assign(&[3, 11, 7], 4),
            vec![Some(2), Some(1), Some(0)]
        );
        // Past the capacity, a marker gets none, and a repeated id gets none the second time.
        assert_eq!(
            slots.assign(&[3, 11, 7, 12, 13, 3], 4),
            vec![Some(2), Some(1), Some(0), Some(3), None, None]
        );
        // A shrunken capacity drops the cells beyond it, and their markers take the free ones.
        assert_eq!(slots.assign(&[12, 3, 7], 2), vec![Some(1), None, Some(0)]);
    }

    #[test]
    fn local_placement_inverts_the_parent() {
        let parent = Placement {
            position: Vec2::new(1247.0, 673.0),
            scale: Vec2::new(1.0, 2.0),
        };
        let local = local_placement(parent, Vec2::new(100.0, -60.0), 0.5).unwrap();
        // Map the local placement back through the parent.
        let stage = parent.position + local.position * parent.scale;
        assert!((stage - Vec2::new(100.0, -60.0)).length() < 1e-4);
        assert_eq!(local.scale * parent.scale, Vec2::splat(0.5));
        let flat = Placement {
            scale: Vec2::new(0.0, 1.0),
            ..parent
        };
        assert_eq!(local_placement(flat, Vec2::ZERO, 1.0), None);
    }

    #[test]
    fn extents_from_the_edges() {
        let extents = extents_from_edges([160.0, -150.0, -220.0, 200.0], 10.0).unwrap();
        assert_eq!(
            extents,
            CropExtents {
                left: 230.0,
                right: 210.0,
                up: 160.0,
                down: 170.0,
            }
        );
        // An unmeasured (zero) or inverted extent gives none.
        assert_eq!(extents_from_edges([0.0; 4], 10.0), None);
        assert_eq!(extents_from_edges([-5.0, 5.0, -1.0, 1.0], 0.0), None);
        assert_eq!(extents_from_edges([f32::NAN, 0.0, -1.0, 1.0], 0.0), None);
    }

    #[test]
    fn texel_rects_stay_inside_the_cell() {
        let frame = frame(1.3);
        let texture = (2600, 2000);
        let rect = StageRect {
            min: Vec2::new(10.0, -100.0),
            size: Vec2::new(50.0, 40.0),
        };
        let texels = texel_rect(&frame, texture, &rect).unwrap();
        let per = frame.texels_per_unit(texture.0);
        let uv_min = frame.to_uv(rect.min) * Vec2::new(2600.0, 2000.0);
        assert!(texels.x0 as f32 >= uv_min.x - 1e-3);
        assert!(texels.y0 as f32 >= uv_min.y - 1e-3);
        assert!(texels.width() as f32 <= 50.0 * per + 1e-3);
        assert!(texels.width() as f32 >= 50.0 * per - 2.0);
    }

    /// The band texels cover the texture above and below the stage and never the stage itself.
    #[test]
    fn band_texels_border_the_stage() {
        let frame = frame(1.3);
        let texture = (2600, 2000);
        let [top, bottom] = band_texels(&frame, texture, STAGE).unwrap();
        let stage_top = frame.to_uv(Vec2::ZERO).y * 2000.0;
        let stage_bottom = frame.to_uv(STAGE).y * 2000.0;
        assert_eq!(top.y0, 0);
        assert!(top.y1 as f32 <= stage_top);
        assert!(top.y1 as f32 > stage_top - 1.0);
        assert!(bottom.y0 as f32 >= stage_bottom);
        assert_eq!(bottom.y1, 2000);
        assert_eq!((top.x0, top.x1), (0, 2600));
        assert_eq!(
            band_texels(&StageFrame::new(STAGE, 2.0).unwrap(), texture, STAGE),
            None
        );
    }
}
