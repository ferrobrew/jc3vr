//! The debug panel chord: pulling the right trigger while the system button is held shows or hides
//! the mod's debug panel, as the F8 key does ([`crate::hud::egui_panel::toggle`]).
//!
//! The panel captures egui's input, which disables the game's device poll, and the bindings stand
//! down while it is disabled. So the chord is read from the raw controller frame and its edges on
//! every input tick, before the bindings decide whether to inject, and writes no game action: the
//! panel can be closed the way it was opened.
//!
//! The chord's presses belong to it alone. The system button's press is consumed: its short press
//! does not pause and its long press does not open the commlink, from the chord until the button is
//! released. The trigger press is masked from the bindings and the native UI pointer until the
//! trigger is released, so it neither fires a weapon nor clicks a menu. While the system button is
//! held, a right trigger press is always the chord's.
//!
//! The system button is read from either hand, as the bindings read it, since runtimes report it on
//! one hand or both.

use crate::controllers::frame::{Button, ControllerFrame, Edges, Hand};

/// The hand whose trigger completes the chord.
pub const TRIGGER_HAND: Hand = Hand::Right;

/// One tick of the chord's inputs.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ChordInput {
    /// Either hand's system button is down at the latest frame.
    pub system_down: bool,
    /// Either hand's system button was pressed since the last tick.
    pub system_pressed: bool,
    /// The chord trigger is down at the latest frame.
    pub trigger_down: bool,
    /// The chord trigger was pressed since the last tick.
    pub trigger_pressed: bool,
}

impl ChordInput {
    /// Read the chord's inputs from a frame and the edges since the last tick.
    pub fn of(frame: &ControllerFrame, edges: &Edges) -> Self {
        Self {
            system_down: Hand::BOTH
                .into_iter()
                .any(|hand| frame.hand(hand).buttons.contains(Button::SystemClick)),
            system_pressed: Hand::BOTH
                .into_iter()
                .any(|hand| edges.pressed(hand, Button::SystemClick)),
            trigger_down: frame
                .hand(TRIGGER_HAND)
                .buttons
                .contains(Button::TriggerPress),
            trigger_pressed: edges.pressed(TRIGGER_HAND, Button::TriggerPress),
        }
    }
}

/// What the chord does on one tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct ChordTick {
    /// Show or hide the debug panel.
    pub toggle: bool,
    /// The system button's press was consumed this tick: forget the press the bindings were timing.
    pub cancel_system: bool,
    /// Mask the system button from the bindings.
    pub mask_system: bool,
    /// Mask the chord trigger from the bindings and the native UI pointer.
    pub mask_trigger: bool,
}

/// The chord's latches.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct PanelChord {
    /// The system button's press is consumed until it is released.
    consume_system: bool,
    /// The trigger's press is masked until it is released.
    mask_trigger: bool,
}

impl PanelChord {
    pub const fn new() -> Self {
        Self {
            consume_system: false,
            mask_trigger: false,
        }
    }

    /// Advance the chord by one tick. A press that starts and ends between two ticks is masked for
    /// that tick, so its tap never reaches the bindings either.
    pub fn step(&mut self, input: ChordInput) -> ChordTick {
        let system_held = input.system_down || input.system_pressed;
        let toggle = !self.mask_trigger && system_held && input.trigger_pressed;
        let cancel_system = toggle && !self.consume_system;
        self.mask_trigger |= system_held && (input.trigger_down || input.trigger_pressed);
        self.consume_system |= toggle;
        let tick = ChordTick {
            toggle,
            cancel_system,
            mask_system: self.consume_system,
            mask_trigger: self.mask_trigger,
        };
        if !input.trigger_down {
            self.mask_trigger = false;
        }
        if !input.system_down {
            self.consume_system = false;
        }
        tick
    }
}

/// Mask what `tick` masks from `frame` and `edges`, so that neither the bindings nor the native UI
/// pointer sees it.
pub fn mask(frame: &mut ControllerFrame, edges: &mut Edges, tick: ChordTick) {
    if tick.mask_system {
        for hand in Hand::BOTH {
            clear(frame, edges, hand, Button::SystemClick);
            frame.hands[hand.index()].inputs.system_click = false;
        }
    }
    if tick.mask_trigger {
        clear(frame, edges, TRIGGER_HAND, Button::TriggerPress);
        let inputs = &mut frame.hands[TRIGGER_HAND.index()].inputs;
        inputs.trigger = 0.0;
        inputs.trigger_click = false;
    }
}

fn clear(frame: &mut ControllerFrame, edges: &mut Edges, hand: Hand, button: Button) {
    let i = hand.index();
    frame.hands[i].buttons.set(button, false);
    edges.pressed[i].set(button, false);
    edges.released[i].set(button, false);
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use jc3gi::input::input_action_map::Action;

    use super::*;
    use crate::controllers::{
        bindings::{controls::Controls, tables::Bindings},
        config::BindingsConfig,
        frame::ActionSetKind,
        mode::{Mode, OnFootVariant},
    };

    fn frame() -> ControllerFrame {
        ControllerFrame {
            sequence: 0,
            display_time_ns: 0,
            published_at: Instant::now(),
            focused: true,
            action_set: ActionSetKind::OnFoot,
            hands: Default::default(),
        }
    }

    fn input(system: bool, trigger: bool, trigger_pressed: bool) -> ChordInput {
        ChordInput {
            system_down: system,
            system_pressed: false,
            trigger_down: trigger,
            trigger_pressed,
        }
    }

    #[test]
    fn the_chord_toggles_once_per_trigger_press() {
        let mut chord = PanelChord::new();
        assert_eq!(chord.step(input(true, false, false)), ChordTick::default());
        let fired = chord.step(input(true, true, true));
        assert_eq!(
            fired,
            ChordTick {
                toggle: true,
                cancel_system: true,
                mask_system: true,
                mask_trigger: true,
            }
        );
        let held = chord.step(input(true, true, false));
        assert!(!held.toggle && held.mask_system && held.mask_trigger);
        // The release tick is masked too, so the bindings never see the trigger's release edge.
        let released = chord.step(input(true, false, false));
        assert!(!released.toggle && released.mask_system && released.mask_trigger);
        let rest = chord.step(input(true, false, false));
        assert!(rest.mask_system && !rest.mask_trigger);
        // Pulled again with the system button still held: a second toggle, and the system press is
        // already consumed.
        let again = chord.step(input(true, true, true));
        assert!(again.toggle && !again.cancel_system);
    }

    #[test]
    fn the_system_press_stays_consumed_until_released() {
        let mut chord = PanelChord::new();
        chord.step(input(true, true, true));
        chord.step(input(true, false, false));
        let trigger_up = chord.step(input(true, false, false));
        assert!(trigger_up.mask_system && !trigger_up.mask_trigger);
        // The release tick is masked too, so the bindings never see a short press end.
        let system_up = chord.step(input(false, false, false));
        assert!(system_up.mask_system);
        assert_eq!(chord.step(input(false, false, false)), ChordTick::default());
    }

    #[test]
    fn the_trigger_alone_or_first_is_not_the_chord() {
        let mut chord = PanelChord::new();
        assert_eq!(chord.step(input(false, true, true)), ChordTick::default());
        // The system button pressed while the trigger is held: no toggle, but the held trigger is
        // masked from then on, since the system button is held.
        let late = chord.step(input(true, true, false));
        assert!(!late.toggle && late.mask_trigger && !late.mask_system);
    }

    #[test]
    fn masking_clears_the_buttons_and_their_edges() {
        let mut frame = frame();
        let mut edges = Edges::default();
        for hand in Hand::BOTH {
            frame.hands[hand.index()]
                .buttons
                .set(Button::SystemClick, true);
            edges.pressed[hand.index()].set(Button::SystemClick, true);
        }
        let right = TRIGGER_HAND.index();
        frame.hands[right].buttons.set(Button::TriggerPress, true);
        frame.hands[right].inputs.trigger = 1.0;
        edges.pressed[right].set(Button::TriggerPress, true);
        let left = TRIGGER_HAND.other().index();
        frame.hands[left].buttons.set(Button::TriggerPress, true);
        mask(
            &mut frame,
            &mut edges,
            ChordTick {
                mask_system: true,
                mask_trigger: true,
                ..Default::default()
            },
        );
        for hand in Hand::BOTH {
            assert!(!frame.hand(hand).buttons.contains(Button::SystemClick));
            assert!(!edges.pressed(hand, Button::SystemClick));
        }
        assert!(!frame.hands[right].buttons.contains(Button::TriggerPress));
        assert!(!edges.pressed(TRIGGER_HAND, Button::TriggerPress));
        assert_eq!(frame.hands[right].inputs.trigger, 0.0);
        // The other hand's trigger is not the chord's.
        assert!(frame.hands[left].buttons.contains(Button::TriggerPress));
    }

    #[test]
    fn the_masked_chord_neither_fires_nor_pauses_nor_opens_the_commlink() {
        let config = BindingsConfig::new();
        let mut chord = PanelChord::new();
        let mut bindings = Bindings::new();
        let mut toggles = 0;
        let mut previous = (false, false);
        // The system button pressed, the trigger pulled and held, the trigger let go, and the
        // system button let go, each over a few ticks.
        let ticks = [
            (true, false),
            (true, true),
            (true, true),
            (true, false),
            (false, false),
            (false, false),
        ];
        for (system, trigger) in ticks {
            let mut frame = frame();
            let mut edges = Edges::default();
            let right = TRIGGER_HAND.index();
            frame.hands[0].buttons.set(Button::SystemClick, system);
            frame.hands[right]
                .buttons
                .set(Button::TriggerPress, trigger);
            frame.hands[right].inputs.trigger = if trigger { 1.0 } else { 0.0 };
            edges.pressed[0].set(Button::SystemClick, system && !previous.0);
            edges.released[0].set(Button::SystemClick, !system && previous.0);
            edges.pressed[right].set(Button::TriggerPress, trigger && !previous.1);
            edges.released[right].set(Button::TriggerPress, !trigger && previous.1);
            previous = (system, trigger);

            let tick = chord.step(ChordInput::of(&frame, &edges));
            toggles += usize::from(tick.toggle);
            if tick.cancel_system {
                bindings.cancel_system();
            }
            mask(&mut frame, &mut edges, tick);
            let controls =
                Controls::from_frame(&frame, &edges, &config, Hand::Right, 0.0, [None; 2]);
            let step = bindings.step(
                Mode::OnFoot(OnFootVariant::Ground),
                &controls,
                1.0 / 30.0,
                &config,
            );
            for action in [Action::FIRE_RIGHT, Action::GUI_PAUSE, Action::GUI_PDA] {
                assert!(
                    step.desired.iter().all(|d| d.target.action != action),
                    "{action:?}"
                );
            }
        }
        assert_eq!(toggles, 1);
    }
}
