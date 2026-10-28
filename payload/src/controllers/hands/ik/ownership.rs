//! Controller ownership and blend for placements inside one pose-finalization pass.

use std::cell::Cell;

/// The effective plan weights. Tracking and controller activation are resolved before publication.
pub(super) fn effective_weights(tracked: [bool; 2], tier: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|i| {
        if tracked[i] && tier[i].is_finite() {
            tier[i].clamp(0.0, 1.0)
        } else {
            0.0
        }
    })
}

pub(super) fn publish(character: usize, controller: usize, weights: [f32; 2]) {
    PLAN.set(Some(PlacementPlan {
        character,
        controller,
        weights,
    }));
}

pub(super) fn weights(character: usize, controller: usize) -> [f32; 2] {
    PLAN.get()
        .map_or([0.0; 2], |plan| plan.weights_for(character, controller))
}

pub(super) fn clear() {
    PLAN.set(None);
}

#[derive(Copy, Clone)]
struct PlacementPlan {
    character: usize,
    controller: usize,
    weights: [f32; 2],
}

impl PlacementPlan {
    fn weights_for(self, character: usize, controller: usize) -> [f32; 2] {
        if character != 0
            && controller != 0
            && self.character == character
            && self.controller == controller
        {
            self.weights
        } else {
            [0.0; 2]
        }
    }
}

// The producer, placement, and end-of-pass reset run on the same pose-finalization thread.
thread_local! {
    static PLAN: Cell<Option<PlacementPlan>> = const { Cell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_or_untracked_hands_ignore_positive_stale_tiers() {
        for tracked in [[false; 2], [true, false], [false, true]] {
            assert_eq!(
                effective_weights(tracked, [1.0; 2]),
                tracked.map(|v| if v { 1.0 } else { 0.0 })
            );
        }
        // Disabled controllers, the gamepad scheme, and disabled IK all provide no active hands.
        assert_eq!(effective_weights([false; 2], [0.6, 0.8]), [0.0; 2]);
    }

    #[test]
    fn driven_hand_preserves_the_yield_blend_without_a_queued_target() {
        let weights = effective_weights([true; 2], [0.15, 0.7]);
        publish(11, 22, weights);
        assert_eq!(self::weights(11, 22), [0.15, 0.7]);
        publish(11, 22, effective_weights([true; 2], [0.0; 2]));
        assert_eq!(self::weights(11, 22), [0.0; 2]);
        clear();
    }

    #[test]
    fn replacement_and_pass_reset_reject_prior_publications() {
        publish(11, 22, [1.0; 2]);
        assert_eq!(weights(12, 22), [0.0; 2]);
        assert_eq!(weights(11, 23), [0.0; 2]);
        clear();
        assert_eq!(weights(11, 22), [0.0; 2]);
        publish(11, 22, [0.0; 2]);
        assert_eq!(weights(11, 22), [0.0; 2]);
        clear();
    }
}
