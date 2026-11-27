//! One tick of the bindings, as ordered stages.
//!
//! 1. [`input::sample`]: the debug panel's capture of the controls, which may cancel the routes.
//! 2. The controllers disabled stand the bindings down ([`Reason::Disabled`]); otherwise the mode
//!    is detected, the hand tiers step, and the hand anchor reads the tick.
//! 3. [`input::route`]: the tick's frames and edges, the policy's arbitration of the system chords
//!    (which may toggle the panel and cancel the routes), and the panel pointer.
//! 4. The gate: an idle tick ([`Idle`]) stands down, and a tick whose frames are still in another
//!    action set than the mode's pauses ([`Reason::ActionSetPending`]).
//! 5. [`input::filter`]: the policy's and the recovery's masks over the frame, and the game
//!    pointer.
//! 6. [`hands::offers`]: the weapon and vehicle handling's tick starts, every subsystem's grip
//!    targets, and the resolve of each hand among them.
//! 7. [`hands::controls`], [`hands::triggers`], and [`hands::grapple`]: the logical controls, the
//!    triggers' ownership, and the grapple button's owners and the physical grapple.
//! 8. [`acts::evaluate`]: the tables, the hang under a helicopter, the grapple lock, the rail, the
//!    firm ragdoll, the seat grapple, the wingsuit gestures' evaluation, and the grip model's
//!    finish.
//! 9. [`acts::finish`]: the subsystems' acts in the order [`acts::ACTS`] lists, or in a game menu
//!    the pause of them ([`Reason::Menu`]).
//! 10. [`publish::settle`] and [`publish::publish`]: the triggers' ownership after the acts, the
//!     controls summary, and the writes.
//!
//! Each stage consumes what the stages before it produced, which fixes their order; the order the
//! data leaves open, the finish half's acts and each stand-down, is a table
//! ([`acts::ACTS`], [`Reason::steps`]).

mod acts;
mod hands;
mod input;
mod publish;

use jc3vr_body::controllers::{
    clock::TimeScale,
    hands as body_hands,
    hands::tiers,
    intent::{controls::Controls, hold::DevicePoll},
    mode,
    mode::{Detection, Mode},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;

use crate::controllers::bindings::{
    Idle, STATE, State, TickReport, loss,
    settings::Settings,
    stage::Segment,
    stand_down,
    stand_down::{Context, Reason},
};

/// One tick of the bindings, pushing its writes to `sink` in order. `dt` is the engine's tick
/// delta, seconds, and `poll` is whether the device poll that preceded the tick ran. Returns the
/// controllers' body turn, or `None` when they are not injecting.
pub(super) fn tick(
    game: &GameTick<'_>,
    dt: f32,
    poll: DevicePoll,
    sink: &mut Vec<Segment>,
) -> Option<f32> {
    // What the player's body does is timed in real time, and `dt` is the game's step, which its
    // slow motion shortens.
    // SAFETY: the bindings tick runs while the game runs.
    let real_dt = unsafe { TimeScale::current() }.real_dt(dt);
    let settings = Settings::snapshot();
    let capture = input::sample(game, &settings, poll, sink);
    if !settings.enabled {
        let mut state = STATE.lock();
        state.observe_poll(poll);
        stand_down::run(
            Reason::Disabled,
            &mut Context {
                game,
                poll,
                settings: &settings,
                state: &mut state,
                sink,
            },
        );
        return None;
    }
    let mut detection = mode::tick(game);
    tiers::tick(game, dt);
    // SAFETY: the game stage runs on the game thread before the world update.
    unsafe { body_hands::anchor::input_tick(settings.acting) };
    let mut routed = input::route(game, &settings, poll, sink, &capture, &mut detection);

    let mut state = STATE.lock();
    state.observe_poll(poll);
    let mut live = Live {
        game,
        settings: &settings,
        poll,
        dt,
        real_dt,
        sink,
        state: &mut state,
        detection,
        // A game menu or the map pauses the weapon and vehicle handling rather than standing it
        // down, so that what the hands hold (a foregrip, a primed grenade, a physical control) is
        // still held when gameplay resumes.
        in_menu: detection.mode == Mode::Ui,
        dominant: settings.acting,
        grapple_hand: settings.acting.other(),
    };
    let frame = match (routed.frame.take(), routed.idle) {
        (Some(frame), None) => frame,
        (_, idle) => {
            // Without a frame the tick is idle for want of controllers.
            let idle = idle.unwrap_or(Idle::NoControllers);
            live.stand_down(Reason::Idle(idle));
            live.state.report = TickReport {
                detection,
                idle: Some(idle),
                ..Default::default()
            };
            return None;
        }
    };
    live.state.recovery.observe(&frame);
    loss::cancel_throws(Hand::BOTH.map(|hand| !live.state.recovery.ready(hand)));
    // Mode selection precedes the next OpenXR sample. Preserve paused holds while waiting for it;
    // the poll runs here, so it releases their effectors, and only the clicks need undoing.
    let set_pending = frame.action_set != detection.mode.action_set()
        || routed
            .semantic
            .as_ref()
            .is_some_and(|s| s.action_set != detection.mode.action_set());
    if set_pending {
        live.stand_down(Reason::ActionSetPending);
        live.state.report = TickReport {
            detection,
            ..Default::default()
        };
        return None;
    }

    let filtered = input::filter(&mut live, frame, &routed, &capture.neutral_policy);
    let mut offers = hands::offers(&mut live, &filtered, &routed);
    let mut controls = hands::controls(&mut live, &filtered, &offers);
    let triggers = hands::triggers(&mut live, &filtered, &mut offers, &mut controls);
    let grapple = hands::grapple(
        &mut live,
        &routed,
        &capture,
        &filtered,
        &mut offers,
        &mut controls,
    );
    let mut hands = Hands {
        filtered,
        offers,
        controls,
        triggers,
        grapple,
    };
    let mut evaluated = acts::evaluate(&mut live, &routed, &hands);
    let acted = acts::finish(&mut live, &mut hands, &mut evaluated);
    let settled = publish::settle(&mut live, &hands, &acted, &mut evaluated);
    Some(publish::publish(
        &mut live, &routed, &hands, &acted, &settled, evaluated,
    ))
}

/// What every stage past the gate reads and writes.
struct Live<'a, 'g> {
    game: &'a GameTick<'g>,
    settings: &'a Settings,
    poll: DevicePoll,
    /// The engine's tick delta, game seconds.
    dt: f32,
    /// The tick delta in real seconds, which the game's slow motion does not shorten.
    real_dt: f32,
    sink: &'a mut Vec<Segment>,
    state: &'a mut State,
    detection: Detection,
    in_menu: bool,
    dominant: Hand,
    grapple_hand: Hand,
}

impl Live<'_, '_> {
    /// Stand down for `reason` from a stage past the gate.
    fn stand_down(&mut self, reason: Reason) {
        stand_down::run(
            reason,
            &mut Context {
                game: self.game,
                poll: self.poll,
                settings: self.settings,
                state: self.state,
                sink: self.sink,
            },
        );
    }
}

/// What the hands' stages produced, for the stages after them.
struct Hands {
    filtered: input::Filtered,
    offers: hands::Offers,
    controls: Controls,
    triggers: hands::Triggers,
    grapple: hands::GrappleTick,
}
