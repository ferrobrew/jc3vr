//! The pause-menu detours: H1 and H6 add the VR entry to the root menu when the movie loads, H2
//! installs a VR page when the settings screen reads its pages, H3 takes the VR entry's, rows', and
//! tabs' commands, opens the VR screen from the root menu, and returns there when the player backs
//! out, H4 puts the VR tabs on the tab bar, switches tabs, applies deferred rebuilds, keeps the
//! highlight off section headers, and writes live values each frame, H5 ends the session when the
//! menu deactivates, and H6 turns the shoulder buttons into tab steps. See
//! `docs/mod/pause-menu.md`.
//!
//! Every path that ends a VR page's stay (a tab switch, the player backing out, the menu
//! deactivating by any route, the movie unloading, a failed or off-thread reinstall) goes through
//! [`transition`], which runs the page's `on_close` once, after the session lock is released.
//!
//! Adding the entry (H1, and H6 for `InitMenu`) is the only work gated on
//! [`PauseMenuConfig::enabled`](crate::pause_menu::PauseMenuConfig): without the entry the session
//! stays closed, and H2, H4, and H6 do nothing in a closed session,
//! so the toggle takes effect at the next activation of the menu and never pulls a page out from
//! under the player. H3 routes every command regardless, which for a closed session passes
//! everything through except a command carrying a VR row id, a tab command, or the VR entry's
//! event (only possible if an entry was added or a page installed).

use std::{
    ffi::CStr,
    mem::take,
    ptr::{addr_of, addr_of_mut},
    sync::{
        LazyLock,
        atomic::{AtomicBool, AtomicPtr, Ordering},
    },
};

use detours_macro::detour;
use jc3gi::{
    hash::hashlittle,
    ui::{
        pause_ui::{PauseMenuType, PauseUI, SettingsUI},
        scaleform::{MovieImpl, Value},
        ui_base::UIBase,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    pause_menu::{
        gfx::{CaptureThread, GfxValue, Screen, decode_arg},
        model::{
            command::{Action, Arg, Command, Route, parse, route},
            headers::skip_header,
            page::{Applied, ButtonOutcome, Item, PageDef},
            root::RootPlan,
            state::{MenuState, PopulateSource},
            tabs::TabCommands,
        },
        pages::{ENTRY_LABEL, MAX_PAGES, PAGES},
        runtime::{render, root, session::SESSION},
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&ON_ACTIVE_BINDER)
        .with_static_binder(&POPULATE_SETTINGS_BINDER)
        .with_static_binder(&HANDLE_COMMAND_BINDER)
        .with_static_binder(&UPDATE_BINDER)
        .with_static_binder(&ON_DEACTIVATE_BINDER)
        .with_static_binder(&INVOKE_BINDER)
}

/// The most command arguments decoded; the commands the menu handles take at most two.
const MAX_ARGS: usize = 4;

/// The tab commands, hashed with the engine's command hash ([`hashlittle`]) on first use.
static TAB_COMMANDS: LazyLock<TabCommands> =
    LazyLock::new(|| TabCommands::new(|name| hashlittle(name.as_bytes()) as u32));

/// The pause screen whose [`PauseUI::OnActiveImpl`] is running, so H6 recognises that
/// activation's `InitMenu` call. Null outside an activation.
static ACTIVATING: AtomicPtr<PauseUI> = AtomicPtr::new(std::ptr::null_mut());

/// The registered pages that have a tab.
fn tabbed_pages() -> &'static [PageDef] {
    &PAGES[..PAGES.len().min(MAX_PAGES)]
}

/// H1: around the game filling the freshly loaded movie. The VR entry is added from inside the
/// fill, when the movie's `InitMenu` runs (H6); when that did not happen (the call was queued, or
/// the plan's first steps failed), the entry is appended to the end of the root menu here.
#[detour(address = PauseUI::OnActiveImpl_ADDRESS)]
fn on_active(this: *mut PauseUI) {
    // A fresh movie: whatever the last session held is gone with the old one. H5 has normally
    // closed it already; this catches an unload that bypassed the deactivation.
    transition(MenuState::closed);

    ACTIVATING.store(this, Ordering::Relaxed);
    ON_ACTIVE.get().unwrap().call(this);
    ACTIVATING.store(std::ptr::null_mut(), Ordering::Relaxed);

    if SESSION.lock().state != MenuState::Closed || !Config::lock_query(|c| c.pause_menu.enabled) {
        return;
    }
    // SAFETY: the detour's own `this`, the live pause screen, or null.
    let Some(plan) = (unsafe { root_plan(this) }) else {
        return;
    };
    // SAFETY: as above.
    let Some(screen) = (unsafe { pause_screen(this) }) else {
        return;
    };
    let Some(thread) = CaptureThread::current() else {
        tracing::warn!("pause menu: the menu activated off the capture thread; no VR entry");
        return;
    };
    tracing::warn!("pause menu: the VR entry was not added with the menu's entries; appending it");
    match root::add_entry(&thread, &screen, plan.menu, ENTRY_LABEL) {
        Ok(()) => record_entry(this, plan.menu),
        Err(error) => tracing::error!("pause menu: adding the VR entry: {error}"),
    }
}

/// H2: after the game has filled the settings pages, put the VR page in place of the page the
/// screen shows, when the VR entry opened the screen (the opening raised the session's flag) or a
/// VR page is being refreshed.
#[detour(address = PauseUI::PopulateSettings_ADDRESS)]
fn populate_settings(this: *mut PauseUI) {
    POPULATE_SETTINGS.get().unwrap().call(this);

    let (state, opening, vr, tab) = {
        let mut session = SESSION.lock();
        let opening = session.opening;
        // Lowered here, so the VR entry's opening can tell that the screen populated.
        let vr = opening && take(&mut session.opening_vr);
        (session.state, opening, vr, session.tabs.last())
    };
    if state == MenuState::Closed {
        return;
    }
    // SAFETY: the detour's own `this`, the live pause screen.
    let Some(screen) = (unsafe { pause_screen(this) }) else {
        return;
    };
    let Some(thread) = CaptureThread::current() else {
        tracing::warn!("pause menu: the settings populated off the capture thread; left native");
        // The game has just refilled the page slot with its own page, which the screen shows next.
        transition(MenuState::install_failed);
        return;
    };

    let source = if opening {
        PopulateSource::Opened { vr }
    } else {
        PopulateSource::Refresh
    };
    let (next, install) = state.populate(source, tab);

    let Some(page_index) = install else {
        transition(|_| next);
        leave_screen(&thread, &screen);
        return;
    };
    let Some(def) = PAGES.get(page_index) else {
        transition(|_| next.install_failed());
        leave_screen(&thread, &screen);
        return;
    };
    let page = (def.build)();
    match render::install_page(&thread, &screen, page_index, &page) {
        Ok(()) => {
            let left = {
                let mut session = SESSION.lock();
                let left = session.transition(next);
                session.set_page(Some(page));
                session.tabs.shown(page_index);
                if opening {
                    session.setup_tabs = true;
                }
                left
            };
            run_on_close(left);
        }
        Err(error) => {
            tracing::error!("pause menu: installing the page {:?}: {error}", def.tab);
            transition(|_| next.install_failed());
            leave_screen(&thread, &screen);
        }
    }
}

/// H3: take the commands of the VR entry, rows, and tabs; pass the rest to the game.
#[detour(address = PauseUI::HandleCommandImpl_ADDRESS)]
fn handle_command(
    this: *mut PauseUI,
    command: u32,
    args: *const Value,
    num_args: u32,
    movie: *mut MovieImpl,
) {
    let original = || {
        HANDLE_COMMAND
            .get()
            .unwrap()
            .call(this, command, args, num_args, movie)
    };

    let decoded: Vec<Arg> = if args.is_null() {
        Vec::new()
    } else {
        let count = (num_args as usize).min(MAX_ARGS);
        // SAFETY: the movie passes `num_args` values at `args` for the duration of the call.
        unsafe { std::slice::from_raw_parts(args, count) }
            .iter()
            .map(decode_arg)
            .collect()
    };
    let command = parse(command, &decoded, &TAB_COMMANDS);
    let state = {
        let mut session = SESSION.lock();
        if command == Command::SelectSound && take(&mut session.mute_select_sound) {
            return;
        }
        session.state
    };
    match route(command, state) {
        Route::Pass => original(),
        Route::Populate => {
            SESSION.lock().opening = true;
            original();
            SESSION.lock().opening = false;
        }
        Route::Swallow(action) => act(action, this),
        Route::PassThen(action) => {
            original();
            act(action, this);
        }
    }
}

/// H4: after the game's update, finish opening the VR tabs, switch to a chosen tab, run the shown
/// page's per-frame work, rebuild the page if asked, move the highlight off a header, and write
/// the rows' styles and live values.
#[detour(address = PauseUI::UpdateImpl_ADDRESS)]
fn update(this: *mut PauseUI, dt: f32, dtf: f32) {
    UPDATE.get().unwrap().call(this, dt, dtf);

    if SESSION.lock().state.shown_page().is_none() {
        return;
    }
    // SAFETY: the detour's own `this`, the live pause screen.
    let Some(screen) = (unsafe { pause_screen(this) }) else {
        return;
    };
    if !screen.is_active() {
        return;
    }
    let Some(thread) = CaptureThread::current() else {
        warn_off_thread_once();
        return;
    };

    if take(&mut SESSION.lock().setup_tabs) {
        open_tabs(&thread, &screen);
    }
    let in_root_state = || {
        render::settings_in_root_state(&thread, &screen).unwrap_or_else(|error| {
            // The work that follows reports what is missing.
            tracing::debug!("pause menu: reading the list's state: {error}");
            true
        })
    };
    if SESSION.lock().pending_tab.is_some() && in_root_state() {
        let tab = SESSION.lock().pending_tab.take();
        if let Some(tab) = tab {
            switch_tab(&thread, &screen, tab);
        }
    }

    let Some(page_index) = SESSION.lock().state.shown_page() else {
        return;
    };
    if let Some(on_frame) = PAGES.get(page_index).and_then(|def| def.on_frame)
        && on_frame() == ButtonOutcome::Rebuild
    {
        SESSION.lock().rebuild = true;
    }
    let root = in_root_state();
    if SESSION.lock().take_rebuild(root) {
        rebuild(&thread, &screen, page_index);
    }
    if root {
        skip_headers(&thread, &screen);
    }

    let (page, mut live, mut styles, generation) = {
        let mut session = SESSION.lock();
        let Some(page) = session.page.clone() else {
            return;
        };
        (
            page,
            take(&mut session.live),
            take(&mut session.styles),
            session.generation,
        )
    };
    if let Err(error) =
        render::refresh_rows(&thread, &screen, page_index, &page, &mut live, &mut styles)
    {
        tracing::debug!("pause menu: row pass: {error}");
    }
    let mut session = SESSION.lock();
    // The clips' styles belong to the movie, not the page, so they are kept whatever happened
    // meanwhile; a close in between forgot them, and the movie with them.
    if session.state != MenuState::Closed {
        session.styles = styles;
    }
    if session.generation == generation {
        session.live = live;
    }
}

/// H5: after the game's own deactivation work, end the session. Every deactivation of the loaded
/// menu runs this, including those that close it from the settings screen without the movie
/// sending `persist_changes` (the pause key, the UI manager forcing the menu closed, and game-state
/// changes), so the shown page's `on_close` runs here for them. It may run off the capture thread;
/// it touches only the session and the page's close work, never the movie.
#[detour(address = PauseUI::OnDeactivateImpl_ADDRESS)]
fn on_deactivate(this: *mut PauseUI) {
    ON_DEACTIVATE.get().unwrap().call(this);
    transition(MenuState::closed);
}

/// H6: around a screen's AS3 call. The pause screen's `InitMenu` during an activation (H1) adds
/// the VR entry below the options entry ([`init_menu`]). After the call, when it was the pause
/// screen's shoulder-button handler and the VR tabs are shown, step the tab bar: the pause document
/// ignores those buttons on the settings screen, and the tab bar's own step reports the tab through
/// its callback like a click.
#[detour(address = UIBase::InvokeImpl_ADDRESS)]
fn invoke(
    this: *mut UIBase,
    method: *const u8,
    args: *const Value,
    num_args: u32,
    result: *mut Value,
    delay: f32,
) -> bool {
    let original = || {
        INVOKE
            .get()
            .unwrap()
            .call(this, method, args, num_args, result, delay)
    };
    if method.is_null() {
        return original();
    }
    // SAFETY: the engine passes the method name as a NUL-terminated string for the call.
    let name = unsafe { CStr::from_ptr(method.cast()) }.to_bytes();
    let activating = ACTIVATING.load(Ordering::Relaxed);
    if name == b"InitMenu"
        && !activating.is_null()
        // SAFETY: a non-null `ACTIVATING` is the pause screen whose activation is running; its
        // base is reached through its place.
        && unsafe { addr_of_mut!((*activating).base) } == this
    {
        return init_menu(activating, this, args, num_args, original);
    }

    let called = original();
    let forward = match name {
        b"OnNextTabButton" => true,
        b"OnPrevTabButton" => false,
        _ => return called,
    };
    let ours = {
        let session = SESSION.lock();
        session.state.shown_page().is_some() && session.screen == this as usize
    };
    if !ours {
        return called;
    }
    // The handler queued rather than ran (off the capture thread): the step waits for the call
    // that runs it.
    let Some(thread) = CaptureThread::current() else {
        return called;
    };
    // SAFETY: the detour's own `this`, the live pause screen's base.
    if let Some(screen) = unsafe { Screen::new(this) }
        && let Err(error) = render::step_tab(&thread, &screen, forward)
    {
        tracing::debug!("pause menu: stepping the tab bar: {error}");
    }
    called
}

/// Run the activation's `InitMenu` (`original`) on the pause screen `pause` (whose base is `this`)
/// so that the VR entry lands directly below the root menu's options entry (see
/// [`root`](crate::pause_menu::model::root)): with the main-menu list marked invalid in the
/// valid-menus array `args[0]` when the in-game menu would add its main-menu entry, then the VR
/// entry, then `InitMenu` again for the main-menu list and the main-menu entry. Anything that
/// cannot be done that way calls through and leaves the entry to H1.
fn init_menu(
    pause: *mut PauseUI,
    this: *mut UIBase,
    args: *const Value,
    num_args: u32,
    original: impl Fn() -> bool,
) -> bool {
    if !Config::lock_query(|c| c.pause_menu.enabled) {
        return original();
    }
    // SAFETY: `pause` is the live pause screen whose activation is running.
    let Some(plan) = (unsafe { root_plan(pause) }) else {
        return original();
    };
    // The engine runs the call at once only on the capture thread and with the screen active;
    // otherwise it queues it, and the entry would precede the menu's own.
    let Some(thread) = CaptureThread::current() else {
        return original();
    };
    // SAFETY: the detour's own `this`, the live pause screen's base.
    let Some(screen) = (unsafe { Screen::new(this) }) else {
        return original();
    };
    if !screen.is_active() {
        return original();
    }

    let valid = if plan.split_main_menu && !args.is_null() && num_args >= 1 {
        // SAFETY: the engine passes `num_args` live values at `args` for the call, and the movie
        // outlives it.
        let valid = unsafe { GfxValue::copy_of(&thread, &*args) };
        (valid.is_object() && root::set_menu_valid(&valid, PauseMenuType::PAUSE_MAIN_MENU, false))
            .then_some(valid)
    } else {
        None
    };
    if plan.split_main_menu && valid.is_none() {
        tracing::warn!(
            "pause menu: could not hold the main-menu list back; the VR entry follows the \
             main-menu entry"
        );
    }

    let called = original();
    if let Some(valid) = &valid
        && !root::set_menu_valid(valid, PauseMenuType::PAUSE_MAIN_MENU, true)
    {
        tracing::warn!("pause menu: could not restore the main-menu list's valid flag");
    }
    if !called {
        tracing::warn!("pause menu: the movie refused InitMenu; no VR entry");
        return called;
    }
    match root::add_entry(&thread, &screen, plan.menu, ENTRY_LABEL) {
        Ok(()) => {
            record_entry(pause, plan.menu);
            tracing::debug!("pause menu: added the VR entry below the options entry");
        }
        Err(error) => tracing::error!("pause menu: adding the VR entry: {error}"),
    }
    if valid.is_some() {
        // Every other valid list is initialised by now, so the second call builds the main-menu
        // list alone.
        if !original() {
            tracing::warn!("pause menu: the movie refused InitMenu for the main-menu list");
        }
        if let Err(error) = root::add_main_menu_entry(&thread, &screen) {
            tracing::error!("pause menu: adding the main-menu entry back: {error}");
        }
    }
    called
}

/// Record that the VR entry is in `menu` of the pause screen `this`.
fn record_entry(this: *mut PauseUI, menu: PauseMenuType) {
    if PAGES.len() > MAX_PAGES {
        tracing::warn!(
            "pause menu: {} pages registered, but only the first {MAX_PAGES} fit the tab bar",
            PAGES.len()
        );
    }
    let mut session = SESSION.lock();
    session.state = MenuState::entry_added(tabbed_pages().len());
    session.screen = this as usize;
    session.root = Some(menu);
}

/// Where the VR entry goes in the pause screen `this`'s menus, from its valid menus.
///
/// # Safety
/// `this` must be null or the live pause screen.
unsafe fn root_plan(this: *mut PauseUI) -> Option<RootPlan> {
    if this.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; the field is read through its place, without a reference to
    // the screen.
    RootPlan::new(unsafe { addr_of!((*this).m_IsValidMenu).read() })
}

/// Carry out a command's action. `this` is the pause screen the command came from.
fn act(action: Action, this: *mut PauseUI) {
    match action {
        Action::SetValue { row, value } => {
            let Some(item) = shown_item(row.item) else {
                return;
            };
            match item.apply(value) {
                Applied::Set | Applied::NotAValue => {}
                Applied::OutOfRange => {
                    tracing::debug!("pause menu: row {} ignored the value {value}", row.item)
                }
            }
            // The list redrew the row from its object; rewrite its live text.
            SESSION.lock().live.invalidate();
        }
        Action::Press { row } => {
            let Some(Item::Button(button)) = shown_item(row.item) else {
                return;
            };
            let outcome = (button.action)();
            let mut session = SESSION.lock();
            // The list replaced the button's caption with its key-capture prompt.
            session.live.invalidate();
            if outcome == ButtonOutcome::Rebuild {
                session.rebuild = true;
            }
        }
        Action::Drop => tracing::debug!("pause menu: dropped a command for a page not shown"),
        Action::ResetPage => {
            let Some(page) = SESSION.lock().page.clone() else {
                return;
            };
            page.reset_to_defaults();
            SESSION.lock().rebuild = true;
        }
        // Deferred to the update: the tab bar is still inside its own handler.
        Action::SelectTab { tab } => SESSION.lock().pending_tab = Some(tab),
        Action::OpenEntry => open_from_root(this),
        Action::LeavePage => {
            // The screen is closing; keep the tab's position for the next visit and give the
            // native pages their tab bar and row styles back.
            // SAFETY: the detour's own `this`, the live pause screen.
            let screen = unsafe { pause_screen(this) };
            let thread = CaptureThread::current();
            if let (Some(screen), Some(thread)) = (&screen, &thread) {
                save_scroll(thread, screen);
            }
            transition(MenuState::left_settings);
            if let (Some(screen), Some(thread)) = (&screen, &thread) {
                leave_screen(thread, screen);
                // The document has just put itself in the options menu's state over the root
                // menu's list; the VR screen was opened from the root menu, so it returns there.
                let root = SESSION.lock().root;
                if let Some(menu) = root
                    && let Err(error) = root::return_to_root(thread, screen, menu)
                {
                    tracing::warn!("pause menu: returning to the root menu: {error}");
                }
            }
        }
        Action::Close => transition(MenuState::closed),
    }
}

/// Open the settings screen on the VR tabs from the VR entry, which the player has just selected
/// in the root menu of the pause screen `this`. The movie is inside its own selection handler; the
/// opening runs the document's settings selection synchronously, which reaches H3 and H2 again.
fn open_from_root(this: *mut PauseUI) {
    // SAFETY: the detour's own `this`, the live pause screen.
    let Some(screen) = (unsafe { pause_screen(this) }) else {
        return;
    };
    let Some(thread) = CaptureThread::current() else {
        tracing::warn!("pause menu: the VR entry was selected off the capture thread");
        return;
    };
    let root = {
        let mut session = SESSION.lock();
        session.opening_vr = true;
        session.mute_select_sound = true;
        session.root
    };
    let opened = root::open_settings(&thread, &screen);
    let populated = {
        let mut session = SESSION.lock();
        session.mute_select_sound = false;
        !take(&mut session.opening_vr)
    };
    if let Err(error) = opened {
        tracing::error!("pause menu: opening the VR screen: {error}");
    }
    // Without a populate the screen never opened, and the document may be left in the options
    // menu's state over the root menu's list; put it back.
    if !populated
        && let Some(menu) = root
        && let Err(error) = root::return_to_root(&thread, &screen, menu)
    {
        tracing::warn!("pause menu: returning to the root menu: {error}");
    }
}

/// A copy of item `index` of the shown page, so it can run without the session lock.
fn shown_item(index: usize) -> Option<Item> {
    SESSION
        .lock()
        .page
        .as_ref()
        .and_then(|page| page.items.get(index).cloned())
}

/// Move the session to the state `next` computes from the current one, then run the `on_close` of
/// the page the move left, if any, without the session lock.
fn transition(next: impl FnOnce(MenuState) -> MenuState) {
    let left = {
        let mut session = SESSION.lock();
        let state = next(session.state);
        session.transition(state)
    };
    run_on_close(left);
}

/// Run the `on_close` of registered page `page`, if any.
fn run_on_close(page: Option<usize>) {
    if let Some(on_close) = page
        .and_then(|index| PAGES.get(index))
        .and_then(|def| def.on_close)
    {
        on_close();
    }
}

/// The pause screen at `this`, held by pointer.
///
/// # Safety
/// `this` must be null or the live pause screen for as long as the value is used.
unsafe fn pause_screen(this: *mut PauseUI) -> Option<Screen> {
    if this.is_null() {
        return None;
    }
    // SAFETY: the caller's contract; the base is reached through its place, without a reference.
    unsafe { Screen::new(addr_of_mut!((*this).base)) }
}

/// Give the native pages the settings screen as the game draws it: the tab bar hidden and the row
/// clips' own label style. Each failure is only logged: the movie is closing the screen or showing
/// a native page either way.
fn leave_screen(thread: &CaptureThread, screen: &Screen) {
    if let Err(error) = render::hide_tabs(thread, screen) {
        tracing::debug!("pause menu: hiding the tab bar: {error}");
    }
    let mut styles = take(&mut SESSION.lock().styles);
    if let Err(error) = render::restore_row_styles(thread, screen, &mut styles) {
        tracing::warn!("pause menu: restoring the rows' label style: {error}");
    }
    SESSION.lock().styles = styles;
}

/// Record the shown tab's scroll position, for the next time it is shown this pause.
fn save_scroll(thread: &CaptureThread, screen: &Screen) {
    let Some(page) = SESSION.lock().state.shown_page() else {
        return;
    };
    match render::list_scroll(thread, screen) {
        Ok(state) => SESSION.lock().tabs.save(page, state),
        Err(error) => tracing::debug!("pause menu: reading the list's position: {error}"),
    }
}

/// Put the VR tabs on the tab bar after the screen opened, and restore the shown tab's position.
/// Without the tabs the shown page still works; the others are then out of reach until the screen
/// opens again.
fn open_tabs(thread: &CaptureThread, screen: &Screen) {
    let (page_index, rows) = {
        let session = SESSION.lock();
        let Some(page_index) = session.state.shown_page() else {
            return;
        };
        let rows = session.page.as_ref().map_or(0, |page| page.items.len());
        (page_index, rows)
    };
    let labels: Vec<&str> = tabbed_pages().iter().map(|def| def.tab).collect();
    if let Err(error) = render::setup_tabs(thread, screen, &labels, page_index) {
        tracing::error!("pause menu: putting the VR tabs on the tab bar: {error}");
        if let Err(error) = render::hide_tabs(thread, screen) {
            tracing::debug!("pause menu: hiding the tab bar: {error}");
        }
    }
    restore_tab_scroll(thread, screen, page_index, rows);
}

/// Restore tab `page_index`'s saved position, fitted to its `rows` rows, if it has one.
fn restore_tab_scroll(thread: &CaptureThread, screen: &Screen, page_index: usize, rows: usize) {
    let saved = SESSION
        .lock()
        .tabs
        .restore(page_index, rows, SettingsUI::ROW_CLIP_COUNT as usize);
    if let Some(state) = saved {
        if let Err(error) = render::restore_scroll(thread, screen, state) {
            tracing::debug!("pause menu: restoring the tab's position: {error}");
        }
        SESSION.lock().last_selected = Some(state.selected);
    }
}

/// Switch the shown VR tab to `tab`: keep the old tab's position, leave its page (running its
/// `on_close`), install the new page's rows in the same page slot, restore the new tab's position,
/// and move the tab bar's selection. The screen's own tab switch is never used (see
/// [`tabs`](crate::pause_menu::model::tabs)).
fn switch_tab(thread: &CaptureThread, screen: &Screen, tab: usize) {
    let Some(from) = SESSION.lock().state.shown_page() else {
        return;
    };
    if tab == from || tab >= tabbed_pages().len() {
        return;
    }
    save_scroll(thread, screen);
    transition(|state| state.switch_tab(tab));
    let generation = {
        let mut session = SESSION.lock();
        if session.state.shown_page() != Some(tab) {
            return;
        }
        session.tabs.shown(tab);
        session.generation
    };
    let def = &PAGES[tab];
    let page = (def.build)();
    let rows = page.items.len();
    if let Err(error) = render::replace_rows(thread, screen, tab, &page, None) {
        // The screen still draws the old tab's rows, which the session no longer resolves.
        tracing::error!("pause menu: switching to the tab {:?}: {error}", def.tab);
        return;
    }
    if !SESSION.lock().rebuilt(generation, tab, page) {
        tracing::debug!("pause menu: the session moved on during a tab switch");
        return;
    }
    restore_tab_scroll(thread, screen, tab, rows);
    if let Err(error) = render::select_tab(thread, screen, from, tab) {
        tracing::debug!("pause menu: moving the tab bar's selection: {error}");
    }
}

/// When the highlight is on a section header, move it on in the direction it was going.
fn skip_headers(thread: &CaptureThread, screen: &Screen) {
    let scroll = match render::list_scroll(thread, screen) {
        Ok(scroll) => scroll,
        Err(error) => {
            tracing::debug!("pause menu: reading the list's position: {error}");
            return;
        }
    };
    let step = {
        let mut session = SESSION.lock();
        let Some(page) = session.page.as_ref() else {
            return;
        };
        let step = skip_header(
            scroll.selected,
            session.last_selected,
            |row| page.is_header(row),
            page.items.len(),
        );
        session.last_selected = Some(scroll.selected);
        step
    };
    if let Some(step) = step
        && let Err(error) = render::step_highlight(thread, screen, step)
    {
        tracing::debug!("pause menu: moving the highlight off a header: {error}");
    }
}

/// Rebuild the shown page and install its rows, focusing its first button. The session records the
/// new page once the screen shows it, unless the session moved on meanwhile.
fn rebuild(thread: &CaptureThread, screen: &Screen, page_index: usize) {
    let Some(def) = PAGES.get(page_index) else {
        return;
    };
    let generation = SESSION.lock().generation;
    let page = (def.build)();
    let focus = page.first_button();
    match render::replace_rows(thread, screen, page_index, &page, focus) {
        Ok(()) => {
            if !SESSION.lock().rebuilt(generation, page_index, page) {
                tracing::debug!("pause menu: the session moved on during a rebuild");
            }
        }
        Err(error) => tracing::error!("pause menu: rebuilding the page {:?}: {error}", def.tab),
    }
}

/// Warn once that the update ran off the capture thread (an open question the live values depend
/// on), rather than every frame.
fn warn_off_thread_once() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "pause menu: the pause screen's update runs off the capture thread; live values, tab \
             switches, and page rebuilds are skipped"
        );
    }
}
