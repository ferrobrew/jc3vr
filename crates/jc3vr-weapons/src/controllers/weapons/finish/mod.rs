//! The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
//! throw and the reload, and hold the fire inputs back during a weapon action.
//!
//! The half is a table of stages ([`STAGES`]) run in order over one [`Finish`] context: the
//! begin half's handoff ([`TickContext`]), the bindings' inputs, and what each stage leaves for
//! the ones after it. The grabs act first, then the throws, the reload, and the releases; the fire
//! decision reads the hands after all of them, so a gun that changed hands this tick fires from
//! its new hand.

use jc3vr_body::controllers::{
    fingers::{grip, grip::Grip},
    grab::{GrabVolume, resolve::dock_target},
    hands,
    hands::ActiveHands,
    haptics,
    intent::{finish::FinishInputs, hold::Desired},
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::{
    frame::{self, Hand},
    haptics::cues::Event,
};

use crate::controllers::{
    loadout::{
        Socket,
        contents::{self, HandContents},
        live,
    },
    weapons::{
        c4,
        c4::tuning::C4,
        capabilities::{GunOwner, gun_owners_now},
        contents::sample,
        context::TickContext,
        engine::HeldWeapon,
        foregrip,
        foregrip::{GripGeometry, Inputs, Transition, TwoHandedWeapon},
        grenade, grip_sources,
        handling_tuning::HANDLING,
        regions,
        regions::GunRegion,
        reload,
        state::STATE,
        types::GrabEvent,
    },
};

mod fire;
mod grabs;
mod release;
#[cfg(test)]
mod tests;

/// The second half of the weapon handling's input tick: act on the tick's grabs, run the grenade
/// throw and the reload, and hold the fire inputs back during a weapon action. `desired` is the
/// tables' output, which the grenade throw adds to and the fire hold-back trims.
pub fn finish_tick(
    game: &GameTick<'_>,
    inputs: &FinishInputs,
    desired: &mut Vec<Desired>,
) -> Option<reload::BeltEligibility> {
    let context = TickContext::handed_over();
    let active = hands::active_hands_raw();
    let mut finish = Finish {
        world_scale: active.as_ref().map_or(1.0, |a| a.world_scale),
        context,
        inputs,
        desired,
        active,
        source_guns: gun_owners_now(),
        opened: [false; 2],
        transition: None,
        events: Vec::new(),
        reload: None,
        current: None,
        held: [HandContents::default(); 2],
        fire_blocked: false,
        belt: None,
    };
    for stage in &STAGES {
        (stage.run)(game, &mut finish);
    }
    finish.belt
}

/// The finish half's context: the begin half's handoff, the bindings' inputs, and what each stage
/// leaves for the ones after it.
struct Finish<'a> {
    context: TickContext,
    inputs: &'a FinishInputs,
    /// The tables' output, which the grenade throw adds to and the fire hold-back trims.
    desired: &'a mut Vec<Desired>,
    active: Option<ActiveHands>,
    world_scale: f32,
    /// The guns the hands held as the half began, for the belt reload's proof of life.
    source_guns: [Option<GunOwner>; 2],
    /// Each hand whose fingers opened this tick, by [`Hand::index`].
    opened: [bool; 2],
    /// The foregrip's transition this tick.
    transition: Option<Transition>,
    /// What the tick's grabs did, for the readout.
    events: Vec<GrabEvent>,
    /// The reload's half.
    reload: Option<reload::FinishResult>,
    /// The wielded weapon after the grabs, the throws, the reload, and the releases.
    current: Option<HeldWeapon>,
    /// What each hand holds after them, by [`Hand::index`].
    held: [HandContents; 2],
    /// Whether the fire inputs are held back.
    fire_blocked: bool,
    /// The belt reload's offer.
    belt: Option<reload::BeltEligibility>,
}

/// One stage of the finish half.
struct Stage {
    /// The stage, for the order's test.
    #[cfg_attr(not(test), expect(dead_code, reason = "read by the order's test"))]
    name: &'static str,
    run: fn(&GameTick<'_>, &mut Finish<'_>),
}

/// The finish half's stages, in the order they run.
const STAGES: [Stage; 9] = [
    // The fingers opening lets a grenade or a charge go.
    Stage {
        name: "fingers",
        run: fingers,
    },
    // Brace the lone long gun, or let it go.
    Stage {
        name: "foregrip",
        run: foregrip_stage,
    },
    // Act on each hand's grab: a handover, a draw or a dock, a take from the world.
    Stage {
        name: "grabs",
        run: grabs::grabs,
    },
    Stage {
        name: "grenades",
        run: grenades,
    },
    Stage {
        name: "C4",
        run: charges,
    },
    Stage {
        name: "reload",
        run: reload_stage,
    },
    // Dock, pass, or drop what an opened grip lets go of.
    Stage {
        name: "release",
        run: release::release_stage,
    },
    // After every move of the tick: each hand's own fire input, and the fire hold-back.
    Stage {
        name: "fire",
        run: fire::fire,
    },
    // The hands' grips and contents, the readout, and the belt reload's offer.
    Stage {
        name: "publish",
        run: publish,
    },
];

/// Step the fingers' opening, which lets a grenade or a charge go, not the grip's press relaxing.
fn fingers(_game: &GameTick<'_>, f: &mut Finish<'_>) {
    let values = frame::with_latest(|frame| {
        Hand::BOTH.map(|h| {
            let hand = frame.hand(h);
            (frame.focused && hand.active).then_some(hand.inputs.squeeze)
        })
    })
    .unwrap_or([None; 2]);
    f.opened = STATE
        .lock()
        .opening
        .step_available(values, f.inputs.grips, f.inputs.unavailable);
}

/// Step the foregrip: brace the lone long gun where the free hand grabbed it, slide along it, or let
/// go of it, and pulse the off-hand on a release.
fn foregrip_stage(_game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs {
        grabs, grips, dt, ..
    } = *f.inputs;
    let capabilities = f.context.modes;
    let (lone, touch, slide) = (f.context.lone, f.context.touch, f.context.slide);
    let foregrip_grab = grabs
        .iter()
        .zip(Hand::BOTH)
        .find(|(g, _)| **g == Some(GrabVolume::Foregrip))
        .map(|(_, hand)| hand);
    let two_handed = lone
        .filter(|(w, _)| w.two_handed && capabilities.guns)
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
        active: f.active.is_some(),
        hands: f.active.as_ref().map_or([None; 2], |a| a.hands),
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
    f.transition = transition;
}

/// The grenade throw: the pouch, the pin, the locker, and the catch, with a grenade in a hand whose
/// fingers open over a hip socket stowed there rather than let go.
fn grenades(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs {
        grabs,
        triggers,
        dt,
        ..
    } = *f.inputs;
    let opened = f.opened;
    let grenade_opened = Hand::BOTH.map(|hand| {
        if opened[hand.index()] && grenade::safe::in_hand(hand).is_some() {
            let socket = docked_at(hand, |volume| {
                matches!(volume, GrabVolume::Socket(socket) if matches!(socket, Socket::Hip(_)))
            }).and_then(|volume| match volume {
                GrabVolume::Socket(socket) => Some(socket),
                _ => None,
            });
            if grabs::stow_safe_grenade(hand, socket) == Some(true) {
                haptics::play(hand, Event::VolumeGrab);
                return false;
            }
        }
        opened[hand.index()]
    });
    grenade::input_tick(
        game,
        &grenade::TickInputs {
            acting: f.context.modes.pouches && f.active.is_some(),
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
            cockpit_grips: f
                .active
                .as_ref()
                .map_or([None; 2], |a| a.hands.map(|h| h.map(|h| h.grip.position))),
            triggers,
            dominant: f.context.settings.dominant,
            opened: grenade_opened,
            dt,
        },
        f.desired,
    );
}

/// The C4 throw and the wrist detonator.
fn charges(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs {
        grabs,
        triggers,
        unavailable,
        ..
    } = *f.inputs;
    let dominant = f.context.settings.dominant;
    let free = sample(f.context.weapon.as_ref().map(|(w, _)| w)).map(|hand| hand.free());
    c4::input_tick(
        game,
        &c4::TickInputs {
            acting: f.context.modes.pouches && f.active.is_some(),
            pouch_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::C4Pouch)),
            triggers,
            placed_grab: Hand::BOTH
                .into_iter()
                .find(|h| grabs[h.index()] == Some(GrabVolume::PlacedCharge)),
            opened: f.opened,
            free: c4::wheel::WheelHands::available(free, unavailable, dominant).free,
            dominant,
        },
        C4.get(),
    );
}

/// The reload's half: the rounds, the magazines, and the action.
fn reload_stage(game: &GameTick<'_>, f: &mut Finish<'_>) {
    let FinishInputs {
        grabs,
        grips,
        fire_presses,
        reload_button,
        dt,
        real_dt,
        ..
    } = *f.inputs;
    f.reload = Some(reload::finish_tick(
        game,
        &reload::FinishInputs {
            grabs,
            grips,
            reload_button,
            fire_presses,
            dt: real_dt,
            game_dt: dt,
        },
        &f.context.settings,
    ));
}

/// Publish the hands: each hand's grip for its finger pose, the hands on the long guns, and the
/// hands' contents; then the readout, and the belt reload's offer.
fn publish(_game: &GameTick<'_>, f: &mut Finish<'_>) {
    declare_grips(&f.held);
    record_hands();
    contents::publish(sample(f.current.as_ref()));

    let mut state = STATE.lock();
    state.status.fire_blocked = f.fire_blocked;
    state.status.events.extend(std::mem::take(&mut f.events));
    drop(state);
    let Some(sequence) = frame::with_latest(|frame| frame.sequence) else {
        return;
    };
    f.belt = f.reload.take().and_then(|reload| {
        reload.belt_eligibility(
            sequence,
            &f.context.settings,
            f.source_guns,
            gun_owners_now(),
        )
    });
}

/// The target of the weapon handling's last tick that `fits` and that `hand` is nearest among those
/// it is within reach of, to dock what it lets go in.
fn docked_at(hand: Hand, fits: impl Fn(GrabVolume) -> bool) -> Option<GrabVolume> {
    let state = STATE.lock();
    dock_target(hand, &state.placed, state.points[hand.index()], fits)
}

/// Declare what each hand holds of the weapons, for its finger pose: a gun by its grip in each hand
/// that holds one, the foregrip, a grenade or a charge, the detonator, which is held as a gun's
/// grip is, and a round for the reload.
fn declare_grips(hands: &[HandContents; 2]) {
    let held = |pick: fn(&HandContents) -> bool| {
        Hand::BOTH
            .into_iter()
            .filter(move |hand| pick(&hands[hand.index()]))
    };
    for hand in held(HandContents::gun) {
        grip::declare(hand, Grip::new(grip_sources::GUN));
    }
    if let Some(two) = foregrip::two_hand_grip() {
        grip::declare(
            two.holder.other(),
            Grip::new(grip_sources::FOREGRIP)
                .pose(two.region.finger_pose())
                .share(two.weight),
        );
    }
    for hand in held(|hand| hand.grenade.is_some()) {
        grip::declare(hand, Grip::new(grip_sources::GRENADE));
    }
    for hand in held(|hand| hand.charge.is_some()) {
        grip::declare(hand, Grip::new(grip_sources::C4));
    }
    for hand in held(|hand| hand.detonator) {
        grip::declare(hand, Grip::new(grip_sources::GUN));
    }
    for hand in held(|hand| hand.round) {
        grip::declare(hand, Grip::new(grip_sources::MAGAZINE));
    }
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
