//! The debug panel chord: pressing the right B while the left stick click is held shows or hides
//! the mod's debug panel, as the F8 key does ([`crate::hud::egui_panel::toggle`]).
//!
//! The panel captures egui's input, which disables the game's device poll, and the bindings stand
//! down while it is disabled. So the chord is read from the raw controller frame and its edges on
//! every input tick, before the bindings decide whether to inject, and writes no game action: the
//! panel can be closed the way it was opened.
//!
//! The chord's presses belong to it alone. A lone left stick click is withheld from the tables for
//! [`BindingsConfig::panel_chord_window_s`](crate::controllers::config::BindingsConfig::panel_chord_window_s)
//! ([`BindingsConfig::stick_click_windows`](crate::controllers::config::BindingsConfig::stick_click_windows)),
//! and the chord counts only a B pressed within that window, so the stick click never reaches the
//! game before the chord claims it. Once the chord fires, the left stick click and the right B are
//! masked from the bindings, the native UI pointer, and the debug panel's own pointer until each is
//! released, so neither acts in the game nor right-clicks on the panel. The caller also tells the
//! tables to forget the withheld stick click ([`crate::controllers::bindings::tables::Bindings::cancel_stick_clicks`]),
//! which would otherwise pass through as a tap when the mask hides it.
//!
//! The chord stands aside for the recenter chord: once the right stick click is held too, the left
//! stick click belongs to the recenter chord until it is released. A B pressed after the window is
//! the B's own binding (a horn with nitrous, a tether retract with a reload), not the chord.

use crate::controllers::{
    bindings::controls::ButtonControl,
    frame::{Button, ControllerFrame, Edges, Hand},
};

/// The hand whose stick click starts the chord.
pub const CLICK_HAND: Hand = Hand::Left;

/// The hand whose B completes the chord.
pub const B_HAND: Hand = Hand::Right;

/// One tick of the chord's inputs.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ChordInput {
    /// The chord's stick click, on [`CLICK_HAND`].
    pub click: ButtonControl,
    /// The other hand's stick click, which makes the recenter chord instead.
    pub other_click: ButtonControl,
    /// The chord's B, on [`B_HAND`].
    pub b: ButtonControl,
}

impl ChordInput {
    /// Read the chord's inputs from a frame and the edges since the last tick.
    pub fn of(frame: &ControllerFrame, edges: &Edges) -> Self {
        let read = |hand: Hand, button: Button| ButtonControl {
            down: frame.hand(hand).buttons.contains(button),
            pressed: edges.pressed(hand, button),
            released: edges.released(hand, button),
        };
        Self {
            click: read(CLICK_HAND, Button::ThumbstickClick),
            other_click: read(CLICK_HAND.other(), Button::ThumbstickClick),
            b: read(B_HAND, Button::BClick),
        }
    }
}

/// What the chord does on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ChordTick {
    /// Show or hide the debug panel. The tables must forget the stick click they withhold.
    pub toggle: bool,
    /// Mask the chord's stick click.
    pub mask_click: bool,
    /// Mask the chord's B.
    pub mask_b: bool,
}

/// The chord's state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct PanelChord {
    phase: Phase,
    /// The stick click is masked until it is released.
    mask_click: bool,
    /// The B is masked until it is released.
    mask_b: bool,
}

impl PanelChord {
    pub const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            mask_click: false,
            mask_b: false,
        }
    }

    /// Advance the chord by one tick of `dt` seconds. `window_s` is how long after the stick click
    /// the B counts, seconds. A press that starts and ends between two ticks is masked for that
    /// tick, so its tap never reaches the bindings either.
    pub fn step(&mut self, input: ChordInput, dt: f32, window_s: f32) -> ChordTick {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut phase = match self.phase {
            _ if !input.click.held() => Phase::Idle,
            Phase::Idle => Phase::Armed { held_s: 0.0 },
            Phase::Armed { held_s } => Phase::Armed {
                held_s: held_s + dt,
            },
            Phase::Spent => Phase::Spent,
        };
        if let Phase::Armed { held_s } = phase
            && (input.other_click.held() || held_s >= window_s)
        {
            phase = Phase::Spent;
        }
        let toggle = matches!(phase, Phase::Armed { .. }) && input.b.pressed;
        if toggle {
            phase = Phase::Spent;
            self.mask_click = true;
            self.mask_b = true;
        }
        self.phase = phase;
        let tick = ChordTick {
            toggle,
            mask_click: self.mask_click,
            mask_b: self.mask_b,
        };
        if !input.click.down {
            self.mask_click = false;
        }
        if !input.b.down {
            self.mask_b = false;
        }
        tick
    }
}

/// Mask what `tick` masks from `frame` and `edges`, so that neither the bindings, the native UI
/// pointer, nor the debug panel's pointer sees it.
pub fn mask(frame: &mut ControllerFrame, edges: &mut Edges, tick: ChordTick) {
    if tick.mask_click {
        clear(frame, edges, CLICK_HAND, Button::ThumbstickClick);
        frame.hands[CLICK_HAND.index()].inputs.thumbstick_click = false;
    }
    if tick.mask_b {
        clear(frame, edges, B_HAND, Button::BClick);
        frame.hands[B_HAND.index()].inputs.b_click = false;
    }
}

/// Where the chord stands on the current stick click.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum Phase {
    /// The stick click is not held.
    #[default]
    Idle,
    /// The stick click has been held `held_s` seconds, within the window: a B press now is the
    /// chord.
    Armed { held_s: f32 },
    /// The stick click is held, but no longer the chord's: it fired, the window passed, or the
    /// other stick click joined.
    Spent,
}

fn clear(frame: &mut ControllerFrame, edges: &mut Edges, hand: Hand, button: Button) {
    let i = hand.index();
    frame.hands[i].buttons.set(button, false);
    edges.pressed[i].set(button, false);
    edges.released[i].set(button, false);
}

#[cfg(test)]
mod tests;
