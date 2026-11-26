//! The grab hint's publish gate
//! ([`jc3vr_weapons::controllers::weapons::hud_feed::grab_hint::set_publish_gate`]): a hint is published
//! only while the game shows its gameplay HUD ([`crate::hud::gameplay_hints_active`]).

pub fn register() {
    jc3vr_weapons::controllers::weapons::hud_feed::grab_hint::set_publish_gate(
        crate::hud::gameplay_hints_active,
    );
}
