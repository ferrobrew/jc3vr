//! The controller pointer: the dominant hand's aim ray against the rendered panel, with its trigger
//! as the primary button.
//!
//! The input tick updates the pointer once per tick ([`update_controller`]) and the UI mouse
//! delivery reads the latest sample ([`selected_controller`]). The trigger is armed only after a
//! released sample, so a trigger held across a mode transition, focus loss, or tracking recovery
//! never becomes a fresh click.

use std::time::Instant;

use parking_lot::Mutex;

use crate::{
    config::Config,
    controllers::{
        frame::{ActionSetKind, Button, ControllerFrame, Edges, Hand, MAX_FRAME_AGE},
        scheme,
    },
    headpose,
    hud::pointer::{
        panel::{PanelHit, aim_at_panel, intersect_panel},
        route,
    },
};

/// The controller-owned game-UI pointer for this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ControllerPointer {
    pub hand: Hand,
    pub uv: (f32, f32),
    /// Bit zero is the primary Scaleform button.
    pub buttons: u32,
    /// Whether the controller source owns the UI this tick: the ray is on the panel, or a captured
    /// press is being released outside it.
    pub owns_ui: bool,
}

/// Update the controller pointer from the post-recovery controller frame and the edges taken this
/// tick. The caller supplies the already-detected UI mode so a gameplay trigger can never become a
/// game-menu click.
pub fn update_controller(
    frame: Option<&ControllerFrame>,
    edges: &Edges,
    ui_mode: bool,
    hand: Hand,
    recovery_ready: bool,
) -> Option<ControllerPointer> {
    let now = Instant::now();
    let mut state = CONTROLLER_STATE.lock();
    state.expire(now);
    let trigger = TriggerSample {
        level: frame.is_some_and(|frame| {
            frame.focused && frame.hand(hand).buttons.contains(Button::TriggerPress)
        }),
        pressed: edges.pressed(hand, Button::TriggerPress),
        released: edges.released(hand, Button::TriggerPress),
    };
    let trigger_down = trigger.down(state.trigger_down);
    let ui_frame = frame.filter(|frame| {
        frame.focused && frame.action_set == ActionSetKind::Ui && frame.is_fresh_at(now)
    });
    let usable = native_pointer_enabled()
        && recovery_ready
        && ui_mode
        && headpose::source() == headpose::Source::Vr
        && scheme::is_controllers();
    let Some(frame) = ui_frame.filter(|_| usable) else {
        state.disarm(now);
        state.trigger_down = trigger_down;
        return None;
    };
    let side = frame.hand(hand);
    let hit = (side.active && side.grip.is_valid() && side.aim.is_valid())
        .then(|| aim_at_panel(side.aim.pose))
        .flatten()
        .and_then(|(ray, corners)| intersect_panel(ray, &corners));
    state.suppress_desktop_press(route::desktop_capture(), trigger_down);
    state.step(hit, trigger_down, hand, now)
}

/// The latest controller pointer while it owns the UI. A stalled input tick never keeps a button
/// held indefinitely: the sample expires after [`MAX_FRAME_AGE`].
pub fn selected_controller() -> Option<ControllerPointer> {
    if !native_pointer_enabled() {
        return None;
    }
    let mut state = CONTROLLER_STATE.lock();
    state.expire(Instant::now());
    state.output.filter(|pointer| pointer.owns_ui)
}

/// Clear controller ownership after focus, tracking, or input-set loss. The next controller press
/// must follow a real released sample before it can click.
pub fn clear_controller() {
    let mut state = CONTROLLER_STATE.lock();
    let trigger_down = state.trigger_down;
    *state = ControllerState::new();
    state.trigger_down = trigger_down;
}

/// The hand whose menu trigger belongs to the pointer. The UI binding table uses that trigger for
/// page navigation; the pointer owns it whenever it shows, and for the rest of a press that began
/// on the panel. So a press aimed at a button just off the panel's edge clicks nothing rather than
/// turning a page, and the pages stay on the other trigger and on the grip-held trigger.
pub fn owns_menu_trigger() -> Option<Hand> {
    let state = CONTROLLER_STATE.lock();
    (state.showing || state.trigger_claimed)
        .then_some(state.hand)
        .flatten()
}

/// The trigger as one input tick sees it: the level, and the edges latched since the last tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(super) struct TriggerSample {
    pub(super) level: bool,
    pub(super) pressed: bool,
    pub(super) released: bool,
}

impl TriggerSample {
    /// Whether the pointer treats the trigger as down this tick, given whether it did last tick. A
    /// tap that starts and ends between two ticks reads as down for one tick, and a release followed
    /// by a new press between two ticks reads as up for one tick, so neither is lost.
    pub(super) fn down(self, was_down: bool) -> bool {
        if was_down && self.released {
            false
        } else {
            self.level || self.pressed
        }
    }
}

fn native_pointer_enabled() -> bool {
    Config::lock_query(|c| c.hud.redirect && c.hud.quad && c.hud.cursor.enabled)
}

/// One pointing hand's press state against a panel. The game-UI pointer keeps one in
/// [`CONTROLLER_STATE`]; the egui panel's pointer keeps its own.
pub(super) struct ControllerState {
    armed: bool,
    captured: bool,
    output: Option<ControllerPointer>,
    updated: Option<Instant>,
    /// The trigger as the pointer saw it on the previous tick.
    trigger_down: bool,
    /// Whether the current trigger press began with the ray on the panel.
    trigger_claimed: bool,
    /// Whether the pointer is usable this tick: a game menu is up and the native pointer is on.
    showing: bool,
    hand: Option<Hand>,
}

static CONTROLLER_STATE: Mutex<ControllerState> = Mutex::new(ControllerState::new());

impl ControllerState {
    pub(super) const fn new() -> Self {
        Self {
            armed: false,
            captured: false,
            output: None,
            updated: None,
            trigger_down: false,
            trigger_claimed: false,
            showing: false,
            hand: None,
        }
    }

    /// A fresh state for a hand whose trigger is `trigger_down` now. A held trigger must be released
    /// before it can click.
    pub(super) const fn after(trigger_down: bool) -> Self {
        let mut state = Self::new();
        state.trigger_down = trigger_down;
        state
    }

    /// Whether a press that began on the panel is held.
    pub(super) const fn captured(&self) -> bool {
        self.captured
    }

    fn disarm(&mut self, now: Instant) {
        self.armed = false;
        self.captured = false;
        self.output = None;
        self.trigger_claimed = false;
        self.showing = false;
        self.updated = Some(now);
    }

    fn suppress_desktop_press(&mut self, desktop_capture: bool, trigger_down: bool) {
        if desktop_capture && trigger_down {
            self.armed = false;
            self.captured = false;
        }
    }

    fn expire(&mut self, now: Instant) {
        if self
            .updated
            .is_some_and(|time| now.saturating_duration_since(time) > MAX_FRAME_AGE)
        {
            self.armed = false;
            self.captured = false;
            self.output = None;
            self.trigger_claimed = false;
            self.showing = false;
            self.updated = None;
        }
    }

    pub(super) fn step(
        &mut self,
        hit: Option<PanelHit>,
        trigger_down: bool,
        hand: Hand,
        now: Instant,
    ) -> Option<ControllerPointer> {
        self.expire(now);
        let was_armed = self.armed;
        let press_began = trigger_down && !self.trigger_down;
        if trigger_down {
            self.armed = false;
        }
        let output = if let Some(hit) = hit {
            if !trigger_down {
                let owns_ui = hit.inside || self.captured;
                self.armed = true;
                self.captured = false;
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 0,
                    owns_ui,
                })
            } else if self.captured {
                // Continue a drag outside the panel while the ray still meets its plane.
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 1,
                    owns_ui: true,
                })
            } else if was_armed && hit.inside {
                self.captured = true;
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 1,
                    owns_ui: true,
                })
            } else {
                // A held trigger that entered UI before the pointer was usable remains inert until a
                // complete release/press cycle. It can still provide hover feedback.
                Some(ControllerPointer {
                    hand,
                    uv: hit.uv,
                    buttons: 0,
                    owns_ui: hit.inside,
                })
            }
        } else {
            self.captured = false;
            self.armed = !trigger_down;
            None
        };

        if press_began {
            self.trigger_claimed = output.is_some_and(|pointer| pointer.owns_ui);
        } else if !trigger_down {
            self.trigger_claimed = false;
        }
        self.trigger_down = trigger_down;
        self.showing = true;
        self.hand = Some(hand);
        self.output = output;
        self.updated = Some(now);
        output
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const INSIDE: Option<PanelHit> = Some(PanelHit {
        uv: (0.5, 0.5),
        inside: true,
        distance: 1.0,
    });

    const OUTSIDE: Option<PanelHit> = Some(PanelHit {
        uv: (1.0, 0.5),
        inside: false,
        distance: 1.0,
    });

    fn buttons(
        state: &mut ControllerState,
        hit: Option<PanelHit>,
        down: bool,
        now: Instant,
    ) -> u32 {
        state
            .step(hit, down, Hand::Right, now)
            .map_or(0, |pointer| pointer.buttons)
    }

    #[test]
    fn captured_release_outside_retains_ownership_and_current_uv() {
        let mut state = ControllerState::new();
        let now = Instant::now();
        let inside = Some(PanelHit {
            uv: (0.2, 0.3),
            inside: true,
            distance: 1.0,
        });
        state.step(inside, false, Hand::Right, now);
        state.step(inside, true, Hand::Right, now);
        let outside = Some(PanelHit {
            uv: (1.0, 0.8),
            inside: false,
            distance: 1.0,
        });
        let release = state.step(outside, false, Hand::Right, now).unwrap();
        assert_eq!(
            (release.uv, release.buttons, release.owns_ui),
            ((1.0, 0.8), 0, true)
        );
        assert!(
            !state
                .step(outside, false, Hand::Right, now)
                .unwrap()
                .owns_ui
        );
    }

    #[test]
    fn press_during_desktop_capture_requires_real_release() {
        let mut state = ControllerState::new();
        let now = Instant::now();
        state.step(INSIDE, false, Hand::Right, now);
        state.suppress_desktop_press(true, true);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 0);
        state.suppress_desktop_press(false, true);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 0);
        state.step(INSIDE, false, Hand::Right, now);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 1);
    }

    #[test]
    fn outside_press_is_consumed_before_entering_held() {
        let mut state = ControllerState::new();
        let now = Instant::now();
        state.step(OUTSIDE, false, Hand::Right, now);
        state.step(OUTSIDE, true, Hand::Right, now);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 0);
        state.step(INSIDE, false, Hand::Right, now);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 1);
    }

    #[test]
    fn stale_capture_requires_neutral_even_without_an_output_read() {
        for read_output in [false, true] {
            let mut state = ControllerState::new();
            let now = Instant::now();
            state.step(INSIDE, false, Hand::Right, now);
            assert_eq!(buttons(&mut state, INSIDE, true, now), 1);
            let later = now + MAX_FRAME_AGE + Duration::from_millis(1);
            if read_output {
                state.expire(later);
                assert!(state.output.is_none());
            }
            assert_eq!(buttons(&mut state, INSIDE, true, later), 0);
            state.step(INSIDE, false, Hand::Right, later);
            assert_eq!(buttons(&mut state, INSIDE, true, later), 1);
        }
    }

    #[test]
    fn a_tap_between_ticks_clicks_once() {
        let tap = TriggerSample {
            level: false,
            pressed: true,
            released: true,
        };
        let rest = TriggerSample {
            level: false,
            pressed: false,
            released: false,
        };
        let mut state = ControllerState::new();
        let now = Instant::now();
        state.step(INSIDE, false, Hand::Right, now);
        let down = tap.down(state.trigger_down);
        assert_eq!(buttons(&mut state, INSIDE, down, now), 1);
        let down = rest.down(state.trigger_down);
        assert_eq!(buttons(&mut state, INSIDE, down, now), 0);
    }

    #[test]
    fn a_release_and_repress_between_ticks_clicks_twice() {
        let repress = TriggerSample {
            level: true,
            pressed: true,
            released: true,
        };
        let held = TriggerSample {
            level: true,
            pressed: false,
            released: false,
        };
        assert!(!repress.down(true));
        assert!(held.down(false));
        let mut state = ControllerState::new();
        let now = Instant::now();
        state.step(INSIDE, false, Hand::Right, now);
        assert_eq!(buttons(&mut state, INSIDE, true, now), 1);
        let down = repress.down(state.trigger_down);
        assert_eq!(buttons(&mut state, INSIDE, down, now), 0);
        let down = held.down(state.trigger_down);
        assert_eq!(buttons(&mut state, INSIDE, down, now), 1);
    }

    #[test]
    fn the_menu_trigger_claim_latches_at_the_press() {
        let now = Instant::now();
        let claimed = |state: &ControllerState| state.trigger_claimed;

        let mut state = ControllerState::new();
        state.step(OUTSIDE, false, Hand::Right, now);
        state.step(OUTSIDE, true, Hand::Right, now);
        assert!(!claimed(&state));
        state.step(INSIDE, true, Hand::Right, now);
        assert!(
            !claimed(&state),
            "a page press from off the panel is held to its release"
        );

        let mut state = ControllerState::new();
        state.step(INSIDE, false, Hand::Right, now);
        state.step(INSIDE, true, Hand::Right, now);
        assert!(claimed(&state));
        state.step(None, true, Hand::Right, now);
        assert!(claimed(&state), "a panel press stays claimed off the panel");
        state.step(None, false, Hand::Right, now);
        assert!(!claimed(&state));
    }

    #[test]
    fn the_pointer_owns_its_trigger_whenever_it_shows() {
        let now = Instant::now();
        let owner = |state: &ControllerState| {
            (state.showing || state.trigger_claimed)
                .then_some(state.hand)
                .flatten()
        };
        let mut state = ControllerState::new();
        assert_eq!(owner(&state), None);
        // A press just off the panel's edge is the pointer's, not a page.
        state.step(OUTSIDE, false, Hand::Right, now);
        state.step(OUTSIDE, true, Hand::Right, now);
        assert_eq!(owner(&state), Some(Hand::Right));
        // With the menu gone, the trigger is the table's again.
        state.disarm(now);
        assert_eq!(owner(&state), None);
    }
}
