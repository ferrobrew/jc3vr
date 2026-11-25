//! Stale native success is repaired from the current session, not the previous page.

use std::cell::RefCell;

use super::*;
use crate::pause_menu::model::state::MenuState;

fn session() -> Mutex<Session> {
    let session = Mutex::new(Session {
        state: MenuState::Shown { pages: 3, page: 0 },
        ..Session::default()
    });
    session.lock().set_page(Some(Page::new("OLD")));
    session
}

fn stale_install(
    session: &Mutex<Session>,
    native: &RefCell<Option<String>>,
    rebuilding: bool,
    close: bool,
) {
    let install = |page: &Page| {
        assert!(session.try_lock().is_some());
        *native.borrow_mut() = Some(page.title.to_string());
        let mut state = session.lock();
        if close {
            state.transition(MenuState::Closed);
        } else {
            state.state = MenuState::Shown { pages: 3, page: 2 };
            state.set_page(Some(Page::new("CURRENT")));
        }
        Ok::<_, &str>(())
    };
    let committed = if rebuilding {
        rebuild(session, 0, || Page::new("STALE"), |page, _| install(page)).unwrap()
    } else {
        switch(
            session,
            1,
            || Page::new("STALE"),
            install,
            |_| panic!("stale switch must not close another stay"),
        )
        .unwrap()
    };
    assert!(!committed);
    assert!(session.lock().reconcile);
    assert_eq!(native.borrow().as_deref(), Some("STALE"));
}

#[test]
fn stale_success_for_both_replacements_reinstates_current_page_not_old_page() {
    for rebuilding in [false, true] {
        let session = session();
        let native = RefCell::new(Some("OLD".to_owned()));
        stale_install(&session, &native, rebuilding, false);
        assert!(
            reconcile(
                &session,
                Instant::now(),
                |index, page| {
                    assert!(session.try_lock().is_some());
                    assert_eq!(index, 2);
                    assert_eq!(page.title, "CURRENT");
                    *native.borrow_mut() = Some(page.title.to_string());
                    Ok::<_, &str>(())
                },
                || panic!("shown current page must not be cleared")
            )
            .unwrap()
        );
        assert_eq!(native.borrow().as_deref(), Some("CURRENT"));
        assert!(!session.lock().reconcile);
    }
}

#[test]
fn stale_success_after_close_retains_failed_cleanup_then_removes_native_rows() {
    for rebuilding in [false, true] {
        let session = session();
        let native = RefCell::new(Some("OLD".to_owned()));
        stale_install(&session, &native, rebuilding, true);
        let now = Instant::now();
        assert_eq!(
            reconcile(
                &session,
                now,
                |_, _| panic!("closed session has no page"),
                || Err("movie unavailable")
            ),
            Err("movie unavailable")
        );
        assert!(session.lock().reconcile);
        assert!(
            !reconcile(
                &session,
                now,
                |_, _| -> Result<(), &str> { panic!("retry is bounded") },
                || panic!("retry is bounded")
            )
            .unwrap_or(false)
        );
        assert!(
            reconcile(
                &session,
                now + Duration::from_secs(1),
                |_, _| panic!("closed session has no page"),
                || {
                    assert!(session.try_lock().is_some());
                    *native.borrow_mut() = None;
                    Ok::<_, &str>(())
                }
            )
            .unwrap()
        );
        assert!(native.borrow().is_none());
        assert!(!session.lock().reconcile);
    }
}

#[test]
fn reentry_during_reconciliation_keeps_request_for_the_new_generation() {
    let session = session();
    session.lock().reconcile = true;
    let now = Instant::now();
    assert!(
        !reconcile(
            &session,
            now,
            |_, _| {
                session.lock().set_page(Some(Page::new("LATEST")));
                Ok::<_, &str>(())
            },
            || Ok(())
        )
        .unwrap()
    );
    assert!(session.lock().reconcile);
    assert!(
        reconcile(
            &session,
            now + Duration::from_secs(1),
            |_, page| {
                assert_eq!(page.title, "LATEST");
                Ok::<_, &str>(())
            },
            || Ok(())
        )
        .unwrap()
    );
}
