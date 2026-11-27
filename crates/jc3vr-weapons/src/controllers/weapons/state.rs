//! The weapon handling's tick-to-tick state: the readout the debug UI reads
//! ([`WeaponsStatus`](crate::controllers::weapons::WeaponsStatus)), the wielded weapon and its
//! measured geometry, and the bookkeeping the two tick halves share.

use glam::Vec3;
use jc3vr_body::controllers::{
    grab::{GrabVolume, GripTarget},
    hands::ik,
};
use jc3vr_core::{game_tick::GameTick, published::EventLog};
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::controllers::{
    loadout::contents,
    weapons::{
        ads, c4, collide,
        config::WeaponSettings,
        docks::attract::{Attractor, ZoneWatch},
        dual_wield, engine,
        engine::HeldWeapon,
        foregrip,
        foregrip::GripGeometry,
        grenade, grip_watch, heft, hold,
        hold::armed,
        inventory,
        opening::OpenWatch,
        pouch_models, pouch_return, recoil, regions,
        regions::{GunRegion, RegionHit},
        reload,
        types::WeaponsStatus,
        wheel,
    },
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

/// Why the weapon handling stands down, which decides how much of it stops.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDown {
    /// The controllers are disabled in the settings.
    Disabled,
    /// The gamepad scheme is in effect.
    GamepadScheme,
    /// No controller frame, or no active controller.
    NoControllers,
    /// The hands are not active (no tracked hands to act with).
    HandsInactive,
    /// The game's device manager is disabled, so its device poll does not run.
    PollDisabled,
    /// The mod's debug UI owns the controls.
    ModPanel,
    /// The debug UI's capture of the controls cancelled the tick's routes.
    CaptureCancel,
    /// A game menu or the map is open.
    Menu,
}

/// How much of the weapon handling a [`StandDown`] stops.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StandDownScope {
    /// Skip the tick halves but keep what the hands hold.
    Pause,
    /// Stop acting and forget the tick's weapons, keeping the loadout table.
    Hands,
    /// Stop acting, and put the loadout table back into the game's slots.
    Loadout,
}

impl StandDown {
    /// How much of the weapon handling this reason stops.
    pub fn scope(self) -> StandDownScope {
        match self {
            Self::Disabled | Self::GamepadScheme => StandDownScope::Loadout,
            Self::NoControllers | Self::HandsInactive => StandDownScope::Hands,
            Self::PollDisabled | Self::ModPanel | Self::CaptureCancel | Self::Menu => {
                StandDownScope::Pause
            }
        }
    }
}

/// Stand the weapon handling down for `reason`, as far as its [`StandDown::scope`] says. Called on
/// every input tick the weapon handling's tick halves do not run.
pub fn stand_down(game: &GameTick<'_>, reason: StandDown) {
    match reason.scope() {
        StandDownScope::Pause => pause(),
        StandDownScope::Hands => stand_down_hands(game),
        StandDownScope::Loadout => {
            stand_down_hands(game);
            inventory::stand_down(game);
        }
    }
}

/// Pause for a game menu, the map, or the debug panel, which skips the weapon handling's tick
/// halves but keeps what the hands hold: physical magazine work is cancelled, and the grip release
/// stands down, so that the first tick back ends a gun's hold with the grip as it is then. The
/// debug panel stood the weapons down instead, which took the left hand's SMG out of it the moment
/// the panel opened on the 14 November take.
pub(crate) fn pause() {
    reload::cancel_extraction_on_standdown();
    hold::act::cancel_releases();
}

/// Stop acting: drop the foregrip hold and forget the tick's weapon.
fn stand_down_hands(game: &GameTick<'_>) {
    foregrip::clear();
    contents::clear();
    jc3vr_body::controllers::held::guns::publish(
        jc3vr_body::controllers::held::guns::GunHolds::default(),
    );
    heft::observe([None; 2]);
    heft::freeze([false; 2]);
    ads::observe([None; 2]);
    collide::observe([None; 2]);
    grip_watch::clear();
    recoil::clear();
    grenade::stand_down();
    // SAFETY: the weapon handling stands down in the game stage, on the game thread before the
    // world update.
    unsafe { pouch_models::stand_down() };
    c4::stand_down();
    // SAFETY: as above.
    unsafe { c4::detonator::release_model() };
    pouch_return::stand_down();
    // SAFETY: the game stage runs on the game thread before the world update.
    unsafe { dual_wield::single::stand_down() };
    hold::clear();
    hold::act::clear();
    hold::tick::stand_down();
    hold::take::stand_down();
    regions::stand_down();
    armed::stand_down(game);
    reload::stand_down();
    wheel::stand_down(game);
    let mut state = STATE.lock();
    state.weapon = None;
    state.lone = None;
    state.beside = None;
    state.status.active = false;
    state.status.volumes = [None; 2];
    state.status.fire_blocked = false;
    state.placed.clear();
    state.points = [None; 2];
    state.attractors.clear();
    state.zones = ZoneWatch::new();
    state.last_volumes = [None; 2];
    state.touch = None;
    state.slide = None;
}

pub(super) struct State {
    pub(super) status: WeaponsStatus,
    /// The tick's wielded weapon and its two-handed geometry.
    pub(super) weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// The tick's gun held in one hand while the other is free, wielded or armed, and its
    /// two-handed geometry when it is a long gun ([`crate::controllers::weapons::engine::lone_gun`]).
    pub(super) lone: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// The tick's gun a hand holds beside the wielded weapon.
    pub(super) beside: Option<HeldWeapon>,
    /// Whether the tick's mode is one the weapon handling acts in.
    pub(super) gameplay: bool,
    /// Whether the tick's mode is one the explosive pouches act in
    /// ([`crate::controllers::weapons::begin::pouches`]).
    pub(super) pouches: bool,
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
    /// Where the free hand touches the lone long gun on this tick.
    pub(super) touch: Option<OffTouch>,
    /// Where the bracing hand has slid to along the gun on this tick, in the holding wrist's frame.
    pub(super) slide: Option<(Vec3, GunRegion)>,
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
    weapon: None,
    lone: None,
    beside: None,
    gameplay: false,
    pouches: false,
    measured: [None; 2],
    placed: Vec::new(),
    points: [None; 2],
    attractors: Vec::new(),
    zones: ZoneWatch::new(),
    last_holder: None,
    last_volumes: [None; 2],
    gun_side: Hand::Right,
    touch: None,
    slide: None,
    opening: OpenWatch::new(),
});
