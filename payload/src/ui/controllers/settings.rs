//! Every controller setting except those with their own section's file.

use egui::Ui;

use crate::{
    config,
    controllers::{
        bindings::{Blocker, HandRecovery},
        config::InputConfig,
        frame::{self, Button, GripPress, Hand, PressThresholds},
    },
    ui::controllers::{locomotion, unarmed},
};

pub(super) fn devices(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.input.hand_tracking, "Hand tracking")
        .on_hover_text("Locate the hand skeleton through XR_EXT_hand_tracking when offered.");
    ui.checkbox(&mut c.bindings.enabled, "Inject bindings")
        .on_hover_text(
            "Map the controllers to the game's actions while the controller scheme is in effect.",
        );
    let input = &mut c.input;
    threshold_pair(ui, &mut input.trigger_press, "Trigger", None);
    threshold_pair(
        ui,
        &mut input.squeeze_press,
        "Squeeze",
        Some("The grip press on a controller without grip force (Touch)."),
    );
    threshold_pair(
        ui,
        &mut input.squeeze_force_press,
        "Grip force",
        Some(
            "The grip press on a controller with grip force (Index), whose capacitive squeeze \
             value is full whenever the hand rests round the handle.",
        ),
    );
    threshold_pair(
        ui,
        &mut input.trackpad_press,
        "Trackpad",
        Some("The trackpad force that presses a half, which pages the menu tabs."),
    );
}

/// Each hand's input recovery and its grip and trackpad readings beside their thresholds.
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
    let input = config::Config::lock_query(|c| c.controllers.input);
    grip_readout(ui, &input);
}

pub(super) fn grapple_settings(ui: &mut Ui, cfg: &mut config::Config) {
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.bindings.yank.enabled, "Yank to zip").on_hover_text(
        "A sharp pull of the grapple hand toward the body taps the grapple while a line is live.",
    );
    ui.checkbox(&mut c.grapple.pull_to_reel, "Pull to reel")
        .on_hover_text(
            "The grapple hand's pull speeds the reel, a yank during it boosts it, and a pull back \
             tightens attached tethers.",
        );
    ui.checkbox(&mut c.grapple.pump, "Pump the hang swing");
}

pub(super) fn locomotion_settings(ui: &mut Ui, cfg: &mut config::Config) {
    let l = &mut cfg.controllers.locomotion;
    locomotion::locomotion_settings(ui, &mut l.stick, &mut l.acts, &mut l.view);
}

pub(super) fn animation_settings(ui: &mut Ui, cfg: &mut config::Config) {
    ui.checkbox(&mut cfg.body_ik.enabled, "Body IK")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
    let c = &mut cfg.controllers;
    ui.checkbox(&mut c.aim_state.skip_arm_ik, "Skip the game's arm aim IK");
    ui.checkbox(&mut c.body.remap_head, "Remap head height")
        .on_hover_text("Moves the view for every player, controllers or not.");
    ui.checkbox(&mut c.body.remap_hands, "Remap hand reach");
    unarmed::unarmed_settings(ui, &mut c.body.unarmed);
}

/// The press and release sliders of one threshold pair, kept in order and valid.
fn threshold_pair(ui: &mut Ui, pair: &mut PressThresholds, label: &str, hover: Option<&str>) {
    let before = *pair;
    let response =
        ui.add(egui::Slider::new(&mut pair.press, 0.05..=1.0).text(format!("{label} press")));
    if let Some(hover) = hover {
        response.on_hover_text(hover);
    }
    settle_threshold(pair, before, true);
    let before = *pair;
    ui.add(egui::Slider::new(&mut pair.release, 0.0..=1.0).text(format!("{label} release")));
    settle_threshold(pair, before, false);
}

fn settle_threshold(pair: &mut PressThresholds, before: PressThresholds, press: bool) {
    let value = if press { pair.press } else { pair.release };
    *pair = before;
    set_threshold(pair, press, value);
}

fn set_threshold(pair: &mut PressThresholds, press: bool, value: f32) {
    let candidate = if press {
        PressThresholds {
            press: value,
            release: pair.release.min(value),
        }
    } else {
        PressThresholds {
            press: pair.press.max(value),
            release: value,
        }
    };
    if config::validate::press_thresholds("input", candidate).is_ok() {
        *pair = candidate;
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod widget_tests;

/// Each hand's grip and trackpad readings beside their thresholds, for tuning them.
fn grip_readout(ui: &mut Ui, input: &InputConfig) {
    let Some(hands) =
        frame::with_latest(|f| f.hands.each_ref().map(|h| (h.profile, h.inputs, h.buttons)))
    else {
        ui.label("Grip: no controller frame.");
        return;
    };
    for hand in Hand::BOTH {
        let (profile, inputs, buttons) = hands[hand.index()];
        let source = match input.grip_press(profile, hand) {
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
