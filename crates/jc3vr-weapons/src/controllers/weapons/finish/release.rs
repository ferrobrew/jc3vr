//! The finish half's release stage: what opening a grip on a held weapon does.

use std::time::Instant;

use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_body::controllers::{
    grab::GrabVolume,
    hands::ActiveHands,
    haptics,
    intent::{controls::ButtonControl, finish::FinishInputs},
    physics::tuning::PHYSICS,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Event, motion};

use crate::controllers::{
    loadout::{Socket, grab, grab::Release, live},
    weapons::{
        docks::attract,
        dual_wield::single,
        engine,
        finish::{Finish, docked_at},
        foregrip, hold,
        hold::tuning::HOLD,
        regions, reload,
        state::STATE,
        weighted_hands,
    },
};

/// Let go of what each opened grip holds, unless the physical action holds it: dock it on the socket
/// under the hand or the one that draws it in, pass it to the hand on its action or the bracing
/// hand, or let it fall. Without the release, every pending release is called off.
pub(super) fn release_stage(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs { grips, real_dt, .. } = *f.inputs;
    // A holding hand that opens while the other hand holds its gun's slide or bolt passes the gun
    // to that hand, rather than waiting for the action and dropping the gun once it lets go.
    let passes = passes_by_action(reload::action_held_by_other(), grips);
    let action_hands = waiting_on_action(reload::action_hands(), passes);
    let release_active =
        f.context.settings.weapons.hold.grip_to_hold && f.context.modes.guns && f.active.is_some();
    let released = if release_active {
        hold::act::release_tick(game, grips.map(ButtonControl::held), action_hands)
    } else {
        hold::act::cancel_releases();
        [false; 2]
    };
    if !release_active {
        return;
    }
    for hand in Hand::BOTH {
        let pending = hold::act::pending_release(hand);
        let near = pending
            .and_then(|intent| intent.socket)
            .is_some_and(|socket| {
                docked_at(hand, |volume| volume == GrabVolume::Socket(socket)).is_some()
                    || attraction_for(hand, None).is_some_and(|a| a.socket == socket)
            });
        // Only a press calls the release off: fingers resting round a force-sensing grip
        // after the press let go are not a hold ([`hold::act::pressed_grips`]).
        let retry = hold::act::retry_release(
            hand,
            grips[hand.index()].held() || hold::act::pressed_grips()[hand.index()] == Some(true),
            near,
            real_dt,
        );
        if !released[hand.index()] && !retry {
            continue;
        }
        if pending.is_none() && passes[hand.index()] && pass_by_action(game, hand) {
            continue;
        }
        let gun = f.context.lone.map(|(weapon, _)| weapon);
        release(game, hand, gun, f.active.as_ref(), f.world_scale);
    }
}

/// Carry out what opening `hand`'s grip on its weapon does, with `weapon` the lone gun, as the table
/// decides it: dock it on the nearest socket under the hand that takes it, leave a long gun
/// the other hand braces in that hand, or let it fall with the hand's motion.
fn release(
    game: &GameTick<'_>,
    hand: Hand,
    weapon: Option<engine::HeldWeapon>,
    active: Option<&ActiveHands>,
    world_scale: f32,
) {
    let Some(snapshot) = live::snapshot_now() else {
        return;
    };
    let Some(object) = live::in_hand(hand) else {
        return;
    };
    // SAFETY: the game stage runs on the game thread before the world update, and a slot or the
    // carry set keeps the table's weapon in the hand alive.
    let dockable = unsafe { hold::act::dockable(object) };
    let pending = hold::act::pending_release(hand);
    // The socket the other hand's weapon is on its way to is spoken for: two guns let go of
    // together over one free shoulder would both travel there, and the second would fall when
    // the first docked.
    let reserved = hold::act::pending_release(hand.other()).and_then(|intent| intent.socket);
    let over = docked_at(
        hand,
        |v| matches!(v, GrabVolume::Socket(s) if dockable && Some(s) != reserved && grab::fits(&snapshot, hand, s)),
    )
    .and_then(|v| match v {
        GrabVolume::Socket(socket) => Some(socket),
        _ => None,
    });
    // Outside every socket's reach, a free socket's attraction may still take the weapon, unless
    // the other hand braces it, which then keeps it.
    let braced_by_other = foregrip::status()
        .hold
        .is_some_and(|h| h.engaged && h.weapon == object && h.holder == hand);
    let over = retry_socket(pending, over);
    let attracted = over
        .is_none()
        .then(|| attraction_for(hand, reserved))
        .flatten()
        .filter(|a| {
            dockable
                && !braced_by_other
                && pending.is_none_or(|intent| intent.socket == Some(a.socket))
        });
    hold::act::intend_release(hand, object, over.or(attracted.map(|a| a.socket)));
    // A taken socket on the back swaps: its weapon falls clear first, which frees the socket for
    // the dock below.
    let swapped =
        attracted
            .filter(|a| a.swap)
            .is_some_and(|a| match hold::swap::eject(game, a.socket) {
                Ok(()) => true,
                Err(refusal) => {
                    tracing::info!(
                        target: "controllers",
                        %refusal,
                        socket = a.socket.label(),
                        "hold: the swap on the {} was not done: {refusal}",
                        a.socket.label()
                    );
                    false
                }
            });
    let attracted = attracted.filter(|a| !a.swap || swapped);
    let snapshot = if swapped {
        match live::snapshot_now() {
            Some(snapshot) => snapshot,
            None => return,
        }
    } else {
        snapshot
    };
    // SAFETY: the game stage runs on the game thread before the world update.
    let release = grab::release(
        &snapshot,
        hand,
        over.or(attracted.map(|a| a.socket)),
        unsafe { droppable(object) },
    );
    match release {
        Release::Dock(socket) => {
            hold::act::intend_release(hand, object, Some(socket));
            let docked = match attracted.filter(|a| a.socket == socket) {
                Some(a) => {
                    let tuning = HOLD.get();
                    let travel =
                        attract::travel_s(a.distance, tuning.hold.snap_s, &tuning.docks.attraction);
                    tracing::info!(
                        target: "controllers",
                        hand = hand.label(),
                        socket = socket.label(),
                        distance_m = a.distance,
                        travel_s = travel,
                        "hold: the {} drew in the weapon the {} hand let go of",
                        socket.label(),
                        hand.label()
                    );
                    hold::act::attract(game, hand, socket, a.toward, travel)
                }
                None => hold::act::dock(game, hand, socket, HOLD.get().hold.snap_s),
            };
            match docked {
                Ok(()) if swapped => haptics::play(hand, Event::SocketSwap),
                // A dock by opening the grip confirms as a dock by a grab does.
                Ok(()) => haptics::play(hand, Event::VolumeGrab),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the weapon was not docked");
                }
            }
        }
        Release::LetGo => {
            // A gun the other hand braces passes to that hand, which slides onto its grip.
            let braced = pending.is_none()
                && weapon.is_some_and(|w| w.address == object)
                && active.is_some_and(|a| leave_in_brace(game, object, hand, a, world_scale));
            if braced {
                return;
            }
            if let Err(refusal) = hold::act::let_go(game, hand, PHYSICS.get().release.max_hand_mps)
            {
                tracing::debug!(target: "controllers", %refusal, "hold: the weapon was not let go of");
            }
        }
        Release::Keep => {}
    }
}

/// Pass the long gun `weapon`, whose holding hand `holder` let go, to the hand that braces it: the
/// bracing hand slides onto the grip from wherever it braced, the gun turning onto the canonical
/// grip as it glides through the hand ([`regions::grip_from_brace`]), and fires it. Returns whether
/// the gun stayed in the bracing hand.
fn leave_in_brace(
    game: &GameTick<'_>,
    weapon: usize,
    holder: Hand,
    active: &ActiveHands,
    world_scale: f32,
) -> bool {
    let Some(hold) = foregrip::status()
        .hold
        .filter(|h| h.engaged && h.weapon == weapon && h.holder == holder)
    else {
        return false;
    };
    let brace = holder.other();
    let hands = weighted_hands(active);
    let (Some(holding), Some(bracing)) = (hands[holder.index()], hands[brace.index()]) else {
        return false;
    };
    let gun = holding.wrist.compose(hold.geometry.gun);
    let anchor = regions::into_gun(hold.geometry.gun, hold.geometry.foregrip, world_scale);
    let turn = regions::turn_on_wrist(bracing.wrist.orientation, gun.orientation);
    if !anchor.is_finite() || !turn.is_finite() || !hold::hand_over(game, brace, weapon) {
        return false;
    }
    foregrip::clear();
    regions::grip_from_brace(weapon, brace, hold.region, turn, anchor, Instant::now());
    true
}

pub(super) fn retry_socket(
    pending: Option<hold::dock::ReleaseIntent>,
    over: Option<Socket>,
) -> Option<Socket> {
    over.filter(|socket| pending.is_none_or(|intent| intent.socket == Some(*socket)))
}

/// Which holding hands pass their gun on when they open, by [`Hand::index`]: the other hand holds
/// the gun's action by its handle (`action_held`), with its grip still closed on it.
pub(super) fn passes_by_action(action_held: [bool; 2], grips: [ButtonControl; 2]) -> [bool; 2] {
    Hand::BOTH.map(|hand| action_held[hand.index()] && grips[hand.other().index()].held())
}

/// The hands whose release waits for the physical action (`action_hands`), less those that pass
/// their gun to the hand on the action.
pub(super) fn waiting_on_action(action_hands: [bool; 2], passes: [bool; 2]) -> [bool; 2] {
    Hand::BOTH.map(|hand| action_hands[hand.index()] && !passes[hand.index()])
}

/// Pass the gun `hand` let go of to the other hand, which holds its action: the table moves it,
/// and the gun glides from where it was into the other hand's grip over the hold's
/// [`snap_s`](crate::controllers::weapons::hold::tuning::HoldTuning::snap_s), so it is seen to move
/// into the palm rather than jump there. The action lets go of its handle once the hand on it holds
/// the gun. `false` when the other hand could not take it, for the ordinary release to decide.
fn pass_by_action(game: &GameTick<'_>, hand: Hand) -> bool {
    let Some(object) = live::in_hand(hand) else {
        return false;
    };
    // SAFETY: the game stage runs on the game thread before the world update, and a slot or the
    // carry set keeps the table's weapon in the hand alive.
    let from = unsafe { (object as *const WeaponBase).as_ref() }
        .and_then(|weapon| unsafe { engine::gun_pose(weapon) });
    let to = hand.other();
    if !hold::hand_over(game, to, object) {
        return false;
    }
    if let Some(from) = from {
        hold::act::slide_in(object, to, from, HOLD.get().hold.snap_s);
    }
    tracing::info!(
        target: "controllers",
        from = hand.label(),
        to = to.label(),
        "hold: the {} hand let go, and the {} hand holding the action takes the gun",
        hand.label(),
        to.label()
    );
    true
}

/// The free socket whose attraction takes the weapon `hand` lets go of from outside every socket's
/// reach, leaving out `reserved`, the socket the other hand's weapon is already on its way to: the
/// hand's grip as it is drawn against the tick's attractors, and the hand's fastest motion just
/// before the release, which must not be a throw. A hand without motion history counts as still.
fn attraction_for(hand: Hand, reserved: Option<Socket>) -> Option<attract::Attraction> {
    let speed = motion::latest_time()
        .and_then(|latest| {
            let since = latest - f64::from(HOLD.get().docks.attraction.speed_window_s);
            motion::peak_velocity(&motion::history(hand), since)
        })
        .map_or(0.0, |(_, velocity)| velocity.length());
    let state = STATE.lock();
    let grip = state.points[hand.index()]?;
    let open: Vec<attract::Attractor> = state
        .attractors
        .iter()
        .filter(|a| Some(a.socket) != reserved)
        .copied()
        .collect();
    attract::attract(&open, hand, grip, speed, &HOLD.get().docks.attraction)
}

/// Whether the weapon at `object` can lie in the world: every weapon but the game's dual-wield pair,
/// one object of two guns, which a single pistol is not.
///
/// # Safety
///
/// `object` must be a weapon a slot or the carry set keeps alive, on the game thread before the
/// world update.
unsafe fn droppable(object: usize) -> bool {
    // SAFETY: forwarded from the caller.
    unsafe { (object as *const WeaponBase).as_ref() }
        .is_none_or(|weapon| !unsafe { single::is_pair(weapon) })
}
