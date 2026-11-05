//! Arm-stroke swimming: a hand sweeping back through the water, palm first, pushes the swimmer
//! forward, and an uneven stroke turns them. It adds to the stick swimming
//! (`docs/mod/body/swim-yaw.md`) and acts only while the swim gate reads swimming
//! ([`crate::hooks::input::swim::swimming`]).
//!
//! Each stroking hand is a paddle. Its velocity relative to the body (the cockpit frame rides the
//! body, so a hand's motion there is its motion relative to the swimmer) pushes the water, and the
//! water pushes the swimmer the other way, as far as the palm faces the way the hand moves: a palm
//! slicing edge-on through its recovery pushes nothing. The game moves a swimmer only the way it
//! faces, so the push is split in two:
//!
//! - its share along the body's forward feeds a forward drive that decays over a second or so,
//!   written to the move stick's forward with the larger of it and the stick winning, so the game's
//!   own swim cores swim the body at the speed the strokes keep up ([`model::StrokeModel`]);
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

use glam::Vec2;
use jc3gi::state::StateBitFlag;
use parking_lot::Mutex;

use crate::{
    controllers::{
        bindings::hold::Desired,
        frame::Hand,
        hands::ActiveHands,
        haptics::{self, Event},
        locomotion::{
            config::SwimStrokeConfig,
            strokes::model::{StrokeHand, StrokeInput, StrokeModel, StrokeStep},
        },
        mode,
        vehicles::axis_output::{read_move, write_move},
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
    /// The move stick's forward the tables wrote, and the forward written over it.
    pub stick_forward: f32,
    pub written_forward: f32,
    /// How many strokes have caught the water, per hand.
    pub strokes: [u64; 2],
}

/// The last tick's status.
pub fn status() -> StrokeStatus {
    STATE.lock().status
}

/// Run the strokes for this input tick while the local player swims, and write their forward over
/// the move stick's in `desired`. `armed` is whether each hand holds a gun, and `dt` the real
/// tick's length, seconds. Out of the water, switched off, or without tracked hands, the strokes
/// forget their motion and write nothing.
pub(crate) fn tick(
    active: Option<&ActiveHands>,
    armed: [bool; 2],
    config: &SwimStrokeConfig,
    desired: &mut Vec<Desired>,
    dt: f32,
) {
    let mut state = STATE.lock();
    let Some(active) = active.filter(|_| config.enabled && swim::swimming()) else {
        state.model = StrokeModel::new();
        return;
    };
    // A pause in the ticks (a menu, a hitch, the panel) forgets the hands' motion, so the first
    // tick back does not read the whole gap as one fast stroke.
    if state.status.at.is_none_or(|at| at.elapsed() > STALE_AFTER) {
        state.model = StrokeModel::new();
    }
    let underwater = mode::last_snapshot()
        .and_then(|snapshot| snapshot.character)
        .is_some_and(|character| character.has(StateBitFlag::SB_IS_UW_SWIMMING));
    let hands = Hand::BOTH.map(|hand| {
        active.hands[hand.index()]
            .filter(|_| !armed[hand.index()])
            .map(|h| StrokeHand {
                position: h.grip.position,
                palm: h.wrist.orientation * model::WRIST_PALM,
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
    let stick = read_move(desired);
    let forward = stick.y.max(step.forward);
    if forward > stick.y {
        write_move(desired, Vec2::new(stick.x, forward));
    }
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
    state.status.stick_forward = stick.y;
    state.status.written_forward = forward;
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
        stick_forward: 0.0,
        written_forward: 0.0,
        strokes: [0; 2],
    },
});
