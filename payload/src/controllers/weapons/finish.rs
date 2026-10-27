//! The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
//! throw and the reload, and hold the fire inputs back during a weapon action.

use std::time::{Duration, Instant};

use jc3gi::{
    character::inventory::WeaponSlot, equipment::weapon::WeaponBase,
    input::input_action_map::Action,
};

use crate::{
    config::Config,
    controllers::{
        bindings::{
            controls::ButtonControl,
            hold::{Desired, MapKind},
        },
        config::ControllersConfig,
        fingers::grip::{self, Grip, GripSource},
        frame::{self, Hand},
        grab::{GrabVolume, resolve::dock_target},
        hands::{
            self, ActiveHands, pose as hand_pose,
            tiers::{self, HandTier, TierState},
        },
        haptics::{self, Event},
        loadout::{
            Socket,
            grab::{self, GrabRules, Release, SocketMove, SocketRefusal},
            live,
        },
        mode,
        weapons::{
            begin::with_holder,
            c4,
            capabilities::{
                Capabilities, gun_hands_now, gun_owners_now, tracked_hands, trigger_claims,
            },
            dual_wield::single,
            engine,
            foregrip::{self, GripGeometry, Inputs, Transition, TwoHandedWeapon},
            grenade,
            hold::{self, armed, tick},
            inventory::engine::slot_of,
            regions::{self, Carry, CarryCause, GunRegion},
            reload,
            state::STATE,
            types::{GrabEvent, GrabOutcome, SocketOutcome},
            weighted_hands,
        },
    },
};

/// What the bindings hand the second half of the weapon handling's input tick.
pub(crate) struct FinishInputs {
    /// The grab volume each hand grabbed this tick, indexed by [`Hand::index`].
    pub grabs: [Option<GrabVolume>; 2],
    /// Each hand's grip over the tick.
    pub grips: [ButtonControl; 2],
    /// Each hand's trigger press over the tick.
    pub triggers: [ButtonControl; 2],
    /// The reload action's press over the tick.
    pub reload_button: ButtonControl,
    /// The engine's tick delta, seconds.
    pub dt: f32,
}

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw and the reload, and hold the fire inputs back during a weapon action. `desired` is the
/// tables' output, which the grenade throw adds to and the fire hold-back trims.
pub(crate) fn finish_tick(
    inputs: &FinishInputs,
    desired: &mut Vec<Desired>,
) -> Option<reload::BeltEligibility> {
    let FinishInputs {
        grabs,
        grips,
        triggers,
        reload_button,
        dt,
    } = *inputs;
    let config = Config::lock_query(|c| c.controllers);
    let active = hands::active_hands_raw();
    let capabilities = Capabilities::of(mode::current().mode);
    let now = Instant::now();
    let source_guns = gun_owners_now();
    // The fingers opening lets a grenade or a charge go, not the grip's press relaxing.
    let values = frame::with_latest(|f| {
        Hand::BOTH.map(|h| {
            let hand = f.hand(h);
            (f.focused && hand.active).then_some(hand.inputs.squeeze)
        })
    })
    .unwrap_or([None; 2]);
    let opened = STATE.lock().opening.step(values, grips);
    let (weapon, lone, gameplay, gun_side, touch, slide, promote, docked_touch) = {
        let state = STATE.lock();
        (
            state.weapon,
            state.lone,
            state.gameplay,
            state.gun_side,
            state.touch,
            state.slide,
            state.promote,
            state.docked_touch,
        )
    };
    let world_scale = active.as_ref().map_or(1.0, |a| a.world_scale);
    // A bracing hand that slid onto the grip of a gun the other hand carries takes the grip.
    let promoted = match (promote, lone) {
        (Some(hand), Some((w, _))) => take_grip(w.address, hand, &grips, now),
        _ => false,
    };

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
    // The off-hand braces where it touched the gun, or where it held the gun before the other hand
    // took its grip.
    let rebrace = two_handed.and_then(|w| {
        let geometry = w.geometry?;
        let off = w.holder.other();
        // The geometry is the new hand's only once the gun has been placed on it.
        let placed = hands::grip::status().hands[w.holder.index()]
            .is_some_and(|g| g.weapon == w.address && g.pinned);
        if !placed {
            return None;
        }
        if !grips[off.index()].held() {
            regions::drop_rebrace();
            return None;
        }
        let wait =
            Duration::try_from_secs_f32(config.weapons.regions.rebrace_s).unwrap_or(Duration::ZERO);
        let (region, anchor) = regions::take_rebrace(w.address, off, now, wait)?;
        Some((
            region,
            regions::out_of_gun(geometry.gun, anchor, world_scale),
        ))
    });
    let at_touch = touch
        .filter(|t| foregrip_grab == Some(t.hand) && t.hit.region != GunRegion::Grip)
        .map(|t| (t.hit.region, t.anchor_in_wrist));
    let two_handed = two_handed.map(|w| match rebrace.or(at_touch) {
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
    let inputs = Inputs {
        active: config.weapons.foregrip.enabled && active.is_some(),
        hands: active.as_ref().map_or([None; 2], |a| a.hands),
        weapon: two_handed,
        // The hand that just took the grip does not brace with the same press.
        grab_pressed: !promoted
            && (rebrace.is_some()
                || foregrip_grab
                    .is_some_and(|hand| two_handed.is_some_and(|w| w.holder.other() == hand))),
        grip_held: two_handed.is_some_and(|w| grips[w.holder.other().index()].held()),
        slide,
    };
    let transition = foregrip::tick(&inputs, dt, &config.weapons.foregrip);
    if let (Some(Transition::Grabbed), Some(w)) = (transition, two_handed)
        && config.weapons.regions.enabled
    {
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
                let moved =
                    crate::controllers::weapons::grenade_moves::transfer(hand.other(), hand)
                        .unwrap_or_else(|| match gun {
                            Some(w) if regions::carrying(w).is_some() => {
                                take_grip(w, hand, &grips, now)
                            }
                            Some(w) => hold::hand_over(hand, w),
                            None => false,
                        });
                if moved {
                    // The hand that took the gun let go of the foregrip to take it.
                    foregrip::clear();
                }
                GrabOutcome::Handover { moved }
            }
            GrabVolume::Foregrip => match transition {
                Some(t) => GrabOutcome::Foregrip(t),
                None => GrabOutcome::ForegripUnmeasured,
            },
            GrabVolume::GrenadePouch | GrabVolume::GrenadeLocker => GrabOutcome::Pouch,
            GrabVolume::LiveGrenade => GrabOutcome::Catch,
            GrabVolume::MagazinePouch
            | GrabVolume::WorldMagazine
            | GrabVolume::InsertedMagazine => GrabOutcome::Magazine,
            GrabVolume::WeaponAction => GrabOutcome::Action,
            GrabVolume::C4Pouch => GrabOutcome::C4,
            GrabVolume::WorldWeapon => {
                let Some(active) = active.as_ref() else {
                    continue;
                };
                let inputs = hold::take::TakeInputs {
                    active,
                    settings: &config.weapons.hold,
                    free: [true; 2],
                    gameplay,
                };
                // SAFETY: the input tick runs on the main thread outside the world update.
                let taken = unsafe { hold::take::finish_grab(hand, &inputs) };
                GrabOutcome::Take { taken }
            }
            GrabVolume::Socket(socket) => {
                // A draw at a docked gun's barrel or stock takes the gun there.
                match docked_touch[hand.index()].filter(|t| t.site == socket) {
                    Some(touched) => regions::pend_draw(touched.weapon, hand, touched.carry, now),
                    None => regions::cancel_draw(hand),
                }
                match grab_socket(hand, socket, &config) {
                    Some(outcome) => outcome,
                    None => continue,
                }
            }
            // The other subsystems' grabs: the bindings hand the weapon handling only its own.
            GrabVolume::MagazineWell
            | GrabVolume::ChargeSurface
            | GrabVolume::VehicleControl(_)
            | GrabVolume::CockpitButton(_)
            | GrabVolume::ParachuteHandle(_)
            | GrabVolume::MountedHandle(_)
            | GrabVolume::Pickup(_)
            | GrabVolume::Climb
            | GrabVolume::StuntHold
            | GrabVolume::Snatch(_) => {
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
                matches!(volume, GrabVolume::Socket(socket) if matches!(socket, Socket::Hip(_) | Socket::Chest))
            }).and_then(|volume| match volume {
                GrabVolume::Socket(socket) => Some(socket),
                _ => None,
            });
            if crate::controllers::weapons::grenade_moves::stow_before_open(hand, socket) == Some(true) {
                haptics::play(hand, Event::VolumeGrab);
                return false;
            }
        }
        opened[hand.index()]
    });
    grenade::input_tick(
        &grenade::TickInputs {
            acting: config.weapons.grenade_throw && gameplay && active.is_some(),
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
            dominant: config.acting_hand(),
            opened: grenade_opened,
            world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
            dt,
        },
        &config,
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
    let free = Hand::BOTH.map(|h| {
        !gun_hands[h.index()]
            && !armed_hands[h.index()]
            && live::held_kind(h).is_none()
            && !grenade::holds(h)
            && reload::holding_hand() != Some(h)
    });
    c4::input_tick(
        &c4::TickInputs {
            acting: config.weapons.c4_throw && gameplay && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::C4Pouch)),
            opened,
            free,
            dominant: config.acting_hand(),
            world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
        },
        &config.weapons.c4,
    );
    c4::wrist::input_tick(
        &c4::wrist::TickInputs {
            acting: config.weapons.wrist_detonator && active.is_some(),
            presser: gun_side,
            dt,
        },
        &config,
    );
    let reload_finish = reload::finish_tick(
        &reload::FinishInputs {
            grabs,
            grips,
            triggers,
            reload_button,
            dt,
        },
        &config,
    );
    let action_hands = reload::action_hands();
    let release_active =
        config.weapons.hold.grip_to_hold && capabilities.release && active.is_some();
    let released = if release_active {
        hold::act::release_tick(grips.map(ButtonControl::held), action_hands)
    } else {
        hold::act::cancel_releases();
        [false; 2]
    };
    if release_active {
        for hand in Hand::BOTH.into_iter().filter(|hand| released[hand.index()]) {
            let gun = lone.map(|(weapon, _)| weapon);
            release(hand, gun, active.as_ref(), world_scale, &config);
        }
    }
    // SAFETY: actions have completed on the input thread, outside the world update.
    let current = unsafe { engine::local_player().and_then(|owner| engine::held_weapon(owner)) }
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
            .all(|h| !gun_hands[h.index()] || reload_finish.held[h.index()]);

    // Only a hand on the grip fires the gun.
    let carries = regions::carries();
    let carried = current.and_then(|weapon| regions::carrying(weapon.address));
    let holder = current.and_then(|weapon| weapon.holder);
    let docking = hold::act::docking();
    let gun_roles = gun_hands_now();
    let ready = if active.is_some() {
        tracked_hands()
    } else {
        [false; 2]
    };
    let claims = trigger_claims(capabilities, gun_roles, ready);
    tick::publish_fire(hand_fire(&HandFire {
        allowed: claims.map(|claim| claim.fire),
        triggers: &triggers,
        holder,
        carrying: carries.map(|c| c.is_some()),
        docking,
        action_running: Hand::BOTH
            .map(|h| config.weapons.block_fire_during_actions && weapon_action_running(h)),
    }));
    let explosive_blocked = Hand::BOTH.into_iter().any(|hand| {
        (grenade::holds(hand) || c4::throwing_hand() == Some(hand))
            && explosive_holds_fire(Some(hand), gun_hands, armed_hands)
    });
    let fire_blocked = reload_blocks
        || explosive_blocked
        || carried.is_some()
        || holder.is_some_and(|h| docking[h.index()])
        || primary_trigger_idle(current, &triggers)
        || primary_action_blocked(
            holder,
            Hand::BOTH.map(|hand| {
                config.weapons.block_fire_during_actions && weapon_action_running(hand)
            }),
        );
    if fire_blocked {
        hold_fire_back(desired);
    }
    declare_grips(
        Hand::BOTH.map(|h| gun_hands[h.index()] || armed_hands[h.index()]),
        carries,
    );
    record_hands(&config);

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

/// Whether the fire input is held back for a grenade or a charge in the hand `explosive`: with no
/// gun in the other hand (the wielded weapon's in `gun_hands`, or one armed there in
/// `armed_hands`), the fire input would draw a gun into the hand that holds it.
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

/// Declare what each hand holds of the weapons, for its finger pose: a gun in each hand that holds
/// one (`gun_hands`, the wielded weapon's and the armed ones'), by the part each hand carries its
/// gun by (`carries`), the foregrip, a grenade or a charge, and a round for the reload.
fn declare_grips(gun_hands: [bool; 2], carries: [Option<Carry>; 2]) {
    for hand in Hand::BOTH.into_iter().filter(|h| gun_hands[h.index()]) {
        let grip = match carries[hand.index()] {
            Some(carry) => Grip::new(GripSource::Gun).pose(carry.region.finger_pose()),
            None => Grip::new(GripSource::Gun),
        };
        grip::declare(hand, grip);
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
    if let Some(hand) = reload::holding_hand() {
        grip::declare(hand, Grip::new(GripSource::Magazine));
    }
}

/// Hand `weapon` to `hand`, which takes its grip: a gun the other hand carried by another part is
/// braced by that hand where it carried it, once the gun is measured in its new hand, while its
/// grip stays held. Returns whether the gun moved; for a gun held at its grip, this is a plain
/// handover.
fn take_grip(weapon: usize, hand: Hand, grips: &[ButtonControl; 2], now: Instant) -> bool {
    let carried = regions::carrying(weapon).filter(|(carrier, _)| *carrier == hand.other());
    if !hold::hand_over(hand, weapon) {
        return false;
    }
    foregrip::clear();
    if let Some((carrier, carry)) = carried {
        regions::end_carry(weapon, "the other hand took the grip");
        if grips[carrier.index()].held() {
            regions::took_grip(weapon, hand, carry.region, carry.anchor, now);
        }
    }
    true
}

/// Leave the long gun `weapon`, whose holding hand `holder` let go, in the hand that braces it,
/// carried where that hand holds it. Returns whether it did.
fn leave_in_brace(weapon: usize, holder: Hand, active: &ActiveHands, world_scale: f32) -> bool {
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
    let carry = Carry {
        region: hold.region,
        anchor: regions::into_gun(hold.geometry.gun, hold.geometry.foregrip, world_scale),
        turn: regions::turn_on_wrist(bracing.wrist.orientation, gun.orientation),
    };
    if !carry.anchor.is_finite() || !carry.turn.is_finite() || !hold::hand_over(brace, weapon) {
        return false;
    }
    foregrip::clear();
    regions::begin_carry(weapon, brace, carry, CarryCause::LetGo);
    true
}

/// Record the hands on the long guns for the readout: each hand holding one at its grip or the part
/// it carries it by, and the bracing hand at its part.
fn record_hands(config: &ControllersConfig) {
    if !config.weapons.regions.enabled {
        regions::record([None; 2]);
        return;
    }
    let carries = regions::carries();
    let holders = Hand::BOTH.map(|hand| {
        live::in_hand(hand).map(|_| carries[hand.index()].map_or(GunRegion::Grip, |c| c.region))
    });
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
    /// Each hand carrying its gun by a part other than its grip, which cannot fire it, by
    /// [`Hand::index`].
    pub carrying: [bool; 2],
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
            && !f.carrying[hand.index()]
            && !f.docking[hand.index()]
            && !(wielding && primary_action_blocked(f.holder, f.action_running))
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
fn grab_socket(hand: Hand, socket: Socket, config: &ControllersConfig) -> Option<GrabOutcome> {
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
    let settings = &config.weapons.hold;
    let rules = GrabRules {
        both_hands: settings.either_hand && settings.second_weapon,
    };
    let outcome = match grab::grab(&snapshot, hand, socket, rules) {
        // SAFETY: the input tick runs on the main thread outside the world update.
        Ok(SocketMove::Draw { .. }) => {
            match unsafe { hold::act::draw(hand, socket, settings.snap_s) } {
                Ok(()) => SocketOutcome::Drew,
                Err(refusal) => SocketOutcome::Failed(refusal),
            }
        }
        // SAFETY: as above.
        Ok(SocketMove::Dock { .. }) => {
            match unsafe { hold::act::dock(hand, socket, settings.snap_s) } {
                Ok(()) => SocketOutcome::Docked,
                Err(refusal) => SocketOutcome::Failed(refusal),
            }
        }
        Err(SocketRefusal::Empty(_)) => return None,
        Err(refusal) => {
            tracing::debug!(target: "controllers", %refusal, "hold: the socket grab did nothing");
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
    hand: Hand,
    weapon: Option<engine::HeldWeapon>,
    active: Option<&ActiveHands>,
    world_scale: f32,
    config: &ControllersConfig,
) {
    let Some(snapshot) = live::snapshot_now() else {
        return;
    };
    let Some(object) = live::in_hand(hand) else {
        return;
    };
    // SAFETY: the input tick runs on the main thread outside the world update, and a slot or the
    // carry set keeps the table's weapon in the hand alive.
    let dockable = unsafe { hold::act::dockable(object) };
    let over = docked_at(
        hand,
        |v| matches!(v, GrabVolume::Socket(s) if dockable && grab::fits(&snapshot, hand, s)),
    )
    .and_then(|v| match v {
        GrabVolume::Socket(socket) => Some(socket),
        _ => None,
    });
    let settings = &config.weapons.hold;
    // SAFETY: the input tick runs on the main thread outside the world update.
    match grab::release(&snapshot, hand, over, unsafe { droppable(object) }) {
        Release::Dock(socket) => {
            // SAFETY: as above.
            match unsafe { hold::act::dock(hand, socket, settings.snap_s) } {
                // A dock by opening the grip confirms as a dock by a grab does.
                Ok(()) => haptics::play(hand, Event::VolumeGrab),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the weapon was not docked");
                }
            }
        }
        Release::LetGo => {
            // A gun the other hand braces stays in that hand, carried where it holds it.
            let braced = config.weapons.regions.enabled
                && weapon.is_some_and(|w| w.address == object)
                && active.is_some_and(|a| leave_in_brace(object, hand, a, world_scale));
            if braced {
                return;
            }
            // SAFETY: as above.
            if let Err(refusal) = unsafe { hold::act::let_go(hand, settings.max_throw_mps) } {
                tracing::debug!(target: "controllers", %refusal, "hold: the weapon was not let go of");
            }
        }
        Release::Keep => {}
    }
}

/// Whether the weapon at `object` can lie in the world: every weapon but the game's dual-wield pair,
/// one object of two guns, which a single pistol is not.
///
/// # Safety
///
/// `object` must be a weapon a slot or the carry set keeps alive, on the main thread outside the
/// world update.
unsafe fn droppable(object: usize) -> bool {
    // SAFETY: forwarded from the caller.
    let slot = unsafe { (object as *const WeaponBase).as_ref() }
        .and_then(|weapon| unsafe { slot_of(weapon) });
    slot != Some(WeaponSlot::E_SLOT_DUAL_WIELD) || single::is(object)
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
            carrying: [false; 2],
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
                carrying: [true, false],
                ..free
            }),
            [false, true]
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
    fn current_primary_holder_owns_the_action_block_after_same_tick_moves() {
        let source = include_str!("finish.rs");
        let compact: String = source.split_whitespace().collect();
        assert!(compact.contains(
            "letcurrent=unsafe{engine::local_player().and_then(|owner|engine::held_weapon(owner))}"
        ));
        assert!(source.contains("primary_trigger_idle(current, &triggers)"));
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
                carrying: [false; 2],
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
