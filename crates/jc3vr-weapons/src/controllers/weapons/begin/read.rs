//! The begin half's reading stages: the local player, the guns in the hands, and their geometry.

use glam::Mat4;
use jc3gi::equipment::weapon::WeaponBase;
use jc3vr_body::controllers::{
    held::guns::{self, GunHolds},
    physics::local_player::LocalPlayer,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;

use crate::controllers::{
    loadout::{contents::HandContents, live},
    weapons::{
        ads,
        begin::{Begin, with_holder},
        capabilities::{gun_hands_now, tracked_hands, trigger_claims},
        collide, contents, docks,
        docks::{SocketInputs, attachments::Sample},
        engine,
        engine::HeldWeapon,
        grip_watch, gun_bodies, heft, hold,
        hold::armed,
        hud_feed::hand_ammo,
        inventory,
        inventory::tuning::INVENTORY,
        regions,
        regions::{Observed, engine as region_engine, engine::DockedLongGun},
        reload, scope,
        state::STATE,
        volumes,
    },
};

/// What the weapon handling read of the local player on the input tick.
#[derive(Default)]
pub(super) struct Ticked {
    pub(super) weapon: Option<HeldWeapon>,
    /// The gun in each hand, by [`Hand::index`]: the wielded weapon and the ones armed beside it.
    pub(super) guns: [Option<HeldWeapon>; 2],
    /// The gun a hand holds beside the wielded weapon.
    pub(super) beside: Option<HeldWeapon>,
    /// What each hand holds as the tick read its weapons, by [`Hand::index`].
    pub(super) contents: [HandContents; 2],
    /// Native socket observations and hand offers from this input tick.
    pub(super) sockets: Option<Sample>,
    /// The character's model-to-world transform.
    pub(super) character: Option<Mat4>,
    /// The long guns docked on the shoulders, for a draw at any part of one.
    pub(super) docked_long: Vec<DockedLongGun>,
}

/// Read the local player: the loadout table's and the hold's tick, the weapons in the hands, the
/// hand bodies' guns, the sockets, and the long guns docked on the shoulders.
pub(super) fn player(game: &GameTick<'_>, b: &mut Begin) {
    let config = &b.context.settings;
    let capabilities = b.context.modes;
    // SAFETY: the local player, in the game tick.
    b.ticked = unsafe {
        LocalPlayer::get(game).map_or(Ticked::default(), |mut player| {
            let owner = player.character_mut();
            armed::sample_readouts(owner);
            // The table lands the wielded weapon in a hand first; the hold then follows it there.
            let wielded = hold::wielded(owner);
            let arrival = inventory::input_tick(
                game,
                owner,
                &inventory::TickInputs {
                    dominant: config.dominant,
                    wielded,
                },
            );
            let holder = hold::input_tick(game, wielded, arrival);
            hold::tick::input_tick(owner, wielded);
            reload::native::refill_armed(owner, config, capabilities.reload);
            let weapon = engine::held_weapon(owner).map(|w| with_holder(w, holder));
            let guns = engine::in_hands(weapon, Hand::BOTH.map(|h| armed::held_weapon(owner, h)));
            // The gun a hand holds beside the wielded weapon, armed there.
            let beside = Hand::BOTH
                .into_iter()
                .filter_map(|h| guns[h.index()])
                .find(|w| weapon.is_none_or(|wielded| wielded.address != w.address));
            // The guns the hands hold, for their bodies.
            guns::publish(if capabilities.support {
                gun_bodies::read_holds([
                    weapon.map(|w| (w.address, w.class)),
                    beside.map(|w| (w.address, w.class)),
                ])
            } else {
                GunHolds::default()
            });
            let snapshot = live::snapshot_now();
            let contents = contents::sample(weapon.as_ref());
            let free = contents.map(|hand| hand.free());
            let sockets = snapshot
                .as_ref()
                .filter(|_| capabilities.dock)
                .map(|snapshot| {
                    docks::socket_points(
                        owner,
                        &SocketInputs {
                            snapshot,
                            free,
                            offsets: &INVENTORY.get().sockets,
                        },
                    )
                });
            let docked_long = match &snapshot {
                Some(snapshot) if capabilities.dock => {
                    region_engine::docked_long_guns(owner, snapshot, config.dominant)
                }
                _ => Vec::new(),
            };
            Ticked {
                weapon,
                guns,
                beside,
                contents,
                sockets,
                character: Some(Mat4::from(owner.m_WorldMatrixT1)),
                docked_long,
            }
        })
    };
}

/// Observe the guns in the hands (the scope, the ammo readout, the grip watch, and the heft), take
/// each hand's free state, and reserve the trigger of each hand on a gun.
pub(super) fn guns(_game: &GameTick<'_>, b: &mut Begin) {
    let capabilities = b.context.modes;
    let guns = b.ticked.guns;
    scope::input_tick(guns.map(|g| g.map(|w| w.address)));
    // SAFETY: the weapons were read this tick, on the game thread before the world update.
    unsafe { hand_ammo::observe(guns.map(|g| g.map(|w| w.address))) };
    // SAFETY: the weapon was read this tick, on the game thread before the world update.
    if let Some(w) = b.ticked.weapon
        && let Some(base) = unsafe { (w.address as *const WeaponBase).as_ref() }
    {
        unsafe { grip_watch::observe(base, w.class) };
    } else {
        // With no gun held the hold has ended, and the next one starts its own grab.
        grip_watch::clear();
    }
    heft::observe(Hand::BOTH.map(|hand| {
        guns[hand.index()]
            .filter(|_| capabilities.support)
            .map(|w| heft::HeftTarget {
                hand,
                class: w.class,
                weapon: w.address,
            })
    }));
    // One notion of a free hand for every target: the C4 the planting mode wields leaves its hand
    // free once the charge is planted, and the drawn detonator fills it.
    b.free = b.ticked.contents.map(|hand| hand.free());
    let claims = trigger_claims(capabilities, gun_hands_now(), tracked_hands());
    b.held = claims
        .iter()
        .any(|claim| claim.reserved)
        .then(|| claims.map(|claim| claim.reserved));
}

/// Measure each hand's long gun on its own wrist, and pick the lone gun (held in one hand while
/// the other is free), the braced long gun, and its shape.
pub(super) fn geometry(_game: &GameTick<'_>, b: &mut Begin) {
    let config = &b.context.settings;
    let capabilities = b.context.modes;
    let guns = b.ticked.guns;
    let weapon = b.ticked.weapon;
    let mut state = STATE.lock();
    let holder = weapon.and_then(|w| w.holder);
    b.gun_side = volumes::gun_side(holder, state.last_holder, config.dominant);
    state.last_holder = holder.or(state.last_holder);
    state.gun_side = b.gun_side;
    let world_scale = b.active.world_scale;
    b.geometries = Hand::BOTH.map(|hand| {
        guns[hand.index()]
            .filter(|w| w.two_handed && capabilities.support)
            .and_then(|w| state.geometry_for(hand, &w, config, world_scale))
    });
    drop(state);
    b.geometry = weapon
        .and_then(|w| w.holder)
        .and_then(|hand| b.geometries[hand.index()]);
    // The gun held in one hand while the other is free, wielded or armed: everything the free hand
    // does to it (the handover, and on a long gun the foregrip and a touch along it) acts on it.
    b.lone =
        engine::lone_gun(guns, b.free).filter(|_| capabilities.transfer || capabilities.support);
    b.braced = b.lone.filter(|w| w.two_handed);
    b.braced_geometry = b
        .braced
        .and_then(|w| w.holder)
        .and_then(|hand| b.geometries[hand.index()]);
    // SAFETY: the weapon was read this tick, on the game thread before the world update.
    b.shape = b.braced.and_then(|w| unsafe {
        region_engine::shape(
            (w.address as *const WeaponBase).as_ref()?,
            w.holder.unwrap_or(config.dominant),
        )
    });
}

/// Hand the guns to the regions, the sights, and the collision.
pub(super) fn gun_observers(_game: &GameTick<'_>, b: &mut Begin) {
    let capabilities = b.context.modes;
    let guns = b.ticked.guns;
    let geometries = b.geometries;
    regions::observe(&Observed {
        guns: guns.map(|g| {
            g.filter(|w| capabilities.support && w.two_handed)
                .map(|w| w.address)
        }),
        lone: b.braced.map(|w| (w.address, b.shape)),
    });
    ads::observe(Hand::BOTH.map(|hand| {
        let w = guns[hand.index()]?;
        let g = geometries[hand.index()]?;
        Some(ads::AdsWeapon {
            holder: hand,
            sight: ads::solve::WristSight {
                origin: g.sight,
                direction: g.barrel,
            },
            scoped: scope::is_scoped(w.address),
        })
    }));
    collide::observe(Hand::BOTH.map(|hand| {
        geometries[hand.index()].map(|g| collide::CollideWeapon {
            holder: hand,
            muzzle: g.muzzle,
            barrel: g.barrel,
        })
    }));
}
