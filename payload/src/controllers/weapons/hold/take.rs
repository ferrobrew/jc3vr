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
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    grab::Candidate,
    hands::ActiveHands,
    physical_hands::grab_view,
    weapons::{
        config::HoldConfig,
        docks, engine,
        hold::{
            self,
            holder::Cause,
            pickup::TakeOptions,
            rack::{self, RackRefusal},
            steal::{self, StealRefusal},
            world::{self, TargetKind, WorldTarget},
        },
    },
};

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

/// What the weapon handling hands the take on each input tick.
pub(crate) struct TakeInputs<'a> {
    pub active: &'a ActiveHands,
    pub settings: &'a HoldConfig,
    /// The hands holding no gun, by [`Hand::index`].
    pub free: [bool; 2],
    pub independent_pistols: bool,
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
    world::candidates(
        &state.targets,
        settings.take_radius_m,
        inputs.free,
        inputs.active.hands.map(|h| h.map(|h| h.grip.position)),
    )
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
    let Some(point) = inputs.active.hands[hand.index()].map(|h| h.grip.position) else {
        return false;
    };
    let reach = inputs.settings.take_radius_m * REACH_SLACK;
    let target = world::chosen(&STATE.lock().targets, point, reach);
    let result = match target {
        None => Err(TakeRefusal::NothingInReach),
        Some(target) => {
            let options = TakeOptions {
                independent_pistols: inputs.independent_pistols,
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
        }
    };
    match result {
        Ok(_) => true,
        Err(refusal) => {
            tracing::info!(target: "controllers", hand = hand.label(), %refusal, "hold: no weapon taken");
            STATE.lock().status.last_refusal = Some(refusal.to_string());
            grab_view::refused(hand, None, &refusal.to_string());
            hold::refused(refusal);
            false
        }
    }
}

/// Forget the targets. Called when the weapon handling stands down.
pub(crate) fn stand_down() {
    steal::publish_probe(None);
    rack::stand_down();
    let mut state = STATE.lock();
    state.targets.clear();
    state.status.held_targets = 0;
    state.status.rack_targets = 0;
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

/// How much further than the volumes' radius a grab still finds its target: the volumes are spheres
/// along the gun, and the target is the segment between them.
const REACH_SLACK: f32 = 1.5;

struct State {
    /// The tick's targets, in the cockpit frame.
    targets: Vec<WorldTarget>,
    status: TakeStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    targets: Vec::new(),
    status: TakeStatus {
        held_targets: 0,
        rack_targets: 0,
        steals: 0,
        rack_takes: 0,
        rack_copies: 0,
        last_refusal: None,
    },
});
