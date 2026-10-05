//! The HUD atlas: parts of the gameplay HUD moved into their own cells of the HUD texture for the
//! capture, and drawn from there on their own quads.
//!
//! The HUD renders into one texture, taller than the stage's aspect, so the show-all scaling leaves
//! a band above and below the stage. With the atlas on, the gameplay HUD's safe area hugs the stage
//! (the stage-shaped safe-area input of [`crate::hud::binding`]), so nothing the HUD lays out lands
//! in the bands, and at each capture the parts below are moved into cells there:
//!
//! - the weapon readout (`MCI_weapons_parent`), drawn above the gun hand
//!   ([`crate::hud::anchored`]);
//! - the heat readout (`MCI_heat_container`), drawn on the off-hand's wrist;
//! - the weapon selection cross (`MCI_weapon_selection_wheel`), drawn at the hand steering the
//!   weapon wheel ([`crate::controllers::weapons::wheel`]) while the wheel shows it;
//! - the tether readout (`MCI_dual_tether`), drawn above the off-hand's forearm, where the
//!   grapple sits;
//! - the visible world-placed markers ([`crate::hud::world_markers`]), each in a cell of its own
//!   instead of cropped from the panel, so a marker's quad never carries a neighbour or a centre
//!   notice.
//!
//! The pieces:
//!
//! - [`layout`]: the pure geometry of the bands, the cells, and the marker cells' assignment;
//! - [`seam`] (the capture thread): the moves before the capture and the restores after it, and
//!   the published layout, keyed by the snapshot the capture produced;
//! - [`render`] (the render thread): the layout matching the displayed snapshot, each part's cell
//!   copied into its own texture per eye, and the bands cleared before the panel samples the HUD
//!   texture, so moved parts vanish from the panel.
//!
//! There is one capture per frame, the moves are node-local display-info writes undone before the
//! next advance, and the engine's own render runs unchanged. See
//! `docs/mod/rendering/ui-decoupling.md`.

pub(crate) mod layout;
mod render;
mod seam;

use std::collections::VecDeque;

use parking_lot::Mutex;
pub(crate) use render::{Part, clear_frame, part_texture, prepare_eye, prepare_frame};
pub(crate) use seam::{after_capture, begin_capture};

use crate::hud::{
    atlas::layout::{AtlasLayout, Cell},
    world_markers::layout::StageFrame,
};

/// The atlas's counters and the last frame's readout, for the debug UI.
#[derive(Debug, Clone, Default)]
pub(crate) struct AtlasStatus {
    /// Clips moved into cells, since the payload loaded.
    pub moves: u64,
    /// Clips moved back after their capture.
    pub restores: u64,
    /// Moves that could not read or write the clip.
    pub move_failures: u64,
    /// Restores that could not reach the clip.
    pub restore_failures: u64,
    /// Seams at which a moved clip was not where its last restore put it: something else moved it
    /// between captures.
    pub drift: u64,
    /// The layout drawn on the last frame, if the atlas drew one.
    pub frame: Option<FrameStatus>,
}

/// The layout the last frame drew.
#[derive(Debug, Clone)]
pub(crate) struct FrameStatus {
    pub snapshot: u64,
    pub displayed_snapshot: u64,
    /// Whether the layout's snapshot is the displayed one (otherwise the newest layout is used).
    pub matched: bool,
    pub layout: AtlasLayout,
    pub ammo: Option<Cell>,
    pub heat: Option<Cell>,
    pub cross: Option<Cell>,
    pub tether: Option<Cell>,
    /// The texture the cells were copied from.
    pub texture: (u32, u32),
}

/// The latest status.
pub(crate) fn status() -> AtlasStatus {
    STATUS.lock().clone()
}

/// One capture's atlas, as the render side uses it.
#[derive(Debug, Clone)]
pub(crate) struct AtlasCapture {
    /// The movie's snapshot id after the capture.
    pub snapshot: u64,
    /// The stage's place in the texture the layout was made for.
    pub frame: StageFrame,
    /// The weapon readout's cell, when it was moved into it.
    pub ammo: Option<Cell>,
    /// The heat readout's cell, when it was moved into it.
    pub heat: Option<Cell>,
    /// The weapon selection cross's cell, when it was moved into it.
    pub cross: Option<Cell>,
    /// The tether readout's cell, when it was moved into it.
    pub tether: Option<Cell>,
    pub layout: AtlasLayout,
}

/// How many recent captures are kept for matching the displayed snapshot.
const HISTORY: usize = 4;

/// The recent captures, newest last. Written at the capture seam, read by the render thread.
static CAPTURES: Mutex<VecDeque<AtlasCapture>> = Mutex::new(VecDeque::new());

static STATUS: Mutex<AtlasStatus> = Mutex::new(AtlasStatus {
    moves: 0,
    restores: 0,
    move_failures: 0,
    restore_failures: 0,
    drift: 0,
    frame: None,
});

/// Publish a capture's atlas, or clear the history when the atlas did not act for the capture.
fn publish(capture: Option<AtlasCapture>) {
    let mut captures = CAPTURES.lock();
    match capture {
        Some(capture) => {
            if captures.len() >= HISTORY {
                captures.pop_front();
            }
            captures.push_back(capture);
        }
        None => captures.clear(),
    }
}

/// The capture for the displayed snapshot `displayed`, or the newest one when none matches, with
/// whether it matched.
fn capture_for(displayed: u64) -> Option<(AtlasCapture, bool)> {
    let captures = CAPTURES.lock();
    if let Some(capture) = captures.iter().rev().find(|c| c.snapshot == displayed) {
        return Some((capture.clone(), true));
    }
    captures.back().map(|c| (c.clone(), false))
}
