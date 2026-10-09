//! On-foot stick locomotion, the per-tick mode detector, and the raw input interpretation and
//! controller-to-game bindings that drive them.

use serde::{Deserialize, Serialize};

use crate::{
    controllers::{
        bindings::yank::YankConfig,
        frame::{GripPress, Hand, InteractionProfile, PressRules, PressThresholds},
        locomotion::config::{ActFilterConfig, StableViewConfig, StickConfig},
    },
    vr::input::layout::{FloatInput, Input, provides},
};

/// Raw input interpretation.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// The trigger values that press and release
    /// [`Button::TriggerPress`](crate::controllers::frame::Button::TriggerPress).
    pub trigger_press: PressThresholds,
    /// The squeeze values that press and release
    /// [`Button::SqueezePress`](crate::controllers::frame::Button::SqueezePress) on a controller
    /// without a force-sensing grip (Touch, or a profile the runtime emulates). The grip is used
    /// only as an edge, so the press threshold sits high enough that resting a hand on the grip
    /// does not press it.
    pub squeeze_press: PressThresholds,
    /// The squeeze forces that press and release
    /// [`Button::SqueezePress`](crate::controllers::frame::Button::SqueezePress) on a controller
    /// whose profile provides the grip force (the Index). The Index grip's value is capacitive and
    /// reads a full squeeze with the hand merely closed round the handle, so the press waits for a
    /// deliberate squeeze, and the release sits low enough that a held grab does not need a hard
    /// squeeze to keep.
    pub squeeze_force_press: PressThresholds,
    /// The trackpad forces that press and release
    /// [`Button::TrackpadPressLeft`](crate::controllers::frame::Button::TrackpadPressLeft) and
    /// [`Button::TrackpadPressRight`](crate::controllers::frame::Button::TrackpadPressRight), on
    /// a controller with a trackpad. A resting thumb reads no force, so a light press is enough.
    pub trackpad_press: PressThresholds,
    /// Thumbstick and trackpad magnitudes below this count as centred when deciding whether the
    /// controllers are in use (see [`crate::controllers::scheme`]).
    pub stick_deadzone: f32,
    /// Locate the hand skeleton through `XR_EXT_hand_tracking` when the runtime offers it. The
    /// joints feed the finger curls; with this off, or without runtime support, the curls are
    /// estimated from the inputs.
    pub hand_tracking: bool,
    /// Run the input self-check once per session, logging how the runtime treats the action
    /// layout (see `crate::vr::input::self_check`).
    pub self_check: bool,
}

impl InputConfig {
    pub const fn new() -> Self {
        Self {
            trigger_press: PressThresholds {
                press: 0.7,
                release: 0.55,
            },
            squeeze_press: PressThresholds {
                press: 0.75,
                release: 0.5,
            },
            squeeze_force_press: PressThresholds {
                press: 0.3,
                release: 0.1,
            },
            trackpad_press: PressThresholds {
                press: 0.4,
                release: 0.2,
            },
            stick_deadzone: 0.3,
            hand_tracking: true,
            self_check: true,
        }
    }

    /// What derives `hand`'s grip press under `profile`: the squeeze force where the profile
    /// provides it, and the squeeze value elsewhere.
    pub fn grip_press(&self, profile: InteractionProfile, hand: Hand) -> GripPress {
        if provides(profile, hand, Input::Float(FloatInput::SqueezeForce)) {
            GripPress::Force(self.squeeze_force_press)
        } else {
            GripPress::Value(self.squeeze_press)
        }
    }

    /// How `hand`'s derived presses are read under `profile`.
    pub fn press_rules(&self, profile: InteractionProfile, hand: Hand) -> PressRules {
        PressRules {
            trigger: self.trigger_press,
            grip: self.grip_press(profile, hand),
            trackpad: self.trackpad_press,
        }
    }
}

impl Default for InputConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// On-foot stick locomotion. Turning is configured in `headpose.vr_turn` (see the module docs).
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LocomotionConfig {
    /// The frame the off-hand stick's direction is relative to.
    pub direction: LocomotionDirection,
    /// The stick's velocity, with fixed acceleration.
    pub stick: StickConfig,
    /// The filter that keeps the turning animations off the body.
    pub acts: ActFilterConfig,
    /// The on-foot view anchored to Rico's root rather than his animated head.
    pub view: StableViewConfig,
}

impl LocomotionConfig {
    pub const fn new() -> Self {
        Self {
            direction: LocomotionDirection::Head,
            stick: StickConfig::new(),
            acts: ActFilterConfig::new(),
            view: StableViewConfig::new(),
        }
    }
}

impl Default for LocomotionConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The frame the locomotion stick's direction is relative to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LocomotionDirection {
    /// Forward is where the head faces (yaw only).
    #[default]
    Head,
    /// Forward is where the off-hand controller points (yaw only).
    OffHand,
}

/// The per-tick mode detector (see [`crate::controllers::mode`]). It reads the game's state and
/// selects the OpenXR action set whenever the controllers are enabled, and changes nothing the game
/// sees.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeConfig {
    /// How many consecutive ticks a new gameplay mode must be detected before it takes effect. One
    /// switches at once; the default rides out a one-tick flicker during an animation blend.
    /// Counted at the game's 30 Hz tick, and scaled to the same duration at another sim rate
    /// ([`crate::sim_rate::scale_ticks`]).
    pub debounce_ticks: u32,
}

impl ModeConfig {
    pub const fn new() -> Self {
        Self { debounce_ticks: 2 }
    }
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The controller-to-game bindings (see [`crate::controllers::bindings`]). They inject only under
/// the controller scheme while a controller is active, so they default on.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BindingsConfig {
    /// Inject the controllers' input into the game's actions.
    pub enabled: bool,
    /// The radial deadzone of the stick that moves, steers, and navigates, `0..1`. The output is
    /// rescaled so that it starts from zero at the deadzone's edge.
    pub move_deadzone: f32,
    /// The trigger pull below which an analog trigger action (throttle, collective, thrust) reads
    /// as zero, `0..1`.
    pub trigger_deadzone: f32,
    /// How long the system button must be held to open the commlink rather than pause, seconds.
    pub long_press_s: f32,
    /// How far the move stick must be pushed when the wingsuit's A is pressed for the press to
    /// evade rather than open the parachute, `0..1`. The stick rests below half of it.
    pub evade_flick: f32,
    /// How recently the move stick must have left its rest for the wingsuit's A to evade, seconds:
    /// a flick, not a stick held over to steer.
    pub evade_window_s: f32,
    /// How far down the dominant stick must be flicked on foot to toggle the seated crouch, `0..1`.
    /// The stick must come back above half of it before the next flick counts.
    pub crouch_flick: f32,
    /// How far either stick must be pushed along one axis to step a menu, `0..1`. The step holds,
    /// and the game repeats it, until the push falls back below half of it.
    pub menu_stick_push: f32,
    /// How long a lone stick click waits for the other before it acts, seconds: both clicks pressed
    /// within it are the recenter chord, and neither acts on its own.
    pub stick_chord_window_s: f32,
    /// How long both stick clicks must be held together to recenter the VR view, seconds.
    pub recenter_hold_s: f32,
    /// How soon after the left stick click the right B must be pressed to show or hide the debug
    /// panel, seconds ([`crate::controllers::bindings::panel_chord`]). The click is not withheld
    /// for it: a lone stick click waits only the stick click chord window.
    pub panel_chord_window_s: f32,
    /// The yank gesture: a sharp pull of the grapple hand toward the body zips along an attached
    /// line.
    pub yank: YankConfig,
}

impl BindingsConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            move_deadzone: 0.15,
            trigger_deadzone: 0.05,
            long_press_s: 0.5,
            evade_flick: 0.8,
            evade_window_s: 0.25,
            crouch_flick: 0.8,
            menu_stick_push: 0.5,
            stick_chord_window_s: 0.1,
            recenter_hold_s: 1.0,
            panel_chord_window_s: 0.35,
            yank: YankConfig::new(),
        }
    }

    /// How long a lone stick click waits before it acts, seconds, by [`Hand::index`]: the stick
    /// click chord window on both hands. The debug panel chord does not lengthen the left click's
    /// wait, so that the horn, the weapon step, and the dual-wield retract act as soon as the
    /// recenter chord allows.
    pub fn stick_click_windows(&self) -> [f32; 2] {
        [self.stick_chord_window_s; 2]
    }
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self::new()
    }
}
