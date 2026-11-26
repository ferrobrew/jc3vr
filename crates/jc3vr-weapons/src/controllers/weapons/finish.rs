//! The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
//! throw and the reload, and hold the fire inputs back during a weapon action.

use std::time::Instant;

use jc3gi::{
    character::inventory::WeaponSlot, equipment::weapon::WeaponBase,
    input::input_action_map::Action,
};
use jc3vr_body::controllers::{
    fingers::{
        grip,
        grip::{Grip, GripSource},
    },
    grab::{GrabVolume, resolve::dock_target},
    hands,
    hands::{
        ActiveHands, pose as hand_pose, tiers,
        tiers::{HandTier, TierState},
    },
    haptics, intent,
    intent::{
        controls::ButtonControl,
        finish::FinishInputs,
        hold::{Desired, MapKind},
    },
    mode,
    physics::local_player::LocalPlayer,
    telemetry,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{
    frame::{self, Hand},
    haptics::cues::Event,
    motion,
};

use crate::controllers::{
    loadout::{
        Socket, grab,
        grab::{Release, SocketMove, SocketRefusal},
        live,
    },
    weapons::{
        begin::with_holder,
        c4,
        c4::tuning::C4,
        capabilities::{
            Capabilities, GunHand, gun_hands_now, gun_owners_now, tracked_hands, trigger_claims,
        },
        config::WeaponSettings,
        docks::{attract, attract::ATTRACTION},
        dual_wield::single,
        engine, foregrip,
        foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
        grenade,
        handling_tuning::HANDLING,
        hold,
        hold::{armed, tick},
        regions,
        regions::GunRegion,
        reload,
        reload::fire_hold::FireHold,
        state::STATE,
        types::{GrabEvent, GrabOutcome, SocketOutcome},
        weighted_hands,
    },
};

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw and the reload, and hold the fire inputs back during a weapon action. `desired` is the
/// tables' output, which the grenade throw adds to and the fire hold-back trims.
pub fn finish_tick(
    game: &GameTick<'_>,
    inputs: &FinishInputs,
    desired: &mut Vec<Desired>,
) -> Option<reload::BeltEligibility> {
    let FinishInputs {
        grabs,
        grips,
        triggers,
        fire_presses,
        unavailable,
        reload_button,
        dt,
        real_dt,
    } = *inputs;
    let config = WeaponSettings::read();
    let active = hands::active_hands_raw();
    let capabilities = Capabilities::of(mode::current().mode);
    let source_guns = gun_owners_now();
    // The fingers opening lets a grenade or a charge go, not the grip's press relaxing.
    let values = frame::with_latest(|f| {
        Hand::BOTH.map(|h| {
            let hand = f.hand(h);
            (f.focused && hand.active).then_some(hand.inputs.squeeze)
        })
    })
    .unwrap_or([None; 2]);
    let opened = STATE
        .lock()
        .opening
        .step_available(values, grips, unavailable);
    let (weapon, lone, gameplay, pouches, touch, slide) = {
        let state = STATE.lock();
        (
            state.weapon,
            state.lone,
            state.gameplay,
            state.pouches,
            state.touch,
            state.slide,
        )
    };
    let world_scale = active.as_ref().map_or(1.0, |a| a.world_scale);

    let foregrip_grab = grabs
        .iter()
        .zip(Hand::BOTH)
        .find(|(g, _)| **g == Some(GrabVolume::Foregrip))
        .map(|(_, hand)| hand);
    let two_handed = lone
        .filter(|(w, _)| w.two_handed && capabilities.support)
        .and_then(|(w, geometry)| {
            Some(TwoHandedWeapon {
                holder: w.holder?,
                address: w.address,
                geometry,
                region: GunRegion::Handguard,
            })
        });
    // The off-hand braces where it touched the gun.
    let at_touch = touch
        .filter(|t| foregrip_grab == Some(t.hand) && t.hit.region != GunRegion::Grip)
        .map(|t| (t.hit.region, t.anchor_in_wrist));
    let two_handed = two_handed.map(|w| match at_touch {
        Some((region, point)) => TwoHandedWeapon {
            geometry: w.geometry.map(|g| GripGeometry {
                foregrip: point,
                ..g
            }),
            region,
            ..w
        },
        None => w,
    });
    // Taken every tick, so a break left over from an ended hold never lets go of the next.
    let support_broke = Hand::BOTH.map(jc3vr_body::controllers::held::guns::take_support_break);
    let inputs = Inputs {
        active: active.is_some(),
        hands: active.as_ref().map_or([None; 2], |a| a.hands),
        weapon: two_handed,
        grab_pressed: foregrip_grab
            .is_some_and(|hand| two_handed.is_some_and(|w| w.holder.other() == hand)),
        grip_held: two_handed.is_some_and(|w| grips[w.holder.other().index()].held()),
        slide,
        support_broke: two_handed.is_some_and(|w| support_broke[w.holder.other().index()]),
    };
    let transition = foregrip::tick(&inputs, dt, &HANDLING.get().foregrip);
    if let (Some(Transition::Grabbed), Some(w)) = (transition, two_handed) {
        regions::braced(w.holder.other(), w.region);
    }
    if slide.is_some() {
        regions::slid();
    }
    // A release pulses the off-hand whatever ended the hold: a press, a move away, or the weapon
    // leaving the hands.
    if let (Some(Transition::Released(_)), Some(w)) = (transition, two_handed) {
        haptics::play(w.holder.other(), Event::VolumeGrab);
    }

    let mut events = Vec::new();
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
                let mut gun_moved = false;
                let moved =
                    crate::controllers::weapons::grenade_moves::transfer(hand.other(), hand)
                        .or_else(|| {
                            c4::held()
                                .filter(|&(holder, _)| holder == hand.other())
                                .map(|_| c4::hand_over(hand))
                        })
                        .unwrap_or_else(|| {
                            gun_moved = gun.is_some_and(|w| hold::hand_over(game, hand, w));
                            gun_moved
                        });
                if moved {
                    if let (Some(object), true, Some(from)) = (gun, gun_moved, from) {
                        hold::act::slide_in(object, hand, from, hold::SNAP_S);
                    }
                    // The hand that took the gun let go of the foregrip to take it.
                    foregrip::clear();
                }
                GrabOutcome::Handover { moved }
            }
            GrabVolume::Foregrip => match transition {
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
                let Some(active) = active.as_ref() else {
                    continue;
                };
                let inputs = hold::take::TakeInputs {
                    active,
                    free: [true; 2],
                    gameplay,
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
        events.push(GrabEvent {
            hand,
            volume,
            outcome,
        });
    }
    let grenade_opened = Hand::BOTH.map(|hand| {
        if opened[hand.index()] && grenade::safe::in_hand(hand).is_some() {
            let socket = docked_at(hand, |volume| {
                matches!(volume, GrabVolume::Socket(socket) if matches!(socket, Socket::Hip(_)))
            }).and_then(|volume| match volume {
                GrabVolume::Socket(socket) => Some(socket),
                _ => None,
            });
            if crate::controllers::weapons::grenade_moves::stow_before_open(hand, socket)
                == Some(true)
            {
                haptics::play(hand, Event::VolumeGrab);
                return false;
            }
        }
        opened[hand.index()]
    });
    grenade::input_tick(
        game,
        &grenade::TickInputs {
            acting: pouches && active.is_some(),
            // The pin is on the grenade in the other hand.
            pin_grabs: Hand::BOTH
                .map(|holder| grabs[holder.other().index()] == Some(GrabVolume::GrenadePin)),
            locker_grabs: Hand::BOTH
                .map(|hand| grabs[hand.index()] == Some(GrabVolume::GrenadeLocker)),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePouch)),
            catch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::LiveGrenade)),
            cockpit_grips: active
                .as_ref()
                .map_or([None; 2], |a| a.hands.map(|h| h.map(|h| h.grip.position))),
            triggers,
            dominant: config.dominant,
            opened: grenade_opened,
            dt,
        },
        desired,
    );
    // A hand holds nothing when it holds no gun (the C4 the game's planting mode wields is not one),
    // no weapon armed in it, nothing of the loadout table's, no grenade, and no round.
    let gun_hands = weapon.map_or([false; 2], |(w, _)| {
        if w.wielded.slot == WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE {
            [false; 2]
        } else {
            w.wielded.hands
        }
    });
    let armed_hands = Hand::BOTH.map(|h| armed::in_hand(h).is_some());
    let detonator = c4::detonator_hand();
    let free = Hand::BOTH.map(|h| {
        !gun_hands[h.index()]
            && !armed_hands[h.index()]
            && live::held_kind(h).is_none()
            && !grenade::holds(h)
            && detonator != Some(h)
            && reload::holding_hand() != Some(h)
    });
    c4::input_tick(
        game,
        &c4::TickInputs {
            acting: pouches && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::C4Pouch)),
            triggers,
            placed_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::PlacedCharge)),
            opened,
            free: c4::wheel::WheelHands::available(free, unavailable, config.dominant).free,
            dominant: config.dominant,
        },
        C4.get(),
    );
    let reload_finish = reload::finish_tick(
        game,
        &reload::FinishInputs {
            grabs,
            grips,
            reload_button,
            fire_presses,
            dt: real_dt,
            game_dt: dt,
        },
        &config,
    );
    // A holding hand that opens while the other hand holds its gun's slide or bolt passes the gun
    // to that hand, rather than waiting for the action and dropping the gun once it lets go.
    let passes = passes_by_action(reload::action_held_by_other(), grips);
    let action_hands = waiting_on_action(reload::action_hands(), passes);
    let release_active =
        config.weapons.hold.grip_to_hold && capabilities.release && active.is_some();
    let released = if release_active {
        hold::act::release_tick(game, grips.map(ButtonControl::held), action_hands)
    } else {
        hold::act::cancel_releases();
        [false; 2]
    };
    if release_active {
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
                grips[hand.index()].held()
                    || hold::act::pressed_grips()[hand.index()] == Some(true),
                near,
                real_dt,
            );
            if !released[hand.index()] && !retry {
                continue;
            }
            if pending.is_none() && passes[hand.index()] && pass_by_action(game, hand) {
                continue;
            }
            let gun = lone.map(|(weapon, _)| weapon);
            release(game, hand, gun, active.as_ref(), world_scale);
        }
    }
    // SAFETY: the local player, in the game tick.
    let current =
        unsafe { LocalPlayer::get(game).and_then(|owner| engine::held_weapon(owner.character())) }
            .map(|weapon| with_holder(weapon, hold::owned_holder(weapon.address)));
    let gun_hands = current.map_or([false; 2], |weapon| {
        if weapon.wielded.slot == WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE {
            [false; 2]
        } else {
            weapon.wielded.hands
        }
    });
    let armed_hands = Hand::BOTH.map(|hand| armed::in_hand(hand).is_some());
    // A gun the reload holds cannot fire. The dual-wield split keeps a held pistol's trigger from
    // the game itself, so the fire input is held back only when no drawn gun may fire.
    let reload_blocks = gun_hands.contains(&true)
        && Hand::BOTH
            .iter()
            .all(|h| !gun_hands[h.index()] || reload_finish.held[h.index()].is_some());

    // Only a hand on the grip fires the gun.
    let holder = current.and_then(|weapon| weapon.holder);
    let docking = hold::act::docking();
    let gun_roles = gun_hands_now();
    let ready = if active.is_some() {
        tracked_hands()
    } else {
        [false; 2]
    };
    let claims = trigger_claims(capabilities, gun_roles, ready);
    let fire = HandFire {
        allowed: claims.map(|claim| claim.fire),
        triggers: &fire_presses,
        holder,
        docking,
        action_running: Hand::BOTH.map(weapon_action_running),
    };
    let fired = hand_fire(&fire);
    tick::publish_fire(fired);
    let refusals = refused_fire(&RefusedFire {
        fire: &fire,
        fired,
        reserved: claims.map(|claim| claim.reserved),
        roles: gun_roles,
        ready,
        may_fire: capabilities.fire,
        reload_held: reload_finish.held,
    });
    for (hand, refusal) in Hand::BOTH
        .into_iter()
        .filter_map(|hand| refusals[hand.index()].map(|r| (hand, r)))
    {
        haptics::play(hand, Event::DryFire);
        telemetry::report("trigger", Some(hand), "the trigger fired nothing", || {
            Some(refusal.label().to_owned())
        });
    }
    let explosive_blocked = Hand::BOTH.into_iter().any(|hand| {
        (grenade::holds(hand)
            || c4::throwing_hand() == Some(hand)
            || c4::detonator_hand() == Some(hand))
            && explosive_holds_fire(Some(hand), gun_hands, armed_hands)
    });
    let fire_blocked = reload_blocks
        || explosive_blocked
        || holder.is_some_and(|h| docking[h.index()])
        || primary_trigger_idle(current, &fire_presses)
        || primary_action_blocked(holder, Hand::BOTH.map(weapon_action_running));
    if fire_blocked {
        hold_fire_back(desired);
    }
    declare_grips(Hand::BOTH.map(|h| gun_hands[h.index()] || armed_hands[h.index()]));
    record_hands();

    let mut state = STATE.lock();
    state.status.fire_blocked = fire_blocked;
    state.status.grabs += events.len() as u64;
    state.status.events.extend(events);
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
    drop(state);
    let sequence = frame::with_latest(|frame| frame.sequence)?;
    reload_finish.belt_eligibility(sequence, &config, source_guns, gun_owners_now())
}

/// The target of the weapon handling's last tick that `fits` and that `hand` is nearest among those
/// it is within reach of, to dock what it lets go in.
fn docked_at(hand: Hand, fits: impl Fn(GrabVolume) -> bool) -> Option<GrabVolume> {
    let state = STATE.lock();
    dock_target(hand, &state.placed, state.points[hand.index()], fits)
}

/// Whether the fire input is held back for a grenade, a charge, or the detonator in the hand
/// `explosive`: with no gun in the other hand (the wielded weapon's in `gun_hands`, or one armed
/// there in `armed_hands`), the fire input would draw a gun into the hand that holds it.
fn explosive_holds_fire(
    explosive: Option<Hand>,
    gun_hands: [bool; 2],
    armed_hands: [bool; 2],
) -> bool {
    explosive.is_some_and(|hand| {
        let other = hand.other();
        !gun_hands[other.index()] && !armed_hands[other.index()]
    })
}

/// The fire inputs a weapon action holds back.
const FIRE_ACTIONS: [Action; 2] = [Action::FIRE_RIGHT, Action::R2];

/// How many grabs the readout keeps.
const MAX_EVENTS: usize = 6;

/// Declare what each hand holds of the weapons, for its finger pose: a gun by its grip in each hand
/// that holds one (`gun_hands`, the wielded weapon's and the armed ones'), the foregrip, a grenade
/// or a charge, the detonator, which is held as a gun's grip is, and a round for the reload.
fn declare_grips(gun_hands: [bool; 2]) {
    for hand in Hand::BOTH.into_iter().filter(|h| gun_hands[h.index()]) {
        grip::declare(hand, Grip::new(GripSource::Gun));
    }
    if let Some(two) = foregrip::two_hand_grip() {
        grip::declare(
            two.holder.other(),
            Grip::new(GripSource::Foregrip)
                .pose(two.region.finger_pose())
                .share(two.weight),
        );
    }
    for hand in Hand::BOTH.into_iter().filter(|hand| grenade::holds(*hand)) {
        grip::declare(hand, Grip::new(GripSource::Grenade));
    }
    if let Some(hand) = c4::throwing_hand() {
        grip::declare(hand, Grip::new(GripSource::C4));
    }
    if let Some(hand) = c4::detonator_hand() {
        grip::declare(hand, Grip::new(GripSource::Gun));
    }
    if let Some(hand) = reload::holding_hand() {
        grip::declare(hand, Grip::new(GripSource::Magazine));
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
    let gun = hand_pose::mul(holding.wrist, hold.geometry.gun);
    let anchor = regions::into_gun(hold.geometry.gun, hold.geometry.foregrip, world_scale);
    let turn = regions::turn_on_wrist(bracing.wrist.orientation, gun.orientation);
    if !anchor.is_finite() || !turn.is_finite() || !hold::hand_over(game, brace, weapon) {
        return false;
    }
    foregrip::clear();
    regions::grip_from_brace(weapon, brace, hold.region, turn, anchor, Instant::now());
    true
}

/// Record the hands on the long guns for the readout: each hand holding one at its grip, and the
/// bracing hand at its part.
fn record_hands() {
    let holders = Hand::BOTH.map(|hand| live::in_hand(hand).is_some());
    let brace = foregrip::status()
        .hold
        .filter(|h| h.engaged && live::in_hand(h.holder) == Some(h.weapon))
        .map(|h| (h.holder.other(), h.region));
    regions::record(regions::hands_on_guns(holders, brace));
}

/// Whether the fire input is held back because no hand holding the wielded weapon pulls its
/// trigger while another hand's does: the one fire action reaches the weapon from either trigger,
/// and only a hand holding the weapon may fire it. The other hand is empty, or holds a gun of its
/// own, which its trigger fires instead.
pub(super) fn primary_trigger_idle(
    weapon: Option<engine::HeldWeapon>,
    triggers: &[ButtonControl; 2],
) -> bool {
    weapon.is_some_and(|w| {
        let hands = w.wielded.hands;
        let pulled = |h: Hand| triggers[h.index()].held();
        hands.contains(&true)
            && Hand::BOTH.into_iter().any(pulled)
            && !Hand::BOTH
                .into_iter()
                .any(|h| hands[h.index()] && pulled(h))
    })
}

/// What decides each hand's own fire input.
pub(super) struct HandFire<'a> {
    pub allowed: [bool; 2],
    pub triggers: &'a [ButtonControl; 2],
    /// The hand holding the wielded weapon.
    pub holder: Option<Hand>,
    /// Each hand whose gun a dock's slide is moving onto its socket, by [`Hand::index`].
    pub docking: [bool; 2],
    /// A reload, a draw, or a holster plays with each hand, and holds its fire back, by
    /// [`Hand::index`].
    pub action_running: [bool; 2],
}

/// Each hand's own fire input, by [`Hand::index`]: its trigger pulled with permission for that
/// hand's firing grip while nothing holds its gun back. The true reload and shooting permission are checked at
/// the gun's update.
pub(super) fn hand_fire(f: &HandFire) -> [bool; 2] {
    Hand::BOTH.map(|hand| {
        let wielding = f.holder == Some(hand);
        f.allowed[hand.index()]
            && f.triggers[hand.index()].held()
            && !f.docking[hand.index()]
            && !(wielding && primary_action_blocked(f.holder, f.action_running))
    })
}

/// Why a hand's trigger press fired nothing, for the dead click and its log line.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) enum FireRefusal {
    /// The hand braces the gun; only the hand on the grip fires it.
    Bracing,
    /// The hand's controller is not tracked.
    Untracked,
    /// The mode allows no firing.
    Mode,
    /// A dock's slide is moving the gun onto its socket.
    Docking,
    /// A reload, a draw, or a holster plays with the hand.
    Action,
    /// The reload holds the gun, for its reason.
    Reload(FireHold),
    /// The hand holds a gun the mod's fire path does not fire.
    Other,
}

impl FireRefusal {
    pub(super) const fn label(self) -> &'static str {
        match self {
            FireRefusal::Bracing => "the hand braces the gun, and only the hand on the grip fires",
            FireRefusal::Untracked => "the controller is not tracked",
            FireRefusal::Mode => "firing is not allowed here",
            FireRefusal::Docking => "the gun is moving onto its socket",
            FireRefusal::Action => "a reload, draw, or holster is playing",
            FireRefusal::Reload(hold) => hold.label(),
            FireRefusal::Other => "the hand's gun is not fired from its trigger",
        }
    }
}

/// What decides whether a trigger press fired nothing, and why.
pub(super) struct RefusedFire<'a> {
    pub fire: &'a HandFire<'a>,
    /// Each hand's own fire input this tick ([`hand_fire`]).
    pub fired: [bool; 2],
    /// Each hand on some part of a gun, whose trigger belongs to it.
    pub reserved: [bool; 2],
    pub roles: [GunHand; 2],
    pub ready: [bool; 2],
    /// Whether the mode allows firing at all.
    pub may_fire: bool,
    /// Why the reload holds each hand's gun, when it does.
    pub reload_held: [Option<FireHold>; 2],
}

/// Each hand whose trigger was pressed this tick, on a gun it holds or braces, without firing it,
/// with the reason, by [`Hand::index`]: the one decision of the dead click. A press on an empty
/// hand, a hand holding something other than a gun, or the native pair's shared trigger on a gun
/// the reload does not hold is not refused here.
pub(super) fn refused_fire(r: &RefusedFire) -> [Option<FireRefusal>; 2] {
    Hand::BOTH.map(|hand| {
        let i = hand.index();
        let pressed = r.fire.triggers[i].pressed;
        if !pressed || !r.reserved[i] {
            return None;
        }
        if let Some(hold) = r.reload_held[i]
            && matches!(r.roles[i], GunHand::Grip | GunHand::NativePair(_))
        {
            return Some(FireRefusal::Reload(hold));
        }
        if matches!(r.roles[i], GunHand::NativePair(_)) {
            return None;
        }
        if r.fired[i] {
            return None;
        }
        Some(if !r.may_fire {
            FireRefusal::Mode
        } else if r.roles[i] == GunHand::Support {
            FireRefusal::Bracing
        } else if !r.ready[i] {
            FireRefusal::Untracked
        } else if r.fire.docking[i] {
            FireRefusal::Docking
        } else if r.fire.holder == Some(hand)
            && primary_action_blocked(r.fire.holder, r.fire.action_running)
        {
            FireRefusal::Action
        } else {
            FireRefusal::Other
        })
    })
}

fn primary_action_blocked(holder: Option<Hand>, action_running: [bool; 2]) -> bool {
    holder.is_some_and(|hand| action_running[hand.index()])
}

/// Drop the fire inputs from the tables' output.
pub(super) fn hold_fire_back(desired: &mut Vec<Desired>) {
    desired
        .retain(|d| !(d.target.map == MapKind::Player && FIRE_ACTIONS.contains(&d.target.action)));
}

/// Whether a reload, a draw, or a holster is playing with `gun_hand`, the hand holding the weapon
/// (or the hand the last one rode, during a draw), in the action-runs tier.
fn weapon_action_running(gun_hand: Hand) -> bool {
    let status = tiers::status();
    status.state == Some(TierState::WeaponAction)
        && status.effective[gun_hand.index()] == HandTier::ActionRuns
}

/// Carry out `hand`'s grab of `socket` as the table decides it: draw the weapon on it into the
/// empty hand, or dock the hand's weapon there. `None` for a grab that did nothing worth reporting:
/// a free socket with nothing to put there, or no table to decide against.
fn grab_socket(game: &GameTick<'_>, hand: Hand, socket: Socket) -> Option<GrabOutcome> {
    if hold::act::docking()[hand.index()] || hold::act::pending_release(hand).is_some() {
        return None;
    }
    if let Some(moved) = crate::controllers::weapons::grenade_moves::grab_socket(hand, socket) {
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
        Ok(SocketMove::Draw { .. }) => match hold::act::draw(game, hand, socket, hold::SNAP_S) {
            Ok(()) => SocketOutcome::Drew,
            Err(refusal) => SocketOutcome::Failed(refusal),
        },
        Ok(SocketMove::Dock { .. }) => match hold::act::dock(game, hand, socket, hold::SNAP_S) {
            Ok(()) => SocketOutcome::Docked,
            Err(refusal) => SocketOutcome::Failed(refusal),
        },
        Err(SocketRefusal::Empty(_)) => return None,
        Err(refusal) => {
            hold::refused(refusal);
            SocketOutcome::Refused(refusal)
        }
    };
    Some(GrabOutcome::Socket(outcome))
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
                    let travel = attract::travel_s(a.distance, hold::SNAP_S, &ATTRACTION);
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
                None => hold::act::dock(game, hand, socket, hold::SNAP_S),
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
            if let Err(refusal) = hold::act::let_go(game, hand, hold::MAX_THROW_MPS) {
                tracing::debug!(target: "controllers", %refusal, "hold: the weapon was not let go of");
            }
        }
        Release::Keep => {}
    }
}

fn bindings_closed(hand: Hand) -> bool {
    intent::grip_holds_now(hand) == Some(true)
}

fn holstering(hand: Hand, closed: bool) -> bool {
    hold::world::handover_blocked(
        hold::act::docking()[hand.index()],
        hold::act::pending_release(hand).is_some(),
        closed || bindings_closed(hand),
        intended_socket(hand).is_some(),
    )
}

fn intended_socket(hand: Hand) -> Option<Socket> {
    let snapshot = live::snapshot_now()?;
    docked_at(hand, |volume| matches!(volume, GrabVolume::Socket(socket) if grab::fits(&snapshot, hand, socket)))
        .and_then(|volume| match volume {
            GrabVolume::Socket(socket) => Some(socket),
            _ => None,
        })
}

fn retry_socket(
    pending: Option<hold::dock::ReleaseIntent>,
    over: Option<Socket>,
) -> Option<Socket> {
    over.filter(|socket| pending.is_none_or(|intent| intent.socket == Some(*socket)))
}

/// Which holding hands pass their gun on when they open, by [`Hand::index`]: the other hand holds
/// the gun's action by its handle (`action_held`), with its grip still closed on it.
fn passes_by_action(action_held: [bool; 2], grips: [ButtonControl; 2]) -> [bool; 2] {
    Hand::BOTH.map(|hand| action_held[hand.index()] && grips[hand.other().index()].held())
}

/// The hands whose release waits for the physical action (`action_hands`), less those that pass
/// their gun to the hand on the action.
fn waiting_on_action(action_hands: [bool; 2], passes: [bool; 2]) -> [bool; 2] {
    Hand::BOTH.map(|hand| action_hands[hand.index()] && !passes[hand.index()])
}

/// Pass the gun `hand` let go of to the other hand, which holds its action: the table moves it,
/// and the gun glides from where it was into the other hand's grip over [`hold::SNAP_S`], so it is
/// seen to move into the palm rather than jump there. The action lets go of its handle once the
/// hand on it holds the gun. `false` when the other hand could not take it, for the ordinary
/// release to decide.
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
        hold::act::slide_in(object, to, from, hold::SNAP_S);
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
/// reach: the hand's grip as it is drawn
/// against the tick's attractors, and the hand's fastest motion just before the release, which
/// must not be a throw. A hand without motion history counts as still.
/// The socket `hand`'s release is attracted to, leaving out `reserved`, the socket the other
/// hand's weapon is already on its way to.
fn attraction_for(hand: Hand, reserved: Option<Socket>) -> Option<attract::Attraction> {
    let speed = motion::latest_time()
        .and_then(|latest| {
            let since = latest - f64::from(ATTRACTION.speed_window_s);
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
    attract::attract(&open, hand, grip, speed, &ATTRACTION)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::weapons::capabilities::NativePair;

    #[test]
    fn a_pending_release_never_retargets_another_socket_or_redocks_after_timeout() {
        let left = Socket::Hip(Hand::Left);
        let right = Socket::Hip(Hand::Right);
        let intent = hold::dock::ReleaseIntent::new(7, Some(left));
        assert_eq!(retry_socket(None, Some(right)), Some(right));
        assert_eq!(retry_socket(Some(intent), Some(left)), Some(left));
        assert_eq!(retry_socket(Some(intent), Some(right)), None);
        assert_eq!(retry_socket(Some(intent), None), None);
        let dropping = hold::dock::ReleaseIntent::new(7, None);
        assert_eq!(retry_socket(Some(dropping), Some(left)), None);
    }

    #[test]
    fn a_holder_opening_while_the_other_hand_holds_the_action_passes_the_gun_at_once() {
        let grip = |down| ButtonControl {
            down,
            ..ButtonControl::default()
        };
        // The right hand holds the gun, and the left holds its slide with its grip closed.
        let action_held = [false, true];
        let passes = passes_by_action(action_held, [grip(true), grip(false)]);
        assert_eq!(passes, [false, true]);
        // The pass fires on the opening rather than waiting for the action to let go.
        assert_eq!(waiting_on_action([true, true], passes), [true, false]);
        // A hand that opened its grip on the slide passes nothing: the release waits as before.
        let open_on_slide = passes_by_action(action_held, [grip(false), grip(false)]);
        assert_eq!(open_on_slide, [false, false]);
        assert_eq!(waiting_on_action([true, true], open_on_slide), [true, true]);
    }

    #[test]
    fn each_hand_fires_from_its_own_trigger_unless_its_gun_is_held_back() {
        let pulled = |down| ButtonControl {
            down,
            ..ButtonControl::default()
        };
        let triggers = [pulled(true), pulled(true)];
        let free = HandFire {
            allowed: [true; 2],
            triggers: &triggers,
            holder: Some(Hand::Right),
            docking: [false; 2],
            action_running: [false; 2],
        };
        assert_eq!(hand_fire(&free), [true, true]);
        assert_eq!(
            hand_fire(&HandFire {
                docking: [false, true],
                ..free
            }),
            [true, false],
            "a dock's slide holds only its own hand's gun"
        );
        assert_eq!(
            hand_fire(&HandFire {
                allowed: [false; 2],
                ..free
            }),
            [false, false]
        );
        let idle = [pulled(false), pulled(true)];
        assert_eq!(
            hand_fire(&HandFire {
                triggers: &idle,
                ..free
            }),
            [false, true]
        );
    }

    #[test]
    fn a_trigger_press_that_fires_nothing_is_refused_once_with_its_reason() {
        let press = |pressed| ButtonControl {
            down: pressed,
            pressed,
            ..ButtonControl::default()
        };
        let triggers = [press(true), press(true)];
        let fire = HandFire {
            allowed: [false, true],
            triggers: &triggers,
            holder: Some(Hand::Right),
            docking: [false; 2],
            action_running: [false; 2],
        };
        // The right hand holds the grip and fires; the left braces.
        let refused = RefusedFire {
            fire: &fire,
            fired: hand_fire(&fire),
            reserved: [true; 2],
            roles: [GunHand::Support, GunHand::Grip],
            ready: [true; 2],
            may_fire: true,
            reload_held: [None; 2],
        };
        assert_eq!(refused_fire(&refused), [Some(FireRefusal::Bracing), None]);
        // A trigger held down from an earlier tick is not refused again.
        let held = [
            ButtonControl {
                down: true,
                ..ButtonControl::default()
            },
            press(false),
        ];
        let fire_held = HandFire {
            triggers: &held,
            ..fire
        };
        assert_eq!(
            refused_fire(&RefusedFire {
                fire: &fire_held,
                fired: hand_fire(&fire_held),
                ..refused
            }),
            [None, None]
        );
        // An empty hand's press is the game's.
        assert_eq!(
            refused_fire(&RefusedFire {
                reserved: [false, true],
                roles: [GunHand::Empty, GunHand::Grip],
                ..refused
            }),
            [None, None]
        );
        let docking = HandFire {
            docking: [false, true],
            ..fire
        };
        assert_eq!(
            refused_fire(&RefusedFire {
                fire: &docking,
                fired: hand_fire(&docking),
                ..refused
            })[Hand::Right.index()],
            Some(FireRefusal::Docking)
        );
        assert_eq!(
            refused_fire(&RefusedFire {
                ready: [true, false],
                fired: [false; 2],
                ..refused
            })[Hand::Right.index()],
            Some(FireRefusal::Untracked)
        );
        assert_eq!(
            refused_fire(&RefusedFire {
                reload_held: [None, Some(FireHold::Empty)],
                ..refused
            })[Hand::Right.index()],
            Some(FireRefusal::Reload(FireHold::Empty))
        );
        assert_eq!(
            FireRefusal::Reload(FireHold::ActionOpen).label(),
            FireHold::ActionOpen.label(),
            "the click's line says what the reload checked"
        );
        // The native pair's shared trigger is the game's, unless the reload holds the gun.
        let pair = GunHand::NativePair(NativePair {
            first_hand: Hand::Right,
        });
        let native = RefusedFire {
            roles: [pair; 2],
            ..refused
        };
        assert_eq!(refused_fire(&native), [None, None]);
        assert_eq!(
            refused_fire(&RefusedFire {
                reload_held: [Some(FireHold::MagazineOut); 2],
                ..native
            }),
            [Some(FireRefusal::Reload(FireHold::MagazineOut)); 2]
        );
    }

    #[test]
    fn current_primary_holder_owns_the_action_block_after_same_tick_moves() {
        let source = include_str!("finish.rs");
        let compact: String = source.split_whitespace().collect();
        assert!(compact.contains(
            "letcurrent=unsafe{LocalPlayer::get(game).and_then(|owner|engine::held_weapon(owner.character()))}"
        ));
        assert!(source.contains("primary_trigger_idle(current, &fire_presses)"));
        let triggers = [ButtonControl {
            down: true,
            ..Default::default()
        }; 2];
        for current in Hand::BOTH {
            let old = current.other();
            let running = Hand::BOTH.map(|hand| hand == current);
            let fire = HandFire {
                allowed: [true; 2],
                triggers: &triggers,
                holder: Some(current),
                docking: [false; 2],
                action_running: running,
            };
            assert!(
                !hand_fire(&fire)[current.index()],
                "the new primary remains held by its action"
            );
            assert!(
                hand_fire(&fire)[old.index()],
                "the previous holder's other gun is independent"
            );
            assert!(primary_action_blocked(Some(current), running));
            assert!(!primary_action_blocked(Some(old), running));
            assert!(
                !primary_action_blocked(None, running),
                "dropping the primary removes its action block"
            );
        }
    }

    #[test]
    fn a_grenade_in_the_hand_holds_fire_back_unless_the_other_hand_has_a_gun() {
        assert!(!explosive_holds_fire(None, [false; 2], [false; 2]));
        assert!(explosive_holds_fire(
            Some(Hand::Left),
            [false; 2],
            [false; 2]
        ));
        // The gun in the hand that now holds the grenade does not count.
        assert!(explosive_holds_fire(
            Some(Hand::Left),
            [true, false],
            [false; 2]
        ));
        assert!(!explosive_holds_fire(
            Some(Hand::Left),
            [false, true],
            [false; 2]
        ));
        assert!(!explosive_holds_fire(
            Some(Hand::Left),
            [false; 2],
            [false, true]
        ));
    }
}
