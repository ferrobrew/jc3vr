//! The pause-menu detours: H1 adds the VR entries when the movie loads, H2 installs a VR page when
//! the settings screen reads its pages, H3 takes the VR rows' commands, and H4 applies deferred
//! rebuilds and writes live values each frame. See `docs/mod/pause-menu.md`.
//!
//! H1 is the only hook gated on [`PauseMenuConfig::enabled`](crate::pause_menu::PauseMenuConfig):
//! without the entries the session stays closed, and H2 and H4 do nothing in a closed session, so
//! the toggle takes effect at the next activation of the menu and never pulls a page out from under
//! the player. H3 routes every command regardless, which for a closed session passes everything
//! through except a command carrying a VR row id (only possible if a page was installed).

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::ui::{
    overlay_ui::UIBaseState,
    pause_ui::{PauseMenuType, PauseUI},
    scaleform::{MovieImpl, Value},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    pause_menu::{
        gfx::{CaptureThread, decode_arg},
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
}

/// The most command arguments decoded; the commands the menu handles take at most two.
const MAX_ARGS: usize = 4;

/// H1: after the game has filled the freshly loaded movie, add the VR entries to the options menu.
#[detour(address = PauseUI::OnActiveImpl_ADDRESS)]
fn on_active(this: *mut PauseUI) {
    ON_ACTIVE.get().unwrap().call(this);

    // A fresh movie: whatever the last session held is gone with the old one.
    SESSION.lock().close();

    if !Config::lock_query(|c| c.pause_menu.enabled) {
        return;
    }
    // SAFETY: the detour's own `this`, the live pause screen.
    let Some(pause) = (unsafe { this.as_ref() }) else {
        return;
    };
    if !pause.m_IsValidMenu[PauseMenuType::PAUSE_OPTIONS as usize] {
        return;
    }
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
    match render::add_entries(&thread, pause, &PAGES[..PAGES.len().min(MAX_PAGES)]) {
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
    let Some(pause) = (unsafe { this.as_mut() }) else {
        return;
    };
    let Some(thread) = CaptureThread::current() else {
        tracing::warn!("pause menu: the settings populated off the capture thread; left native");
        return;
    };

    let source = if opening {
        let selected = render::selected_entry(&thread, pause, PAGES, state.entry_count())
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
        let mut session = SESSION.lock();
        session.state = next;
        if next.shown_page().is_none() {
            session.set_page(None);
        }
        return;
    };
    let Some(def) = PAGES.get(page_index) else {
        SESSION.lock().state = next.install_failed();
        return;
    };
    let page = (def.build)();
    match render::install_page(&thread, pause, page_index, &page) {
        Ok(()) => {
            let mut session = SESSION.lock();
            session.state = next;
            session.set_page(Some(page));
        }
        Err(error) => {
            tracing::error!("pause menu: installing the page {:?}: {error}", def.entry);
            let mut session = SESSION.lock();
            session.state = next.install_failed();
            session.set_page(None);
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

/// H4: after the game's update, rebuild the shown page if asked and write its live values.
#[detour(address = PauseUI::UpdateImpl_ADDRESS)]
fn update(this: *mut PauseUI, dt: f32, dtf: f32) {
    UPDATE.get().unwrap().call(this, dt, dtf);

    let Some(page_index) = SESSION.lock().state.shown_page() else {
        return;
    };
    // SAFETY: the detour's own `this`, the live pause screen.
    let Some(pause) = (unsafe { this.as_mut() }) else {
        return;
    };
    if pause.base.m_Current != UIBaseState::ACTIVE {
        return;
    }
    let Some(thread) = CaptureThread::current() else {
        warn_off_thread_once();
        return;
    };

    if std::mem::take(&mut SESSION.lock().rebuild) {
        rebuild(&thread, pause, page_index);
    }

    let (page, mut live, generation) = {
        let mut session = SESSION.lock();
        let Some(page) = session.page.clone() else {
            return;
        };
        (page, std::mem::take(&mut session.live), session.generation)
    };
    if let Err(error) = render::push_live(&thread, pause, page_index, &page, &mut live) {
        tracing::debug!("pause menu: live values: {error}");
    }
    let mut session = SESSION.lock();
    if session.generation == generation {
        session.live = live;
    }
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
        Action::LeavePage => {
            let page_index = {
                let mut session = SESSION.lock();
                let page_index = session.state.shown_page();
                session.state = session.state.left_settings();
                session.set_page(None);
                page_index
            };
            if let Some(on_close) = page_index
                .and_then(|index| PAGES.get(index))
                .and_then(|def| def.on_close)
            {
                on_close();
            }
        }
        Action::Close => SESSION.lock().close(),
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

/// Rebuild the shown page and install its rows, focusing its first button.
fn rebuild(thread: &CaptureThread, pause: &mut PauseUI, page_index: usize) {
    let Some(def) = PAGES.get(page_index) else {
        return;
    };
    let page = (def.build)();
    let focus = page.first_button();
    match render::replace_rows(thread, pause, page_index, &page, focus) {
        Ok(()) => SESSION.lock().set_page(Some(page)),
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
