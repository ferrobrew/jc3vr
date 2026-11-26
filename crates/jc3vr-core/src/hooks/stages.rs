//! The ordered steps other modules attach to hook installation and removal.
//!
//! [`crate::hooks::install`] and [`crate::hooks::uninstall`] own the shared `Patcher` and the
//! composed hook library, but the work that has to bracket them belongs to the modules whose
//! detours need it: opening admission once a native callback's detour is live, tearing down
//! detours managed outside the hook library, and proving that no native callback is still running
//! in payload code once everything is unpatched. Each module registers its steps here from its own
//! `install` before the hooks are installed, and the steps run in registration order.

use parking_lot::Mutex;

/// Run `activate` once the composed hook library is enabled and its state is recorded. A failed
/// installation runs none of them.
pub fn register_post_enable(activate: fn()) {
    POST_ENABLE.lock().push(activate);
}

/// Run `guard` at hook removal, after the shutdown commit is confirmed and before the hook library
/// is disabled. A guard returns false to refuse removal, which leaves the image mapped; the guards
/// after it do not run. A guard that refuses logs its own reason.
pub fn register_pre_unpatch(guard: fn() -> bool) {
    PRE_UNPATCH.lock().push(guard);
}

/// Run `check` at hook removal, after the hook library is disabled and every patch is reverted. A
/// check returns false when payload code may still be running, which leaves the image mapped; the
/// checks after it do not run. A check that refuses logs its own reason.
pub fn register_post_unpatch(check: fn() -> bool) {
    POST_UNPATCH.lock().push(check);
}

pub(super) fn run_post_enable() {
    for activate in snapshot(&POST_ENABLE) {
        activate();
    }
}

pub(super) fn run_pre_unpatch() -> bool {
    all_permit(&PRE_UNPATCH)
}

pub(super) fn run_post_unpatch() -> bool {
    all_permit(&POST_UNPATCH)
}

static POST_ENABLE: Mutex<Vec<fn()>> = Mutex::new(Vec::new());
static PRE_UNPATCH: Mutex<Vec<fn() -> bool>> = Mutex::new(Vec::new());
static POST_UNPATCH: Mutex<Vec<fn() -> bool>> = Mutex::new(Vec::new());

/// Run the steps in order, stopping at the first that refuses.
fn all_permit(registry: &Mutex<Vec<fn() -> bool>>) -> bool {
    snapshot(registry).into_iter().all(|step| step())
}

/// Copy the steps out so none runs under the registry lock: a step may take locks of its own or
/// suspend threads.
fn snapshot<T: Copy>(registry: &Mutex<Vec<T>>) -> Vec<T> {
    registry.lock().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_run_in_registration_order_and_stop_at_the_first_refusal() {
        static CALLS: Mutex<Vec<&str>> = Mutex::new(Vec::new());
        let registry: Mutex<Vec<fn() -> bool>> = Mutex::new(Vec::new());
        registry.lock().push(|| {
            CALLS.lock().push("first");
            true
        });
        registry.lock().push(|| {
            CALLS.lock().push("second");
            false
        });
        registry.lock().push(|| {
            CALLS.lock().push("third");
            true
        });
        assert!(!all_permit(&registry));
        assert_eq!(*CALLS.lock(), ["first", "second"]);
    }

    #[test]
    fn an_empty_registry_permits_removal() {
        assert!(all_permit(&Mutex::new(Vec::new())));
    }
}
