//! The debug tools' step at hook removal ([`crate::hooks::register_pre_unpatch`]).

use crate::{debug::texture_census, hooks};

pub(crate) fn register() {
    hooks::register_pre_unpatch(texture_census_unarmed);
}

/// The texture census's detours, once enabled, pin the payload in the process.
fn texture_census_unarmed() -> bool {
    if texture_census::armed() {
        tracing::error!(
            restart_required = true,
            "hook removal: the texture census's detours were enabled; the payload must remain mapped and the game must be restarted"
        );
        return false;
    }
    true
}
