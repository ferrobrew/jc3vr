//! The input tick's side of taking a weapon that has no body for the hands' reach (a gun in a
//! character's hands, [`crate::controllers::weapons::hold::steal`], or a weapon on a rack,
//! [`crate::controllers::weapons::hold::rack`]): it gathers the targets
//! ([`crate::controllers::weapons::hold::world`]), places their grab volumes for the free hands
//! ([`begin_tick`]), and carries out a grab of one ([`finish_grab`]).
//!
//! The targets come from the world in world space and are carried into the cockpit frame the grab
//! volumes live in ([`crate::controllers::weapons::docks::cockpit_frames`]).

use std::fmt;

use glam::{Mat4, Vec3};
use jc3gi::{
    equipment::weapon::{WeaponBase, WeaponGripHand},
    types::math::Matrix4,
};
use parking_lot::Mutex;

use crate::controllers::{
    aim::gun,
    frame::{Hand, Pose},
    grab::Candidate,
    hands::{ActiveHands, CockpitHand},
    physical_hands::grab_view,
    weapons::{
        config::HoldConfig,
        docks, engine,
        hold::{
            self, Cause,
            pickup::TakeOptions,
            rack::{self, RackRefusal},
            steal::{self, StealRefusal},
            take::placement::{Identity, Records, Sample},
            world::{self, TargetKind, WorldTarget},
        },
    },
};

mod placement;

/// What taking weapons from the world did, for the readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TakeStatus {
    /// The guns in characters' hands within reach of a grab on the last tick.
    pub held_targets: usize,
    /// The weapons on racks within reach of a grab on the last tick.
    pub rack_targets: usize,
    pub steals: u64,
    /// Weapons a rack handed over, and copies asked of racks with unlimited stock.
    pub rack_takes: u64,
    pub rack_copies: u64,
    /// Why the last take did not happen.
    pub last_refusal: Option<String>,
}

pub fn status() -> TakeStatus {
    let (rack_takes, rack_copies) = rack::counts();
    let state = STATE.lock();
    TakeStatus {
        steals: steal::steals(),
        rack_takes,
        rack_copies,
        ..state.status.clone()
    }
}

/// Forget the measured grips before native placement and return its capture generation.
pub(crate) fn begin_handle_placement() -> u64 {
    HANDLE_PLACEMENTS.lock().clear()
}

/// Invalidate captured grips when hand ownership changes, including adoption before placement.
pub(crate) fn invalidate_handle_placements() {
    HANDLE_PLACEMENTS.lock().clear();
}

/// The primary gun and its solved wrist immediately after native attachment placement.
pub(crate) struct HandlePlacement {
    pub generation: u64,
    pub weapon: usize,
    pub primary_gun: usize,
    pub holder: Hand,
    pub wrist_world: Pose,
    pub weight: f32,
    pub pinned: bool,
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
    pub settings: &'a HoldConfig,
    /// The hands holding no gun, by [`Hand::index`].
    pub free: [bool; 2],
    /// Whether the tick's mode is one the weapon handling acts in.
    pub gameplay: bool,
}

/// Gather the targets and return their grab volumes.
///
/// # Safety
///
/// The main thread, outside the world update.
pub(crate) unsafe fn begin_tick(inputs: &TakeInputs) -> Vec<Candidate> {
    let settings = inputs.settings;
    rack::tick();
    let acting = inputs.gameplay && settings.either_hand && settings.direct_pickup;
    let owner = unsafe { engine::local_player() }.filter(|_| acting);
    let player = owner.as_ref().map(|o| Mat4::from(o.m_WorldMatrixT1));
    steal::publish_probe(
        player
            .filter(|_| settings.steal)
            .map(|m| m.w_axis.truncate()),
    );
    let mut state = STATE.lock();
    state.targets.clear();
    state.offered.clear();
    state.status.held_targets = 0;
    state.status.rack_targets = 0;
    let Some(player) = player else {
        return Vec::new();
    };
    let Some(frames) = docks::cockpit_frames(inputs.active, player) else {
        return Vec::new();
    };
    let to_cockpit = |p: Vec3| frames.world_to_cockpit(p);
    if settings.steal {
        state.targets.extend(steal::targets(to_cockpit));
    }
    let held = state.targets.len();
    if settings.racks
        && let Some(owner) = owner
    {
        state
            .targets
            .extend(unsafe { rack::targets(owner, to_cockpit) });
    }
    state.status.held_targets = held;
    state.status.rack_targets = state.targets.len() - held;
    let points = inputs.active.hands.map(|h| h.map(|h| h.grip.position));
    state.offered =
        world::Offers::gather(&state.targets, settings.take_radius_m, inputs.free, points);
    world::candidates(&state.targets, settings.take_radius_m, inputs.free, points)
}

/// Take the weapon `hand`'s grab of a
/// [`GrabVolume::WorldWeapon`](crate::controllers::grab::GrabVolume::WorldWeapon) closed
/// on. Returns whether a
/// weapon went into the hand.
///
/// # Safety
///
/// The main thread, outside the world update, after [`begin_tick`] on the same input tick.
pub(crate) unsafe fn finish_grab(hand: Hand, inputs: &TakeInputs) -> bool {
    let point = inputs.active.hands[hand.index()].map(|h| h.grip.position);
    let target = STATE
        .lock()
        .offered
        .take(hand, point, inputs.settings.take_radius_m);
    let result = world::complete_offer(target, |target| {
        let options = TakeOptions {
            drop_replaced: inputs.settings.drop_replaced,
            slide_s: if inputs.settings.take_slide {
                inputs.settings.snap_s
            } else {
                0.0
            },
            cause: match target.kind {
                TargetKind::Held { .. } => Cause::Steal,
                TargetKind::Rack { .. } => Cause::Rack,
            },
            // A gun taken from a character's hands or a rack comes into the hand at its grip.
            touch: None,
        };
        match target.kind {
            TargetKind::Held { .. } => {
                unsafe { steal::take(hand, &target, options) }.map_err(TakeRefusal::Steal)
            }
            TargetKind::Rack { .. } => {
                unsafe { rack::take(hand, &target, options) }.map_err(TakeRefusal::Rack)
            }
        }
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
    state.status.rack_targets = 0;
}

fn report_refusal(hand: Hand, refusal: TakeRefusal) {
    tracing::info!(target: "controllers", hand = hand.label(), %refusal, "hold: no weapon taken");
    STATE.lock().status.last_refusal = Some(refusal.to_string());
    grab_view::refused(hand, None, &refusal.to_string());
    hold::refused(refusal);
}

/// Why a grab of a weapon in the world took nothing.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum TakeRefusal {
    /// No target was within reach of the grabbing hand.
    NothingInReach,
    Steal(StealRefusal),
    Rack(RackRefusal),
}

impl fmt::Display for TakeRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TakeRefusal::NothingInReach => write!(f, "take: no weapon is within the hand's reach"),
            TakeRefusal::Steal(refusal) => write!(f, "take: {refusal}"),
            TakeRefusal::Rack(refusal) => write!(f, "take: {refusal}"),
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
        rack_targets: 0,
        steals: 0,
        rack_takes: 0,
        rack_copies: 0,
        last_refusal: None,
    },
});
