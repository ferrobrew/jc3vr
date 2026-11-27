//! The weapon handling's tick-to-tick state: the readout the debug UI reads
//! ([`WeaponsStatus`](crate::controllers::weapons::WeaponsStatus)), the wielded weapon and its
//! measured geometry, and the bookkeeping the two tick halves share.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{GrabVolume, GripTarget},
    hands::ik,
};
use jc3vr_core::published::EventLog;
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::weapons::{
    config::WeaponSettings,
    context::TickContext,
    docks::attract::{Attractor, ZoneWatch},
    engine,
    engine::HeldWeapon,
    foregrip::GripGeometry,
    opening::OpenWatch,
    regions::RegionHit,
    types::WeaponsStatus,
};

/// The weapon handling's readout, for the debug UI.
pub fn status() -> WeaponsStatus {
    STATE.lock().status.clone()
}

/// The wielded weapon as the last input tick began, and whether that tick ran the weapon handling.
/// A stand-down keeps the weapon and clears the flag.
pub(crate) fn last_wielded() -> (Option<HeldWeapon>, bool) {
    let state = STATE.lock();
    (
        state.status.wielded.map(|readout| readout.weapon),
        state.status.active,
    )
}

pub(super) struct State {
    pub(super) status: WeaponsStatus,
    /// What the begin half hands the finish half, until the weapon handling stands down.
    pub(super) tick: Option<TickContext>,
    /// The last geometry measured on each hand's wrist, by [`Hand::index`], and the weapon it
    /// belongs to: a measurement needs the wrist at full weight, so a tick without one keeps the
    /// last.
    measured: [Option<(usize, GripGeometry)>; 2],
    /// The tick's grip targets and each hand's grip as it is drawn, for the docking of what a hand
    /// lets go.
    pub(super) placed: Vec<GripTarget>,
    pub(super) points: [Option<Vec3>; 2],
    /// The tick's free sockets' attractions, for a release outside every socket's reach.
    pub(super) attractors: Vec<Attractor>,
    /// Which socket's attraction each hand is in, for the tick on coming into one.
    pub(super) zones: ZoneWatch,
    /// The hand that last held a gun, kept while none is drawn.
    pub(super) last_holder: Option<Hand>,
    /// The volume each hand was in on the last tick, for the cue on entering one.
    pub(super) last_volumes: [Option<GrabVolume>; 2],
    /// The tick's hand the pouches are laid out around
    /// ([`crate::controllers::weapons::volumes::gun_side`]).
    pub(super) gun_side: Hand,
    /// Each hand's fingers, for the release of a grenade or a charge.
    pub(super) opening: OpenWatch,
}

/// Where the free hand touches the lone long gun.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct OffTouch {
    pub(super) hand: Hand,
    pub(super) hit: RegionHit,
    /// The anchor in the holding wrist's frame.
    pub(super) anchor_in_wrist: Vec3,
}

impl State {
    /// Forget the tick: what the begin half handed over, the targets and grips it placed, and the
    /// readout's per-tick fields. The measured geometries, the last holder, and the readout's
    /// wielded weapon and events are kept.
    pub(super) fn stand_down(&mut self) {
        self.tick = None;
        self.status.active = false;
        self.status.volumes = [None; 2];
        self.status.fire_blocked = false;
        self.placed.clear();
        self.points = [None; 2];
        self.attractors.clear();
        self.zones = ZoneWatch::new();
        self.last_volumes = [None; 2];
    }

    /// The two-handed geometry of `weapon` in `holder`, measured afresh when possible.
    pub(super) fn geometry_for(
        &mut self,
        holder: Hand,
        weapon: &HeldWeapon,
        config: &WeaponSettings,
        world_scale: f32,
    ) -> Option<GripGeometry> {
        if weapon.holder != Some(holder) {
            return None;
        }
        let target = ik::tick_records().current[holder.index()];
        // SAFETY: the weapon was read this tick on the game thread before the world update.
        let fresh = target.and_then(|target| unsafe {
            engine::measure_geometry(weapon, target, &config.aim, world_scale)
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
        events: EventLog::new(),
    },
    tick: None,
    measured: [None; 2],
    placed: Vec::new(),
    points: [None; 2],
    attractors: Vec::new(),
    zones: ZoneWatch::new(),
    last_holder: None,
    last_volumes: [None; 2],
    gun_side: Hand::Right,
    opening: OpenWatch::new(),
});
