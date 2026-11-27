//! The begin half's last stages: resolve the target each hand would take, and publish the tick for
//! the finish half, the readout, and the hints.

use jc3vr_body::controllers::{
    grab,
    grab::{GrabVolume, resolve as grab_resolve},
    hands, haptics, intent,
    physics::local_player::LocalPlayer,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Event};

use crate::controllers::{
    loadout::{grab::fits, live},
    weapons::{
        begin::{Begin, entered_body_volumes, hint},
        heft, hold,
        hold::tuning::HOLD,
        reload,
        state::STATE,
        types::WieldedReadout,
    },
};

/// Resolve the target each hand would take, as the weapon handling sees each hand: a hand
/// holstering its gun offers it to no other hand, a magazine being pulled out wins, and a hand
/// carrying a round or closing on a foregrip holds the gun in the other hand still.
pub(super) fn resolve(_game: &GameTick<'_>, b: &mut Begin) {
    let active = &b.active;
    // The weapon handling's own view of where each hand is, for the release's docking, the heft,
    // and the cue on entering a socket.
    let grips = grab::tick::hand_grips(active);
    let controller_grips = hands::controller_grips(active);
    grab_resolve::reach_low(&mut b.targets, grips, controller_grips);
    let docking = hold::act::docking();
    let targets = &b.targets;
    let holstering = Hand::BOTH.map(|hand| {
        let near = live::snapshot_now().is_some_and(|snapshot| {
            grab_resolve::dock_target(hand, targets, grips[hand.index()], |volume| {
                matches!(volume, GrabVolume::Socket(socket) if fits(&snapshot, hand, socket))
            })
            .is_some()
        });
        live::in_hand(hand).is_some()
            && hold::world::handover_blocked(
                docking[hand.index()],
                hold::act::pending_release(hand).is_some(),
                intent::grip_holds_now(hand) != Some(false),
                near,
            )
    });
    b.targets.retain(|target| {
        target.volume != GrabVolume::Handle
            || target
                .hand
                .is_none_or(|hand| !holstering[hand.other().index()])
    });
    reload::prioritize_extraction(&mut b.targets, grips);
    let resolved = grab_resolve::resolve(&b.targets, grips);
    let volumes = resolved.map(|r| r.chosen.map(|c| c.volume));
    let foregrip_hand = b.foregrip_hand;
    let closing_on_foregrip = b.hold.is_none()
        && foregrip_hand.is_some_and(|h| volumes[h.index()] == Some(GrabVolume::Foregrip));
    // A hand carrying a round holds still the gun in the other hand, whose well it reaches for, and
    // a hand closing on a foregrip holds still the gun it closes on.
    let carrying_round = reload::holding_hand();
    heft::freeze(Hand::BOTH.map(|hand| {
        carrying_round == Some(hand.other())
            || (closing_on_foregrip && foregrip_hand == Some(hand.other()))
    }));
    b.grips = grips;
    b.volumes = volumes;
}

/// Publish the tick: the readout, the targets and grips for the release's docking, the handoff to
/// the finish half, and the attractions; then the hints, and the cues on entering a body volume or
/// an attraction.
pub(super) fn publish(game: &GameTick<'_>, b: &mut Begin) {
    let (grips, volumes) = (b.grips, b.volumes);
    let weapon = b.ticked.weapon;
    b.context.weapon = weapon.map(|w| (w, b.geometry));
    b.context.lone = b.lone.map(|w| (w, b.braced_geometry));
    let mut state = STATE.lock();
    state.status.active = true;
    state.status.volumes = volumes;
    state.status.candidates = b.targets.len();
    state.placed.clone_from(&b.targets);
    state.points = grips;
    state.status.wielded = weapon.map(|weapon| WieldedReadout {
        weapon,
        geometry: b.geometry,
    });
    state.tick = Some(b.context.clone());
    state.attractors = std::mem::take(&mut b.attractors);
    let entered = entered_body_volumes(state.last_volumes, volumes);
    state.last_volumes = volumes;
    // A hand whose gun the other hand braces keeps it on letting go, so its socket does not call.
    let braced = b.hold.map(|h| h.holder);
    let tracked = &mut *state;
    let attracted = Hand::BOTH.map(|hand| {
        let grip = grips[hand.index()].filter(|_| braced != Some(hand));
        tracked.zones.step(
            hand,
            &tracked.attractors,
            grip,
            &HOLD.get().docks.attraction,
        )
    });
    let in_attraction = Hand::BOTH.map(|hand| tracked.zones.inside(hand));
    drop(state);

    let player = LocalPlayer::get(game);
    // SAFETY: the local player, in the game tick, and the table keeps its live weapons alive.
    unsafe {
        hint::publish(
            player.as_ref().map(LocalPlayer::character),
            &hint::HintInputs {
                snapshot: live::snapshot_now().as_ref(),
                chosen: volumes,
                attracted: in_attraction,
            },
        );
    }

    for hand in Hand::BOTH.into_iter().filter(|h| entered[h.index()]) {
        haptics::play(hand, Event::VolumeEnter);
    }
    for hand in Hand::BOTH.into_iter().filter(|h| attracted[h.index()]) {
        haptics::play(hand, Event::DockAttraction);
    }
}
