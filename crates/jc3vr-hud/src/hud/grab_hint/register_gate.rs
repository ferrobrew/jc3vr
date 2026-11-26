//! The grab hint's publish gate
//! ([`crate::controllers::weapons::hud_feed::grab_hint::set_publish_gate`]): a hint is published
//! only while the game shows its gameplay HUD ([`crate::hud::gameplay_hints_active`]).

pub(crate) fn register() {
    crate::controllers::weapons::hud_feed::grab_hint::set_publish_gate(
        crate::hud::gameplay_hints_active,
    );
}
