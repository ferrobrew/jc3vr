//! The movie side: adding the VR entry to the options menu and telling whether the player chose it,
//! putting the VR tabs on the settings screen's tab bar, installing a page's rows, keeping each
//! tab's scroll position, and, each frame, moving the highlight off section headers, styling the
//! header rows, and writing live text into the visible rows. Everything here goes through
//! [`GfxValue`] and starts from the pause screen's clip.

use std::ffi::{CStr, CString};

use jc3gi::ui::pause_ui::{PauseMenuType, PauseUI, SettingsUI};

use crate::pause_menu::{
    gfx::{CaptureThread, GfxValue, Param, Screen},
    model::{
        entries::{MainButton, vr_entry_selected},
        headers::{HeaderStyles, Step},
        ids::RowId,
        live::LiveSlots,
        page::{LiveField, Page},
        rows::{EncodeError, RowSpec, RowWidget, encode_page},
        tabs::{ScrollState, TAB_LIMIT},
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

/// Add the VR entry, labelled `label`, after the options menu's own entries, opening the settings
/// screen like the graphics entry. Appending leaves the native entries where they were, so the
/// menu still opens with its first native entry highlighted.
pub(crate) fn add_entry(
    thread: &CaptureThread,
    screen: &Screen,
    label: &str,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let label = cstring(label);
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
    Ok(())
}

/// Whether the VR entry, labelled `label`, is the one selected in the options menu. Reads every
/// main button and leaves the decision to [`vr_entry_selected`].
pub(crate) fn entry_selected(
    thread: &CaptureThread,
    screen: &Screen,
    label: &str,
) -> Result<bool, RenderError> {
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
    Ok(vr_entry_selected(&buttons, label))
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

/// The per-frame pass over the visible rows of `page` (registered at `page_index`): give each row
/// clip the label style of the row it shows (the title style for a header, the row style
/// otherwise), and write live text where it changed, into the row object, so the list's own
/// redraws show it, and into the row clip's text field, upper-cased as the list draws it.
pub(crate) fn refresh_rows(
    thread: &CaptureThread,
    screen: &Screen,
    page_index: usize,
    page: &Page,
    live: &mut LiveSlots,
    styles: &mut HeaderStyles,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    for slot in 0..SettingsUI::ROW_CLIP_COUNT as usize {
        let Some(row_clip) = manager.path(&format!("MCI_controls_row_{slot:02}")) else {
            live.clear_slot(slot);
            continue;
        };
        let item = shown_item(&row_clip, page_index);
        let header = item.as_ref().is_some_and(|(_, item)| page.is_header(*item));
        if styles.needs_change(slot, header) {
            match style_row(&manager, &row_clip, header) {
                Ok(()) => styles.changed(slot, header),
                Err(error) => tracing::debug!("pause menu: styling row clip {slot}: {error}"),
            }
        }
        let written = item
            .is_some_and(|(data, item)| push_live_row(&row_clip, &data, slot, item, page, live));
        if !written {
            live.clear_slot(slot);
        }
    }
    Ok(())
}

/// Give every row clip styled as a header its row style back, so a native page drawn on the clips
/// shows its rows as the game draws them.
pub(crate) fn restore_row_styles(
    thread: &CaptureThread,
    screen: &Screen,
    styles: &mut HeaderStyles,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    let styled: Vec<usize> = styles.styled().collect();
    for slot in styled {
        let row_clip = manager
            .path(&format!("MCI_controls_row_{slot:02}"))
            .ok_or(RenderError::Missing("a row clip"))?;
        style_row(&manager, &row_clip, false)?;
        styles.changed(slot, false);
    }
    Ok(())
}

/// The list's scroll position and selected row.
pub(crate) fn list_scroll(
    thread: &CaptureThread,
    screen: &Screen,
) -> Result<ScrollState, RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    let read = |method: &'static CStr, what: &'static str| {
        manager
            .invoke(method, &[])
            .and_then(|result| result.as_int())
            .ok_or(RenderError::Refused(what))
    };
    let top = read(c"GetTopIndex", "GetTopIndex")?;
    let selected = read(c"GetSelectedRow", "GetSelectedRow")?;
    ScrollState::from_list(top, selected).ok_or(RenderError::Refused("a negative list index"))
}

/// Scroll the list to `state` and highlight its selected row, as the screen's own tab switch
/// restores a tab: the current highlight is cleared first, since the list's restore only adds one.
pub(crate) fn restore_scroll(
    thread: &CaptureThread,
    screen: &Screen,
    state: ScrollState,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    let to_int = |index: usize| i32::try_from(index).unwrap_or(i32::MAX);
    manager
        .invoke(c"DeHighlightCurrent", &[])
        .ok_or(RenderError::Refused("DeHighlightCurrent"))?;
    manager
        .invoke(
            c"RestoreSavedState",
            &[
                Param::Int(to_int(state.top)),
                Param::Int(to_int(state.selected)),
            ],
        )
        .ok_or(RenderError::Refused("RestoreSavedState"))?;
    Ok(())
}

/// Move the list's highlight one row in `step`'s direction, as the list's own up and down do.
pub(crate) fn step_highlight(
    thread: &CaptureThread,
    screen: &Screen,
    step: Step,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let manager = list_manager(&clip)?;
    let method = match step {
        Step::Next => c"HighlightNext",
        Step::Prev => c"HighlightPrev",
    };
    manager
        .invoke(method, &[])
        .map(drop)
        .ok_or(RenderError::Refused("the highlight step"))
}

/// Put the VR tabs on the settings screen's tab bar: one unlocked selector per label, the rest
/// locked, `current` selected, and the bar's select callback set to the document's `Invoke`, so a
/// chosen tab arrives as a tab command rather than through the screen's own tab switch. Then show
/// the bar. Called after the screen has opened, since opening it sets the bar up for the native
/// pages and hides it.
pub(crate) fn setup_tabs(
    thread: &CaptureThread,
    screen: &Screen,
    labels: &[&str],
    current: usize,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let navbar = tab_bar(&clip)?;
    let selectors = navbar
        .member(c"m_Selectors")
        .filter(GfxValue::is_object)
        .ok_or(RenderError::Missing("the tab bar's selectors"))?;
    for index in 0..TAB_LIMIT {
        let selector = selectors
            .element(index as u32)
            .filter(GfxValue::is_object)
            .ok_or(RenderError::Missing("a tab selector"))?;
        selector
            .invoke(c"SetDeselected", &[])
            .ok_or(RenderError::Refused("SetDeselected"))?;
        let label = labels.get(index);
        if let Some(label) = label {
            let label = cstring(label);
            selector
                .invoke(c"SetNew", &[Param::Bool(false)])
                .ok_or(RenderError::Refused("SetNew"))?;
            selector
                .invoke(
                    c"SetButtonVariables",
                    &[Param::Str(&label), Param::Str(c"")],
                )
                .ok_or(RenderError::Refused("SetButtonVariables"))?;
        }
        navbar
            .invoke(
                c"SetButtonLocked",
                &[Param::Int(index as i32), Param::Bool(label.is_none())],
            )
            .ok_or(RenderError::Refused("SetButtonLocked"))?;
    }
    let callback = clip
        .member(c"Invoke")
        .filter(GfxValue::is_closure)
        .ok_or(RenderError::Missing("the document's Invoke method"))?;
    navbar
        .invoke(c"SetSelectFunctionCB", &[Param::Value(&callback)])
        .ok_or(RenderError::Refused("SetSelectFunctionCB"))?;
    select_on_bar(&navbar, &selectors, None, current)?;
    if !navbar.set(c"visible", Param::Bool(true)) {
        return Err(RenderError::Refused("the tab bar's visibility"));
    }
    Ok(())
}

/// Move the tab bar's selection from tab `from` to tab `to`.
pub(crate) fn select_tab(
    thread: &CaptureThread,
    screen: &Screen,
    from: usize,
    to: usize,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let navbar = tab_bar(&clip)?;
    let selectors = navbar
        .member(c"m_Selectors")
        .filter(GfxValue::is_object)
        .ok_or(RenderError::Missing("the tab bar's selectors"))?;
    select_on_bar(&navbar, &selectors, Some(from), to)
}

/// Step the tab bar to its next tab when `forward` is set, its previous otherwise, as its own
/// shoulder-button hints do; the bar then calls its select callback with the tab.
pub(crate) fn step_tab(
    thread: &CaptureThread,
    screen: &Screen,
    forward: bool,
) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    let method = if forward {
        c"OnNextTabButton"
    } else {
        c"OnPrevTabButton"
    };
    tab_bar(&clip)?
        .invoke(method, &[])
        .map(drop)
        .ok_or(RenderError::Refused("the tab step"))
}

/// Hide the tab bar, as the native pages and the controls screen have it.
pub(crate) fn hide_tabs(thread: &CaptureThread, screen: &Screen) -> Result<(), RenderError> {
    let clip = clip(thread, screen)?;
    if !tab_bar(&clip)?.set(c"visible", Param::Bool(false)) {
        return Err(RenderError::Refused("the tab bar's visibility"));
    }
    Ok(())
}

/// The row object a row clip shows and its item index, when it is a row of page `page_index`.
fn shown_item<'t>(row_clip: &GfxValue<'t>, page_index: usize) -> Option<(GfxValue<'t>, usize)> {
    let data = row_clip
        .invoke(c"GetData", &[])
        .filter(GfxValue::is_object)?;
    let row = data
        .member(c"m_Type")
        .and_then(|id| id.as_int())
        .and_then(RowId::decode)
        .filter(|row| row.page == page_index)?;
    Some((data, row.item))
}

/// Give a row clip's label the list title's text format (`header`), or the row's own format back.
/// The row's format is kept on the clip's row container (a dynamic class) the first time the clip
/// is styled, so it can be restored however the clip was drawn since. Both the field's current
/// text and its default format change, since the list sets the label's text on every redraw.
fn style_row(
    manager: &GfxValue<'_>,
    row_clip: &GfxValue<'_>,
    header: bool,
) -> Result<(), RenderError> {
    let container = row_clip
        .path("MCI_row")
        .ok_or(RenderError::Missing("a row container"))?;
    let label = row_clip
        .path("MCI_row.MCI_name_text.TXT_name")
        .ok_or(RenderError::Missing("a row label"))?;
    let saved = container
        .member(ROW_FORMAT_MEMBER)
        .filter(GfxValue::is_object);
    let format = if header {
        if saved.is_none() {
            let own = label
                .invoke(c"getTextFormat", &[])
                .filter(GfxValue::is_object)
                .ok_or(RenderError::Refused("the row label's getTextFormat"))?;
            if !container.set_value(ROW_FORMAT_MEMBER, &own) {
                return Err(RenderError::Refused("keeping the row label's format"));
            }
        }
        manager
            .member(c"MCI_title")
            .filter(GfxValue::is_object)
            .ok_or(RenderError::Missing("the list title"))?
            .invoke(c"getTextFormat", &[])
            .filter(GfxValue::is_object)
            .ok_or(RenderError::Refused("the list title's getTextFormat"))?
    } else {
        // A clip never styled has its own format still.
        let Some(saved) = saved else {
            return Ok(());
        };
        saved
    };
    if !label.set_value(c"defaultTextFormat", &format) {
        return Err(RenderError::Refused("the row label's defaultTextFormat"));
    }
    label
        .invoke(c"setTextFormat", &[Param::Value(&format)])
        .map(drop)
        .ok_or(RenderError::Refused("the row label's setTextFormat"))
}

/// The member of a row container that keeps the row label's own text format.
const ROW_FORMAT_MEMBER: &CStr = c"m_VrRowFormat";

/// The tab bar.
fn tab_bar<'t>(clip: &GfxValue<'t>) -> Result<GfxValue<'t>, RenderError> {
    clip.path("MCI_tabs.MCI_navbar")
        .ok_or(RenderError::Missing("the tab bar"))
}

/// Deselect `from` (when given), select `to`, and point the bar's shoulder-button cycling at `to`.
fn select_on_bar(
    navbar: &GfxValue<'_>,
    selectors: &GfxValue<'_>,
    from: Option<usize>,
    to: usize,
) -> Result<(), RenderError> {
    let selector = |index: usize| {
        selectors
            .element(index as u32)
            .filter(GfxValue::is_object)
            .ok_or(RenderError::Missing("a tab selector"))
    };
    if let Some(from) = from {
        selector(from)?
            .invoke(c"SetDeselected", &[])
            .ok_or(RenderError::Refused("SetDeselected"))?;
    }
    selector(to)?
        .invoke(c"SetSelected", &[])
        .ok_or(RenderError::Refused("SetSelected"))?;
    navbar
        .invoke(c"FindUnlockedIndex", &[Param::Int(to as i32)])
        .map(drop)
        .ok_or(RenderError::Refused("FindUnlockedIndex"))
}

/// The live pass for one row clip showing `item` of the page, whose row object is `data`. Returns
/// whether the item is live.
fn push_live_row(
    row_clip: &GfxValue<'_>,
    data: &GfxValue<'_>,
    slot: usize,
    item: usize,
    page: &Page,
    live: &mut LiveSlots,
) -> bool {
    let Some(live_text) = page.items.get(item).and_then(|item| item.live_text()) else {
        return false;
    };
    if !live.needs_write(slot, item, &live_text.text) {
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
