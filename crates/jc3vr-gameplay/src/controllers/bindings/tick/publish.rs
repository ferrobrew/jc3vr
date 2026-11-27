//! The last stages: the triggers' ownership after the acts, the controls summary, and the writes.

use jc3vr_input::frame::Hand;
use jc3vr_vehicles::vehicles;
use jc3vr_weapons::controllers::weapons;

use crate::controllers::bindings::{
    TickReport,
    stage::Segment,
    summary,
    summary::yank_armed_in,
    tick::{
        Hands, Live,
        acts::{Acted, Evaluated},
        input::Routed,
    },
    trigger_ownership,
    trigger_ownership::Owner as TriggerOwner,
};

/// The triggers' ownership after the acts.
pub(super) struct Settled {
    pub(super) owners: [TriggerOwner; 2],
    /// Each hand's trigger kept from its routes: blocked before the acts, or changed owner in or
    /// after them.
    pub(super) blocked: [bool; 2],
}

/// Settle the triggers' ownership after the acts: a trigger whose gun changed late keeps its fire,
/// cancels its gestures, and has its routes suppressed.
pub(super) fn settle(
    live: &mut Live<'_, '_>,
    hands: &Hands,
    acted: &Acted,
    evaluated: &mut Evaluated,
) -> Settled {
    let mode = live.detection.mode;
    let in_menu = live.in_menu;
    let owners = weapons::capabilities::gun_owners_now().map(|gun| TriggerOwner {
        mode,
        gun: gun.filter(|_| !in_menu),
    });
    let late_changed = live.state.trigger_ownership.after_finish(owners);
    let changed =
        Hand::BOTH.map(|hand| acted.trigger_changed[hand.index()] || late_changed[hand.index()]);
    live.state.bindings.cancel_trigger_gestures(changed);
    if changed.contains(&true) {
        weapons::hold::tick::publish_fire(trigger_ownership::retain_fire(acted.fire, changed));
    }
    trigger_ownership::suppress_changed_routes(
        &mut evaluated.step.desired,
        mode,
        &hands.controls,
        &hands.triggers.routes,
        changed,
    );
    Settled {
        owners,
        blocked: Hand::BOTH
            .map(|hand| hands.triggers.blocked[hand.index()] || changed[hand.index()]),
    }
}

/// Refresh the controls summary, resolve the tick's writes into the sink, and report the tick.
/// Returns the controllers' body turn.
pub(super) fn publish(
    live: &mut Live<'_, '_>,
    routed: &Routed,
    hands: &Hands,
    acted: &Acted,
    settled: &Settled,
    evaluated: Evaluated,
) -> f32 {
    let game = live.game;
    let detection = live.detection;
    let in_menu = live.in_menu;
    let controls = &hands.controls;
    let semantic_input = &hands.offers.semantic_input;
    let targets = &hands.offers.targets;
    let step = evaluated.step;
    let state = &mut *live.state;
    let masks = trigger_ownership::masks(settled.owners, settled.blocked);
    let routes = summary::permitted_routes(
        &state.bindings,
        detection.mode,
        controls,
        semantic_input,
        &routed.policy,
        masks,
    );
    let mut eligibility = summary::capture(
        game,
        summary::Refresh {
            sequence: hands.filtered.frame.sequence,
            belt_reload: acted.belt_reload,
            parachute: vehicles::parachute_holds(),
            settings: live.settings,
            owners: settled.owners,
            ready: Hand::BOTH.map(|hand| !in_menu && state.recovery.ready(hand)),
            grapple_routes: summary::grapple_routes(
                &state.bindings,
                detection.mode,
                controls,
                semantic_input,
                &routed.policy,
                masks,
            ),
            trigger_blocked: settled.blocked,
            chosen: Hand::BOTH.map(|hand| {
                targets.chosen()[hand.index()]
                    .filter(|_| !controls.hands[hand.index()].grip.released)
            }),
            selected: Hand::BOTH.map(|hand| {
                targets.selected_for_summary()[hand.index()]
                    .filter(|_| !controls.hands[hand.index()].grip.released)
            }),
            grabs: step.grabs,
            before: &evaluated.summary_before,
            grapple: state.bindings.grapple_permissions(controls.grapple),
            yank: (yank_armed_in(hands.grapple.line, &detection) && hands.offers.grapple_held)
                .then_some(live.grapple_hand),
            flight: state
                .flight
                .as_ref()
                .map_or(Default::default(), |flight| flight.current()),
        },
    );
    eligibility.routes = routes;
    crate::controllers::summary::refresh(detection.mode, controls, live.settings, &eligibility);
    let writes = state.tracker.resolve(&step.desired, live.dt);
    live.sink.push(Segment {
        writes,
        tracked: true,
    });
    state.report = TickReport {
        detection,
        idle: None,
        body_turn: step.body_turn,
        writes: Vec::new(),
    };
    step.body_turn
}
