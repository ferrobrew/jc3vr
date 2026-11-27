//! The input tick's side of taking a weapon that has no body for the hands' reach (a gun in a
//! character's hands or holstered on its body, [`crate::controllers::weapons::hold::steal`], or a
//! weapon on a rack,
//! [`crate::controllers::weapons::hold::rack`]): it gathers the targets
//! ([`crate::controllers::weapons::hold::world`]), places their grab volumes for the free hands
//! ([`begin_tick`]), and carries out a grab of one ([`finish_grab`]).
//!
//! A refusal is also handed to the take-refused contribution
//! ([`crate::contributions::WeaponsContributions::take_refused`]), which shows it in the physical
//! hands' grip view.
//!
//! The targets come from the world in world space and are carried into the cockpit frame the grab
//! volumes live in ([`crate::controllers::weapons::docks::cockpit_frames`]).

use std::fmt;

use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponGripHand},
    types::math::Matrix4,
};
use jc3vr_body::controllers::{
    aim::gun,
    grab::Candidate,
    hands::{ActiveHands, CockpitHand},
    held::HandlePlacement,
    physics::local_player::LocalPlayer,
};
use jc3vr_core::game_tick::GameTick;
use jc3vr_input::frame::Hand;
use parking_lot::Mutex;

use crate::{
    contributions,
    controllers::weapons::{
        docks, hold,
        hold::{
            rack, source,
            source::{SourceRefusal, WeaponSource},
            steal,
            take::placement::{Identity, Records, Sample},
            tuning::HOLD,
            world,
            world::{TakeRadii, TargetKind, WorldTarget},
        },
    },
};

mod placement;

/// What taking weapons from the world did, for the readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TakeStatus {
    /// The guns in characters' hands noted near the player on the last tick.
    pub held_targets: usize,
    /// The guns holstered on characters' bodies noted near the player on the last tick.
    pub holstered_targets: usize,
    /// The weapons on racks near the player on the last tick.
    pub rack_targets: usize,
    pub counts: TakeCounts,
}

jc3vr_core::counters! {
    /// The takes of weapons that have no body in the world, since the payload loaded.
    pub(crate) struct TakeCounters => pub struct TakeCounts {
        /// Guns taken from characters' hands or holsters.
        steals,
        /// Weapons a rack handed over.
        rack_takes,
        /// Copies asked of racks with unlimited stock.
        rack_copies,
    }
}

pub fn status() -> TakeStatus {
    let state = STATE.lock();
    TakeStatus {
        counts: COUNTS.snapshot(),
        ..state.status.clone()
    }
}

/// The take counts, bumped by the sources that give up their weapons.
pub(crate) static COUNTS: TakeCounters = TakeCounters::new();

/// Forget the measured grips before native placement and return its capture generation.
pub(crate) fn begin_handle_placement() -> u64 {
    HANDLE_PLACEMENTS.lock().clear()
}

/// Invalidate captured grips when hand ownership changes, including adoption before placement.
pub(crate) fn invalidate_handle_placements() {
    HANDLE_PLACEMENTS.lock().clear();
}

/// Capture the primary grip and wrist from the same native placement.
///
/// # Safety
///
/// The weapon and primary gun must remain live in the local character's post-placement pass.
/// `wrist_world` is the solved wrist read in that pass, not a later IK target.
pub(crate) unsafe fn record_handle_placement(input: HandlePlacement) {
    HANDLE_PLACEMENTS
        .lock()
        .invalidate_hand(input.generation, input.holder);
    let Some(base) = (unsafe { (input.weapon as *const WeaponBase).as_ref() }) else {
        return;
    };
    let Some(first) = (unsafe { gun::guns(base) }).next().flatten() else {
        return;
    };
    if first as *const _ as usize != input.primary_gun {
        return;
    }
    let mut grip = Matrix4::from(Mat4::IDENTITY);
    // A single gun keeps its primary grip entry when it moves to the other hand.
    if !unsafe { base.GetGripPosition(WeaponGripHand::E_WEAPON_GRIP_HAND_RIGHT, &mut grip) } {
        return;
    }
    HANDLE_PLACEMENTS.lock().capture(
        input.generation,
        Sample {
            identity: Identity {
                weapon: input.weapon,
                primary_gun: input.primary_gun,
                holder: input.holder,
                state: base.m_State,
                gun_world: Mat4::from(first.m_WorldMatrixT1),
            },
            wrist_world: input.wrist_world,
            grip_world: Mat4::from(grip).w_axis.truncate(),
            weight: input.weight,
            pinned: input.pinned,
        },
    );
}

/// The captured primary grip on the current weighted wrist, only while the weapon, gun, holder,
/// state, and native transform still match that actual placement. No record means no measurement.
///
/// # Safety
///
/// `weapon` must be live on the input thread outside the world update. `current` is its weighted
/// drawn hand. Ownership changes must invalidate the capture before another measurement.
pub(crate) unsafe fn measured_handle_point(
    weapon: usize,
    holder: Hand,
    current: &CockpitHand,
    world_scale: f32,
) -> Option<Vec3> {
    if hold::owned_holder(weapon) != Some(holder) {
        invalidate_handle_placements();
        return None;
    }
    let base = unsafe { (weapon as *const WeaponBase).as_ref() }?;
    let first = (unsafe { gun::guns(base) }).next().flatten()?;
    HANDLE_PLACEMENTS.lock().point(
        Identity {
            weapon,
            primary_gun: first as *const _ as usize,
            holder,
            state: base.m_State,
            gun_world: Mat4::from(first.m_WorldMatrixT1),
        },
        current.wrist,
        world_scale,
    )
}

/// What the weapon handling hands the take on each input tick.
pub(crate) struct TakeInputs<'a> {
    pub active: &'a ActiveHands,
    /// The hands holding no gun, by [`Hand::index`].
    pub free: [bool; 2],
    /// Whether the tick's mode is one the weapon handling acts in.
    pub gameplay: bool,
}

/// Gather the targets and return their grab volumes.
pub(crate) fn begin_tick(game: &GameTick<'_>, inputs: &TakeInputs) -> Vec<Candidate> {
    rack::tick();
    let acting = inputs.gameplay;
    let owner = LocalPlayer::get(game).filter(|_| acting);
    let player = owner.as_ref().map(LocalPlayer::frame);
    steal::publish_probe(player.map(|m| m.w_axis.truncate()));
    let mut state = STATE.lock();
    state.targets.clear();
    state.offered.clear();
    state.status.held_targets = 0;
    state.status.holstered_targets = 0;
    state.status.rack_targets = 0;
    let Some(player) = player else {
        return Vec::new();
    };
    let Some(frames) = docks::cockpit_frames(inputs.active, player) else {
        return Vec::new();
    };
    let to_cockpit = |p: Vec3| frames.cockpit.point_to_cockpit(p);
    state.targets.extend(steal::targets(to_cockpit));
    let on_characters = state.targets.len();
    let holstered = state
        .targets
        .iter()
        .filter(|t| matches!(t.kind, TargetKind::Holstered { .. }))
        .count();
    if let Some(mut owner) = owner {
        // SAFETY: the local player, in the game tick.
        state
            .targets
            .extend(unsafe { rack::targets(owner.character_mut(), to_cockpit) });
    }
    state.status.held_targets = on_characters - holstered;
    state.status.holstered_targets = holstered;
    state.status.rack_targets = state.targets.len() - on_characters;
    let points = inputs.active.hands.map(|h| h.map(|h| h.grip.position));
    state.offered = world::Offers::gather(&state.targets, take_radii(), inputs.free, points);
    world::candidates(&state.targets, take_radii(), inputs.free, points)
}

/// Take the weapon `hand`'s grab of a
/// [`GrabVolume::WorldWeapon`](jc3vr_body::controllers::grab::GrabVolume::WorldWeapon) closed
/// on. Returns whether a
/// weapon went into the hand.
///
/// # Safety
///
/// After [`begin_tick`] on the same input tick.
pub(crate) unsafe fn finish_grab(game: &GameTick<'_>, hand: Hand, inputs: &TakeInputs) -> bool {
    let point = inputs.active.hands[hand.index()].map(|h| h.grip.position);
    let target = STATE.lock().offered.take(hand, point, take_radii());
    let result = world::complete_offer(target, |target| {
        WeaponSource::of_target(game, &target)
            .and_then(|source| source::take(game, hand, &source))
            .map_err(TakeRefusal::Source)
    });
    match result {
        Ok(Some(_)) => {
            if let Some(target) = target {
                STATE.lock().offered.acquired(target);
            }
            true
        }
        Ok(None) => {
            report_refusal(hand, TakeRefusal::NothingInReach);
            false
        }
        Err(refusal) => {
            report_refusal(hand, refusal);
            false
        }
    }
}

/// Forget the targets. Called when the weapon handling stands down.
pub(crate) fn stand_down() {
    invalidate_handle_placements();
    steal::publish_probe(None);
    rack::stand_down();
    let mut state = STATE.lock();
    state.targets.clear();
    state.offered.clear();
    state.status.held_targets = 0;
    state.status.holstered_targets = 0;
    state.status.rack_targets = 0;
}

/// The character whose gun the last input tick offered `hand`, as an address: the hand is within
/// its steal reach of a gun in its hands or holstered on its body. `None` when no gun on a
/// character was offered to it, or once a grab consumed the offer.
pub(crate) fn steal_offer(hand: Hand) -> Option<usize> {
    STATE.lock().offered.character(hand)
}

/// The take's reach around each kind of target.
fn take_radii() -> TakeRadii {
    let tuning = &HOLD.get().hold;
    TakeRadii {
        held: tuning.steal_radius_m,
        rack: tuning.take_radius_m,
    }
}

fn report_refusal(hand: Hand, refusal: TakeRefusal) {
    tracing::info!(target: "controllers", hand = hand.label(), %refusal, "hold: no weapon taken");
    if let Some(contributions) = contributions::get() {
        (contributions.take_refused)(hand, &refusal.to_string());
    }
    hold::refused(refusal);
}

/// Why a grab of a weapon in the world took nothing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TakeRefusal {
    /// No target was within reach of the grabbing hand.
    NothingInReach,
    Source(SourceRefusal),
}

impl fmt::Display for TakeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TakeRefusal::NothingInReach => write!(f, "take: no weapon is within the hand's reach"),
            TakeRefusal::Source(refusal) => write!(f, "take: {refusal}"),
        }
    }
}

struct State {
    /// The tick's targets, in the cockpit frame.
    targets: Vec<WorldTarget>,
    /// The nearest target each free hand was offered, with the same boundary as its grab volume.
    offered: world::Offers,
    status: TakeStatus,
}

static HANDLE_PLACEMENTS: Mutex<Records> = Mutex::new(Records::new());

static STATE: Mutex<State> = Mutex::new(State {
    targets: Vec::new(),
    offered: world::Offers::new(),
    status: TakeStatus {
        held_targets: 0,
        holstered_targets: 0,
        rack_targets: 0,
        counts: TakeCounts {
            steals: 0,
            rack_takes: 0,
            rack_copies: 0,
        },
    },
});
