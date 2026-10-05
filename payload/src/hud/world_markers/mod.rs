//! The world-marker prototype: the game's on-screen point-of-interest markers drawn as billboards at
//! their world points instead of on the floating panel.
//!
//! Every world-anchored HUD marker (objectives, waypoints, enemy and vehicle health markers, weapon
//! pickup hints, explosives) is a point of interest whose record in the HUD's list carries both its
//! marker clip and its world point. The prototype takes them from there and crops each marker out
//! of the existing HUD texture, without moving anything in the movie:
//!
//! - [`seam`] (the thread that runs the UI update): inside the HUD's point-of-interest update, a
//!   marker the headset can see is classified on screen even where the panel's narrower projection
//!   would make it an edge arrow; at the capture seam, the visible on-screen markers (id, world
//!   point, clip origin, 2D offset) are snapshotted and published, keyed by the snapshot the capture
//!   produced.
//! - [`render`] (the render thread, at each eye's HUD draw): the published layout matching the
//!   displayed snapshot is cropped out of the HUD texture into per-marker slots, the rectangles are
//!   cut out of the panel, and each slot is drawn as a billboard facing the head at the marker's
//!   world point, with the angular size it had on the panel. The scene depth at each marker's pixel
//!   ([`depth`]) places it on the nearest visible surface along its line of sight.
//! - [`raycast`] (the game thread, in the player aim's simulation update): a physics raycast for
//!   the markers that had no recent depth sample.
//!
//! [`layout`] holds the pure geometry. The whole prototype is behind
//! [`WorldMarkersConfig::enabled`](crate::hud::config::WorldMarkersConfig::enabled). See
//! `docs/mod/rendering/ui-decoupling.md`.

mod depth;
pub(crate) mod layout;
mod raycast;
mod render;
mod seam;

use std::collections::VecDeque;

use glam::{Vec2, Vec3};
use parking_lot::Mutex;
pub(crate) use raycast::resolve_rays;
pub(crate) use render::{FrameInputs, clear_frame, draw_billboards, prepare_eye, prepare_frame};
pub(crate) use seam::{
    Projection, after_capture, begin_update_pois, classify, end_update_pois, on_capture,
};

/// The most markers a frame places in the world; the rest stay on the panel.
pub(crate) const MAX_MARKERS: usize = 24;

/// A live snapshot of the prototype for the debug UI.
#[derive(Clone, Default)]
pub(crate) struct Status {
    /// The snapshot id of the layout drawn last, and whether it matched the displayed snapshot (a
    /// mismatch falls back to the newest layout).
    pub layout_snapshot: Option<u64>,
    pub displayed_snapshot: u64,
    pub layout_matched: bool,
    /// Visible on-screen markers past [`MAX_MARKERS`] in the last layout.
    pub skipped: usize,
    /// The labelled POI's id in the last layout.
    pub labelled_id: u32,
    /// The stage size and the HUD texture size the crops were taken against.
    pub stage: Vec2,
    pub texture: (u32, u32),
    pub markers: Vec<MarkerStatus>,
}

/// One drawn marker, for the debug UI.
#[derive(Clone, Copy)]
pub(crate) struct MarkerStatus {
    pub id: u32,
    pub world: Vec3,
    pub origin: Vec2,
    /// The crop's texel rectangle in the HUD texture `(x0, y0, x1, y1)`.
    pub rect: (u32, u32, u32, u32),
    pub true_distance: f32,
    pub drawn_distance: f32,
    pub source: DistanceSource,
}

/// What set a marker's drawn distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DistanceSource {
    /// The true point: surface placement is off, or nothing is nearer.
    Point,
    /// The scene depth at the marker's pixel.
    Depth,
    /// The fallback raycast.
    Ray,
}

/// The latest status, or `None` while the prototype is not drawing.
pub(crate) fn status() -> Option<Status> {
    STATUS.lock().clone()
}

/// One visible marker as the capture seam saw it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MarkerRecord {
    /// The POI's id, stable while it is registered.
    pub id: u32,
    /// The POI's world point (its transform's translation).
    pub world: Vec3,
    /// The marker clip's origin, in stage coordinates.
    pub origin: Vec2,
    /// The POI's 2D offset, in stage units, which the clip origin includes.
    pub offset: Vec2,
}

/// The visible markers of one capture.
#[derive(Clone, Debug)]
pub(crate) struct CaptureLayout {
    /// The movie's snapshot id after the capture, to match against the displayed snapshot.
    pub snapshot: u64,
    /// The stage size, in stage units.
    pub stage: Vec2,
    /// The id of the POI whose label is shown.
    pub labelled_id: u32,
    pub markers: Vec<MarkerRecord>,
    /// Visible on-screen markers past [`MAX_MARKERS`].
    pub skipped: usize,
}

/// How many recent layouts are kept for matching the displayed snapshot.
const LAYOUT_HISTORY: usize = 4;

/// The recent layouts, newest last. Written at the capture seam, read by the render thread.
static LAYOUTS: Mutex<VecDeque<CaptureLayout>> = Mutex::new(VecDeque::new());

/// The latest status, written by the render thread.
static STATUS: Mutex<Option<Status>> = Mutex::new(None);

/// Publish a capture's layout, or clear them all when the prototype is inactive for the capture.
fn publish_layout(layout: Option<CaptureLayout>) {
    let mut layouts = LAYOUTS.lock();
    match layout {
        Some(layout) => {
            if layouts.len() >= LAYOUT_HISTORY {
                layouts.pop_front();
            }
            layouts.push_back(layout);
        }
        None => layouts.clear(),
    }
}

/// The layout for the displayed snapshot `displayed`, or the newest one when none matches, with
/// whether it matched.
fn layout_for(displayed: u64) -> Option<(CaptureLayout, bool)> {
    let layouts = LAYOUTS.lock();
    if let Some(layout) = layouts.iter().rev().find(|l| l.snapshot == displayed) {
        return Some((layout.clone(), true));
    }
    layouts.back().map(|l| (l.clone(), false))
}
