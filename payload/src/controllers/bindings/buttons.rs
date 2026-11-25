//! Semantic button reads shared by input-tick consumers outside the binding tables.

use crate::{
    controllers::{
        bindings::{controls::ButtonControl, intents::SemanticInput, tables},
        mode::Mode,
    },
    vr::input::semantic::vocabulary::OnFootAction,
};

/// The reload tap and held holster share the same accepted semantic press.
pub(super) fn reload_button(semantic: &SemanticInput) -> ButtonControl {
    semantic
        .get(OnFootAction::Reload.id())
        .map_or_else(ButtonControl::default, |input| {
            input
                .hands
                .iter()
                .fold(ButtonControl::default(), |acc, intent| ButtonControl {
                    down: acc.down || intent.button.down,
                    pressed: acc.pressed || intent.button.pressed,
                    released: acc.released || intent.button.released,
                })
        })
}

pub(super) fn grapple_button_held(semantic: &SemanticInput, mode: Mode) -> bool {
    grapple_button(semantic, mode).held()
}

pub(super) fn grapple_button(semantic: &SemanticInput, mode: Mode) -> ButtonControl {
    tables::grapple_press(mode).map_or_else(ButtonControl::default, |(actions, _)| {
        actions
            .iter()
            .filter_map(|&action| semantic.get(action))
            .flat_map(|input| input.hands.iter().map(|hand| hand.button))
            .fold(ButtonControl::default(), tables::merge_buttons)
    })
}
