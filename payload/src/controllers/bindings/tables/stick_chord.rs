//! The stick-click chord: both stick clicks held together for the recenter hold time recenter the
//! VR view, in every mode. Pure.
//!
//! Each stick click also has its own binding, and the game acts on a press as soon as it sees it
//! (a grenade throw, a weapon step, a horn). So that a chord never leaks one of those, a lone click
//! is withheld from the tables for the chord window: if the other click joins within it, the two are
//! a chord and neither click reaches the tables until both are released; if it does not, the click
//! passes through from then on, as pressed at the window's end. A click released within the window
//! passes through as a tap.

use crate::controllers::{bindings::controls::ButtonControl, frame::Hand};

/// The stick clicks after the chord's arbitration.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Arbitrated {
    /// Each hand's stick click as the tables see it, by [`Hand::index`].
    pub clicks: [ButtonControl; 2],
    /// The chord was held for the recenter hold time this tick.
    pub recenter: bool,
}

/// The chord's arbitration state.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct StickChord {
    phase: Phase,
}

impl StickChord {
    pub const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// Arbitrate one tick of the raw stick `clicks` (by [`Hand::index`]). `window_s` is how long a
    /// lone click waits for the other, and `hold_s` how long the chord must be held to recenter,
    /// both seconds. The recenter fires once per chord.
    pub fn step(
        &mut self,
        clicks: [ButtonControl; 2],
        dt: f32,
        window_s: f32,
        hold_s: f32,
    ) -> Arbitrated {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let held = clicks.map(ButtonControl::held);
        let mut out = Arbitrated::default();
        self.phase = match self.phase {
            Phase::Idle | Phase::Pending { .. } if held == [true, true] => {
                out.recenter = hold_s <= 0.0;
                Phase::Chord {
                    held_s: 0.0,
                    fired: out.recenter,
                }
            }
            Phase::Idle => match Hand::BOTH.into_iter().find(|h| held[h.index()]) {
                Some(hand) => pending(&mut out, clicks, hand, 0.0, window_s),
                None => Phase::Idle,
            },
            Phase::Pending { hand, held_s } => {
                pending(&mut out, clicks, hand, held_s + dt, window_s)
            }
            Phase::Single => {
                out.clicks = clicks;
                if held == [false, false] {
                    Phase::Idle
                } else {
                    Phase::Single
                }
            }
            Phase::Chord { held_s, fired } if held == [true, true] => {
                let held_s = held_s + dt;
                out.recenter = !fired && held_s >= hold_s;
                Phase::Chord {
                    held_s,
                    fired: fired || out.recenter,
                }
            }
            Phase::Chord { .. } | Phase::Release => {
                if held == [false, false] {
                    Phase::Idle
                } else {
                    Phase::Release
                }
            }
        };
        out
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum Phase {
    /// Neither click is held.
    #[default]
    Idle,
    /// One click is held and withheld, waiting the window for the other.
    Pending { hand: Hand, held_s: f32 },
    /// A lone click outlasted the window: both clicks pass through until both are released.
    Single,
    /// Both clicks are held and withheld.
    Chord { held_s: f32, fired: bool },
    /// The chord was let go of: both clicks are withheld until both are released.
    Release,
}

/// A lone click on `hand`, held for `held_s`: a tap once released within the window, a press once
/// the window has passed, and withheld meanwhile.
fn pending(
    out: &mut Arbitrated,
    clicks: [ButtonControl; 2],
    hand: Hand,
    held_s: f32,
    window_s: f32,
) -> Phase {
    let click = &mut out.clicks[hand.index()];
    if !clicks[hand.index()].down {
        *click = ButtonControl {
            down: false,
            pressed: true,
            released: true,
        };
        Phase::Idle
    } else if held_s >= window_s {
        *click = ButtonControl {
            down: true,
            pressed: true,
            released: false,
        };
        Phase::Single
    } else {
        Phase::Pending { hand, held_s }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exact in binary, so that the hold lands on a whole tick.
    const DT: f32 = 0.0625;
    const WINDOW: f32 = 0.1;
    const HOLD: f32 = 1.0;

    const UP: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: false,
    };
    const PRESS: ButtonControl = ButtonControl {
        down: true,
        pressed: true,
        released: false,
    };
    const HELD: ButtonControl = ButtonControl {
        down: true,
        pressed: false,
        released: false,
    };
    const RELEASE: ButtonControl = ButtonControl {
        down: false,
        pressed: false,
        released: true,
    };

    fn run(chord: &mut StickChord, ticks: &[[ButtonControl; 2]]) -> Vec<Arbitrated> {
        ticks
            .iter()
            .map(|&clicks| chord.step(clicks, DT, WINDOW, HOLD))
            .collect()
    }

    #[test]
    fn a_lone_click_passes_after_the_window() {
        let mut chord = StickChord::new();
        let out = run(
            &mut chord,
            &[
                [PRESS, UP],
                [HELD, UP],
                [HELD, UP],
                [HELD, UP],
                [RELEASE, UP],
            ],
        );
        let left: Vec<_> = out.iter().map(|o| o.clicks[0].held()).collect();
        assert_eq!(left, [false, false, true, true, false]);
        assert!(out[2].clicks[0].pressed);
        assert!(out.iter().all(|o| !o.recenter && !o.clicks[1].held()));
    }

    #[test]
    fn a_click_released_within_the_window_is_a_tap() {
        let mut chord = StickChord::new();
        let out = run(&mut chord, &[[UP, PRESS], [UP, RELEASE], [UP, UP]]);
        assert!(!out[0].clicks[1].held());
        assert!(out[1].clicks[1].held() && !out[1].clicks[1].down);
        assert!(!out[2].clicks[1].held());
    }

    #[test]
    fn a_held_chord_recenters_once_and_leaks_no_click() {
        let mut chord = StickChord::new();
        let mut ticks = vec![[PRESS, UP], [HELD, PRESS]];
        ticks.extend(std::iter::repeat_n([HELD, HELD], 40));
        ticks.extend([[RELEASE, HELD], [UP, HELD], [UP, RELEASE], [UP, UP]]);
        let out = run(&mut chord, &ticks);
        assert!(out.iter().all(|o| o.clicks.iter().all(|c| !c.held())));
        let fired: Vec<_> = (0..out.len()).filter(|&i| out[i].recenter).collect();
        assert_eq!(fired.len(), 1);
        // The chord began on the second tick; the hold counts from there.
        assert_eq!(fired[0], 1 + (HOLD / DT).round() as usize);

        // Released, it can fire again.
        let again = run(&mut chord, &[[HELD, HELD]; 30]);
        assert_eq!(again.iter().filter(|o| o.recenter).count(), 1);
    }

    #[test]
    fn a_short_chord_does_nothing() {
        let mut chord = StickChord::new();
        let out = run(
            &mut chord,
            &[
                [PRESS, PRESS],
                [HELD, HELD],
                [RELEASE, HELD],
                [UP, HELD],
                [UP, RELEASE],
            ],
        );
        assert!(out.iter().all(|o| !o.recenter));
        assert!(out.iter().all(|o| o.clicks.iter().all(|c| !c.held())));
    }

    #[test]
    fn a_click_after_the_window_is_not_a_chord() {
        let mut chord = StickChord::new();
        let mut ticks = vec![[PRESS, UP], [HELD, UP], [HELD, UP], [HELD, PRESS]];
        ticks.extend(std::iter::repeat_n([HELD, HELD], 40));
        let out = run(&mut chord, &ticks);
        assert!(out.iter().all(|o| !o.recenter));
        assert!(out[3].clicks[1].held());
    }
}
