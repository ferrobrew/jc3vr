//! The input tick's pickup requests and derived vehicle-entry clicks, without native acquisition.

use jc3gi::input::input_action_map::Action;

use crate::controllers::{
    bindings::hold::{Desired, MapKind, Target, Want},
    frame::Hand,
    grab::GrabVolume,
    physical_hands::{
        grasp::{Reach, TargetKind},
        pickup::{
            diagnostics::{self, ConsumerResult, InputConsumer, InputContext},
            state::{PickupState, Request},
        },
    },
    weapons::FinishInputs,
};

pub(super) fn finish(
    state: &mut PickupState,
    inputs: &FinishInputs,
    reaches: [Option<Reach>; 2],
    fresh: bool,
    desired: &mut Vec<Desired>,
    context: &InputContext,
) -> Vec<(Hand, Reach)> {
    let mut weapons = Vec::new();
    // The vehicle-entry action completes the preceding simulation tick's seated yank.
    for clicked in std::mem::take(&mut state.hijack_clicks) {
        if clicked {
            desired.push(Desired {
                target: Target {
                    map: MapKind::Player,
                    action: Action::ENTER_VEHICLE,
                },
                want: Want::Click,
            });
        }
    }
    for hand in Hand::BOTH {
        let i = hand.index();
        let observation = context.hands[i];
        if !inputs.grips[i].held() {
            diagnostics::input_consumer(hand, observation, InputConsumer::GripOpen);
            if state.has_interaction(hand) {
                state.replace_request(hand, Some(Request::Release), ConsumerResult::Replaced);
            }
            continue;
        }
        if !matches!(inputs.grabs[i], Some(GrabVolume::Pickup(_))) {
            diagnostics::input_consumer(hand, observation, InputConsumer::NotPickup);
            continue;
        }
        let Some(reach) = reaches[i].filter(|_| fresh) else {
            diagnostics::input_consumer(
                hand,
                observation,
                if fresh {
                    InputConsumer::NoReach
                } else {
                    InputConsumer::StaleReach
                },
            );
            continue;
        };
        match reach.kind {
            TargetKind::Weapon => {
                diagnostics::input_consumer(hand, observation, InputConsumer::WeaponPath);
                weapons.push((hand, reach));
            }
            TargetKind::Prop { .. }
            | TargetKind::Articulated { .. }
            | TargetKind::Door(_)
            | TargetKind::Character(_) => {
                let key = diagnostics::queued(hand, observation, reach);
                state.replace_request(
                    hand,
                    Some(Request::ObservedGrab(key)),
                    ConsumerResult::Replaced,
                );
            }
        }
    }
    weapons
}

#[cfg(test)]
mod tests;
