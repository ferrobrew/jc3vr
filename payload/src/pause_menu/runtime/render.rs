//! The movie side: adding the VR entries to the options menu, finding the one the player chose,
//! installing a page's rows into the settings screen, and writing live text into the visible rows.
//! Everything here goes through [`GfxValue`] and starts from the pause screen's clip.

use std::ffi::{CStr, CString};

use jc3gi::ui::pause_ui::{PauseMenuType, PauseUI, SettingsUI};

use crate::pause_menu::{
    gfx::{CaptureThread, GfxValue, Param, Screen},
    model::{
        entries::{MainButton, selected_page},
        ids::RowId,
        live::LiveSlots,
        page::{LiveField, Page, PageDef},
        rows::{EncodeError, RowSpec, RowWidget, encode_page},
    },
};

/// A failure to read or write the movie. The movie is not ours, so every path and member is
/// checked and a failure names what was missing.
#[derive(Debug)]
pub(crate) enum RenderError {
    /// The pause screen has no clip (it is not loaded).
    NoClip,
    /// A member or member path did not resolve to an object.
    Missing(&'static str),
    /// Creating an AS3 object or array failed.
    CreateFailed(&'static str),
    /// A call or write the movie refused.
    Refused(&'static str),
    /// The page could not be encoded.
    Encode(EncodeError),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoClip => write!(f, "pause movie: the pause screen has no clip"),
            RenderError::Missing(what) => write!(f, "pause movie: {what} did not resolve"),
            RenderError::CreateFailed(what) => {
                write!(f, "pause movie: could not create the {what}")
            }
            RenderError::Refused(what) => write!(f, "pause movie: the movie refused {what}"),
            RenderError::Encode(error) => write!(f, "pause movie: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Add one options-menu entry per page, in `pages` order after the menu's own entries, each opening
/// the settings screen like the graphics entry. Appending leaves the native entries where they
/// were, so the menu still opens with its first native entry highlighted. Returns how many were
/// added.
pub(crate) fn add_entries(
    thread: &CaptureThread,
    screen: &Screen,
    pages: &[PageDef],
) -> Result<usize, RenderError> {
    let clip = clip(thread, screen)?;
    for page in pages {
        let label = cstring(page.entry);
        let entry = clip
            .invoke(
                c"AddOption",
                &[
                    Param::Int(PauseMenuType::PAUSE_OPTIONS as i32),
                    Param::Str(c""),
                    Param::Str(&label),
                    Param::Bool(false),
                ],
            )
            .filter(GfxValue::is_object)
            .ok_or(RenderError::Refused("AddOption"))?;
        if !entry.set(c"m_Type", Param::Int(PauseUI::OPTION_GRAPHICS)) {
            return Err(RenderError::Refused("the entry's m_Type"));
        }
    }
    Ok(pages.len())
}

/// The page, among the first `count` of `pages`, whose VR entry is selected in the options menu,
/// or `None` when a native entry is. Reads every main button and leaves the decision to
/// [`selected_page`].
pub(crate) fn selected_entry(
    thread: &CaptureThread,
    screen: &Screen,
    pages: &[PageDef],
    count: usize,
) -> Result<Option<usize>, RenderError> {
    let clip = clip(thread, screen)?;
    let container = clip
        .path("MCI_button_container.MCI_root")
        .ok_or(RenderError::Missing("the main-button container"))?;
    let buttons: Vec<MainButton> = (0..PauseUI::MAIN_BUTTON_COUNT as usize)
        .map(|index| container.path(&format!("MCI_button_{index:02}.MCI_button")))
        .map(|button| {
            // A button that does not resolve keeps its place, so the positions stay the movie's.
            let Some(button) = button else {
                return MainButton {
                    visible: false,
                    selected: false,
                    label: None,
                };
            };
            MainButton {
                visible: button
                    .member(c"visible")
                    .and_then(|visible| visible.as_bool())
                    .unwrap_or(true),
                selected: button
                    .invoke(c"IsSelected", &[])
                    .and_then(|result| result.as_bool())
                    .unwrap_or(false),
                label: button
                    .path("MCI_button.MCI_name_text.TXT_name")
                    .and_then(|field| field.member(c"text"))
                    .and_then(|text| text.as_str().map(|s| s.to_string_lossy().into_owned())),
            }
        })
        .collect();
    let entries: Vec<&str> = pages.iter().take(count).map(|page| page.entry).collect();
    Ok(selected_page(&buttons, &entries))
}

/// Install `page` (registered at `page_index`) as the page the settings screen is about to show,
/// in place of the graphics page.
pub(crate) fn install_page(
    thread: &CaptureThread,
    screen: &Screen,
    page_index: usize,
    page: &Page,
) -> Result<(), RenderError> {
    let specs = encode_page(page_index, page).map_err(RenderError::Encode)?;
    let clip = clip(thread, screen)?;
    let settings = clip
        .member(c"m_SettingsArray")
        .filter(GfxValue::is_object)
        .ok_or(RenderError::Missing("m_SettingsArray"))?;
    let page_object =
        GfxValue::create_object(thread, screen).ok_or(RenderError::CreateFailed("page object"))?;
    let title = cstring(&page.title);
    let rows = build_rows(thread, screen, &specs)?;
    if !page_object.set(c"m_Name", Param::Str(&title))
        || !page_object.set_value(c"m_Settings", &rows)
    {
        return Err(RenderError::Refused("the page object's members"));
    }
    if !settings.set_element(SettingsUI::PAGE_GRAPHICS as u32, &page_object) {
        return Err(RenderError::Refused("the page's slot in m_SettingsArray"));
    }
    Ok(())
}

/// Whether the settings screen's list is in its root state: no option picker or message box of its
/// own is open. The list keeps the row a picker or box was opened on only as its highlighted row,
/// so replacing the rows under an open one would send its answer to whichever row is then
/// highlighted.
pub(crate) fn settings_in_root_state(
    thread: &CaptureThread,
    screen: &Screen,
) -> Result<bool, RenderError> {
    clip(thread, screen)?
        .path("MCI_settings_screen")
        .ok_or(RenderError::Missing("MCI_settings_screen"))?
        .invoke(c"IsInRootState", &[])
        .and_then(|result| result.as_bool())
        .ok_or(RenderError::Refused("IsInRootState"))
}

/// Replace the rows of the shown page with `page`'s and have the screen redraw them, then focus
/// `focus` when it is on the first screenful.
///
/// Returns `Ok` once the screen has redrawn the new rows, so the caller's record of the shown page
/// must follow. Focusing is best effort: a refused focus call is logged and leaves the first row
/// highlighted. An `Err` means the screen still draws the old rows.
pub(crate) fn replace_rows(
    thread: &CaptureThread,
    screen: &Screen,
    page_index: usize,
    page: &Page,
    focus: Option<usize>,
) -> Result<(), RenderError> {
    let specs = encode_page(page_index, page).map_err(RenderError::Encode)?;
    let clip = clip(thread, screen)?;
    let page_object = clip
        .member(c"m_SettingsArray")
        .filter(GfxValue::is_object)
        .and_then(|settings| settings.element(SettingsUI::PAGE_GRAPHICS as u32))
        .filter(GfxValue::is_object)
        .ok_or(RenderError::Missing("the shown page object"))?;
    let settings_screen = clip
        .path("MCI_settings_screen")
        .ok_or(RenderError::Missing("MCI_settings_screen"))?;
    let title = cstring(&page.title);
    let rows = build_rows(thread, screen, &specs)?;
    if !page_object.set(c"m_Name", Param::Str(&title))
        || !page_object.set_value(c"m_Settings", &rows)
    {
        return Err(RenderError::Refused("the page object's members"));
    }
    settings_screen
        .invoke(c"RefreshCurrentScreen", &[])
        .ok_or(RenderError::Refused("RefreshCurrentScreen"))?;

    if let Err(error) = focus_row(&clip, &title, focus) {
        tracing::warn!("pause menu: focusing the rebuilt page: {error}");
    }
    Ok(())
}

/// Write the live text of the visible rows of `page` (registered at `page_index`) where it
/// changed: into the row object, so the list's own redraws show it, and into the row clip's text
/// field, upper-cased as the list draws it.
pub(crate) fn push_live(
    thread: &CaptureThread,
    screen: &Screen,
    page_index: usize,
    page: &Page,
    live: &mut LiveSlots,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    for slot in 0..SettingsUI::ROW_CLIP_COUNT as usize {
        let written = manager
            .path(&format!("MCI_controls_row_{slot:02}"))
            .map(|row_clip| push_live_row(&row_clip, slot, page_index, page, live));
        if written != Some(true) {
            live.clear_slot(slot);
        }
    }
    Ok(())
}

/// The live pass for one row clip. Returns whether the clip shows a live item of the page.
fn push_live_row(
    row_clip: &GfxValue<'_>,
    slot: usize,
    page_index: usize,
    page: &Page,
    live: &mut LiveSlots,
) -> bool {
    let Some(data) = row_clip.invoke(c"GetData", &[]).filter(GfxValue::is_object) else {
        return false;
    };
    let Some(row) = data
        .member(c"m_Type")
        .and_then(|id| id.as_int())
        .and_then(RowId::decode)
        .filter(|row| row.page == page_index)
    else {
        return false;
    };
    let Some(live_text) = page.items.get(row.item).and_then(|item| item.live_text()) else {
        return false;
    };
    if !live.needs_write(slot, row.item, &live_text.text) {
        return true;
    }
    let (member, field_path) = match live_text.field {
        LiveField::Name => (c"m_Name", "MCI_row.MCI_name_text.TXT_name"),
        LiveField::KeyText => (c"m_KeyText", "MCI_row.MCI_key_text.TXT_name"),
    };
    let text = cstring(&live_text.text);
    let shown = cstring(&live_text.text.to_uppercase());
    let wrote_object = data.set(member, Param::Str(&text));
    let wrote_field = row_clip
        .path(field_path)
        .is_some_and(|field| field.set(c"text", Param::Str(&shown)));
    if !(wrote_object && wrote_field) {
        // Retry next frame.
        live.clear_slot(slot);
    }
    true
}

/// A new reference to the pause screen's clip.
fn clip<'t>(thread: &'t CaptureThread, screen: &Screen) -> Result<GfxValue<'t>, RenderError> {
    screen.clip(thread).ok_or(RenderError::NoClip)
}

/// After a refresh, rewrite the list's title and move the highlight and the selection to `focus`
/// when it is on the first screenful (the refresh leaves the first row clip highlighted and
/// selected).
fn focus_row(clip: &GfxValue<'_>, title: &CStr, focus: Option<usize>) -> Result<(), RenderError> {
    let manager = list_manager(clip)?;
    if let Some(title_field) = manager.member(c"MCI_title").filter(GfxValue::is_object) {
        title_field.set(c"text", Param::Str(title));
    }
    if let Some(focus) = focus.filter(|&f| f > 0 && f < SettingsUI::ROW_CLIP_COUNT as usize) {
        let focus = focus as i32;
        manager
            .invoke(
                c"MoveSelectionHighlight",
                &[Param::Int(0), Param::Int(focus)],
            )
            .ok_or(RenderError::Refused("MoveSelectionHighlight"))?;
        manager
            .invoke(c"SetHighlightedRow", &[Param::Int(focus)])
            .ok_or(RenderError::Refused("SetHighlightedRow"))?;
    }
    Ok(())
}

/// The settings screen's list manager.
fn list_manager<'t>(clip: &GfxValue<'t>) -> Result<GfxValue<'t>, RenderError> {
    clip.path("MCI_settings_screen.MCI_panel.MCI_container.MCI_root.MCI_manager")
        .ok_or(RenderError::Missing("the settings list manager"))
}

/// Build the AS3 row array for `specs`.
fn build_rows<'t>(
    thread: &'t CaptureThread,
    screen: &Screen,
    specs: &[RowSpec],
) -> Result<GfxValue<'t>, RenderError> {
    let rows =
        GfxValue::create_array(thread, screen).ok_or(RenderError::CreateFailed("row array"))?;
    for spec in specs {
        let row = build_row(thread, screen, spec)?;
        if !rows.push_value(&row) {
            return Err(RenderError::Refused("a row in the row array"));
        }
    }
    Ok(rows)
}

/// Build one row object.
fn build_row<'t>(
    thread: &'t CaptureThread,
    screen: &Screen,
    spec: &RowSpec,
) -> Result<GfxValue<'t>, RenderError> {
    let row =
        GfxValue::create_object(thread, screen).ok_or(RenderError::CreateFailed("row object"))?;
    let name = cstring(&spec.name);
    let mut ok = row.set(c"m_Type", Param::Int(spec.id))
        && row.set(c"m_Format", Param::Int(spec.widget.format()))
        && row.set(c"m_Name", Param::Str(&name))
        && row.set(c"m_Value", Param::Int(spec.value))
        && row.set(c"m_DefaultValue", Param::Int(spec.default))
        && row.set(c"m_NeedConfirm", Param::Bool(false));
    match &spec.widget {
        RowWidget::Toggle { labels, values } => {
            let label_array = GfxValue::create_array(thread, screen)
                .ok_or(RenderError::CreateFailed("label array"))?;
            let value_array = GfxValue::create_array(thread, screen)
                .ok_or(RenderError::CreateFailed("value array"))?;
            for label in labels {
                let label = cstring(label);
                ok &= label_array.push(Param::Str(&label));
            }
            for &value in values {
                ok &= value_array.push(Param::Int(value));
            }
            ok = ok
                && row.set_value(c"m_Labels", &label_array)
                && row.set_value(c"m_Values", &value_array);
        }
        RowWidget::Slider { min, max } => {
            ok = ok
                && row.set(c"m_MinValue", Param::Int(*min))
                && row.set(c"m_MaxValue", Param::Int(*max));
        }
        RowWidget::Key {
            key_text,
            customizable,
        } => {
            let key_text = cstring(key_text);
            ok = ok
                && row.set(c"m_KeyText", Param::Str(&key_text))
                && row.set(c"m_Image", Param::Str(c""))
                && row.set(c"m_Interaction", Param::Int(0))
                && row.set(c"m_IsCustomizable", Param::Bool(*customizable))
                && row.set(c"m_Action", Param::Int(spec.id));
        }
    }
    ok.then_some(row)
        .ok_or(RenderError::Refused("a row object's members"))
}

/// `text` as a C string, with any interior NUL dropped.
fn cstring(text: &str) -> CString {
    CString::new(text.replace('\0', "")).unwrap_or_default()
}
