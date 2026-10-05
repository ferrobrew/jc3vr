//! The game's selection cross placed on the floating panel where the steering hand was when the
//! wheel opened, seen from the head, instead of over the character's hips.
//!
//! The engine places the cross every HUD update in
//! [`HUDUI::UpdateWeaponSelectionPlacement`]: it projects the hips through the render camera,
//! which in the headset puts them below the panel's view, so the cross sits clamped at the
//! panel's bottom edge. The input tick publishes the panel point under the steering hand at the
//! open ([`publish`]); a post-call detour on the placement then writes the clip's position to
//! that point, clamped as the engine clamps its own so the whole cross stays in the safe area.
//! The publication lingers a moment after the wheel closes, so that the cross's hide animation
//! plays where it was shown rather than over the hips.
//!
//! The detour runs on the thread that runs the UI update, and reads only the published slot (a
//! leaf lock) and the HUD and UI manager it is called with.
//!
//! With the HUD atlas on, the capture seam moves the cross from wherever this placed it into an
//! atlas cell for the capture, and the HUD draws that cell at the steering hand
//! ([`crate::hud::anchored`], from [`crate::controllers::weapons::wheel::cross_anchor`]); the cross is moved back to this
//! placement after the capture, so this stays the placement the game sees and the fallback
//! whenever the atlas does not draw the cross.

use std::time::{Duration, Instant};

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3, Vec4};
use jc3gi::{
    character::character::Character,
    ui::{
        hud_ui::HUDUI,
        scaleform::DisplayInfo,
        ui_manager::{StageRect, UIManager},
    },
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    controllers::frame::Pose,
    hud::{
        pointer::{aim_at_panel, intersect_panel},
        world_markers::layout::StageFrame,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_WEAPON_SELECTION_PLACEMENT_BINDER)
}

/// Where the cross goes on the panel.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PanelPoint {
    /// The point in the panel texture's UV.
    pub uv: Vec2,
    /// The panel's width over its height, which is the HUD texture's.
    pub aspect: f32,
}

/// The panel point under `hand`, seen from `head`, both in the cockpit frame, or `None` when no
/// panel was drawn recently or the hand is not in front of it.
pub fn panel_point(head: Vec3, hand: Vec3) -> Option<PanelPoint> {
    let direction = (hand - head).try_normalize()?;
    let (ray, corners) = aim_at_panel(Pose {
        position: head,
        orientation: Quat::from_rotation_arc(Vec3::NEG_Z, direction),
    })?;
    let hit = intersect_panel(ray, &corners)?;
    let across = (corners[1] - corners[0]).truncate().length();
    let down = (corners[2] - corners[0]).truncate().length();
    let aspect = across / down;
    (aspect.is_finite() && aspect > 0.0).then_some(PanelPoint {
        uv: Vec2::new(hit.uv.0, hit.uv.1),
        aspect,
    })
}

/// Publish where the cross goes while the wheel is open, or after it closes (`open` false) for a
/// short linger; `None` withdraws it at once.
pub fn publish(point: Option<PanelPoint>, open: bool) {
    let now = Instant::now();
    let mut slot = PLACED.lock();
    *slot = match (point, open) {
        (Some(point), true) => Some((point, now + OPEN_TTL)),
        // The close keeps the last point a little longer, for the hide animation.
        (None, false) => slot.map(|(point, _)| (point, now + LINGER)),
        (Some(point), false) => Some((point, now + LINGER)),
        (None, true) => None,
    };
}

/// Withdraw the published point at once.
pub fn clear() {
    *PLACED.lock() = None;
}

/// The clip position for a panel point, in stage coordinates: the stage frame's point at `uv`,
/// for a stage of size `stage` shown in a texture of `aspect`, clamped so that the clip's extent
/// `bb` (the signed bottom, top, left, and right edge offsets, as
/// [`HUDUI::m_SelectionWheelBB`] holds them) stays inside `safe`. On an axis where the extent does
/// not fit, the clip is centred in the safe area.
pub fn stage_position(
    point: PanelPoint,
    stage: Vec2,
    safe: (Vec2, Vec2),
    bb: Vec4,
) -> Option<Vec2> {
    let frame = StageFrame::new(stage, point.aspect)?;
    let at = frame.origin + point.uv * frame.size;
    let (min, max) = safe;
    let lower = Vec2::new(min.x - bb.z, min.y - bb.y);
    let upper = Vec2::new(max.x - bb.w, max.y - bb.x);
    let clamp = |v: f32, lo: f32, hi: f32, mid: f32| if lo <= hi { v.clamp(lo, hi) } else { mid };
    let centre = (min + max) * 0.5;
    let placed = Vec2::new(
        clamp(at.x, lower.x, upper.x, centre.x),
        clamp(at.y, lower.y, upper.y, centre.y),
    );
    placed.is_finite().then_some(placed)
}

/// How long a point published while open lasts without a refresh: past a few input ticks, a
/// stalled tick stops moving the cross.
const OPEN_TTL: Duration = Duration::from_millis(250);

/// How long the point lasts after the wheel closes: the length of the cross's hide animation.
const LINGER: Duration = Duration::from_millis(500);

static PLACED: Mutex<Option<(PanelPoint, Instant)>> = Mutex::new(None);

/// The published point, while it lasts.
fn current(now: Instant) -> Option<PanelPoint> {
    PLACED
        .lock()
        .filter(|(_, until)| now <= *until)
        .map(|(point, _)| point)
}

#[detour(address = jc3gi::ui::hud_ui::HUDUI::UpdateWeaponSelectionPlacement_ADDRESS)]
fn update_weapon_selection_placement(hud: *mut HUDUI, character: *mut Character, dt: f32) {
    UPDATE_WEAPON_SELECTION_PLACEMENT
        .get()
        .unwrap()
        .call(hud, character, dt);
    let Some(point) = current(Instant::now()) else {
        return;
    };
    // SAFETY: the HUD is the live one the engine is updating, on the thread that runs the UI
    // update, where its clip values are written.
    unsafe { place(hud, point) };
}

/// Write the cross's position for `point`.
///
/// # Safety
/// `hud` must be the live HUD, on the thread that runs the UI update.
unsafe fn place(hud: *mut HUDUI, point: PanelPoint) {
    // SAFETY: forwarded from the caller; the UI manager is a live singleton while the HUD updates.
    let (Some(hud), Some(manager)) = (unsafe { hud.as_mut() }, unsafe { UIManager::get() }) else {
        return;
    };
    let stage = Vec2::new(manager.m_CachedStageWidth, manager.m_CachedStageHeight);
    let safe: &StageRect = &manager.m_SafeArea;
    let safe = (Vec2::new(safe.x1, safe.y1), Vec2::new(safe.x2, safe.y2));
    let bb = Vec4::from(hud.m_SelectionWheelBB.data);
    let Some(at) = stage_position(point, stage, safe, bb) else {
        return;
    };
    let clip = &mut hud.m_SelectionWheelUi;
    if clip.value_type() == 0 {
        return;
    }
    // SAFETY: the clip is the HUD's live managed display-object value.
    let Some(interface) = (unsafe { clip.pObjectInterface.as_mut() }) else {
        return;
    };
    let mut info = DisplayInfo::default();
    info.X = f64::from(at.x);
    info.Y = f64::from(at.y);
    info.VarsSet = (DisplayInfo::V_X | DisplayInfo::V_Y) as u16;
    // SAFETY: as above; the write sets the position alone.
    unsafe { interface.SetDisplayInfo(clip.mValue as *mut std::ffi::c_void, &info) };
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Vec2 = Vec2::new(1920.0, 1080.0);
    const SAFE: (Vec2, Vec2) = (Vec2::new(96.0, 54.0), Vec2::new(1824.0, 1026.0));
    /// A cross 440 wide and 320 tall about its origin.
    const BB: Vec4 = Vec4::new(160.0, -160.0, -220.0, 220.0);

    fn at(u: f32, v: f32, aspect: f32) -> Option<Vec2> {
        stage_position(
            PanelPoint {
                uv: Vec2::new(u, v),
                aspect,
            },
            STAGE,
            SAFE,
            BB,
        )
    }

    #[test]
    fn the_panel_centre_is_the_stage_centre() {
        assert_eq!(at(0.5, 0.5, 16.0 / 9.0), Some(Vec2::new(960.0, 540.0)));
        assert_eq!(at(0.5, 0.5, 1.0), Some(Vec2::new(960.0, 540.0)));
    }

    #[test]
    fn a_taller_texture_maps_its_bands_past_the_stage() {
        // A square texture shows the 16:9 stage with bands above and below.
        let p = at(0.6, 0.45, 1.0).unwrap();
        assert!((p.x - 1152.0).abs() < 1e-3, "{p:?}");
        assert!((p.y - 444.0).abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn the_cross_is_kept_inside_the_safe_area() {
        assert_eq!(at(0.0, 0.0, 16.0 / 9.0), Some(Vec2::new(316.0, 214.0)));
        assert_eq!(at(1.0, 1.0, 16.0 / 9.0), Some(Vec2::new(1604.0, 866.0)));
    }

    #[test]
    fn an_extent_wider_than_the_safe_area_centres() {
        let p = stage_position(
            PanelPoint {
                uv: Vec2::new(0.1, 0.5),
                aspect: 16.0 / 9.0,
            },
            STAGE,
            SAFE,
            Vec4::new(160.0, -160.0, -2000.0, 2000.0),
        );
        assert_eq!(p, Some(Vec2::new(960.0, 540.0)));
    }

    #[test]
    fn a_degenerate_stage_places_nothing() {
        assert_eq!(
            stage_position(
                PanelPoint {
                    uv: Vec2::splat(0.5),
                    aspect: 1.0
                },
                Vec2::ZERO,
                SAFE,
                BB
            ),
            None
        );
    }
}
