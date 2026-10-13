//! The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
//! throw and the reload, and hold the fire inputs back during a weapon action.

use std::time::{Duration, Instant};

use jc3gi::{character::inventory::WeaponSlot, input::input_action_map::Action};

use crate::{
    config::Config,
    controllers::{
        bindings::{
            controls::ButtonControl,
            hold::{Desired, MapKind, Target, Want},
        },
        config::ControllersConfig,
        fingers::grip::{self, Grip, GripSource},
        frame::{self, Hand},
        grab::{GrabVolume, HolsterSite, resolve::dock_target},
        hands::{
            self, ActiveHands, pose as hand_pose,
            tiers::{self, HandTier, TierState},
        },
        haptics::{self, Event},
        weapons::{
            c4,
            dual_wield::{
                self,
                single::{
                    self,
                    hips::{HipGrab, HipView},
                },
            },
            engine,
            foregrip::{self, GripGeometry, Inputs, Transition, TwoHandedWeapon},
            grenade,
            hold::{
                self,
                dock::{self, Release},
                second::{self, Source},
            },
            holster::{self, HolsterAction},
            inventory::{
                actions::{self, SocketGrab, SocketRefusal},
                carry,
                decide::{self, ReleasedAt, SecondRelease},
                sockets::SocketId,
            },
            regions::{self, Carry, CarryCause, GunRegion},
            reload,
            state::STATE,
            types::{GrabEvent, GrabOutcome},
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
/// tables' output, which a draw adds its slot-select click to.
pub(crate) fn finish_tick(inputs: &FinishInputs, desired: &mut Vec<Desired>) {
    let FinishInputs {
        grabs,
        grips,
        triggers,
        reload_button,
        dt,
    } = *inputs;
    let config = Config::lock_query(|c| c.controllers);
    let active = hands::active_hands_raw();
    let now = Instant::now();
    // The fingers opening lets a grenade or a charge go, not the grip's press relaxing.
    let values = frame::with_latest(|f| {
        Hand::BOTH.map(|h| {
            let hand = f.hand(h);
            (f.focused && hand.active).then_some(hand.inputs.squeeze)
        })
    })
    .unwrap_or([None; 2]);
    let opened = STATE.lock().opening.step(values, grips);
    let (weapon, gameplay, gun_side, touch, slide, promote, docked_touch) = {
        let state = STATE.lock();
        (
            state.weapon,
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
    let promoted = match (promote, weapon) {
        (Some(hand), Some((w, _))) => take_grip(w.address, hand, &grips, now),
        _ => false,
    };

    let foregrip_grab = grabs
        .iter()
        .zip(Hand::BOTH)
        .find(|(g, _)| **g == Some(GrabVolume::Foregrip))
        .map(|(_, hand)| hand);
    let two_handed = weapon
        .filter(|(w, _)| w.two_handed && gameplay)
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
            GrabVolume::Holster(site) => {
                // A draw at a docked gun's barrel or stock takes the gun there.
                match docked_touch[hand.index()].filter(|t| t.site == site) {
                    Some(touched) => regions::pend_draw(touched.weapon, hand, touched.carry, now),
                    None => regions::cancel_draw(hand),
                }
                let wielded = weapon.map(|(w, _)| w.wielded);
                // A hip with single pistols is the hip of the pistol that sits there.
                let hip = match site {
                    HolsterSite::Hip(side) if single::hips_single() => {
                        Some(single_hip_grab(side, hand, weapon.map(|(w, _)| w), &config))
                    }
                    _ => None,
                };
                match hip {
                    Some(HipGrab::Socket(socket)) => {
                        match grab_socket(hand, socket, weapon.map(|(w, _)| w), &config) {
                            Some(outcome) => outcome,
                            None => continue,
                        }
                    }
                    Some(HipGrab::Nothing) => continue,
                    Some(HipGrab::Rehome) => {
                        let (Some(w), HolsterSite::Hip(side)) = (weapon.map(|(w, _)| w), site)
                        else {
                            continue;
                        };
                        // SAFETY: the wielded weapon was read this tick, on the main thread outside
                        // the world update.
                        unsafe { single::rehome(w.address, side) };
                        let holster = HolsterAction::Holster {
                            slot: WeaponSlot::E_SLOT_DUAL_WIELD,
                        };
                        act(holster, hand, &config, desired)
                    }
                    Some(HipGrab::Slot) | None => {
                        let independent = hip.is_none()
                            && config.weapons.dual_wield.split
                            && config.weapons.dual_wield.independent_draw;
                        let second_weapon = holster::Second {
                            enabled: config.weapons.hold.either_hand
                                && config.weapons.hold.second_weapon,
                            held: second::hand().zip(second::slot()),
                        };
                        match holster::decide(
                            site,
                            hand,
                            config.acting_hand(),
                            wielded,
                            independent,
                            second_weapon,
                        ) {
                            Some(action) => act(action, hand, &config, desired),
                            None => continue,
                        }
                    }
                }
            }
            GrabVolume::Handle => {
                let carried = weapon.and_then(|(w, _)| regions::carrying(w.address).map(|_| w));
                let moved = match carried {
                    Some(w) => take_grip(w.address, hand, &grips, now),
                    None => hold::hand_over(hand),
                };
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
            GrabVolume::GrenadePouch => GrabOutcome::Pouch,
            GrabVolume::GrenadePin => GrabOutcome::Pin,
            GrabVolume::LiveGrenade => GrabOutcome::Catch,
            GrabVolume::MagazinePouch | GrabVolume::WorldMagazine => GrabOutcome::Magazine,
            GrabVolume::WeaponAction => GrabOutcome::Action,
            GrabVolume::C4Pouch => GrabOutcome::C4,
            GrabVolume::WorldWeapon => {
                let Some(active) = active.as_ref() else {
                    continue;
                };
                let dual = config.weapons.dual_wield;
                let inputs = hold::take::TakeInputs {
                    active,
                    settings: &config.weapons.hold,
                    free: [true; 2],
                    independent_pistols: dual.split && dual.independent_draw,
                    gameplay,
                };
                // SAFETY: the input tick runs on the main thread outside the world update.
                let taken = unsafe { hold::take::finish_grab(hand, &inputs) };
                GrabOutcome::Take { taken }
            }
            GrabVolume::Socket(socket) => {
                match grab_socket(hand, socket, weapon.map(|(w, _)| w), &config) {
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
            | GrabVolume::StuntHold => {
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
    let holder = weapon.and_then(|(w, _)| w.holder);
    let released = hold::act::release_tick(holder, grips.map(ButtonControl::held))
        .filter(|_| config.weapons.hold.grip_to_hold && gameplay);
    if let (Some(hand), Some((w, _))) = (released, weapon) {
        let single_pistol = single::is(w.address);
        let at_own = |site| match site {
            // A single pistol goes back to its own hip, or to a free one, which becomes its own.
            HolsterSite::Hip(side) if single_pistol => {
                single::slot_home() == Some(side)
                    || !carry::taken().contains(&single::hips::hip_socket(side))
            }
            _ => holster::site_slot(site, config.acting_hand()) == w.wielded.slot,
        };
        // A free socket that fits takes the gun, or its own holster does, whichever is nearer.
        let dock = docked_at(hand, |v| match v {
            GrabVolume::Socket(socket) => carry::on_socket(socket).is_none(),
            GrabVolume::Holster(site) => at_own(site),
            _ => false,
        });
        let stowed = match dock {
            Some(GrabVolume::Socket(socket)) => {
                // SAFETY: the input tick runs on the main thread outside the world update.
                unsafe { engine::local_player() }.is_some_and(|owner| {
                    unsafe { actions::stow(owner, hand, socket, w.wielded.hands == [true; 2]) }
                        .inspect_err(|refusal| {
                            tracing::debug!(target: "controllers", %refusal, "inventory: the gun was not put onto the socket");
                        })
                        .is_ok()
                })
            }
            _ => false,
        };
        let at_own_holster = matches!(dock, Some(GrabVolume::Holster(_)));
        if let Some(GrabVolume::Holster(HolsterSite::Hip(side))) = dock
            && single_pistol
            && single::slot_home() != Some(side)
        {
            // SAFETY: the wielded weapon was read this tick, on the main thread outside the world
            // update.
            unsafe { single::rehome(w.address, side) };
        }
        // The game's pair cannot fall, since it is one weapon; a single pistol is a weapon alone.
        let droppable = w.wielded.slot != WeaponSlot::E_SLOT_DUAL_WIELD || single_pistol;
        // A gun the other hand braces stays in that hand, carried where it holds it.
        let left_in_brace = !stowed
            && !at_own_holster
            && config.weapons.regions.enabled
            && active
                .as_ref()
                .is_some_and(|a| leave_in_brace(w.address, hand, a, world_scale));
        if !stowed && !left_in_brace {
            release(
                hand,
                dock::on_release(at_own_holster, droppable),
                w.wielded,
                &config,
            );
        }
    }

    let second_released = second::release_tick(grips.map(ButtonControl::held))
        .filter(|_| config.weapons.hold.grip_to_hold && gameplay);
    if let Some(hand) = second_released {
        let carried_second = matches!(second::source(), Some(Source::Carried(_)));
        let own_slot = second::slot();
        let dock = docked_at(hand, |v| match v {
            GrabVolume::Socket(_) => true,
            GrabVolume::Holster(site) => {
                carried_second || own_slot == Some(holster::site_slot(site, config.acting_hand()))
            }
            _ => false,
        });
        // SAFETY: the input tick runs on the main thread outside the world update.
        unsafe { release_second(hand, dock) };
    }

    grenade::input_tick(
        &grenade::TickInputs {
            acting: config.weapons.grenade_throw && gameplay && active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePouch)),
            pin_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::GrenadePin)),
            catch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::LiveGrenade)),
            cockpit_grips: active
                .as_ref()
                .map_or([None; 2], |a| a.hands.map(|h| h.map(|h| h.grip.position))),
            triggers,
            dominant: config.acting_hand(),
            opened,
            world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
            dt,
        },
        &config,
        desired,
    );
    // A hand holds nothing when it holds no gun (the C4 the game's planting mode wields is not one),
    // no second weapon, grenade, or round.
    let gun_hands = weapon.map_or([false; 2], |(w, _)| {
        if w.wielded.slot == WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE {
            [false; 2]
        } else {
            w.wielded.hands
        }
    });
    let free = Hand::BOTH.map(|h| {
        !gun_hands[h.index()]
            && second::hand() != Some(h)
            && grenade::throwing_hand() != Some(h)
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
    let reload_held = reload::finish_tick(
        &reload::FinishInputs {
            grabs,
            grips,
            triggers,
            reload_button,
            dt,
        },
        &config,
    );
    // A gun the reload holds cannot fire. The dual-wield split keeps a held pistol's trigger from
    // the game itself, so the fire input is held back only when no drawn gun may fire.
    let reload_blocks = gun_hands.contains(&true)
        && Hand::BOTH
            .iter()
            .all(|h| !gun_hands[h.index()] || reload_held[h.index()]);

    second::set_trigger(
        gameplay && second::hand().is_some_and(|hand| triggers[hand.index()].held()),
    );
    // Only a hand on the grip fires the gun.
    let carried = weapon.and_then(|(w, _)| regions::carrying(w.address));
    let explosive = grenade::throwing_hand().or_else(c4::throwing_hand);
    let fire_blocked = reload_blocks
        || explosive_holds_fire(explosive, gun_hands, second::hand())
        || carried.is_some()
        || hold::act::holstering()
        || primary_trigger_idle(weapon.map(|(w, _)| w), &triggers)
        || (config.weapons.block_fire_during_actions
            && weapon_action_running(weapon.and_then(|(w, _)| w.holder).unwrap_or(gun_side)));
    if fire_blocked {
        hold_fire_back(desired);
    }
    declare_grips(gun_hands, carried);
    record_hands(weapon.map(|(w, _)| w), carried, &config);

    let mut state = STATE.lock();
    state.status.fire_blocked = fire_blocked;
    state.status.grabs += events.len() as u64;
    state.status.events.extend(events);
    let excess = state.status.events.len().saturating_sub(MAX_EVENTS);
    state.status.events.drain(..excess);
}

/// The target of the weapon handling's last tick that `fits` and that `hand` is nearest among those
/// it is within reach of, to dock what it lets go in.
fn docked_at(hand: Hand, fits: impl Fn(GrabVolume) -> bool) -> Option<GrabVolume> {
    let state = STATE.lock();
    dock_target(hand, &state.placed, state.points[hand.index()], fits)
}

/// Whether the fire input is held back for a grenade or a charge in the hand `explosive`: with no
/// gun in the other hand (`gun_hands`, or the second weapon's `second`), the fire input would draw
/// a gun into the hand that holds it.
fn explosive_holds_fire(
    explosive: Option<Hand>,
    gun_hands: [bool; 2],
    second: Option<Hand>,
) -> bool {
    explosive.is_some_and(|hand| {
        let other = hand.other();
        !gun_hands[other.index()] && second != Some(other)
    })
}

/// The fire inputs a weapon action holds back.
const FIRE_ACTIONS: [Action; 2] = [Action::FIRE_RIGHT, Action::R2];

/// How many grabs the readout keeps.
const MAX_EVENTS: usize = 6;

/// Declare what each hand holds of the weapons, for its finger pose: a gun in each hand that holds
/// one (`gun_hands`, and the second weapon's), the foregrip, a grenade or a charge, and a round for
/// the reload.
fn declare_grips(gun_hands: [bool; 2], carried: Option<(Hand, Carry)>) {
    for hand in Hand::BOTH.into_iter().filter(|h| gun_hands[h.index()]) {
        let grip = match carried {
            Some((carrier, carry)) if carrier == hand => {
                Grip::new(GripSource::Gun).pose(carry.region.finger_pose())
            }
            _ => Grip::new(GripSource::Gun),
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
    if let Some(hand) = grenade::throwing_hand() {
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
    if !hold::hand_over(hand) {
        return false;
    }
    foregrip::clear();
    if let Some((carrier, carry)) = carried {
        regions::end_carry("the other hand took the grip");
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
    if !carry.anchor.is_finite() || !carry.turn.is_finite() || !hold::hand_over(brace) {
        return false;
    }
    foregrip::clear();
    regions::begin_carry(weapon, brace, carry, CarryCause::LetGo);
    true
}

/// Record the hands on the wielded gun for the readout: the holding hand at the grip or the part it
/// carries the gun by, and the bracing hand at its part.
fn record_hands(
    weapon: Option<engine::HeldWeapon>,
    carried: Option<(Hand, Carry)>,
    config: &ControllersConfig,
) {
    if !config.weapons.regions.enabled {
        regions::record([None; 2]);
        return;
    }
    let holder = weapon.and_then(|w| w.holder).map(|hand| match carried {
        Some((carrier, carry)) if carrier == hand => (hand, carry.region),
        _ => (hand, GunRegion::Grip),
    });
    let brace = foregrip::status()
        .hold
        .filter(|h| h.engaged && weapon.is_some_and(|w| w.address == h.weapon))
        .map(|h| (h.holder.other(), h.region));
    regions::record(regions::hands_on_gun(holder, brace));
}

/// Whether the fire input is held back because no hand holding the wielded weapon pulls its
/// trigger while another hand's does: the one fire action reaches the weapon from either trigger,
/// and only a hand holding the weapon may fire it. The other hand is empty, or holds the second
/// weapon, which its trigger fires instead.
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

/// Carry out `hand`'s holster decision. With independent pistol draws, a draw of the dual-wield
/// slot brings out only `hand`'s pistol. With `hold.instant_dock`, a long gun is drawn and
/// holstered without the game's acts ([`crate::controllers::weapons::hold::act`]), falling back
/// to the game's when that is refused.
fn act(
    action: HolsterAction,
    hand: Hand,
    config: &ControllersConfig,
    desired: &mut Vec<Desired>,
) -> GrabOutcome {
    let independent = config.weapons.dual_wield.split && config.weapons.dual_wield.independent_draw;
    let settings = &config.weapons.hold;
    let pistols = docks_pistols(config);
    let single_pistol = single::slot_single();
    match action {
        HolsterAction::Draw { slot, .. } if docks_without_act(config, slot, single_pistol) => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::draw(hand, slot, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the draw falls back to the game's");
                    return act_with_game(action, hand, independent, desired);
                }
            }
        }
        HolsterAction::TakeSecond { slot } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            if let Err(refusal) = unsafe { second::take(hand, slot) } {
                tracing::debug!(target: "controllers", %refusal, "hold: no second weapon taken");
            }
            return GrabOutcome::Holster(action);
        }
        HolsterAction::PutBackSecond { .. } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            unsafe { second::put_back(None) };
            return GrabOutcome::Holster(action);
        }
        HolsterAction::DrawPistol { hand } if pistols => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::draw_pistol(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the pistol's draw falls back to the layout's");
                }
            }
        }
        HolsterAction::HolsterPistol { hand } if pistols => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::holster_pistol(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the pistol's holster falls back to the layout's");
                }
            }
        }
        HolsterAction::Holster { slot } if docks_without_act(config, slot, single_pistol) => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { hold::act::holster(hand, settings.snap_s) } {
                Ok(()) => return GrabOutcome::Holster(action),
                Err(refusal) => {
                    tracing::debug!(target: "controllers", %refusal, "hold: the holster falls back to the game's");
                }
            }
        }
        _ => {}
    }
    act_with_game(action, hand, independent && !single_pistol, desired)
}

/// Whether the pistols are drawn and holstered without the game's acts, each sliding between its hip
/// and its hand: with `hold.pistol_dock`, which needs each hand to draw its own pistol.
pub(super) fn docks_pistols(config: &ControllersConfig) -> bool {
    let dual = config.weapons.dual_wield;
    let hold = config.weapons.hold;
    hold.either_hand && hold.pistol_dock && dual.split && dual.independent_draw
}

/// Whether `slot` is drawn and holstered without the game's act: the long guns with
/// `hold.instant_dock`, the game's pair as [`docks_pistols`] says, and a single pistol
/// (`single_pistol`, for the pistol slot) with `hold.pistol_dock`, as a weapon of one gun.
pub(super) fn docks_without_act(
    config: &ControllersConfig,
    slot: WeaponSlot,
    single_pistol: bool,
) -> bool {
    let hold = config.weapons.hold;
    let long_guns = hold.either_hand && hold.instant_dock && hold::equip::handles(slot);
    let pistols = match single_pistol {
        true => hold.either_hand && hold.pistol_dock,
        false => docks_pistols(config),
    };
    long_guns || (slot == WeaponSlot::E_SLOT_DUAL_WIELD && pistols)
}

/// Carry out `hand`'s holster decision through the game's own draw and holster.
fn act_with_game(
    action: HolsterAction,
    hand: Hand,
    independent: bool,
    desired: &mut Vec<Desired>,
) -> GrabOutcome {
    match action {
        HolsterAction::Draw {
            slot,
            action: select,
        } => {
            if independent && slot == WeaponSlot::E_SLOT_DUAL_WIELD {
                dual_wield::attach::begin_draw(hand);
            }
            hold::begin(hand, hold::holder::Cause::Draw, None);
            // The game acts on a slot select when its input is released, so a one-tick click
            // selects.
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: select,
                },
                want: Want::Click,
            });
        }
        HolsterAction::Holster { .. } => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            match unsafe { engine::local_player() } {
                Some(owner) => unsafe { engine::holster(owner) },
                None => return GrabOutcome::NoCharacter,
            }
        }
        HolsterAction::DrawPistol { hand } => {
            dual_wield::attach::draw(hand);
        }
        HolsterAction::HolsterPistol { hand } => {
            dual_wield::attach::holster(hand);
        }
        HolsterAction::AlreadyDrawn { .. }
        | HolsterAction::Refused { .. }
        | HolsterAction::HandFull { .. }
        | HolsterAction::TakeSecond { .. }
        | HolsterAction::PutBackSecond { .. } => {}
    }
    GrabOutcome::Holster(action)
}

/// Carry out what opening `hand`'s grip on its gun does: holster it (without the game's act with
/// `hold.instant_dock`, through the game's holster otherwise), or let it fall.
fn release(hand: Hand, what: Release, wielded: holster::Wielded, config: &ControllersConfig) {
    let settings = &config.weapons.hold;
    let pair_held = wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD && wielded.hands == [true; 2];
    match what {
        // One pistol of a pair held in both hands goes back alone.
        Release::Holster if pair_held => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            let slid = docks_pistols(config)
                && unsafe { hold::act::holster_pistol(hand, settings.snap_s) }.is_ok();
            if !slid {
                dual_wield::attach::holster(hand);
            }
        }
        Release::Holster => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            let instant = docks_without_act(config, wielded.slot, single::slot_single())
                && unsafe { hold::act::holster(hand, settings.snap_s) }.is_ok();
            if !instant && let Some(owner) = unsafe { engine::local_player() } {
                unsafe { engine::holster(owner) };
            }
        }
        Release::Drop => {
            // SAFETY: the input tick runs on the main thread outside the world update.
            if let Err(refusal) = unsafe { hold::act::drop_held(hand, settings.max_throw_mps) } {
                tracing::debug!(target: "controllers", %refusal, "hold: the gun was not let go of");
            }
        }
    }
}

/// Carry out what opening the second weapon's grip does with the hand over `dock`, the nearest
/// target that takes it back: at a socket, dock it there; at its own holster (any holster for a
/// carried one), put it back; anywhere else, let it fall with the hand's motion.
///
/// # Safety
///
/// The main thread, outside the world update.
unsafe fn release_second(hand: Hand, dock: Option<GrabVolume>) {
    let config = Config::lock_query(|c| c.controllers);
    let at = match dock {
        Some(GrabVolume::Socket(socket)) => ReleasedAt::Socket(socket),
        Some(GrabVolume::Holster(_)) => ReleasedAt::OwnHolster,
        _ => ReleasedAt::Elsewhere,
    };
    let docked = match decide::second_release(at) {
        SecondRelease::Dock(socket) => unsafe { engine::local_player() }.is_some_and(|owner| {
            unsafe { actions::stow_second(owner, hand, socket) }
                .inspect_err(|refusal| {
                    tracing::debug!(target: "controllers", %refusal, "hold: the second weapon was not docked");
                })
                .is_ok()
        }),
        SecondRelease::PutBack => {
            unsafe { second::put_back(None) };
            true
        }
        SecondRelease::Fall => false,
    };
    if docked {
        return;
    }
    if let Err(refusal) = unsafe { second::let_fall(config.weapons.hold.max_throw_mps) } {
        tracing::debug!(target: "controllers", %refusal, "hold: the second weapon was not let go of");
    }
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

/// What `hand`'s grab at the hip on `side` does while the pistols are single, with `weapon` in the
/// hands.
fn single_hip_grab(
    side: Hand,
    hand: Hand,
    weapon: Option<engine::HeldWeapon>,
    config: &ControllersConfig,
) -> HipGrab {
    let socket = single::hips::hip_socket(side);
    let inventory = &config.weapons.inventory;
    single::hips::hip_grab(HipView {
        side,
        slot_home: single::slot_home(),
        socket: inventory.carry && inventory.layout(config.acting_hand()).has(socket),
        socket_taken: carry::taken().contains(&socket),
        holds_slot: weapon.is_some_and(|w| {
            single::is(w.address)
                && w.wielded.slot == WeaponSlot::E_SLOT_DUAL_WIELD
                && w.wielded.hands[hand.index()]
        }),
    })
}

/// Carry out `hand`'s grab of `socket`, with `weapon` in the hands. `None` for a grab that did
/// nothing worth reporting: a free socket with nothing to put there.
fn grab_socket(
    hand: Hand,
    socket: SocketId,
    weapon: Option<engine::HeldWeapon>,
    config: &ControllersConfig,
) -> Option<GrabOutcome> {
    let grab = SocketGrab {
        hand,
        socket,
        wielded: weapon.map(|w| (w.address, w.wielded.hands)),
        second: STATE
            .lock()
            .second
            .and_then(|w| Some((w.address, w.holder?))),
        second_allowed: config.weapons.hold.either_hand && config.weapons.hold.second_weapon,
        snap_s: config.weapons.hold.snap_s,
        independent_pistols: config.weapons.dual_wield.split
            && config.weapons.dual_wield.independent_draw,
    };
    // SAFETY: the input tick runs on the main thread outside the world update.
    match unsafe { actions::grab(&grab) } {
        Ok(_) => Some(GrabOutcome::Socket { done: true }),
        Err(SocketRefusal::Empty) => None,
        Err(refusal) => {
            tracing::debug!(target: "controllers", %refusal, "inventory: the socket grab did nothing");
            hold::refused(refusal);
            Some(GrabOutcome::Socket { done: false })
        }
    }
}

/// Whether a grab's outcome is one the grab cue pulses: a draw, a holster, or a foregrip grab.
/// The pouches pulse their own outcomes, and a release pulses from the transition, whatever
/// caused it.
pub(super) fn cues_grab(outcome: GrabOutcome) -> bool {
    matches!(
        outcome,
        GrabOutcome::Holster(
            HolsterAction::Draw { .. }
                | HolsterAction::Holster { .. }
                | HolsterAction::DrawPistol { .. }
                | HolsterAction::HolsterPistol { .. }
                | HolsterAction::TakeSecond { .. }
                | HolsterAction::PutBackSecond { .. }
        ) | GrabOutcome::Foregrip(Transition::Grabbed)
            | GrabOutcome::Handover { moved: true }
            | GrabOutcome::Take { taken: true }
            | GrabOutcome::Socket { done: true }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grenade_in_the_hand_holds_fire_back_unless_the_other_hand_has_a_gun() {
        assert!(!explosive_holds_fire(None, [false; 2], None));
        assert!(explosive_holds_fire(Some(Hand::Left), [false; 2], None));
        // The gun in the hand that now holds the grenade does not count.
        assert!(explosive_holds_fire(Some(Hand::Left), [true, false], None));
        assert!(!explosive_holds_fire(Some(Hand::Left), [false, true], None));
        assert!(!explosive_holds_fire(
            Some(Hand::Left),
            [false; 2],
            Some(Hand::Right)
        ));
    }
}
