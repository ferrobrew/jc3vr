//! The controller devices: the hand-tracking switch, and each hand's input recovery and grip
//! readings.

use egui::Ui;
use jc3vr_gameplay::controllers::{
    bindings::{Blocker, HandRecovery},
    config::ControllersConfig,
};
use jc3vr_input::{
    frame,
    frame::{Button, GripPress, Hand},
};
use jc3vr_view::vr::input::config::grip_press;

pub(super) fn devices(ui: &mut Ui, c: &mut ControllersConfig) {
    ui.checkbox(&mut c.input.hand_tracking, "Hand tracking")
        .on_hover_text("Locate the hand skeleton through XR_EXT_hand_tracking when offered.");
}

/// Each hand's input recovery and its grip and trackpad readings.
pub(super) fn devices_readout(ui: &mut Ui, recovery: [HandRecovery; 2]) {
    for hand in Hand::BOTH {
        ui.label(format!(
            "Input {}: {}",
            hand.label(),
            recovery_label(recovery[hand.index()])
        ))
        .on_hover_text(
            "After a loss of tracking or focus, a hand's input stays masked until its trigger, \
             grip, buttons, and stick rest.",
        );
    }
    grip_readout(ui);
}

/// Each hand's grip and trackpad readings.
fn grip_readout(ui: &mut Ui) {
    let Some(hands) =
        frame::with_latest(|f| f.hands.each_ref().map(|h| (h.profile, h.inputs, h.buttons)))
    else {
        ui.label("Grip: no controller frame.");
        return;
    };
    for hand in Hand::BOTH {
        let (profile, inputs, buttons) = hands[hand.index()];
        let source = match grip_press(profile, hand) {
            GripPress::Force(_) => "force",
            GripPress::Value(_) => "value",
        };
        let pad = if buttons.contains(Button::TrackpadPressLeft) {
            "pressed left"
        } else if buttons.contains(Button::TrackpadPressRight) {
            "pressed right"
        } else {
            "up"
        };
        ui.label(format!(
            "Grip {}: value {:.2} · force {:.2} · press by {source} · {} · pad force {:.2} {pad}",
            hand.label(),
            inputs.squeeze,
            inputs.squeeze_force,
            if buttons.contains(Button::SqueezePress) {
                "pressed"
            } else {
                "up"
            },
            inputs.trackpad_force,
        ));
    }
}

fn recovery_label(recovery: HandRecovery) -> String {
    match recovery {
        HandRecovery::Unusable => "not injecting".to_owned(),
        HandRecovery::Ready => "ready".to_owned(),
        HandRecovery::Masked(None) => "masked, waiting for a sample".to_owned(),
        HandRecovery::Masked(Some(blocker)) => format!("masked until {}", blocker_label(blocker)),
    }
}

fn blocker_label(blocker: Blocker) -> String {
    match blocker {
        Blocker::Button(button) => format!("{button:?} is released"),
        Blocker::Trigger(value) => format!("the trigger rests (at {value:.2})"),
        Blocker::Squeeze(value) => format!("the grip rests (at {value:.2})"),
        Blocker::Stick { magnitude, neutral } => {
            format!("the stick rests under {neutral:.2} (at {magnitude:.2})")
        }
    }
}
