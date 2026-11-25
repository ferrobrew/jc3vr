//! The ripcord on the input tick ([`crate::controllers::vehicles::ripcord`]): its grip target while
//! the player falls or flies the wingsuit, and the pull that asks the game to open the parachute.
//!
//! The pull is a click of the canopy's own action, the one the canopy's button drives, so the
//! game's animation graph decides whether the canopy opens from the state the character is in. It
//! is written before the wingsuit gestures commit, so a pull suppresses a suit request on the same
//! tick, as the button does.

use jc3gi::input::input_action_map::Action::OPEN_PARACHUTE;
use parking_lot::Mutex;

use crate::controllers::{
    bindings::{
        controls::Controls,
        hold::{Desired, MapKind, Target, Want},
    },
    body,
    config::ControllersConfig,
    fingers::grip::{self, Grip, GripSource},
    frame::Hand,
    grab::{GrabVolume, GripTarget, Mark, MarkAt, MarkStyle, Shown, Tier},
    hands::{self, ActiveHands},
    haptics::{self, Event},
    mode::{AirborneVariant, Mode},
    vehicles::ripcord::{self, RipcordEvent, RipcordState, StepInputs},
};

/// Whether the ripcord acts in `mode`.
pub(super) fn ripcord_acts(mode: Mode, config: &ControllersConfig) -> bool {
    config.vehicles.ripcord.enabled
        && matches!(
            mode,
            Mode::Airborne(AirborneVariant::Falling | AirborneVariant::Wingsuit)
        )
}

/// The ripcord's grip targets this tick: its volume, held by the hand that has it, and its reach
/// margin while it is free. Lets go of it outside a fall and the wingsuit.
pub(super) fn targets(mode: Mode, active: &ActiveHands) -> Vec<GripTarget> {
    let mut state = STATE.lock();
    if !ripcord_acts(mode, &active.config) {
        let mut events = Vec::new();
        ripcord::release_all(&mut state.ripcord, &mut events);
        state.side = None;
        drop(state);
        log_events(&events);
        return Vec::new();
    }
    let side = active.config.acting_hand();
    let player = body::player();
    let torso = active.torso;
    let shoulders = body::shoulder_positions(active.head.position, torso, &player);
    let centre = ripcord::centre(shoulders, torso, &player, side);
    state.side = Some(side);
    let grips = active.hands.map(|h| h.map(|h| h.grip.position));
    let placed = ripcord::candidate(&state.ripcord, centre, side, grips);
    let mark = Mark::ring(MarkAt::Cockpit(placed.centre), ripcord::RADIUS_M)
        .styled(MarkStyle::Handle)
        .showing(Shown::Always);
    let mut targets = Vec::with_capacity(2);
    match state.ripcord.hold {
        Some(hold) => {
            grip::declare(hold.hand, Grip::new(GripSource::ParachuteHandle));
            targets.push(placed.target(mark).held(hold.hand));
        }
        None => targets.push(placed.target(mark)),
    }
    targets.extend(
        ripcord::reach_candidate(&state.ripcord, centre, side).map(|c| {
            c.target(Mark::ring(MarkAt::Cockpit(c.centre), c.radius).showing(Shown::Never))
                .ranked(Tier::Fallback)
        }),
    );
    targets
}

/// Act on this tick's grabs: take, hold, or let go of the ripcord, and click the canopy's action
/// when a hold pulls it. `grabs` are the vehicle handling's, by [`Hand::index`].
pub(crate) fn finish(
    mode: Mode,
    grabs: [Option<GrabVolume>; 2],
    controls: &Controls,
    desired: &mut Vec<Desired>,
) {
    let Some(active) = hands::active_hands_raw() else {
        return;
    };
    let mut state = STATE.lock();
    let Some(side) = state.side.filter(|_| ripcord_acts(mode, &active.config)) else {
        return;
    };
    let inputs = StepInputs {
        side,
        grips: active.hands.map(|h| h.map(|h| h.grip.position)),
        held: Hand::BOTH.map(|hand| controls.hand(hand).hold.held()),
        grabs: grabs.map(|g| g == Some(GrabVolume::Ripcord)),
        pull_m: active.config.vehicles.ripcord.pull_m,
    };
    let mut events = Vec::new();
    let pulled = ripcord::step(&mut state.ripcord, &inputs, &mut events);
    drop(state);
    if pulled {
        desired.push(Desired {
            target: Target {
                map: MapKind::Player,
                action: OPEN_PARACHUTE,
            },
            want: Want::Click,
        });
        haptics::play(side, Event::Unlatch);
    }
    log_events(&events);
}

/// The hand holding the ripcord, if one does.
pub fn holder() -> Option<Hand> {
    STATE.lock().ripcord.hold.map(|hold| hold.hand)
}

struct TickState {
    ripcord: RipcordState,
    /// The ripcord's side while it acts.
    side: Option<Hand>,
}

static STATE: Mutex<TickState> = Mutex::new(TickState {
    ripcord: RipcordState::new(),
    side: None,
});

fn log_events(events: &[RipcordEvent]) {
    for event in events {
        match *event {
            RipcordEvent::Taken(hand) => {
                tracing::info!(target: "controllers", hand = hand.label(), "ripcord: took it");
            }
            RipcordEvent::Pulled(hand) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    "ripcord: pulled it, opening the parachute"
                );
            }
            RipcordEvent::Released(hand, why) => {
                tracing::info!(
                    target: "controllers",
                    hand = hand.label(),
                    reason = why.label(),
                    "ripcord: let go of it"
                );
            }
        }
    }
}
