//! Arm-stroke swimming: a hand sweeping back through the water, palm first, pushes the swimmer
//! forward, and an uneven stroke turns them. It adds to the stick swimming
//! (`docs/mod/body/swim-yaw.md`) and acts only while the swim gate reads swimming
//! ([`crate::hooks::input::swim::swimming`]).
//!
//! Each stroking hand is a paddle. Its velocity relative to the body (the cockpit frame rides the
//! body, so a hand's motion there is its motion relative to the swimmer) pushes the water, and the
//! water pushes the swimmer the other way, as far as the palm faces the way the hand moves: a palm
//! slicing edge-on through its recovery pushes nothing. The hands are the player's own, where the
//! controllers hold them, not the drawn hands the arm remap and the reach limit place, which stop
//! short at full reach, where a stroke catches. The game moves a swimmer only the way it faces, so
//! the push is split in two:
//!
//! - its share along the body's forward feeds a swim speed that decays over a second or so
//!   ([`model::StrokeModel`]). The game's swim speed is the animation's root speed, whatever the
//!   stick's magnitude, so the speed is not written to the stick: the swim cores' detours raise the
//!   velocity the cores wrote to it along the way the swimmer faces ([`propulsion`],
//!   [`model::propel`]), and the left stick's own crawl still wins when it is faster;
//! - its turn about the head, the hands' offsets crossed with their pushes, is smoothed over about
//!   half a second, so the alternate arms of a crawl cancel while a stroke on one side alone turns
//!   the body away from it, and the turn is added to the heading the right stick owns
//!   ([`crate::headpose::xr::add_swim_turn`]).
//!
//! A hand is in the water when it is below the head by
//! [`SwimStrokeConfig::water_below_eyes_m`], or anywhere underwater. A hand holding a gun strokes
//! nothing. Each stroke that catches the water knocks its hand.

pub mod model;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use jc3gi::state::StateBitFlag;
use parking_lot::Mutex;

use crate::{
    controllers::{
        frame::Hand,
        hands::{ActiveHands, controller_grips},
        haptics::{self, Event},
        locomotion::{
            config::SwimStrokeConfig,
            strokes::model::{StrokeHand, StrokeInput, StrokeModel, StrokeStep},
        },
        mode,
    },
    headpose,
    hooks::input::swim,
};

/// What the strokes did on the last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StrokeStatus {
    /// When the strokes last ran in the water.
    pub at: Option<Instant>,
    /// The last tick's step.
    pub step: StrokeStep,
    /// The speed along the swimmer's facing the swim core wrote, and the speed written in its
    /// place, metres per second, on the core's last run with the strokes swimming.
    pub core_mps: f32,
    pub written_mps: f32,
    /// How many strokes have caught the water, per hand.
    pub strokes: [u64; 2],
}

impl StrokeStatus {
    /// Whether the strokes ran in the water recently enough that their motion still counts.
    pub fn fresh(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) <= STALE_AFTER)
    }
}

/// The last tick's status.
pub fn status() -> StrokeStatus {
    STATE.lock().status
}

/// The swim speed the strokes keep up, metres per second along the way the swimmer faces, for the
/// swim cores' detours to join to the velocity the cores wrote. `None` while the strokes are not
/// swimming: out of the water, switched off, without tracked hands, gliding to a stop, or with no
/// stroke tick recent enough to trust.
pub fn propulsion(now: Instant) -> Option<f32> {
    let state = STATE.lock();
    (state.status.fresh(now) && state.status.step.speed > 0.0).then_some(state.status.step.speed)
}

/// Note what the swim core's detour did with [`propulsion`]: the speed along the facing the core
/// wrote and the speed written in its place, for the debug UI.
pub fn record_applied(core_mps: f32, written_mps: f32) {
    let mut state = STATE.lock();
    state.status.core_mps = core_mps;
    state.status.written_mps = written_mps;
}

/// Run the strokes for this input tick while the local player swims. `armed` is whether each hand
/// holds a gun, and `dt` the real tick's length, seconds. Out of the water, switched off, or
/// without tracked hands, the strokes forget their motion and ask for nothing.
pub(crate) fn tick(
    active: Option<&ActiveHands>,
    armed: [bool; 2],
    config: &SwimStrokeConfig,
    dt: f32,
) {
    let mut state = STATE.lock();
    let Some(active) = active.filter(|_| config.enabled && swim::swimming()) else {
        state.model = StrokeModel::new();
        state.status.step = StrokeStep::NONE;
        return;
    };
    // A pause in the ticks (a menu, a hitch, the panel) forgets the hands' motion, so the first
    // tick back does not read the whole gap as one fast stroke.
    if !state.status.fresh(Instant::now()) {
        state.model = StrokeModel::new();
    }
    let underwater = mode::last_snapshot()
        .and_then(|snapshot| snapshot.character)
        .is_some_and(|character| character.has(StateBitFlag::SB_IS_UW_SWIMMING));
    let grips = controller_grips(active);
    let hands = Hand::BOTH.map(|hand| {
        let i = hand.index();
        let (hand, grip) = (active.hands[i]?, grips[i]?);
        (!armed[i]).then(|| StrokeHand {
            position: grip,
            palm: hand.wrist.orientation * model::WRIST_PALM,
        })
    });
    let step = state.model.step(
        &StrokeInput {
            hands,
            head: active.head.position,
            underwater,
            dt,
        },
        config,
    );
    if step.turn_right != 0.0 {
        headpose::xr::add_swim_turn(step.turn_right);
    }
    for hand in Hand::BOTH {
        if let Some(strength) = step.caught[hand.index()] {
            state.status.strokes[hand.index()] += 1;
            haptics::play_scaled(hand, Event::SwimStroke, strength);
        }
    }
    state.status.at = Some(Instant::now());
    state.status.step = step;
}

/// How long the strokes may go without a tick before they forget the hands' motion.
const STALE_AFTER: Duration = Duration::from_millis(250);

struct State {
    model: StrokeModel,
    status: StrokeStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    model: StrokeModel::new(),
    status: StrokeStatus {
        at: None,
        step: StrokeStep::NONE,
        core_mps: 0.0,
        written_mps: 0.0,
        strokes: [0; 2],
    },
});
