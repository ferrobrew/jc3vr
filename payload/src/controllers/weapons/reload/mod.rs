//! The true reload: the plan of record's "Reloads" (`docs/mod/input/controllers-and-roomscale.md`),
//! described in `docs/mod/input/reload.md`, with the engine side in
//! `docs/engine/gameplay/weapon-ammo-and-reload.md`.
//!
//! Under the controller scheme the game's own reloads of the weapons the mod reloads are stopped
//! ([`gate`]), and the player reloads by hand:
//!
//! - **Eject.** A tap of the reload button ejects the dominant hand's gun's magazine, or the other
//!   hand's while the dominant hand holds no gun, and a flick of a gun hand ejects that gun's
//!   ([`eject`]). The game drops the
//!   magazine as a prop from the gun, with the gun's velocity, and hides the magazine part. The
//!   magazine keeps the rounds left in it as an object in the world, to be picked up and seated
//!   again ([`loose`]); with that off, they go back to the reserve when the next magazine seats,
//!   as the game's own reload does.
//! - **Load.** The free hand takes a round from the universal pouch on the belt (a magazine, a
//!   shell, a rocket, or a pod, whatever the gun takes) and pushes it into the gun's well
//!   ([`insert`]). The well is the gun's own, from the wells table ([`wells`]), carried to the
//!   hand through the gun's measured placement ([`frames`]). A seated magazine reloads through the
//!   game's own reload, so the reserve, unlimited ammunition, the upgrades, and the HUD all follow
//!   the game's rules; a shell adds one round.
//! - **Belt.** Pushing a gun's well down onto the pouch reloads it in one hand, swapping a magazine
//!   still in it, and works its action ([`belt`]).
//! - **Chamber.** A gun that ran dry fires again only once its action is worked by hand; a bolt
//!   or a pump is worked after every shot ([`chamber`]). The action is a part that follows the
//!   hand through its travel ([`travel`]), drawn on a separate slide part or on the gun's own
//!   skeleton ([`render`], [`bones`]).
//! - **Two weapons.** With a gun in each hand, the reload acts on each hand's gun at once, each
//!   with its own weapon's plan, magazine, and chamber. With both hands full, no hand is free to
//!   take a round, so a flick ejects and the belt push loads.
//!
//! What the reload remembers of a weapon, its chambers and its action, outlives the weapon's time
//! in the hands ([`records`]).
//!
//! What a weapon loads and how is its plan ([`plan`]). The minigun, the mounted guns, and the
//! special weapons keep the game's reload. With the reload off, every weapon does.

mod action;
mod ammo;
pub mod belt;
mod bones;
pub mod chamber;
pub mod config;
mod deferred;
pub mod eject;
mod engine;
#[cfg(test)]
mod engine_tests;
mod extraction;
pub mod frames;
pub(super) mod gate;
pub mod insert;
pub mod loose;
pub mod plan;
mod records;
mod render;
pub(crate) mod serial;
mod steps;
pub mod travel;
pub mod wells;

use glam::Vec3;
use jc3gi::equipment::weapon::WeaponBase;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::controllers::{
    bindings::{
        controls::ButtonControl,
        tables::describe::{Control, Effect, Gesture, ModAction, Row},
    },
    body::rig::frames::Frames,
    config::ControllersConfig,
    frame::{Hand, Pose},
    grab::{Candidate, GrabVolume, GripTarget},
    hands::{ActiveHands, pose::CockpitHand},
    weapons::{
        capabilities::{GunHand, GunOwner},
        docks,
        dual_wield::single::magazine::Dealt,
        engine::HeldWeapon,
        inventory::engine::strong_to,
        pouch_models,
        reload::{
            belt::{BeltPush, PushStage},
            chamber::{ActionWork, Chamber},
            config::{ReloadButton, ReloadConfig},
            eject::{FlickDetector, TapDetector},
            engine::Reading,
            extraction::{DetachRequest, PendingDetach, Pull},
            frames::GunFrame,
            insert::{Insertion, InsertionSource, Well},
            plan::ReloadPlan,
            records::{RecordKey, Records, gun_entry},
            travel::ActionTravel,
        },
        volumes::{self, BodyFrame},
    },
};

/// The reload's hooks, aggregated by [`crate::controllers::weapons::hook_library`].
pub(crate) fn hook_library() -> HookLibrary {
    engine::hook_library()
        .with_hook_library(loose::hook_library())
        .with_hook_library(bones::hook_library())
}

/// The reload's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReloadStatus {
    /// Whether the last input tick reloaded a weapon by hand.
    pub active: bool,
    /// The weapon of each hand's gun, by [`Hand::index`]; the game's dual-wield pair's is in both
    /// hands.
    pub weapons: [Option<WeaponReadout>; 2],
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
    /// Native serial magazine execution and the most recent refusal.
    pub serial: serial::SerialStatus,
    /// The most recent reload events, newest last.
    pub events: Vec<String>,
}

/// A hand's gun and its weapon as the last input tick saw them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WeaponReadout {
    /// The weapon's address, which tells the game's dual-wield pair's two readouts for one weapon.
    pub address: usize,
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
    /// The hand's gun's chamber.
    pub chamber: Chamber,
    /// The hand's gun's action on its travel, with the physical action on.
    pub action: Option<ActionTravel>,
}

/// The reload's counters since the payload loaded.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ReloadCounts {
    pub ejects: u64,
    pub seats: u64,
    pub belt_reloads: u64,
    pub racks: u64,
    /// Slides caught at the rear.
    pub lock_backs: u64,
    pub dry_fires: u64,
    pub empty_pouch: u64,
    /// Reload starts of the game's that the stop refused.
    pub refused: u64,
}

pub fn status() -> ReloadStatus {
    let mut status = STATE.lock().status.clone();
    status.counts.refused = gate::refused();
    status.serial = serial::status();
    status
}

/// What the first half of the weapon handling's input tick hands the reload.
pub(super) struct BeginInputs<'a> {
    /// The weapons in the hands: the wielded weapon, then the one beside it. Each one's guns are
    /// reloaded in the hands its [`Wielded::hands`] marks.
    ///
    /// [`Wielded::hands`]: crate::controllers::weapons::engine::Wielded::hands
    pub weapons: [Option<&'a HeldWeapon>; 2],
    /// The hands holding any gun, of these weapons or another, by [`Hand::index`].
    pub occupied: [bool; 2],
    pub active: &'a ActiveHands,
    pub config: &'a ControllersConfig,
    /// The character's body the pouch rides, when its skeleton could be read.
    pub body: Option<&'a BodyFrame>,
    /// The hand the pouches are laid out around ([`volumes::gun_side`]).
    pub gun_side: Hand,
    /// Whether the tick's mode is one the weapon handling acts in.
    pub gameplay: bool,
}

/// The first half of the reload's input tick: read the hands' weapons and their plans, carry their
/// guns into the cockpit frame, and place the pouch and the action's handle. Returns the volumes to
/// place.
pub(super) fn begin_tick(inputs: &BeginInputs) -> Vec<Candidate> {
    let BeginInputs {
        weapons,
        occupied,
        active,
        config,
        body,
        gun_side,
        gameplay,
    } = *inputs;
    let reload = &config.weapons.reload;
    // SAFETY: bounded world-record maintenance runs on the input thread before eligibility exits.
    unsafe { loose::maintain(&reload.loose) };
    let mut state = STATE.lock();
    if !(reload.enabled && gameplay) {
        state.clear_tick();
        return Vec::new();
    }
    // SAFETY: the input tick runs on the main thread outside the world update, and the weapons
    // were read this tick.
    let Some(tick) = (unsafe { state.read_tick(weapons, active, config) }) else {
        state.clear_tick();
        return Vec::new();
    };
    let mut candidates = Vec::new();
    let gun_hands = tick.gun_hands();
    let free = Hand::BOTH.into_iter().find(|h| {
        !gun_hands[h.index()] && !occupied[h.index()] && active.hands[h.index()].is_some()
    });
    let loading_hand = insert::loading_hand(gun_hands, free, tick.dominant);
    let pouch_side = loading_hand.map_or(gun_side.other(), Hand::other);
    let pouch = body.map(|b| {
        let pouch = volumes::magazine_pouch(
            b,
            &config.weapons.pouches,
            pouch_side.other(),
            Some(pouch_side),
        );
        Candidate {
            hand: free,
            ..pouch
        }
    });
    // The displayed source stays on the loading gun's opposite side while its taker is occupied.
    let loaded = loading_hand
        .and_then(|hand| tick.gun(hand))
        .map(|(weapon, _)| weapon);
    // A pouch with nothing to give is not offered: no round comes out of it.
    let stocked = loaded.is_some_and(WeaponTick::has_ammo);
    let target_pouch = pouch.map(|mut pouch| {
        if config.weapons.pouches.models.enabled
            && let Some(body) = body
        {
            pouch.centre = pouch_models::display_centre(
                Pose {
                    position: pouch.centre,
                    orientation: body.pelvis,
                },
                &config.weapons.pouches.models.magazine,
                pouch_side,
                tick.world_scale,
            );
        }
        pouch
    });
    if let Some(pouch) = target_pouch
        && free.is_some()
        && loaded.is_some_and(|weapon| stocked && state.can_take(weapon))
        && !state.status.insertion.holding()
    {
        candidates.push(pouch);
    }
    if let Some(handle) = state.handle_candidate(&tick, free, reload) {
        candidates.push(handle);
    }
    // SAFETY: as above.
    let (seen, frames) = unsafe { loose_sightings(&tick, free, active, reload) };
    state.loose_seen = seen;
    state.frames = frames;
    if state.frames.is_some()
        && let Some(magazine) = state.extraction_candidate(&tick, free, reload)
    {
        candidates.push(magazine);
    }
    if let Some(hand) = free {
        candidates.extend(loose::candidates(
            &state.loose_seen,
            reload.loose.radius_m,
            hand,
        ));
    }
    state.tick = Some(tick);
    state.pouch = target_pouch.map(|p| (p.centre, p.radius));
    state.stocked_pouch = pouch
        .zip(loaded)
        .filter(|_| stocked)
        .map(|(p, weapon)| StockedPouch {
            centre: p.centre,
            side: pouch_side,
            weapon: weapon.address,
        });
    state.free = free;
    candidates
}

/// The hands participating in the current physical action, before ordinary release routing.
pub(super) fn action_hands() -> [bool; 2] {
    let mut state = STATE.lock();
    let Some(tick) = state.tick else {
        return [false; 2];
    };
    let working = Hand::BOTH.into_iter().any(|hand| {
        tick.gun(hand).is_some_and(|(weapon, gun)| {
            state.records.get(weapon.key).travels[gun.entry()].holds_release(weapon.plan.action)
        })
    });
    [working; 2]
}

/// Resolve direct magazine intent before ordinary nearest-target selection. An existing action
/// or foregrip remains held; an unheld brace or transfer cannot steal a magazine grip.
pub(super) fn prioritize_extraction(targets: &mut Vec<GripTarget>, grips: [Option<Vec3>; 2]) {
    let (pulling, action) = {
        let mut state = STATE.lock();
        let action = state.tick.and_then(|tick| {
            Hand::BOTH.into_iter().find_map(|hand| {
                let (weapon, gun) = tick.gun(hand)?;
                (weapon.plan.action != chamber::ActionKind::Pump
                    && state.records.get(weapon.key).travels[gun.entry()]
                        .grip
                        .is_some())
                .then_some(hand.other())
            })
        });
        (state.extraction_hand(), action)
    };
    if let Some(hand) = action {
        for target in targets
            .iter_mut()
            .filter(|t| t.volume == GrabVolume::WeaponAction && t.hand == Some(hand))
        {
            *target = target.held(hand);
        }
        targets.retain(|t| !t.for_hand(hand) || t.volume == GrabVolume::WeaponAction);
    }
    if let Some(hand) = pulling {
        for target in targets
            .iter_mut()
            .filter(|t| t.volume == GrabVolume::InsertedMagazine && t.hand == Some(hand))
        {
            *target = target.held(hand);
        }
        targets.retain(|t| !t.for_hand(hand) || t.volume == GrabVolume::InsertedMagazine);
    }
    extraction::prioritize(targets, grips);
}

/// The magazine pouch on a tick it has a round to give: its centre in the cockpit frame, the side
/// of the body it is on, and the weapon of the gun a round from it is for (a `WeaponBase`
/// address).
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct StockedPouch {
    pub centre: Vec3,
    pub side: Hand,
    pub weapon: usize,
}

/// The magazine pouch on this tick, while it has a round to give.
pub(super) fn stocked_pouch() -> Option<StockedPouch> {
    STATE.lock().stocked_pouch
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
/// which guns may fire. The copied finish facts are consumed after weapon ownership settles.
pub(super) fn finish_tick(inputs: &FinishInputs, config: &ControllersConfig) -> FinishResult {
    let reload = &config.weapons.reload;
    let mut state = STATE.lock();
    let Some(tick) = state.tick else {
        state.stand_down();
        drop(state);
        gate::publish(None);
        return FinishResult {
            held: [false; 2],
            belt: None,
        };
    };
    let held = state.step(&tick, inputs, config);
    state.status.fire_held = held;
    state.status.active = true;
    let belt = state.belt_finish(&tick);
    drop(state);
    let weapons = Hand::BOTH.map(|hand| tick.gun(hand).map(|(weapon, _)| weapon));
    gate::publish(Some(gate::Published {
        weapons: weapons.map(|w| w.map_or(0, |w| w.address)),
        button_native: reload.button == ReloadButton::Native,
        manual_action: weapons.map(|w| {
            w.is_some_and(|w| {
                reload.manual_action && w.plan.action.manual() && steps::chambering(&w.plan, reload)
            })
        }),
        held,
    }));
    FinishResult {
        held,
        belt: Some(belt),
    }
}

/// The reload's fire hold and copied post-step belt facts, not its persistent debug readout.
pub(super) struct FinishResult {
    pub held: [bool; 2],
    belt: Option<BeltFinish>,
}

/// A same-input-tick offer for the dominant hand's exact belt reload motion.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct BeltEligibility {
    sequence: u64,
    hand: Hand,
    guns: [Option<GunOwner>; 2],
}

impl BeltEligibility {
    /// Final claims and hand readiness can revoke the offer without another engine query.
    pub(crate) fn row(
        self,
        sequence: u64,
        dominant: Hand,
        guns: [Option<GunOwner>; 2],
        ready: [bool; 2],
    ) -> Option<Row> {
        (self.sequence == sequence
            && self.hand == dominant
            && self.guns == guns
            && ready[self.hand.index()])
        .then_some(Row {
            control: Control::Motion(self.hand),
            gesture: Gesture::Press,
            effect: Effect::Mod(ModAction::BeltReload),
        })
    }
}

/// The reload owner's final read, after all weapon actions, while matching gun claims prove life.
impl FinishResult {
    pub(super) fn belt_eligibility(
        self,
        sequence: u64,
        config: &ControllersConfig,
        source_guns: [Option<GunOwner>; 2],
        guns: [Option<GunOwner>; 2],
    ) -> Option<BeltEligibility> {
        let finish = self.belt?;
        finish.eligibility_with(sequence, config, source_guns, guns, |address| {
            // SAFETY: the matching current firing grip or native pair owns this tick's weapon.
            let weapon =
                unsafe { (address as *const jc3gi::equipment::weapon::WeaponBase).as_ref() }?;
            let reading = unsafe { engine::read(weapon) };
            if reading.exempt {
                return None;
            }
            Some((reading, unsafe { engine::reserve(weapon) }))
        })
    }
}

#[derive(Debug, Copy, Clone)]
struct BeltFinish {
    sequence: Option<u64>,
    hand: Hand,
    weapon: Option<usize>,
    gate: BeltGate,
    well: bool,
    pouch: bool,
}

impl BeltFinish {
    fn eligibility_with(
        self,
        sequence: u64,
        config: &ControllersConfig,
        source_guns: [Option<GunOwner>; 2],
        guns: [Option<GunOwner>; 2],
        mut read: impl FnMut(usize) -> Option<(Reading, i32)>,
    ) -> Option<BeltEligibility> {
        if source_guns != guns {
            return None;
        }
        let address = self.weapon?;
        let current = guns[self.hand.index()]?;
        if !config.weapons.reload.enabled
            || self.sequence != Some(sequence)
            || self.hand != config.acting_hand()
            || current.weapon != address
            || !matches!(current.role, GunHand::Grip | GunHand::NativePair(_))
            || !self.well
            || !self.pouch
        {
            return None;
        }
        let (reading, reserve) = read(address)?;
        let gate = BeltGate {
            has_ammo: reading.unlimited || reserve > 0,
            can_belt: reading.out || reading.has_space(),
            ..self.gate
        };
        gate.allows(&config.weapons.reload)
            .then_some(BeltEligibility {
                sequence,
                hand: self.hand,
                guns,
            })
    }
}

/// The exact non-motion gate shared by belt execution and its current offer.
#[derive(Debug, Copy, Clone)]
struct BeltGate {
    holding: bool,
    extracting: bool,
    blocked: bool,
    since_draw_s: f32,
    has_ammo: bool,
    can_belt: bool,
}

impl BeltGate {
    fn allows(self, reload: &ReloadConfig) -> bool {
        reload.belt.enabled
            && !self.holding
            && !self.extracting
            && !self.blocked
            && self.since_draw_s >= reload.belt.after_draw_s
            && self.has_ammo
            && self.can_belt
    }
}

#[cfg(test)]
pub(crate) fn eligible_belt_for_test(
    config: &ControllersConfig,
) -> (BeltEligibility, [Option<GunOwner>; 2]) {
    steps::eligible_belt_for_test(config)
}

/// Cancel only physical magazine manipulation on a mode boundary. Other weapon holds remain.
/// The resumed input path must observe neutral grips before accepting a fresh magazine claim.
pub(crate) fn cancel_extraction_on_standdown() {
    let mut state = STATE.lock();
    state.cancel_extraction();
    state.cancel_ejects();
}

/// Stop acting: drop any round in hand, forget the tick, and hand the weapons back to the game.
pub(super) fn stand_down() {
    STATE.lock().stand_down();
    gate::publish(None);
    render::clear();
}

/// The magazines in the world that fit the gun the free hand would load (the gun in the other
/// hand), while its magazine is out.
///
/// # Safety
///
/// The main thread, outside the world update, with the tick's weapons read this tick.
unsafe fn loose_sightings(
    tick: &Tick,
    free: Option<Hand>,
    active: &ActiveHands,
    reload: &ReloadConfig,
) -> (Vec<loose::Sighting>, Option<Frames>) {
    let Some((weapon, gun)) = free.and_then(|free| tick.gun(free.other())) else {
        return (Vec::new(), None);
    };
    let wants = reload.loose.enabled
        && weapon.plan.load == plan::LoadKind::Magazine
        && (weapon.reading.out || reload.extraction.enabled);
    if !wants {
        return (Vec::new(), None);
    }
    let Some(tag) = (unsafe { engine::magazine_tag(weapon.address, gun.index) }) else {
        return (Vec::new(), None);
    };
    let Some(player) = (unsafe { engine::local_player_matrix() }) else {
        return (Vec::new(), None);
    };
    let Some(frames) = docks::cockpit_frames(active, player) else {
        return (Vec::new(), None);
    };
    let seen = if weapon.reading.out {
        unsafe { loose::sightings(player.w_axis.truncate(), tag, &frames, &reload.loose) }
    } else {
        Vec::new()
    };
    (seen, Some(frames))
}

/// The hand holding the gun, and the well's mouth and capture radius, while the other hand holds a
/// round for it.
pub(super) fn well(config: &ReloadConfig) -> Option<(Hand, Vec3, f32)> {
    let state = STATE.lock();
    let (_, gun_hand) = state.status.insert_hands?;
    let tick = state.tick.as_ref()?;
    let well = tick.guns[gun_hand.index()].as_ref()?.well?;
    Some((gun_hand, well.mouth, config.capture_radius_m))
}

/// The hand holding a round or pulling an inserted magazine, for occupancy and the overlay.
pub(super) fn holding_hand() -> Option<Hand> {
    let state = STATE.lock();
    state
        .status
        .insert_hands
        .map(|(hand, _)| hand)
        .or_else(|| state.extraction_hand())
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
    let total = if gate::tracks(weapon, hand) {
        ammo::on_shot(weapon, hand.index(), magazine).unwrap_or(magazine)
    } else {
        magazine
    };
    if total != magazine {
        // SAFETY: the per-shot caller keeps this weapon live on its native update thread.
        unsafe { engine::set_shot_ammo(weapon, total) };
    }
    gate::on_shot(weapon, hand, total);
}

/// Whether the gun of `weapon` in `hand` is held from firing, from the ammunition check on the
/// weapon's thread.
pub(super) fn holds_fire(weapon: usize, hand: Hand) -> bool {
    gate::holds_fire(weapon, hand)
        || (gate::tracks(weapon, hand) && ammo::holds_fire(weapon, hand.index()))
}

/// Preserve each chamber when an observed native pair is dealt to two single pistols.
///
/// # Safety
///
/// Both weapons must be live and owned on the input thread at the pair's native ammo allocation.
pub(crate) unsafe fn split_pair(pair: &WeaponBase, twin: &WeaponBase, tuned: i32) -> Option<Dealt> {
    let owner = unsafe { strong_to(twin) }?;
    ammo::split_pair(
        pair as *const _ as usize,
        twin as *const _ as usize,
        owner,
        twin.m_User[1] as usize,
        ammo::NativeAmmo {
            total: pair.m_MagazineAmmo,
            out: pair.m_WeaponIsWithoutMagazine,
        },
        tuned,
    )
}

/// Loaded rounds excluded from a single pistol's magazine-capacity clamp.
pub(crate) fn chamber_rounds(weapon: usize) -> i32 {
    ammo::protected_rounds(weapon)
}

/// The native ammo check may see a magazine-less weapon as loaded only for a live owned chamber.
pub(super) fn chamber_without_magazine(weapon: usize, hand: Hand) -> bool {
    gate::tracks(weapon, hand)
        && !holds_fire(weapon, hand)
        && ammo::active_snapshot(weapon).is_some_and(|ammo| ammo.out && ammo.chamber_rounds() > 0)
}

/// How many events the readout keeps.
const MAX_EVENTS: usize = 8;

/// One input tick's view of the hands' guns.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Tick {
    sequence: Option<u64>,
    /// The weapons the reload acts on, in the order of [`BeginInputs::weapons`]. A weapon the
    /// reload leaves to the game is not among them.
    weapons: [Option<WeaponTick>; 2],
    /// The dominant hand, whose gun a tap ejects first.
    dominant: Hand,
    /// Each hand's gun, by [`Hand::index`].
    guns: [Option<GunTick>; 2],
    /// The tick's hands, as tracked.
    hands: [Option<CockpitHand>; 2],
    world_scale: f32,
}

impl Tick {
    /// `hand`'s gun and the weapon it belongs to.
    fn gun(&self, hand: Hand) -> Option<(&WeaponTick, &GunTick)> {
        let gun = self.guns[hand.index()].as_ref()?;
        Some((self.weapons.get(gun.weapon)?.as_ref()?, gun))
    }

    /// The tick's weapon at `address`.
    fn weapon(&self, address: usize) -> Option<&WeaponTick> {
        self.weapons.iter().flatten().find(|w| w.address == address)
    }

    /// The hands holding a gun of the tick's weapons, by [`Hand::index`].
    fn gun_hands(&self) -> [bool; 2] {
        self.guns.map(|g| g.is_some())
    }
}

/// One weapon of the tick.
#[derive(Debug, Copy, Clone, PartialEq)]
struct WeaponTick {
    address: usize,
    /// The weapon's record.
    key: RecordKey,
    plan: ReloadPlan,
    from_table: bool,
    reading: Reading,
    reserve: i32,
}

impl WeaponTick {
    /// Whether a round for the weapon can be taken from the pouch.
    fn has_ammo(&self) -> bool {
        self.reading.unlimited || self.reserve > 0
    }
}

/// One gun of the tick.
#[derive(Debug, Copy, Clone, PartialEq)]
struct GunTick {
    /// The weapon it belongs to, as an index into [`Tick::weapons`].
    weapon: usize,
    /// The gun's index within its weapon.
    index: usize,
    magazine: Option<serial::Identity>,
    wrist: Pose,
    frame: Option<GunFrame>,
    well: Option<Well>,
    /// The action's handle and the gun's rearward axis.
    handle: Option<(Vec3, Vec3)>,
}

impl GunTick {
    /// The gun's entry in its weapon's record.
    fn entry(&self) -> usize {
        gun_entry(self.index)
    }
}

struct State {
    status: ReloadStatus,
    tick: Option<Tick>,
    pouch: Option<(Vec3, f32)>,
    /// The pouch on the tick it has a round to give.
    stocked_pouch: Option<StockedPouch>,
    free: Option<Hand>,
    /// The magazines in the world that fit the tick's gun, within reach.
    loose_seen: Vec<loose::Sighting>,
    /// The tick's transforms between the cockpit frame and the world, while a magazine is wanted.
    frames: Option<Frames>,
    /// The remembered weapons.
    records: Records,
    /// Each gun hand's measured gun relative to its wrist, and the weapon it belongs to.
    measured: [Option<(usize, Pose)>; 2],
    extraction: Pull,
    pending_detach: Option<PendingDetach>,
    pending_ejects: [Option<DetachRequest>; 2],
    detach_generation: u64,
    extraction_blocks_belt: [bool; 2],
    extraction_needs_neutral: [bool; 2],
    insertion_source: InsertionSource,
    tap: TapDetector,
    flicks: [FlickDetector; 2],
    belts: [BeltPush; 2],
    /// Each hand's grip and each gun's mouth on the last tick, for their velocities.
    last_grips: [Option<Vec3>; 2],
    last_mouths: [Option<Vec3>; 2],
    /// The weapon of each hand's gun as of the last tick, by [`Hand::index`], and the seconds since
    /// either changed.
    drawn: Option<[Option<usize>; 2]>,
    since_draw_s: f32,
}

static STATE: Mutex<State> = Mutex::new(State {
    status: ReloadStatus {
        active: false,
        weapons: [None; 2],
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
            lock_backs: 0,
            dry_fires: 0,
            empty_pouch: 0,
            refused: 0,
        },
        serial: serial::SerialStatus::new(),
        events: Vec::new(),
    },
    tick: None,
    pouch: None,
    stocked_pouch: None,
    free: None,
    loose_seen: Vec::new(),
    frames: None,
    records: Records::new(),
    measured: [None; 2],
    extraction: Pull::new(),
    pending_detach: None,
    pending_ejects: [None; 2],
    detach_generation: 0,
    extraction_blocks_belt: [false; 2],
    extraction_needs_neutral: [false; 2],
    insertion_source: InsertionSource::Pouch,
    tap: TapDetector::new(),
    flicks: [FlickDetector::new(); 2],
    belts: [BeltPush::new(); 2],
    last_grips: [None; 2],
    last_mouths: [None; 2],
    drawn: None,
    since_draw_s: 0.0,
});
