//! The pause-menu detours: H1 adds the VR entries when the movie loads, H2 installs a VR page when
//! the settings screen reads its pages, H3 takes the VR rows' commands, H4 applies deferred
//! rebuilds and writes live values each frame, and H5 ends the session when the menu deactivates.
//! See `docs/mod/pause-menu.md`.
//!
//! Every path that ends a VR page's stay (the player backing out, the menu deactivating by any
//! route, the movie unloading, a failed or off-thread reinstall) goes through
//! [`transition`], which runs the page's `on_close` once, after the session lock is released.
//!
//! H1 is the only hook gated on [`PauseMenuConfig::enabled`](crate::pause_menu::PauseMenuConfig):
//! without the entries the session stays closed, and H2 and H4 do nothing in a closed session, so
//! the toggle takes effect at the next activation of the menu and never pulls a page out from under
//! the player. H3 routes every command regardless, which for a closed session passes everything
//! through except a command carrying a VR row id (only possible if a page was installed).

use std::{
    mem::take,
    ptr::{addr_of, addr_of_mut},
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::ui::{
    pause_ui::{PauseMenuType, PauseUI},
    scaleform::{MovieImpl, Value},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    pause_menu::{
        gfx::{CaptureThread, Screen, decode_arg},
        model::{
            command::{Action, Arg, Route, parse, route},
            page::{Applied, ButtonOutcome, Item},
            state::{MenuState, PopulateSource},
        },
        pages::{MAX_PAGES, PAGES},
        runtime::{render, session::SESSION},
    },
};

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&ON_ACTIVE_BINDER)
        .with_static_binder(&POPULATE_SETTINGS_BINDER)
        .with_static_binder(&HANDLE_COMMAND_BINDER)
        .with_static_binder(&UPDATE_BINDER)
        .with_static_binder(&ON_DEACTIVATE_BINDER)
}

/// The most command arguments decoded; the commands the menu handles take at most two.
const MAX_ARGS: usize = 4;

/// H1: after the game has filled the freshly loaded movie, add the VR entries to the options menu.
#[detour(address = PauseUI::OnActiveImpl_ADDRESS)]
fn on_active(this: *mut PauseUI) {
    ON_ACTIVE.get().unwrap().call(this);

    // A fresh movie: whatever the last session held is gone with the old one. H5 has normally
    // closed it already; this catches an unload that bypassed the deactivation.
    transition(MenuState::closed);

    if !Config::lock_query(|c| c.pause_menu.enabled) || this.is_null() {
        return;
    }
    // SAFETY: the detour's own non-null `this`, the live pause screen; the field is read through
    // its place, without a reference to the screen.
    let options_valid =
        unsafe { addr_of!((*this).m_IsValidMenu).read() }[PauseMenuType::PAUSE_OPTIONS as usize];
    if !options_valid {
        return;
    }
    // SAFETY: as above.
    let Some(screen) = (unsafe { pause_screen(this) }) else {
        return;
    };
    let Some(thread) = CaptureThread::current() else {
        tracing::warn!("pause menu: the menu activated off the capture thread; no VR entries");
        return;
    };
    if PAGES.len() > MAX_PAGES {
        tracing::warn!(
            "pause menu: {} pages registered, but only the first {MAX_PAGES} fit the options menu",
            PAGES.len()
        );
    }
    match render::add_entries(&thread, &screen, &PAGES[..PAGES.len().min(MAX_PAGES)]) {
        Ok(count) => {
            SESSION.lock().state = MenuState::entries_added(count);
            tracing::debug!("pause menu: added {count} VR entries to the options menu");
        }
        Err(error) => tracing::error!("pause menu: adding the VR entries: {error}"),
    }
}

/// H2: after the game has filled the settings pages, put the VR page in place of the page the
/// screen shows, when a VR entry opened the screen or a VR page is being refreshed.
#[detour(address = PauseUI::PopulateSettings_ADDRESS)]
fn populate_settings(this: *mut PauseUI) {
    POPULATE_SETTINGS.get().unwrap().call(this);

    let (state, opening) = {
        let session = SESSION.lock();
        (session.state, session.opening)
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
        let selected = render::selected_entry(&thread, &screen, PAGES, state.entry_count())
            .unwrap_or_else(|error| {
                tracing::warn!("pause menu: finding the selected entry: {error}");
                None
            });
        PopulateSource::Opened { selected }
    } else {
        PopulateSource::Refresh
    };
    let (next, install) = state.populate(source);

    let Some(page_index) = install else {
        transition(|_| next);
        return;
    };
    let Some(def) = PAGES.get(page_index) else {
        transition(|_| next.install_failed());
        return;
    };
    let page = (def.build)();
    match render::install_page(&thread, &screen, page_index, &page) {
        Ok(()) => {
            let left = {
                let mut session = SESSION.lock();
                let left = session.transition(next);
                session.set_page(Some(page));
                left
            };
            run_on_close(left);
        }
        Err(error) => {
            tracing::error!("pause menu: installing the page {:?}: {error}", def.entry);
            transition(|_| next.install_failed());
        }
    }
}

/// H3: take the commands of the VR rows; pass the rest to the game.
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
    let state = SESSION.lock().state;
    match route(parse(command, &decoded), state) {
        Route::Pass => original(),
        Route::Populate => {
            SESSION.lock().opening = true;
            original();
            SESSION.lock().opening = false;
        }
        Route::Swallow(action) => act(action),
        Route::PassThen(action) => {
            original();
            act(action);
        }
    }
}

/// H4: after the game's update, run the shown page's per-frame work, rebuild the page if asked, and
/// write its live values.
#[detour(address = PauseUI::UpdateImpl_ADDRESS)]
fn update(this: *mut PauseUI, dt: f32, dtf: f32) {
    UPDATE.get().unwrap().call(this, dt, dtf);

    let Some(page_index) = SESSION.lock().state.shown_page() else {
        return;
    };
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

    if let Some(on_frame) = PAGES.get(page_index).and_then(|def| def.on_frame)
        && on_frame() == ButtonOutcome::Rebuild
    {
        SESSION.lock().rebuild = true;
    }
    if SESSION.lock().rebuild {
        let in_root_state =
            render::settings_in_root_state(&thread, &screen).unwrap_or_else(|error| {
                // The rebuild itself reports what is missing.
                tracing::debug!("pause menu: reading the list's state: {error}");
                true
            });
        if SESSION.lock().take_rebuild(in_root_state) {
            rebuild(&thread, &screen, page_index);
        }
    }

    let (page, mut live, generation) = {
        let mut session = SESSION.lock();
        let Some(page) = session.page.clone() else {
            return;
        };
        (page, take(&mut session.live), session.generation)
    };
    if let Err(error) = render::push_live(&thread, &screen, page_index, &page, &mut live) {
        tracing::debug!("pause menu: live values: {error}");
    }
    let mut session = SESSION.lock();
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

/// Carry out a command's action.
fn act(action: Action) {
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
        Action::LeavePage => transition(MenuState::left_settings),
        Action::Close => transition(MenuState::closed),
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
        Err(error) => tracing::error!("pause menu: rebuilding the page {:?}: {error}", def.entry),
    }
}

/// Warn once that the update ran off the capture thread (an open question the live values depend
/// on), rather than every frame.
fn warn_off_thread_once() {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "pause menu: the pause screen's update runs off the capture thread; live values and \
             page rebuilds are skipped"
        );
    }
}
