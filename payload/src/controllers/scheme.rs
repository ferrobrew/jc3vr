//! Controller versus gamepad scheme selection.
//!
//! The controller scheme (hand aiming, the controller bindings, controller-owned hands) and the
//! gamepad scheme (gaze aiming, the game's own gamepad and keyboard input, animated hands) switch on
//! the most recent deliberate input. Under [`SchemeSetting::Auto`]:
//!
//! - a controller button press (not a capacitive touch), a trigger or grip press, or a thumbstick
//!   or trackpad pushed past the deadzone selects the controller scheme;
//! - a gamepad button, trigger, or stick, or any keyboard or mouse input, selects the gamepad
//!   scheme;
//! - input from both in the same frame, or from neither, keeps the current scheme.
//!
//! Without a controller frame (no session, controllers disabled, or no controller connected) the
//! scheme is always the gamepad scheme, so the mod's behaviour without controllers is unchanged.
//! [`SchemeSetting::Controllers`] and [`SchemeSetting::Gamepad`] pin the scheme regardless.
//!
//! The observation runs once per rendered frame from the VR frame loop, right after the controller
//! frame is published ([`tick`]). The game's own input reaches the payload in two places that are
//! independent of the engine: XInput for gamepads, polled directly, and `GetLastInputInfo` for the
//! keyboard and mouse, which the OS updates on every hardware input event to the desktop. Engine
//! effectors are deliberately not used: once the bindings inject controller input into them, they
//! no longer tell the two devices apart. While the in-headset debug panel is up, keyboard and mouse
//! input drives the panel rather than the game, so it is not counted.

use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::ERROR_SUCCESS,
    UI::Input::{
        KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
        XboxController::{XINPUT_GAMEPAD, XINPUT_STATE, XInputGetState},
    },
};

use crate::{
    config::Config,
    controllers::{
        config::{ControllersConfig, SchemeSetting},
        frame::{self, ControllerFrame, HandFrame},
    },
};

/// The control scheme in effect.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Scheme {
    /// Hand aiming, the controller bindings, and controller-owned hands.
    Controllers,
    /// Gaze aiming, the game's own gamepad and keyboard input, and animated hands.
    Gamepad,
}

/// The scheme in effect now. Cheap: one config read, one atomic load, and a peek at the published
/// controller frame.
pub fn current() -> Scheme {
    let (enabled, setting) = Config::lock_query(|c| (c.controllers.enabled, c.controllers.scheme));
    if !enabled {
        return Scheme::Gamepad;
    }
    let present = frame::with_latest(ControllerFrame::any_active).unwrap_or(false);
    resolve(setting, selected(), present)
}

/// Whether the controller scheme is in effect: the gate every controller subsystem checks.
pub fn is_controllers() -> bool {
    current() == Scheme::Controllers
}

/// The input sources seen this frame.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Activity {
    pub controllers: bool,
    pub gamepad: bool,
    pub keyboard_mouse: bool,
}

/// The auto-selection state: the scheme the most recent unambiguous input selected.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SchemeSelector {
    selected: Scheme,
}

impl SchemeSelector {
    /// A selector that starts on the gamepad scheme, the mod's behaviour without controllers.
    pub const fn new() -> Self {
        Self {
            selected: Scheme::Gamepad,
        }
    }

    /// Fold one frame's activity in and return the selected scheme.
    pub fn observe(&mut self, activity: Activity) -> Scheme {
        let pad = activity.gamepad || activity.keyboard_mouse;
        match (activity.controllers, pad) {
            (true, false) => self.selected = Scheme::Controllers,
            (false, true) => self.selected = Scheme::Gamepad,
            _ => {}
        }
        self.selected
    }

    pub fn selected(&self) -> Scheme {
        self.selected
    }
}

impl Default for SchemeSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// The scheme in effect for a setting, the auto-selected scheme, and whether any controller is
/// present.
pub fn resolve(setting: SchemeSetting, selected: Scheme, controllers_present: bool) -> Scheme {
    match setting {
        SchemeSetting::Controllers => Scheme::Controllers,
        SchemeSetting::Gamepad => Scheme::Gamepad,
        SchemeSetting::Auto if controllers_present => selected,
        SchemeSetting::Auto => Scheme::Gamepad,
    }
}

/// Whether a controller shows deliberate input: a press of any non-touch button (including the
/// derived trigger and grip presses), or a thumbstick or trackpad-press past `deadzone`.
pub fn controller_activity(hand: &HandFrame, deadzone: f32) -> bool {
    if !hand.active {
        return false;
    }
    let pressed = hand.buttons.iter().any(|b| !b.is_touch());
    let stick = hand.inputs.thumbstick.length() > deadzone;
    let trackpad = hand.inputs.trackpad_force > deadzone;
    pressed || stick || trackpad
}

/// Whether a gamepad shows deliberate input: any button, a trigger past XInput's trigger
/// threshold, or a stick past XInput's recommended deadzone.
pub fn gamepad_activity(pad: &XINPUT_GAMEPAD) -> bool {
    let beyond = |x: i16, y: i16, deadzone: i16| {
        let (x, y) = (i32::from(x), i32::from(y));
        let deadzone = i32::from(deadzone);
        x * x + y * y > deadzone * deadzone
    };
    pad.wButtons.0 != 0
        || pad.bLeftTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD
        || pad.bRightTrigger > XINPUT_GAMEPAD_TRIGGER_THRESHOLD
        || beyond(
            pad.sThumbLX,
            pad.sThumbLY,
            XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE,
        )
        || beyond(
            pad.sThumbRX,
            pad.sThumbRY,
            XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE,
        )
}

/// Observe this frame's input and update the auto-selected scheme. Called by the VR frame loop
/// right after it publishes `frame`.
pub(crate) fn tick(frame: &ControllerFrame, cfg: &ControllersConfig) {
    let panel_up = Config::lock_query(|c| c.hud.egui_panel.enabled);
    let mut poller = POLLER.lock();
    let activity = Activity {
        controllers: frame
            .hands
            .iter()
            .any(|h| controller_activity(h, cfg.input.stick_deadzone)),
        gamepad: poller.gamepad_activity(),
        keyboard_mouse: poller.keyboard_mouse_activity() && !panel_up,
    };
    let mut selector = SchemeSelector {
        selected: selected(),
    };
    let before = selector.selected();
    let after = selector.observe(activity);
    if before != after {
        tracing::info!(target: "controllers", ?activity, "auto scheme selection: {before:?} -> {after:?}");
    }
    SELECTED.store(encode(after), Ordering::Relaxed);
}

/// The scheme the auto-selection last chose, whether or not a setting pins another.
pub fn selected() -> Scheme {
    decode(SELECTED.load(Ordering::Relaxed))
}

/// XInput's recommended deadzones and trigger threshold (`XINPUT_GAMEPAD_*` in `XInput.h`).
const XINPUT_GAMEPAD_LEFT_THUMB_DEADZONE: i16 = 7849;
const XINPUT_GAMEPAD_RIGHT_THUMB_DEADZONE: i16 = 8689;
const XINPUT_GAMEPAD_TRIGGER_THRESHOLD: u8 = 30;

/// XInput's user slots.
const XUSER_MAX_COUNT: u32 = 4;

/// How often an empty XInput slot is probed. Polling a disconnected slot is slow on some systems
/// (the driver re-enumerates devices), so empty slots are probed once a second at most, one per
/// probe.
const EMPTY_SLOT_PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// The auto-selected scheme, encoded by [`encode`].
static SELECTED: AtomicU8 = AtomicU8::new(0);

fn encode(scheme: Scheme) -> u8 {
    match scheme {
        Scheme::Gamepad => 0,
        Scheme::Controllers => 1,
    }
}

fn decode(value: u8) -> Scheme {
    match value {
        1 => Scheme::Controllers,
        _ => Scheme::Gamepad,
    }
}

static POLLER: Mutex<DesktopInputPoller> = Mutex::new(DesktopInputPoller {
    connected: [false; XUSER_MAX_COUNT as usize],
    next_probe: None,
    probe_slot: 0,
    last_input_tick: None,
});

/// Polls XInput and the OS's last-input time.
struct DesktopInputPoller {
    /// Whether each XInput slot answered its last poll.
    connected: [bool; XUSER_MAX_COUNT as usize],
    /// When the next empty slot may be probed.
    next_probe: Option<Instant>,
    /// The next empty slot to probe, round robin.
    probe_slot: u32,
    /// The last `GetLastInputInfo` tick seen; `None` before the first poll.
    last_input_tick: Option<u32>,
}

impl DesktopInputPoller {
    /// Poll every connected XInput slot, and probe one empty slot when due. True if any shows
    /// deliberate input.
    fn gamepad_activity(&mut self) -> bool {
        let now = Instant::now();
        let probe = self.next_probe.is_none_or(|t| now >= t);
        let mut probed = false;
        let mut active = false;
        for offset in 0..XUSER_MAX_COUNT {
            let slot = (self.probe_slot + offset) % XUSER_MAX_COUNT;
            let index = slot as usize;
            if !self.connected[index] {
                if !probe || probed {
                    continue;
                }
                probed = true;
                self.probe_slot = (slot + 1) % XUSER_MAX_COUNT;
                self.next_probe = Some(now + EMPTY_SLOT_PROBE_INTERVAL);
            }
            let mut state = XINPUT_STATE::default();
            // SAFETY: `state` is a valid out-parameter for the duration of the call.
            let result = unsafe { XInputGetState(slot, &mut state) };
            self.connected[index] = result == ERROR_SUCCESS.0;
            if self.connected[index] && gamepad_activity(&state.Gamepad) {
                active = true;
            }
        }
        active
    }

    /// Whether the OS saw keyboard or mouse input since the previous poll.
    fn keyboard_mouse_activity(&mut self) -> bool {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        // SAFETY: `info` is a valid, sized out-parameter for the duration of the call.
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return false;
        }
        let previous = self.last_input_tick.replace(info.dwTime);
        previous.is_some_and(|p| p != info.dwTime)
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec2;
    use windows::Win32::UI::Input::XboxController::XINPUT_GAMEPAD_BUTTON_FLAGS;

    use super::*;
    use crate::controllers::frame::{Button, ButtonSet};

    fn activity(controllers: bool, gamepad: bool, keyboard_mouse: bool) -> Activity {
        Activity {
            controllers,
            gamepad,
            keyboard_mouse,
        }
    }

    #[test]
    fn the_most_recent_unambiguous_input_selects() {
        let mut s = SchemeSelector::new();
        assert_eq!(s.selected(), Scheme::Gamepad);
        assert_eq!(s.observe(activity(true, false, false)), Scheme::Controllers);
        assert_eq!(
            s.observe(activity(false, false, false)),
            Scheme::Controllers
        );
        assert_eq!(s.observe(activity(false, false, true)), Scheme::Gamepad);
        assert_eq!(s.observe(activity(true, false, false)), Scheme::Controllers);
        assert_eq!(s.observe(activity(false, true, false)), Scheme::Gamepad);
    }

    #[test]
    fn simultaneous_input_keeps_the_current_scheme() {
        let mut s = SchemeSelector::new();
        assert_eq!(s.observe(activity(true, true, false)), Scheme::Gamepad);
        s.observe(activity(true, false, false));
        assert_eq!(s.observe(activity(true, false, true)), Scheme::Controllers);
    }

    #[test]
    fn settings_pin_and_auto_needs_a_controller() {
        let pinned = resolve(SchemeSetting::Controllers, Scheme::Gamepad, false);
        assert_eq!(pinned, Scheme::Controllers);
        let pinned = resolve(SchemeSetting::Gamepad, Scheme::Controllers, true);
        assert_eq!(pinned, Scheme::Gamepad);
        let auto = resolve(SchemeSetting::Auto, Scheme::Controllers, true);
        assert_eq!(auto, Scheme::Controllers);
        let absent = resolve(SchemeSetting::Auto, Scheme::Controllers, false);
        assert_eq!(absent, Scheme::Gamepad);
    }

    #[test]
    fn touches_are_not_controller_activity() {
        let mut hand = HandFrame {
            active: true,
            ..Default::default()
        };
        let mut touched = ButtonSet::EMPTY;
        touched.set(Button::ThumbstickTouch, true);
        touched.set(Button::TriggerTouch, true);
        hand.buttons = touched;
        assert!(!controller_activity(&hand, 0.3));

        hand.buttons.set(Button::AClick, true);
        assert!(controller_activity(&hand, 0.3));
    }

    #[test]
    fn a_pushed_stick_is_controller_activity() {
        let mut hand = HandFrame {
            active: true,
            ..Default::default()
        };
        hand.inputs.thumbstick = Vec2::new(0.1, 0.1);
        assert!(!controller_activity(&hand, 0.3));
        hand.inputs.thumbstick = Vec2::new(0.0, 0.5);
        assert!(controller_activity(&hand, 0.3));
        hand.active = false;
        assert!(!controller_activity(&hand, 0.3));
    }

    #[test]
    fn gamepad_deadzones_reject_drift() {
        let mut pad = XINPUT_GAMEPAD {
            sThumbLX: 3000,
            sThumbLY: -3000,
            bLeftTrigger: 10,
            ..Default::default()
        };
        assert!(!gamepad_activity(&pad));
        pad.sThumbLX = 9000;
        assert!(gamepad_activity(&pad));
        pad.sThumbLX = 0;
        pad.bRightTrigger = 200;
        assert!(gamepad_activity(&pad));
        pad.bRightTrigger = 0;
        pad.wButtons = XINPUT_GAMEPAD_BUTTON_FLAGS(0x1000);
        assert!(gamepad_activity(&pad));
    }
}
