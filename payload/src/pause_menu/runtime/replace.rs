//! Install movie rows without holding the session lock, then commit the matching session.

use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::pause_menu::{model::page::Page, runtime::session::Session};

pub(super) fn switch<E>(
    session: &Mutex<Session>,
    tab: usize,
    build: impl FnOnce() -> Page,
    install: impl FnOnce(&Page) -> Result<(), E>,
    close: impl FnOnce(usize),
) -> Result<bool, E> {
    let (generation, from) = {
        let session = session.lock();
        let Some(from) = session.state.shown_page() else {
            return Ok(false);
        };
        (session.generation, from)
    };
    let page = build();
    install(&page)?;
    let committed = {
        let mut session = session.lock();
        let committed = session.switched(generation, from, tab, page);
        if !committed {
            session.reconcile = true;
        }
        committed
    };
    if committed {
        close(from);
    }
    Ok(committed)
}

pub(super) fn rebuild<E>(
    session: &Mutex<Session>,
    page_index: usize,
    build: impl FnOnce() -> Page,
    install: impl FnOnce(&Page, Option<usize>) -> Result<(), E>,
) -> Result<bool, E> {
    let (generation, last) = {
        let session = session.lock();
        if session.state.shown_page() != Some(page_index) {
            return Ok(false);
        }
        (session.generation, session.last_selected)
    };
    let page = build();
    install(&page, page.rebuild_focus(last))?;
    let mut session = session.lock();
    let committed = session.rebuilt(generation, page_index, page);
    if !committed {
        session.reconcile = true;
    }
    Ok(committed)
}

/// Repair only from the current session; never replay a page captured before a native callback.
pub(super) fn reconcile<E>(
    session: &Mutex<Session>,
    now: Instant,
    mut install: impl FnMut(usize, &Page) -> Result<(), E>,
    cleanup: impl FnOnce() -> Result<(), E>,
) -> Result<bool, E> {
    let (generation, page) = {
        let mut session = session.lock();
        if !session.reconcile {
            return Ok(true);
        }
        if session.reconcile_retry.is_some_and(|at| now < at) {
            return Ok(false);
        }
        session.reconcile_retry = Some(now + Duration::from_secs(1));
        let page = session.state.shown_page().zip(session.page.clone());
        if session.state.shown_page().is_some() && page.is_none() {
            return Ok(false);
        }
        (session.generation, page)
    };
    if let Some((index, page)) = page {
        install(index, &page)?;
    } else {
        cleanup()?;
    }
    let mut session = session.lock();
    if session.generation != generation {
        return Ok(false);
    }
    session.reconcile = false;
    session.reconcile_retry = None;
    session.live.invalidate();
    Ok(true)
}

#[cfg(test)]
mod stale_tests;

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::pause_menu::model::state::MenuState;

    #[test]
    fn failed_install_preserves_page_and_same_destination_can_retry_without_duplicate_close() {
        let session = Mutex::new(Session {
            state: MenuState::Shown { pages: 3, page: 0 },
            ..Session::default()
        });
        session.lock().set_page(Some(Page::new("OLD")));
        let generation = session.lock().generation;
        let closed = Cell::new(0);
        assert_eq!(
            switch(
                &session,
                1,
                || Page::new("NEW"),
                |_| {
                    assert!(session.try_lock().is_some());
                    Err("movie refused")
                },
                |_| closed.set(closed.get() + 1)
            ),
            Err("movie refused")
        );
        assert_eq!(session.lock().state.shown_page(), Some(0));
        assert_eq!(session.lock().generation, generation);
        assert_eq!(session.lock().page.as_ref().unwrap().title, "OLD");
        assert_eq!(closed.get(), 0);
        assert_eq!(
            switch(
                &session,
                1,
                || Page::new("NEW"),
                |_| Ok::<_, &str>(()),
                |from| {
                    assert!(session.try_lock().is_some());
                    assert_eq!(from, 0);
                    closed.set(closed.get() + 1);
                }
            ),
            Ok(true)
        );
        assert_eq!(session.lock().state.shown_page(), Some(1));
        assert_eq!(session.lock().page.as_ref().unwrap().title, "NEW");
        assert_eq!(closed.get(), 1);
        assert_eq!(session.lock().transition(MenuState::Closed), Some(1));
        assert_eq!(session.lock().transition(MenuState::Closed), None);
    }
}
