//! A charge from the weapon wheel: what the throw does about the game's planting mode. Pure.
//!
//! Choosing the explosives on the weapon wheel puts the game in its planting mode
//! ([`PlantedExplosiveWeapon::m_IsInPlantingMode`]), whose plant input the controller layout leaves
//! unbound, and in which the trigger would draw a gun. A free hand takes the charge instead: the
//! planting mode is left, and once it has ended the charge comes into that hand as one from the
//! pouch does, the dominant hand first.
//!
//! [`PlantedExplosiveWeapon::m_IsInPlantingMode`]: jc3gi::equipment::weapon::PlantedExplosiveWeapon::m_IsInPlantingMode

use crate::controllers::{frame::Hand, weapons::c4::stage::Stage};

/// The hands a charge from the wheel may go to.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct WheelHands {
    /// The hands holding nothing, by [`Hand::index`].
    pub free: [bool; 2],
    /// The hand that takes the charge when both are free.
    pub dominant: Hand,
}

/// What a tick does about the planting mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WheelStep {
    /// Leave the planting mode, for a charge in this hand.
    Leave(Hand),
    /// The planting mode is left: take the charge into this hand.
    Take(Hand),
    /// Nothing to do, or the planting mode is still being left.
    Wait,
    /// Forget the pending charge: the hand took something else, the throw is busy, or the planting
    /// mode did not end within [`TIMEOUT_S`].
    Drop,
}

/// How long the planting mode has to end once it is left, seconds.
pub const TIMEOUT_S: f32 = 1.0;

/// What to do about the planting mode `planting`, with the throw at `stage`, a charge pending for
/// a hand from an earlier tick (with the seconds since the planting mode was left), and the tick's
/// `hands`.
pub fn step(
    planting: bool,
    stage: Stage,
    pending: Option<(Hand, f32)>,
    hands: WheelHands,
) -> WheelStep {
    let free_hand = [hands.dominant, hands.dominant.other()]
        .into_iter()
        .find(|h| hands.free[h.index()]);
    match (planting, pending) {
        (true, None) if stage == Stage::Idle => match free_hand {
            Some(hand) => WheelStep::Leave(hand),
            None => WheelStep::Wait,
        },
        (true, Some((_, waited))) if waited > TIMEOUT_S => WheelStep::Drop,
        (true, _) => WheelStep::Wait,
        (false, Some((hand, _))) if stage == Stage::Idle && hands.free[hand.index()] => {
            WheelStep::Take(hand)
        }
        (false, Some(_)) => WheelStep::Drop,
        (false, None) => WheelStep::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hands(free: [bool; 2]) -> WheelHands {
        WheelHands {
            free,
            dominant: Hand::Right,
        }
    }

    #[test]
    fn a_charge_from_the_wheel_leaves_the_planting_mode_for_a_free_hand() {
        let both = hands([true; 2]);
        assert_eq!(
            step(true, Stage::Idle, None, both),
            WheelStep::Leave(Hand::Right),
            "the dominant hand first"
        );
        let left_only = hands([true, false]);
        assert_eq!(
            step(true, Stage::Idle, None, left_only),
            WheelStep::Leave(Hand::Left)
        );
        assert_eq!(
            step(true, Stage::Idle, None, hands([false; 2])),
            WheelStep::Wait,
            "no free hand: the planting mode stays"
        );
    }

    #[test]
    fn the_hand_takes_the_charge_once_the_planting_mode_has_ended() {
        let both = hands([true; 2]);
        let pending = Some((Hand::Right, 0.1));
        assert_eq!(step(true, Stage::Idle, pending, both), WheelStep::Wait);
        assert_eq!(
            step(false, Stage::Idle, pending, both),
            WheelStep::Take(Hand::Right)
        );
        assert_eq!(
            step(false, Stage::Idle, None, both),
            WheelStep::Wait,
            "no planting mode, nothing pending"
        );
    }

    #[test]
    fn a_pending_charge_is_dropped_when_its_hand_is_taken_or_the_mode_stays() {
        let both = hands([true; 2]);
        let pending = Some((Hand::Right, 0.1));
        assert_eq!(
            step(false, Stage::Idle, pending, hands([true, false])),
            WheelStep::Drop
        );
        assert_eq!(
            step(
                true,
                Stage::Idle,
                Some((Hand::Right, TIMEOUT_S + 0.1)),
                both
            ),
            WheelStep::Drop
        );
    }
}
