//! The movie side of the root menu: adding the VR entry below the options entry, opening the
//! settings screen from it, and returning to the root menu when the player backs out.
//!
//! The root menus' selection dispatch does not open the settings screen for any entry type; only
//! the options menu's does, and the document's menu state, which picks the dispatch, is private.
//! So the VR entry is an event entry posting [`ENTRY_EVENT`], which the command detour takes, and
//! opening calls the document's public `ShowPauseFromHideCredits` (which puts the document in the
//! options menu's state without changing the shown list) and `OnSelect` with the entry's type
//! switched to the graphics entry's for the length of the call. The entry object is kept between
//! the two on the main-button container, an instance of a dynamic timeline class, because the
//! document's lists are private. See `docs/mod/pause-menu.md`.

use std::ffi::{CStr, CString};

use jc3gi::ui::pause_ui::{PauseMenuType, PauseUI};

use crate::pause_menu::{
    gfx::{CaptureThread, GfxValue, Param, Screen},
    model::command::ENTRY_EVENT,
    runtime::render::RenderError,
};

/// Append the VR entry, labelled `label`, to `menu`'s list: an event entry posting
/// [`ENTRY_EVENT`] that leaves the menu open. The entry is kept on the main-button container for
/// [`open_settings`].
pub(crate) fn add_entry(
    thread: &CaptureThread,
    screen: &Screen,
    menu: PauseMenuType,
    label: &str,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let label = CString::new(label).map_err(|_| RenderError::Refused("the entry's label"))?;
    let event = CString::new(ENTRY_EVENT).map_err(|_| RenderError::Refused("the entry's event"))?;
    let entry = add_option(&clip, menu, &label)?;
    if !entry.set(c"m_Type", Param::Int(PauseUI::OPTION_EVENT))
        || !entry.set(c"m_Event", Param::Str(&event))
        || !entry.set(c"m_NeedConfirm", Param::Bool(false))
        || !entry.set(c"m_CloseMenu", Param::Bool(false))
    {
        return Err(RenderError::Refused("the entry's fields"));
    }
    if !container(&clip)?.set_value(ENTRY_MEMBER, &entry) {
        return Err(RenderError::Refused(
            "keeping the entry on the main-button container",
        ));
    }
    Ok(())
}

/// Append the in-game menu's main-menu entry as the movie's `InitMenuShip` builds it, for when
/// `InitMenu` ran without the main-menu list (see [`root`](crate::pause_menu::model::root)).
pub(crate) fn add_main_menu_entry(
    thread: &CaptureThread,
    screen: &Screen,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let entry = add_option(&clip, PauseMenuType::PAUSE_SHIP, c"&pause_main_menu")?;
    if !entry.set(c"m_Type", Param::Int(PauseUI::OPTION_MAIN_MENU)) {
        return Err(RenderError::Refused("the main-menu entry's m_Type"));
    }
    Ok(())
}

/// Open the settings screen from the VR entry, which the player has just selected in the root
/// menu: the document's own options-menu selection opens it on the graphics page, which the
/// populate hook replaces. Shows the back button the options menu shows without a gamepad.
///
/// The entry's type is restored whatever happened, so a failed opening leaves the root menu as
/// it was apart from the document's menu state (see [`return_to_root`]).
pub(crate) fn open_settings(thread: &CaptureThread, screen: &Screen) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let entry = container(&clip)?
        .member(ENTRY_MEMBER)
        .filter(GfxValue::is_object)
        .ok_or(RenderError::Missing(
            "the VR entry kept on the main-button container",
        ))?;
    if !entry.set(c"m_Type", Param::Int(PauseUI::OPTION_GRAPHICS)) {
        return Err(RenderError::Refused("the entry's m_Type"));
    }
    let opened = clip.invoke(c"ShowPauseFromHideCredits", &[]).is_some()
        && clip.invoke(c"OnSelect", &[]).is_some();
    let restored = entry.set(c"m_Type", Param::Int(PauseUI::OPTION_EVENT));
    if !opened {
        return Err(RenderError::Refused("ShowPauseFromHideCredits or OnSelect"));
    }
    if !restored {
        return Err(RenderError::Refused("restoring the entry's m_Type"));
    }
    let using_gamepad = clip
        .invoke(c"IsUsingGamepad", &[])
        .and_then(|result| result.as_bool())
        .ok_or(RenderError::Refused("IsUsingGamepad"))?;
    if !using_gamepad {
        clip.path("MCI_back_letterbox")
            .ok_or(RenderError::Missing("MCI_back_letterbox"))?
            .invoke(c"gotoAndPlay", &[Param::Str(c"show")])
            .ok_or(RenderError::Refused("showing the back button"))?;
    }
    Ok(())
}

/// Lay out `menu`, the root menu the VR entry is in, and put the document back in its state, with
/// the VR entry still highlighted (the document remembers each list's selection). The back button
/// shown for the settings screen is hidden by the same call.
pub(crate) fn return_to_root(
    thread: &CaptureThread,
    screen: &Screen,
    menu: PauseMenuType,
) -> Result<(), RenderError> {
    clip(thread, screen)?
        .invoke(c"SwitchToMenu", &[Param::Int(menu as i32)])
        .map(|_| ())
        .ok_or(RenderError::Refused("SwitchToMenu"))
}

/// Set element `menu` of the valid-menus array `valid` that the engine passes to `InitMenu`.
pub(crate) fn set_menu_valid(valid: &GfxValue<'_>, menu: PauseMenuType, value: bool) -> bool {
    valid.set_element_param(menu as u32, Param::Bool(value))
}

/// The dynamic member of the main-button container that keeps the VR entry.
const ENTRY_MEMBER: &CStr = c"jc3vr_vr_entry";

/// A new reference to the pause screen's clip.
fn clip<'t>(thread: &'t CaptureThread, screen: &Screen) -> Result<GfxValue<'t>, RenderError> {
    screen.clip(thread).ok_or(RenderError::NoClip)
}

/// The main-button container, `MCI_button_container.MCI_root`.
fn container<'t>(clip: &GfxValue<'t>) -> Result<GfxValue<'t>, RenderError> {
    clip.path("MCI_button_container.MCI_root")
        .ok_or(RenderError::Missing("the main-button container"))
}

/// Append an entry labelled `label` to `menu`'s list with the movie's `AddOption`, returning it.
fn add_option<'t>(
    clip: &GfxValue<'t>,
    menu: PauseMenuType,
    label: &CStr,
) -> Result<GfxValue<'t>, RenderError> {
    clip.invoke(
        c"AddOption",
        &[
            Param::Int(menu as i32),
            Param::Str(c""),
            Param::Str(label),
            Param::Bool(false),
        ],
    )
    .filter(GfxValue::is_object)
    .ok_or(RenderError::Refused("AddOption"))
}
