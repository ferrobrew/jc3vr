//! Arm-stroke swimming: a hand sweeping back through the water, palm first, pushes the swimmer
//! forward, and an uneven stroke turns them. It adds to the stick swimming
//! (`docs/mod/body/swimming.md`) and acts only while the swim gate reads swimming
//! ([`crate::hooks::input::swim::swimming`]).
//!
//! Each stroking hand catches after a short travel from a forward reach. A backward pull drives
//! the swimmer, and a paired outward sweep contributes to a breaststroke. Palm alignment changes
//! the strength without requiring a square palm. Recovery preserves the glide. The hands are the
//! player's own, where the controllers hold them, not the drawn hands the arm remap and the reach
//! limit place. Stroke recognition follows the estimated torso rather than the cockpit's forward.
//! The game moves a swimmer only the way it faces, so the push is split in two:
//!
//! - its share along the body's forward feeds a swim speed that decays over a second or so
//!   ([`model::StrokeModel`]). The game's swim speed is the animation's root speed, whatever the
//!   stick's magnitude, so the speed is not written to the stick: the swim cores' detours raise the
//!   velocity the cores wrote to it along the way the swimmer faces ([`propulsion`],
//!   [`model::propel`]), and the left stick's own crawl still wins when it is faster;
//! - its turn about the head, the hands' offsets crossed with their pushes, is smoothed over about
//!   half a second, so the alternate arms of a crawl cancel while a stroke on one side alone turns
//!   the body away from it, and the turn is queued here until the head pose takes it on its next
//!   input tick ([`take_turn`]) and adds it to the heading the right stick owns.
//!
//! A hand is in the water when it is below the head by
//! [`StrokeTuning::water_below_eyes_m`](model::StrokeTuning::water_below_eyes_m), or anywhere
//! underwater. A hand holding a gun strokes nothing. Each stroke that catches the water knocks its
//! hand.

pub mod model;
#[cfg(test)]
mod tests;

use jc3gi::state::StateBitFlag;
use jc3vr_core::published::Stamp;
use jc3vr_input::{frame::Hand, haptics::cues::Event};
use parking_lot::Mutex;

use crate::{
    controllers::{
        hands::{ActiveHands, controller_grips},
        haptics,
        locomotion::strokes::model::{StrokeHand, StrokeInput, StrokeModel, StrokeStep},
        mode,
    },
    hooks::input::swim,
    tuning::FRESHNESS,
};

/// What the strokes did on the last tick, for the debug UI.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StrokeStatus {
    /// When the strokes last ran in the water.
    pub at: Option<Stamp>,
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
    pub fn fresh(&self, now: Stamp) -> bool {
        FRESHNESS
            .get()
            .locomotion
            .strokes_ms
            .window()
            .holds(self.at, now)
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
pub(crate) fn propulsion(now: Stamp) -> Option<f32> {
    let state = STATE.lock();
    (state.status.fresh(now) && state.status.step.speed > 0.0).then_some(state.status.step.speed)
}

/// Note what the swim core's detour did with [`propulsion`]: the speed along the facing the core
/// wrote and the speed written in its place, for the debug UI.
pub(crate) fn record_applied(core_mps: f32, written_mps: f32) {
    let mut state = STATE.lock();
    state.status.core_mps = core_mps;
    state.status.written_mps = written_mps;
}

/// Take the turn the strokes queued since the last call, radians to the right: the head pose's
/// participant, called once per input tick, which turns the swimming body's heading by it and
/// drops it out of the water.
pub fn take_turn() -> f32 {
    std::mem::take(&mut *TURN.lock())
}

/// Run the strokes for this input tick while the local player swims. `armed` is whether each hand
/// holds a gun, and `dt` the real tick's length, seconds. Out of the water or without tracked
/// hands, the strokes forget their motion and ask for nothing.
pub fn tick(active: Option<&ActiveHands>, armed: [bool; 2], dt: f32) {
    let mut state = STATE.lock();
    let Some(active) = active.filter(|_| swim::swimming()) else {
        state.model = StrokeModel::new();
        state.status.step = StrokeStep::NONE;
        return;
    };
    // A pause in the ticks (a menu, a hitch, the panel) forgets the hands' motion, so the first
    // tick back does not read the whole gap as one fast stroke.
    if !state.status.fresh(Stamp::now()) {
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
    let step = state.model.step(&StrokeInput {
        hands,
        head: active.head.position,
        forward: active.torso * glam::Vec3::NEG_Z,
        underwater,
        dt,
    });
    if step.turn_right != 0.0 && step.turn_right.is_finite() {
        *TURN.lock() += step.turn_right;
    }
    for hand in Hand::BOTH {
        if let Some(strength) = step.caught[hand.index()] {
            state.status.strokes[hand.index()] += 1;
            haptics::play_scaled(hand, Event::SwimStroke, strength);
        }
    }
    state.status.at = Some(Stamp::now());
    state.status.step = step;
}

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

/// The turn [`tick`] queued since [`take_turn`] last took it, radians to the right. A lock of its
/// own, apart from [`STATE`], so the head pose takes it on the input tick without contending with
/// the strokes' state.
static TURN: Mutex<f32> = Mutex::new(0.0);
