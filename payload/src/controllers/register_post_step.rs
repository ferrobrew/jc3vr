//! The observers that run after each simulation step ([`crate::hooks::sim::register_post_step`]).

pub(super) fn register() {
    crate::hooks::sim::register_post_step(crate::controllers::diagnostics::after_step);
}
