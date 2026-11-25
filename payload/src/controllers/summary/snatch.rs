//! Current snatch instructions from the owner's phase and permitted semantic routes.

use crate::{
    controllers::{
        bindings::tables::describe::{Control, Effect, Gesture, ModAction, Row, press_control},
        summary::{current::CurrentContext, eligibility::SnatchHint},
    },
    vr::input::semantic::bound::SourceTable,
};

pub(crate) fn extend(current: &mut CurrentContext, table: &SourceTable, hint: Option<&SnatchHint>) {
    let Some(hint) = hint.filter(|hint| current.routing.ready[hint.hand.index()]) else {
        return;
    };
    let sources: Vec<_> = hint
        .routes
        .iter()
        .filter(|_| hint.modifier_enabled && current.routing.ready.into_iter().all(|ready| ready))
        .flat_map(|&(id, hand)| {
            table
                .sources(id.set, id.index())
                .unwrap_or_default()
                .iter()
                .filter(move |source| source.hand == Some(hand))
                .map(|source| match (source.hand, source.component) {
                    (Some(hand), Some(component)) => press_control(component, hand),
                    (hand, _) => Control::Named(hand, source.name),
                })
        })
        .collect();
    let mut add = |control, action| {
        let row = Row {
            control,
            gesture: Gesture::Press,
            effect: Effect::Mod(action),
        };
        if !current.usable.contains(&row) {
            current.usable.push(row);
        }
    };
    add(
        Control::Grip(hint.hand),
        if hint.swinging {
            ModAction::SnatchThrow
        } else {
            ModAction::SnatchGrip
        },
    );
    if !hint.swinging {
        add(Control::Motion(hint.hand), ModAction::SnatchYank);
    }
    if !sources.is_empty() {
        add(Control::Motion(hint.hand), ModAction::SnatchSwing);
        for source in sources {
            add(
                source,
                if hint.swinging {
                    ModAction::SnatchResume
                } else {
                    ModAction::SnatchModifier
                },
            );
        }
    }
}
