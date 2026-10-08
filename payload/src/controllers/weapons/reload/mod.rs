//! The true reload: the plan of record's "Reloads" (`docs/mod/input/controllers-and-roomscale.md`),
//! described in `docs/mod/input/reload.md`, with the engine side in
//! `docs/engine/gameplay/weapon-ammo-and-reload.md`.
//!
//! Under the controller scheme the game's own reloads of the weapons the mod reloads are stopped
//! ([`gate`]), and the player reloads by hand:
//!
//! - **Eject.** A tap of the reload button ejects the dominant gun's magazine (both pistols' when
//!   dual-wielded), and a flick of a gun hand ejects that gun's ([`eject`]). The game drops the
//!   magazine as a prop from the gun, with the gun's velocity, and hides the magazine part. The
//!   rounds left in it go back to the reserve when the next magazine seats, as the game's own
//!   reload does.
//! - **Load.** The free hand takes a round from the universal pouch on the belt (a magazine, a
//!   shell, a rocket, or a pod, whatever the gun takes) and pushes it into the gun's well
//!   ([`insert`]). The well is the gun's own, from the wells table ([`wells`]), carried to the
//!   hand through the gun's measured placement ([`frames`]). A seated magazine reloads through the
//!   game's own reload, so the reserve, unlimited ammunition, the upgrades, and the HUD all follow
//!   the game's rules; a shell adds one round.
//! - **Belt.** Pushing a gun's well down onto the pouch reloads it in one hand, swapping a magazine
//!   still in it, and works its action ([`belt`]).
//! - **Chamber.** A gun that ran dry fires again only once its action is worked by hand; a bolt
//!   or a pump is worked after every shot ([`chamber`]).
//! - **Dual-wield.** Each pistol has its own half of the magazine ([`dual`]).
//!
//! - **Two weapons.** With a second weapon in the other hand, the reload acts on whichever of the
//!   two is lower ([`focus`]); with both hands full, that is a flick to eject and the belt push.
//!
//! What a weapon loads and how is its plan ([`plan`]). The minigun, the mounted guns, and the
//! special weapons keep the game's reload. With the reload off, every weapon does.

pub mod belt;
pub mod chamber;
pub mod config;
pub mod dual;
pub mod eject;
mod engine;
pub mod focus;
pub mod frames;
pub(super) mod gate;
pub mod insert;
pub mod plan;
mod render;
mod steps;
pub mod wells;

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    bindings::{
        controls::ButtonControl,
        grab::{Candidate, GrabVolume},
    },
    config::ControllersConfig,
    frame::{Hand, Pose},
    hands::{ActiveHands, pose::CockpitHand},
    weapons::{
        engine::HeldWeapon,
        reload::{
            belt::{BeltPush, PushStage},
            chamber::{ActionWork, Chamber},
            config::{ReloadButton, ReloadConfig},
            dual::SplitMagazines,
            eject::{FlickDetector, TapDetector},
            engine::Reading,
            focus::{Fill, ReloadTarget},
            frames::GunFrame,
            insert::{Insertion, Well},
            plan::ReloadPlan,
        },
        volumes::{self, BodyFrame},
    },
};

/// The reload's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    engine::hook_library()
}

/// The reload's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReloadStatus {
    /// Whether the last input tick reloaded a weapon by hand.
    pub active: bool,
    pub weapon: Option<WeaponReadout>,
    pub insertion: Insertion,
    /// The hand holding a round, and the gun's hand it is for.
    pub insert_hands: Option<(Hand, Hand)>,
    /// Each gun hand's belt push, by [`Hand::index`].
    pub belt: [PushStage; 2],
    pub work: ActionWork,
    /// The angular speed of each hand over the last tick, degrees per second.
    pub flick_dps: [f32; 2],
    /// The hands whose guns may not fire, by [`Hand::index`].
    pub fire_held: [bool; 2],
    pub counts: ReloadCounts,
    /// The most recent reload events, newest last.
    pub events: Vec<String>,
    /// The weapon the reload acts on while the hands hold two.
    pub focus: ReloadTarget,
}

/// The reloaded weapon as the last input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WeaponReadout {
    pub name_hash: u32,
    pub plan: ReloadPlan,
    /// Whether the plan's well came from the wells table.
    pub from_table: bool,
    pub magazine: i32,
    pub capacity: i32,
    pub reserve: i32,
    pub unlimited: bool,
    /// The game's magazine-out flag.
    pub out: bool,
    /// Each gun hand's chamber, by [`Hand::index`].
    pub chambers: [Option<Chamber>; 2],
    /// The dual-wield split, when it acts.
    pub split: Option<SplitMagazines>,
}

/// The reload's counters since the payload loaded.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReloadCounts {
    pub ejects: u64,
    pub seats: u64,
    pub belt_reloads: u64,
    pub racks: u64,
    pub dry_fires: u64,
    pub empty_pouch: u64,
    /// Reload starts of the game's that the stop refused.
    pub refused: u64,
}

pub fn status() -> ReloadStatus {
    let mut status = STATE.lock().status.clone();
    status.counts.refused = gate::refused();
    status
}

/// The hands whose guns may not fire, as of the last input tick, for the dual-wield split's
/// triggers.
pub(crate) fn fire_held() -> [bool; 2] {
    STATE.lock().status.fire_held
}

/// What the first half of the weapon handling's input tick hands the reload.
pub(super) struct BeginInputs<'a> {
    pub weapon: Option<&'a HeldWeapon>,
    /// The hands holding any gun, of this weapon or another, by [`Hand::index`].
    pub occupied: [bool; 2],
    pub active: &'a ActiveHands,
    pub config: &'a ControllersConfig,
    pub body_frame: &'a BodyFrame,
    pub dims: &'a crate::controllers::body::BodyDimensions,
    /// The hand the pouches are laid out around ([`volumes::gun_side`]).
    pub gun_side: Hand,
    /// Each hand's point for the body's volumes, by [`Hand::index`].
    pub body_points: [Option<Vec3>; 2],
    /// Whether the tick's mode is one the weapon handling acts in.
    pub gameplay: bool,
}

/// The first half of the reload's input tick: read the weapon and its plan, carry its guns into
/// the cockpit frame, and place the pouch and the action's handle. Returns the volumes to place.
pub(super) fn begin_tick(inputs: &BeginInputs) -> Vec<Candidate> {
    let BeginInputs {
        weapon,
        occupied,
        active,
        config,
        body_frame,
        dims,
        gun_side,
        body_points,
        gameplay,
    } = *inputs;
    let reload = &config.weapons.reload;
    let mut state = STATE.lock();
    let acting = reload.enabled && gameplay;
    let Some(weapon) = weapon.filter(|_| acting) else {
        state.clear_tick();
        return Vec::new();
    };
    // SAFETY: the input tick runs on the main thread outside the world update, and the weapon was
    // read this tick.
    let Some(tick) = (unsafe { state.read_tick(weapon, active, config, body_points) }) else {
        state.clear_tick();
        return Vec::new();
    };
    let mut candidates = Vec::new();
    let gun_hands = weapon.wielded.hands;
    let free = Hand::BOTH.into_iter().find(|h| {
        !gun_hands[h.index()] && !occupied[h.index()] && active.hands[h.index()].is_some()
    });
    let pouch = volumes::magazine_pouch(body_frame, dims, &config.weapons.pouches, gun_side, free);
    if free.is_some() {
        candidates.push(pouch);
    }
    if let Some(handle) = state.handle_candidate(&tick, free, reload) {
        candidates.push(handle);
    }
    state.tick = Some(tick);
    state.pouch = Some((pouch.centre, pouch.radius));
    state.free = free;
    candidates
}

/// What the bindings hand the second half of the reload's input tick.
pub(super) struct FinishInputs {
    /// The grab volume each hand grabbed this tick, by [`Hand::index`].
    pub grabs: [Option<GrabVolume>; 2],
    pub grips: [ButtonControl; 2],
    pub triggers: [ButtonControl; 2],
    /// The reload action's press over the tick.
    pub reload_button: ButtonControl,
    pub dt: f32,
}

/// The second half of the reload's input tick: eject, load, push, and work the action, and decide
/// which guns may fire. Returns the hands whose guns may not fire.
pub(super) fn finish_tick(inputs: &FinishInputs, config: &ControllersConfig) -> [bool; 2] {
    let reload = &config.weapons.reload;
    let mut state = STATE.lock();
    let Some(tick) = state.tick else {
        state.stand_down();
        drop(state);
        gate::publish(None);
        return [false; 2];
    };
    let held = state.step(&tick, inputs, config);
    state.status.fire_held = held;
    state.status.active = true;
    drop(state);
    gate::publish(Some(gate::Published {
        weapon: tick.weapon,
        button_native: reload.button == ReloadButton::Native,
        manual_action: reload.manual_action
            && tick.plan.action.manual()
            && steps::chambering(&tick.plan, reload),
        held,
    }));
    held
}

/// Stop acting: drop any round in hand, forget the tick, and hand the weapons back to the game.
pub(super) fn stand_down() {
    STATE.lock().stand_down();
    gate::publish(None);
    render::clear();
}

/// The weapon the reload acts on this tick, from the wielded weapon's magazine (`primary`) and the
/// second weapon's (`second`, `None` without one). Logs a turn from one to the other.
pub(super) fn focus(primary: Option<Fill>, second: Option<Fill>) -> ReloadTarget {
    let mut state = STATE.lock();
    let busy = state.status.insert_hands.is_some() || state.belts.iter().any(BeltPush::holds_fire);
    let current = state.status.focus;
    let next = focus::choose(current, primary, second, busy);
    state.status.focus = next;
    drop(state);
    if next != current {
        tracing::info!(
            target: "controllers",
            target_weapon = next.label(),
            "reload: turned to {}",
            next.label()
        );
    }
    next
}

/// The magazine of the weapon at `address`, for [`focus`].
///
/// # Safety
///
/// `address` must be a live weapon read this tick, on the main thread outside the world update.
pub(super) unsafe fn fill_of(address: usize) -> Option<Fill> {
    let weapon = unsafe { (address as *const WeaponBase).as_ref() }?;
    let reading = unsafe { engine::read(weapon) };
    Some(Fill {
        magazine: reading.magazine,
        capacity: reading.capacity,
        out: reading.out,
    })
}

/// The well's mouth and radius to ring while a round is in hand.
pub(super) fn well_ring(config: &ReloadConfig) -> Option<(Vec3, f32)> {
    let state = STATE.lock();
    let (_, gun_hand) = state.status.insert_hands?;
    let tick = state.tick.as_ref()?;
    let well = tick.guns[gun_hand.index()].as_ref()?.well?;
    config
        .show_well
        .then_some((well.mouth, config.capture_radius_m))
}

/// The hand holding a round, for the overlay's marker.
pub(super) fn holding_hand() -> Option<Hand> {
    STATE.lock().status.insert_hands.map(|(hand, _)| hand)
}

/// The matrix to push for `part` of the weapon at `weapon` in place of `matrix`, from the late
/// update of the held gun's parts on the render thread: a slide held back or worked, or a magazine
/// part in the hand. `None` leaves the part as it is.
pub(crate) fn adjust_part(
    part: &jc3gi::equipment::weapon::ModelAttachementWeaponComponent,
    weapon: usize,
    matrix: glam::Mat4,
    dtf: f32,
) -> Option<glam::Mat4> {
    render::adjust(part, weapon, matrix, dtf)
}

/// A shot of `weapon` from `hand` that left `magazine` rounds, from the per-shot seam on the
/// weapon's thread.
pub(super) fn on_shot(weapon: usize, hand: Hand, magazine: i32) {
    gate::on_shot(weapon, hand, magazine);
}

/// Whether the gun of `weapon` in `hand` is held from firing, from the ammunition check on the
/// weapon's thread.
pub(super) fn holds_fire(weapon: usize, hand: Hand) -> bool {
    gate::holds_fire(weapon, hand)
}

/// How many weapons' chambers and splits are remembered across weapon changes.
const MAX_RECORDS: usize = 8;
/// How many events the readout keeps.
const MAX_EVENTS: usize = 8;

/// One input tick's view of the reloaded weapon.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Tick {
    weapon: usize,
    holder: Hand,
    /// The dominant hand, which takes the split's odd round.
    dominant: Hand,
    /// Whether the weapon is the dual-wield pair, split per hand.
    split: bool,
    plan: ReloadPlan,
    from_table: bool,
    reading: Reading,
    reserve: i32,
    /// Each gun hand's gun, by [`Hand::index`].
    guns: [Option<GunTick>; 2],
    /// The tick's hands, as tracked.
    hands: [Option<CockpitHand>; 2],
    world_scale: f32,
}

/// One gun of the tick.
#[derive(Debug, Copy, Clone, PartialEq)]
struct GunTick {
    index: usize,
    wrist: Pose,
    frame: Option<GunFrame>,
    well: Option<Well>,
    /// The action's handle and the gun's rearward axis.
    handle: Option<(Vec3, Vec3)>,
    /// The gun hand's grip as drawn, and where the same hand reaches the body's pouches
    /// ([`belt::into_body_frame`]).
    drawn_grip: Vec3,
    body_grip: Option<Vec3>,
}

/// What the reload remembers about a weapon.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
struct Record {
    chambers: [Chamber; 2],
    split: Option<SplitMagazines>,
    /// The game's magazine as of the last tick.
    last_magazine: Option<i32>,
}

struct State {
    status: ReloadStatus,
    tick: Option<Tick>,
    pouch: Option<(Vec3, f32)>,
    free: Option<Hand>,
    /// The remembered weapons, by address, the most recent last.
    records: Vec<(usize, Record)>,
    /// Each gun hand's measured gun relative to its wrist, and the weapon it belongs to.
    measured: [Option<(usize, Pose)>; 2],
    tap: TapDetector,
    flicks: [FlickDetector; 2],
    belts: [BeltPush; 2],
    /// Each hand's grip and each gun's mouth on the last tick, for their velocities.
    last_grips: [Option<Vec3>; 2],
    last_mouths: [Option<Vec3>; 2],
    /// The weapon and its gun hands as of the last tick, and the seconds since they changed.
    drawn: Option<(usize, [bool; 2])>,
    since_draw_s: f32,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: ReloadStatus {
        active: false,
        weapon: None,
        insertion: Insertion::new(),
        insert_hands: None,
        belt: [PushStage::Outside; 2],
        work: ActionWork::new(),
        flick_dps: [0.0; 2],
        fire_held: [false; 2],
        counts: ReloadCounts {
            ejects: 0,
            seats: 0,
            belt_reloads: 0,
            racks: 0,
            dry_fires: 0,
            empty_pouch: 0,
            refused: 0,
        },
        events: Vec::new(),
        focus: ReloadTarget::Primary,
    },
    tick: None,
    pouch: None,
    free: None,
    records: Vec::new(),
    measured: [None; 2],
    tap: TapDetector::new(),
    flicks: [FlickDetector::new(); 2],
    belts: [BeltPush::new(); 2],
    last_grips: [None; 2],
    last_mouths: [None; 2],
    drawn: None,
    since_draw_s: 0.0,
});
