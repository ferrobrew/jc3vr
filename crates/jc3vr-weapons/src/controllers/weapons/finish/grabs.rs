//! The finish half's grab stage: what each hand's grab of a weapon-handling volume does.

use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_body::controllers::{grab::GrabVolume, haptics, intent, intent::finish::FinishInputs};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{frame::Hand, haptics::cues::Event};

use crate::controllers::{
    loadout::{
        Place, Socket,
        contents::Passed,
        grab,
        grab::{SocketMove, SocketRefusal},
        live,
    },
    weapons::{
        c4,
        contents::sample,
        docks, engine,
        finish::{Finish, docked_at},
        foregrip,
        foregrip::Transition,
        grenade, hold,
        hold::tuning::HOLD,
        types::{GrabEvent, GrabOutcome, SocketOutcome},
    },
};

/// Act on each hand's grab of a weapon-handling volume: a handover of the lone gun or of a held
/// explosive, the foregrip, a take from the world, and a draw or a dock at a socket. The pouches,
/// the pin, the catch, the magazines, and the action are acted on by their own stages; a grab here
/// only reports them.
pub(super) fn grabs(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs { grabs, grips, .. } = *f.inputs;
    let (weapon, lone) = (f.context.weapon, f.context.lone);
    for hand in Hand::BOTH {
        let Some(volume) = grabs[hand.index()] else {
            continue;
        };
        let outcome = match volume {
            GrabVolume::Handle => {
                let gun = lone.map(|(w, _)| w.address);
                if gun.is_some_and(|object| {
                    live::in_hand(hand.other()) == Some(object)
                        && holstering(hand.other(), grips[hand.other().index()].held())
                }) {
                    continue;
                }
                // The gun glides into the hand from where it was, rather than jumping there.
                let from = gun
                    .and_then(|object| unsafe { (object as *const WeaponBase).as_ref() })
                    .and_then(|weapon| unsafe { engine::gun_pose(weapon) });
                let giver = hand.other();
                let mut gun_moved = false;
                let moved = match sample(weapon.as_ref().map(|(w, _)| w))[giver.index()].passed() {
                    Passed::Grenade => grenade::safe::transfer(giver, hand),
                    Passed::Charge => c4::hand_over(hand),
                    Passed::Gun => {
                        gun_moved = gun.is_some_and(|w| hold::hand_over(game, hand, w));
                        gun_moved
                    }
                };
                if moved {
                    if let (Some(object), true, Some(from)) = (gun, gun_moved, from) {
                        hold::act::slide_in(object, hand, from, HOLD.get().hold.snap_s);
                    }
                    // The hand that took the gun let go of the foregrip to take it.
                    foregrip::clear();
                }
                GrabOutcome::Handover { moved }
            }
            GrabVolume::Foregrip => match f.transition {
                Some(t) => GrabOutcome::Foregrip(t),
                None => GrabOutcome::ForegripUnmeasured,
            },
            GrabVolume::GrenadePin => GrabOutcome::Pin,
            GrabVolume::GrenadePouch | GrabVolume::GrenadeLocker => GrabOutcome::Pouch,
            GrabVolume::LiveGrenade => GrabOutcome::Catch,
            GrabVolume::MagazinePouch
            | GrabVolume::WorldMagazine
            | GrabVolume::InsertedMagazine => GrabOutcome::Magazine,
            GrabVolume::WeaponAction => GrabOutcome::Action,
            GrabVolume::C4Pouch | GrabVolume::PlacedCharge => GrabOutcome::C4,
            GrabVolume::WorldWeapon => {
                let Some(active) = f.active.as_ref() else {
                    continue;
                };
                let inputs = hold::take::TakeInputs {
                    active,
                    free: [true; 2],
                    gameplay: f.context.modes.world,
                };
                // SAFETY: the game stage runs on the game thread before the world update.
                let taken = unsafe { hold::take::finish_grab(game, hand, &inputs) };
                GrabOutcome::Take { taken }
            }
            // A draw takes the gun by its grip wherever on it the hand closed.
            GrabVolume::Socket(socket) => match grab_socket(game, hand, socket) {
                Some(outcome) => outcome,
                None => continue,
            },
            // The other subsystems' grabs: the bindings hand the weapon handling only its own.
            GrabVolume::MagazineWell
            | GrabVolume::ChargeSurface
            | GrabVolume::VehicleControl(_)
            | GrabVolume::CockpitButton(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::Ripcord
            | GrabVolume::MountedHandle(_)
            | GrabVolume::Pickup(_)
            | GrabVolume::Climb
            | GrabVolume::StuntHold
            | GrabVolume::Snatch(_)
            | GrabVolume::GrappleLine => {
                continue;
            }
        };
        tracing::debug!(target: "controllers", ?hand, volume = volume.label(), outcome = %outcome.label(), "grab");
        if cues_grab(outcome) {
            haptics::play(hand, Event::VolumeGrab);
        }
        f.events.push(GrabEvent {
            hand,
            volume,
            outcome,
        });
    }
}

/// Carry out `hand`'s grab of `socket` as the table decides it: draw the weapon on it into the
/// empty hand, or dock the hand's weapon there. `None` for a grab that did nothing worth reporting:
/// a free socket with nothing to put there, or no table to decide against.
fn grab_socket(game: &GameTick<'_>, hand: Hand, socket: Socket) -> Option<GrabOutcome> {
    if hold::act::docking()[hand.index()] || hold::act::pending_release(hand).is_some() {
        return None;
    }
    if let Some(moved) = grab_safe_grenade(hand, socket) {
        let outcome = if moved {
            if grenade::safe::in_hand(hand).is_some() {
                SocketOutcome::Drew
            } else {
                SocketOutcome::Docked
            }
        } else {
            SocketOutcome::Failed(hold::equip::DockRefusal::NotDone)
        };
        return Some(GrabOutcome::Socket(outcome));
    }
    let snapshot = live::snapshot_now()?;
    let outcome = match grab::grab(&snapshot, hand, socket) {
        Ok(SocketMove::Draw { .. }) => {
            match hold::act::draw(game, hand, socket, HOLD.get().hold.snap_s) {
                Ok(()) => SocketOutcome::Drew,
                Err(refusal) => SocketOutcome::Failed(refusal),
            }
        }
        Ok(SocketMove::Dock { .. }) => {
            match hold::act::dock(game, hand, socket, HOLD.get().hold.snap_s) {
                Ok(()) => SocketOutcome::Docked,
                Err(refusal) => SocketOutcome::Failed(refusal),
            }
        }
        Err(SocketRefusal::Empty(_)) => return None,
        Err(refusal) => {
            hold::refused(refusal);
            SocketOutcome::Refused(refusal)
        }
    };
    Some(GrabOutcome::Socket(outcome))
}

/// Handle a safe grenade's socket grab without entering the native weapon path.
///
/// `None` means the socket interaction is unrelated to a safe grenade. `Some(false)` means the
/// safe move was relevant but refused, so the caller must not fall through to native weapon code.
fn grab_safe_grenade(hand: Hand, socket: Socket) -> Option<bool> {
    if grenade::safe::in_hand(hand).is_some() {
        if !matches!(socket, Socket::Hip(_)) {
            return Some(false);
        }
        let Some(world) = docks::socket_world_pose(socket) else {
            return Some(false);
        };
        return Some(grenade::safe::stow(hand, socket, world));
    }
    let has_safe_item = live::safe_items()
        .into_iter()
        .any(|item| item.place == Place::Socket(socket));
    has_safe_item.then(|| grenade::safe::draw_stowed(socket, hand))
}

/// Stow a held safe grenade on `socket`, the hip socket its opening hand would dock in, before the
/// grenade stage sees the fingers open. `None` when the hand holds no safe grenade, and
/// `Some(false)` when there is no hip socket to stow it on or the stow is refused.
pub(super) fn stow_safe_grenade(hand: Hand, socket: Option<Socket>) -> Option<bool> {
    grenade::safe::in_hand(hand)?;
    let Some(socket) = socket.filter(|socket| matches!(socket, Socket::Hip(_))) else {
        return Some(false);
    };
    let Some(world) = docks::socket_world_pose(socket) else {
        return Some(false);
    };
    Some(grenade::safe::stow(hand, socket, world))
}

fn holstering(hand: Hand, closed: bool) -> bool {
    hold::world::handover_blocked(
        hold::act::docking()[hand.index()],
        hold::act::pending_release(hand).is_some(),
        closed || bindings_closed(hand),
        intended_socket(hand).is_some(),
    )
}

fn bindings_closed(hand: Hand) -> bool {
    intent::grip_holds_now(hand) == Some(true)
}

fn intended_socket(hand: Hand) -> Option<Socket> {
    let snapshot = live::snapshot_now()?;
    docked_at(hand, |volume| matches!(volume, GrabVolume::Socket(socket) if grab::fits(&snapshot, hand, socket)))
        .and_then(|volume| match volume {
            GrabVolume::Socket(socket) => Some(socket),
            _ => None,
        })
}

/// Whether a grab's outcome is one the grab cue pulses: a draw, a dock, or a foregrip grab.
/// The pouches pulse their own outcomes, and a release pulses from the transition, whatever
/// caused it.
pub(super) fn cues_grab(outcome: GrabOutcome) -> bool {
    matches!(
        outcome,
        GrabOutcome::Socket(SocketOutcome::Drew | SocketOutcome::Docked)
            | GrabOutcome::Foregrip(Transition::Grabbed)
            | GrabOutcome::Handover { moved: true }
            | GrabOutcome::Take { taken: true }
    )
}
