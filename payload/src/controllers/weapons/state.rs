//! The weapon handling's tick-to-tick state: the readout the debug UI reads
//! ([`WeaponsStatus`](crate::controllers::weapons::WeaponsStatus)), the wielded weapon and its
//! measured geometry, and the bookkeeping the two tick halves share.

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    config::ControllersConfig,
    frame::Hand,
    grab::{GrabVolume, GripTarget},
    hands::ik,
    loadout::Socket,
    physical_hands::physics::guns,
    weapons::{
        ads, c4, collide, dual_wield,
        engine::{self, HeldWeapon},
        foregrip::{self, GripGeometry},
        grenade, grip_watch, heft,
        hold::{self, armed},
        opening::OpenWatch,
        pouch_models, recoil,
        regions::{self, Carry, GunRegion, RegionHit},
        reload,
        types::WeaponsStatus,
        wheel,
    },
};

/// The weapon handling's readout, for the debug UI.
pub fn status() -> WeaponsStatus {
    STATE.lock().status.clone()
}

/// Stop acting: drop the foregrip hold and forget the tick's weapon. Called whenever the bindings
/// do not inject.
pub(crate) fn stand_down() {
    foregrip::clear();
    guns::publish(guns::GunHolds::default());
    heft::observe([None; 2]);
    heft::freeze([false; 2]);
    ads::observe(None);
    collide::observe(None);
    grip_watch::clear();
    recoil::clear();
    grenade::stand_down();
    // SAFETY: the weapon handling stands down on the input tick, on the game thread outside the
    // world update.
    unsafe { pouch_models::stand_down() };
    c4::stand_down();
    c4::wrist::stand_down();
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { dual_wield::single::stand_down() };
    hold::clear();
    hold::act::clear();
    hold::tick::stand_down();
    hold::take::stand_down();
    hold::pips::submit([None; 2]);
    regions::stand_down();
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { armed::stand_down() };
    reload::stand_down();
    wheel::stand_down();
    let mut state = STATE.lock();
    state.weapon = None;
    state.beside = None;
    state.status.active = false;
    state.status.volumes = [None; 2];
    state.status.fire_blocked = false;
    state.placed.clear();
    state.points = [None; 2];
    state.last_volumes = [None; 2];
    state.touch = None;
    state.slide = None;
    state.promote = None;
    state.docked_touch = [None; 2];
}

pub(super) struct State {
    pub(super) status: WeaponsStatus,
    /// The tick's wielded weapon and its two-handed geometry.
    pub(super) weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// The tick's gun a hand holds beside the wielded weapon.
    pub(super) beside: Option<HeldWeapon>,
    /// Whether the tick's mode is one the weapon handling acts in.
    pub(super) gameplay: bool,
    /// The last geometry measured on each hand's wrist, by [`Hand::index`], and the weapon it
    /// belongs to: a measurement needs the wrist at full weight, so a tick without one keeps the
    /// last.
    measured: [Option<(usize, GripGeometry)>; 2],
    /// The tick's grip targets and each hand's grip as it is drawn, for the docking of what a hand
    /// lets go.
    pub(super) placed: Vec<GripTarget>,
    pub(super) points: [Option<Vec3>; 2],
    /// The hand that last held a gun, kept while none is drawn.
    pub(super) last_holder: Option<Hand>,
    /// The volume each hand was in on the last tick, for the cue on entering one.
    pub(super) last_volumes: [Option<GrabVolume>; 2],
    /// The tick's hand the pouches and the wrist detonator are laid out around
    /// ([`crate::controllers::weapons::volumes::gun_side`]).
    pub(super) gun_side: Hand,
    /// Where the hand without the wielded long gun touches it on this tick.
    pub(super) touch: Option<OffTouch>,
    /// Where the bracing hand has slid to along the gun on this tick, in the holding wrist's frame.
    pub(super) slide: Option<(Vec3, GunRegion)>,
    /// The bracing hand that slid onto the grip of a gun the other hand carries, on this tick.
    pub(super) promote: Option<Hand>,
    /// Where each hand touches a long gun docked on a shoulder, by [`Hand::index`].
    pub(super) docked_touch: [Option<DockTouch>; 2],
    /// Each hand's fingers, for the release of a grenade or a charge.
    pub(super) opening: OpenWatch,
}

/// Where the hand without the wielded long gun touches it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct OffTouch {
    pub(super) hand: Hand,
    pub(super) hit: RegionHit,
    /// The anchor in the holding wrist's frame.
    pub(super) anchor_in_wrist: Vec3,
}

/// Where a hand touches a long gun docked on a shoulder, and how it would carry it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct DockTouch {
    pub(super) site: Socket,
    pub(super) weapon: usize,
    pub(super) carry: Carry,
}

impl State {
    /// The two-handed geometry of `weapon` in `holder`, measured afresh when possible.
    pub(super) fn geometry_for(
        &mut self,
        holder: Hand,
        weapon: &HeldWeapon,
        config: &ControllersConfig,
        world_scale: f32,
    ) -> Option<GripGeometry> {
        if weapon.holder != Some(holder) {
            return None;
        }
        let target = ik::tick_records().current[holder.index()];
        // SAFETY: the weapon was read this tick on the main thread outside the world update.
        let fresh = target.and_then(|target| unsafe {
            engine::measure_geometry(
                weapon,
                target,
                &config.aim,
                &config.weapons.foregrip,
                world_scale,
            )
        });
        match fresh {
            Some(geometry) => {
                self.measured[holder.index()] = Some((weapon.address, geometry));
                Some(geometry)
            }
            // A geometry measured on the other hand's wrist is wrong for this one, so each hand
            // keeps its own.
            None => self.measured[holder.index()]
                .filter(|(address, _)| *address == weapon.address)
                .map(|(_, geometry)| geometry),
        }
    }
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    status: WeaponsStatus {
        active: false,
        volumes: [None; 2],
        candidates: 0,
        wielded: None,
        fire_blocked: false,
        events: Vec::new(),
        grabs: 0,
        docks_followed: 0,
    },
    weapon: None,
    beside: None,
    gameplay: false,
    measured: [None; 2],
    placed: Vec::new(),
    points: [None; 2],
    last_holder: None,
    last_volumes: [None; 2],
    gun_side: Hand::Right,
    touch: None,
    slide: None,
    promote: None,
    docked_touch: [None; 2],
    opening: OpenWatch::new(),
});
