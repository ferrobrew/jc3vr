//! Putting a held grenade or C4 charge back by pressing it onto its chest pouch.
//!
//! A press counts when the hand holding the item enters the pouch from outside and reaches
//! [`PRESS`]'s depth into it. An item that came into the hand inside the pouch, as one drawn from
//! it does, counts only after the hand has left the pouch by the leave margin, so a draw never
//! puts its grenade straight back; after a press, accepted or refused, the hand has to leave the
//! same way before it can press again. The begin tick, which places the pouches, folds the hands
//! in ([`observe`]); the grenade's and the charge's own input ticks take the presses
//! ([`take_grenades`], [`take_charges`]) and decide whether the pouch takes the item.

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{frame::Hand, loadout::ItemId};

/// How a press onto a pouch is told from a hand passing by it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PressTuning {
    /// How far outside the pouch's surface the hand must go before it can press again, metres.
    pub leave_margin_m: f32,
    /// How far into the pouch the hand must go for a press, metres.
    pub depth_m: f32,
}

pub const PRESS: PressTuning = PressTuning {
    leave_margin_m: 0.03,
    depth_m: 0.02,
};

/// A pouch's volume in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Pouch {
    pub centre: Vec3,
    pub radius: f32,
}

/// What the begin tick hands the presses.
pub(crate) struct Inputs {
    /// The grenade pouch, while the grenade throw is on and the body is placed.
    pub grenade_pouch: Option<Pouch>,
    /// The C4 pouch, likewise.
    pub charge_pouch: Option<Pouch>,
    /// The hands' grips in the cockpit frame, by [`Hand::index`].
    pub grips: [Option<Vec3>; 2],
    /// The grenade each hand holds that could go back: its pin is in.
    pub grenades: [Option<ItemId>; 2],
    /// The charge each hand holds that could go back, by its address.
    pub charges: [Option<usize>; 2],
}

/// Fold one begin tick into each hand's presses.
pub(crate) fn observe(inputs: &Inputs) {
    let mut state = STATE.lock();
    for hand in Hand::BOTH {
        let i = hand.index();
        let grip = inputs.grips[i];
        if state.grenades[i].step(inputs.grenades[i], grip, inputs.grenade_pouch, &PRESS) {
            state.pressed_grenades[i] = true;
        }
        if state.charges[i].step(inputs.charges[i], grip, inputs.charge_pouch, &PRESS) {
            state.pressed_charges[i] = true;
        }
    }
}

/// The hands that pressed a grenade onto its pouch since the last call, by [`Hand::index`].
pub(crate) fn take_grenades() -> [bool; 2] {
    std::mem::take(&mut STATE.lock().pressed_grenades)
}

/// The hands that pressed a charge onto its pouch since the last call, by [`Hand::index`].
pub(crate) fn take_charges() -> [bool; 2] {
    std::mem::take(&mut STATE.lock().pressed_charges)
}

/// Forget the hands' presses: the weapon handling stood down.
pub(crate) fn stand_down() {
    *STATE.lock() = State::new();
}

/// Where one hand's press onto one pouch is.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
enum Stage {
    /// The hand is outside the pouch and may press.
    #[default]
    Outside,
    /// The hand came into the pouch without a press, or pressed; nothing happens until it leaves.
    Spent,
}

/// One hand's press detector for one pouch, over the identity `I` of the item the hand holds.
#[derive(Debug, Copy, Clone, PartialEq)]
struct Press<I> {
    stage: Stage,
    holding: Option<I>,
}

impl<I: Copy + PartialEq> Press<I> {
    const fn new() -> Self {
        Self {
            stage: Stage::Outside,
            holding: None,
        }
    }

    /// Fold in one tick: the item the hand holds that could go back, the hand's grip, and the pouch.
    /// True when the hand pressed the item onto the pouch.
    fn step(
        &mut self,
        holding: Option<I>,
        grip: Option<Vec3>,
        pouch: Option<Pouch>,
        tuning: &PressTuning,
    ) -> bool {
        let (Some(item), Some(grip), Some(pouch)) = (holding, grip, pouch) else {
            self.holding = None;
            return false;
        };
        let distance = grip.distance(pouch.centre);
        if !distance.is_finite() || !pouch.radius.is_finite() {
            self.holding = None;
            return false;
        }
        let outside = distance > pouch.radius + tuning.leave_margin_m;
        if self.holding != Some(item) {
            self.holding = Some(item);
            self.stage = if outside {
                Stage::Outside
            } else {
                Stage::Spent
            };
            return false;
        }
        if outside {
            self.stage = Stage::Outside;
            return false;
        }
        if self.stage == Stage::Outside && distance <= pouch.radius - tuning.depth_m {
            self.stage = Stage::Spent;
            return true;
        }
        false
    }
}

struct State {
    grenades: [Press<ItemId>; 2],
    charges: [Press<usize>; 2],
    pressed_grenades: [bool; 2],
    pressed_charges: [bool; 2],
}

impl State {
    const fn new() -> Self {
        Self {
            grenades: [Press::new(); 2],
            charges: [Press::new(); 2],
            pressed_grenades: [false; 2],
            pressed_charges: [false; 2],
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

#[cfg(test)]
mod tests {
    use super::*;

    const POUCH: Pouch = Pouch {
        centre: Vec3::new(0.2, -0.3, -0.25),
        radius: 0.075,
    };

    fn at(distance: f32) -> Option<Vec3> {
        Some(POUCH.centre + Vec3::Z * -distance)
    }

    #[test]
    fn a_press_from_outside_counts_once_until_the_hand_leaves() {
        let mut press = Press::new();
        assert!(!press.step(Some(1), at(0.3), Some(POUCH), &PRESS));
        // Inside the surface but short of the depth.
        assert!(!press.step(Some(1), at(0.07), Some(POUCH), &PRESS));
        assert!(press.step(Some(1), at(0.04), Some(POUCH), &PRESS));
        assert!(!press.step(Some(1), at(0.0), Some(POUCH), &PRESS));
        // Within the leave margin is not out.
        assert!(!press.step(Some(1), at(0.09), Some(POUCH), &PRESS));
        assert!(!press.step(Some(1), at(0.04), Some(POUCH), &PRESS));
        assert!(!press.step(Some(1), at(0.2), Some(POUCH), &PRESS));
        assert!(press.step(Some(1), at(0.04), Some(POUCH), &PRESS));
    }

    #[test]
    fn an_item_drawn_inside_the_pouch_waits_for_the_hand_to_leave() {
        let mut press = Press::new();
        assert!(!press.step(Some(7), at(0.0), Some(POUCH), &PRESS));
        assert!(!press.step(Some(7), at(0.02), Some(POUCH), &PRESS));
        assert!(!press.step(Some(7), at(0.3), Some(POUCH), &PRESS));
        assert!(press.step(Some(7), at(0.0), Some(POUCH), &PRESS));
    }

    #[test]
    fn a_new_item_or_a_lost_hand_starts_over() {
        let mut press = Press::new();
        assert!(!press.step(Some(1), at(0.3), Some(POUCH), &PRESS));
        // Another item appears in the hand inside the pouch: no press until it leaves.
        assert!(!press.step(Some(2), at(0.0), Some(POUCH), &PRESS));
        assert!(!press.step(Some(2), at(0.0), Some(POUCH), &PRESS));
        // Nothing to put back, no grip, or no pouch: nothing counts.
        assert!(!press.step(None, at(0.0), Some(POUCH), &PRESS));
        assert!(!press.step(Some(2), None, Some(POUCH), &PRESS));
        assert!(!press.step(Some(2), at(0.0), None, &PRESS));
        assert!(!press.step(Some(2), Some(Vec3::NAN), Some(POUCH), &PRESS));
    }
}
