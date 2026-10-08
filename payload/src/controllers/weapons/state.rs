//! The weapon handling's tick-to-tick state: the readout the debug UI reads
//! ([`WeaponsStatus`](crate::controllers::weapons::WeaponsStatus)), the wielded weapon and its
//! measured geometry, and the bookkeeping the two tick halves and the near-miss log share.

use std::time::Instant;

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    bindings::grab::{Candidate, GrabVolume},
    config::ControllersConfig,
    frame::Hand,
    hands::ik,
    overlay::{primitive::Source, store},
    physical_hands::physics::guns,
    roomscale::{self, posture::EffectivePosture},
    weapons::{
        ads, c4, collide, dual_wield,
        engine::{self, HeldWeapon},
        foregrip::{self, GripGeometry},
        grenade, grip_watch, heft,
        hold::{self, second},
        recoil, reload,
        types::WeaponsStatus,
        volumes::Stance,
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
    heft::observe(None);
    heft::freeze(false);
    ads::observe(None);
    collide::observe(None);
    grip_watch::clear();
    recoil::clear();
    grenade::stand_down();
    c4::stand_down();
    c4::wrist::stand_down();
    dual_wield::observe_triggers(None);
    dual_wield::attach::clear();
    hold::clear();
    hold::act::clear();
    hold::take::stand_down();
    hold::pips::submit(None);
    // SAFETY: the input tick runs on the main thread outside the world update.
    unsafe { second::stand_down() };
    reload::stand_down();
    wheel::stand_down();
    let mut state = STATE.lock();
    state.weapon = None;
    state.second = None;
    state.status.active = false;
    state.status.volumes = [None; 2];
    state.status.fire_blocked = false;
    state.status.stance = None;
    state.placed.clear();
    state.grab_points = [None; 2];
    state.body_y = None;
    state.last_volumes = [None; 2];
    store::clear(Source::GrabVolumes);
}

pub(super) struct State {
    pub(super) status: WeaponsStatus,
    /// The tick's wielded weapon and its two-handed geometry.
    pub(super) weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// The tick's second weapon.
    pub(super) second: Option<HeldWeapon>,
    /// Whether the tick's mode is one the weapon handling acts in.
    pub(super) gameplay: bool,
    /// The last geometry measured, and the weapon it belongs to: a measurement needs the wrist at
    /// full weight, so a tick without one keeps the last.
    measured: Option<(usize, GripGeometry)>,
    /// The tick's placed volumes and each hand's grab point for the body's volumes, for the
    /// near-miss log.
    pub(super) placed: Vec<Candidate>,
    pub(super) grab_points: [Option<Vec3>; 2],
    pub(super) last_miss_log: Option<Instant>,
    /// The hand that last held a gun, kept while none is drawn.
    pub(super) last_holder: Option<Hand>,
    /// The body frame's height on the last tick ([`crate::controllers::weapons::volumes::body_frame`]).
    pub(super) body_y: Option<f32>,
    /// The volume each hand was in on the last tick, for the cue on entering one.
    pub(super) last_volumes: [Option<GrabVolume>; 2],
    /// The tick's hand the pouches and the wrist detonator are laid out around
    /// ([`crate::controllers::weapons::volumes::gun_side`]).
    pub(super) gun_side: Hand,
}

impl State {
    /// The stance for this tick, from the roomscale posture (the single source of truth for
    /// seated versus standing): seated for the seated posture, standing for the standing and
    /// roomscale postures, and standing without a recent roomscale frame (the roomscale
    /// subsystems off, or not yet run). Logs a change of stance.
    pub(super) fn step_stance(&mut self) -> Stance {
        let now = Instant::now();
        let posture = roomscale::frame_shared()
            .filter(|f| f.fresh(now))
            .map(|f| f.posture);
        let stance = match posture {
            Some(EffectivePosture::Seated) => Stance::Seated,
            _ => Stance::Standing,
        };
        let previous = self.status.stance;
        if previous.map(Stance::label) != Some(stance.label()) {
            tracing::debug!(target: "controllers", stance = stance.label(), "holsters: placed for a new stance");
        }
        stance
    }

    /// The two-handed geometry of `weapon`, measured afresh when possible.
    pub(super) fn geometry_for(
        &mut self,
        weapon: &HeldWeapon,
        config: &ControllersConfig,
        world_scale: f32,
    ) -> Option<GripGeometry> {
        let target = weapon
            .holder
            .and_then(|holder| ik::tick_records().current[holder.index()]);
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
                self.measured = Some((weapon.address, geometry));
                Some(geometry)
            }
            None => self
                .measured
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
        last_miss: None,
        stance: None,
        docks_followed: 0,
    },
    weapon: None,
    second: None,
    gameplay: false,
    measured: None,
    placed: Vec::new(),
    grab_points: [None; 2],
    last_miss_log: None,
    last_holder: None,
    body_y: None,
    last_volumes: [None; 2],
    gun_side: Hand::Right,
});
