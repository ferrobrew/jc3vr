//! `CHUDUI` detours: the point-of-interest update brackets the projections the world markers
//! classify (see [`crate::hud::world_markers`]).

use detours_macro::detour;
use jc3gi::{
    character::{character::Character, player::Player},
    ui::hud_ui::HUDUI,
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&UPDATE_POIS_BINDER)
}

/// Mark the HUD's point-of-interest update, so the `Get2DInfo` detour knows the projections inside
/// it are the points of interest's.
#[detour(address = HUDUI::UpdatePOIs_ADDRESS)]
fn update_pois(this: *mut HUDUI, player: *mut Player, character: *mut Character, dt: f32) {
    crate::hud::world_markers::begin_update_pois();
    UPDATE_POIS.get().unwrap().call(this, player, character, dt);
    crate::hud::world_markers::end_update_pois();
}
