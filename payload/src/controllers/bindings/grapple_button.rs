//! The grapple button's gestures. Pure.
//!
//! The grapple is on the off-hand's upper face button (B on the Index controller, Y on the left
//! Touch controller). With nothing attached, the button is the gamepad's grapple button and the
//! game reads it as it does that one: a tap fires the grapple (a zip, or a grab of what it hits),
//! and a hold on a target, then aiming at a second target and letting go, tethers the two. With
//! tethers or a line attached, the button does more, and the gesture is read before the game sees
//! it:
//!
//! - a tap fires the grapple, once a second tap has not followed within [`DOUBLE_TAP_S`];
//! - a double tap releases every tether, and fires nothing: on foot and in the wingsuit it is the
//!   gamepad's B, which also cancels and steps off a ledge, since the grapple took that button;
//! - a hold with a grapple target under the aim is the game's hold, a new tether;
//! - a hold with nothing new under the aim tightens the tethers, the retract rising from
//!   [`TIGHTEN_START`] to [`TIGHTEN_HELD`] over [`TIGHTEN_RAMP_S`]; pulling the hand back while
//!   holding tightens harder, by the physical grapple's pull ([`crate::grapple::physical`]),
//!   which the bindings merge with this retract, the larger winning;
//! - a sharp pull of the hand while the button is held, with a line attached, zips along the
//!   line, as a tap would ([`crate::controllers::bindings::yank`]).
//!
//! The pulls act only while the button is held, so a pull of the hand never zips or tightens by
//! itself.

use jc3gi::input::input_action_map::Action;

use crate::controllers::bindings::{
    controls::ButtonControl,
    hold::{Desired, MapKind, Target, Want},
};

/// What the grapple's gestures need to know of the game.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct GrappleContext {
    /// Dual tethers are attached.
    pub tethers: bool,
    /// The grapple holds a line of its own to something, not carrying the character.
    pub line: bool,
    /// The game holds a grapple target in range under the grapple's aim.
    pub aim_target: bool,
}

impl GrappleContext {
    /// Whether the button's presses are read as gestures before the game sees them.
    const fn gestures(self) -> bool {
        self.tethers || self.line
    }
}

/// One tick of the button, for [`GrappleButton::step`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GrappleTick {
    pub button: ButtonControl,
    pub context: GrappleContext,
    /// The yank gesture fired this tick: a sharp pull of the grapple hand.
    pub yank: bool,
    pub dt: f32,
}

/// What the button asks of the game this tick, when its gestures are read.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GrappleOutput {
    /// The button's own presses are replaced by these, rather than passed through.
    pub gestures: bool,
    /// The grapple's fire input.
    pub fire: Option<Want>,
    /// Release every tether.
    pub release: bool,
    /// The tethers' retract, `0..=1`.
    pub tighten: Option<f32>,
}

/// The gesture under way.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
enum Phase {
    /// The button is up, with no tap waiting.
    #[default]
    Idle,
    /// The button is up after a tap, which fires once no second tap follows: `waited_s` since it.
    TapWaiting { waited_s: f32 },
    /// The button is down, for `held_s`; `second` when a tap was waiting at the press.
    Pressed { held_s: f32, second: bool },
    /// The button is held as the game's own hold: a new tether.
    Firing,
    /// The button is held with nothing new under the aim: the tethers tighten, `held_s` into it.
    Tightening { held_s: f32 },
    /// The press is spent (a zip or a release); nothing more until the button is up.
    Spent,
    /// The button is passed through to the game, nothing being attached at its press.
    Passing,
}

/// The button's gesture state across ticks.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct GrappleButton {
    phase: Phase,
}

/// How long a press may be held and still be a tap, seconds.
pub const TAP_S: f32 = 0.25;
/// How soon a second tap must follow a first for a double tap, seconds.
pub const DOUBLE_TAP_S: f32 = 0.3;
/// The retract a tightening hold starts at.
pub const TIGHTEN_START: f32 = 0.35;
/// The retract a tightening hold rises to on its own; the hand's pull tightens past it.
pub const TIGHTEN_HELD: f32 = 0.7;
/// How long a tightening hold takes to rise to [`TIGHTEN_HELD`], seconds.
pub const TIGHTEN_RAMP_S: f32 = 1.0;

impl GrappleButton {
    pub const fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// Advance one tick.
    pub fn step(&mut self, tick: GrappleTick) -> GrappleOutput {
        let GrappleTick {
            button,
            context,
            yank,
            dt,
        } = tick;
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let held = button.held();
        let fresh = button.pressed;
        let mut out = GrappleOutput {
            gestures: true,
            ..GrappleOutput::default()
        };
        self.phase = match self.phase {
            Phase::Idle | Phase::TapWaiting { .. } if fresh && !context.gestures() => {
                // A tap still waiting fires first, and the new press reaches the game from the
                // next tick.
                if matches!(self.phase, Phase::TapWaiting { .. }) {
                    out.fire = Some(Want::Click);
                } else {
                    out.gestures = false;
                }
                Phase::Passing
            }
            Phase::Idle if fresh => Phase::Pressed {
                held_s: 0.0,
                second: false,
            },
            Phase::Idle => {
                out.gestures = context.gestures();
                Phase::Idle
            }
            Phase::TapWaiting { .. } if fresh => Phase::Pressed {
                held_s: 0.0,
                second: true,
            },
            Phase::TapWaiting { waited_s } => {
                let waited_s = waited_s + dt;
                if waited_s >= DOUBLE_TAP_S {
                    out.fire = Some(Want::Click);
                    Phase::Idle
                } else {
                    Phase::TapWaiting { waited_s }
                }
            }
            Phase::Passing if held => {
                out.gestures = false;
                Phase::Passing
            }
            Phase::Passing => {
                out.gestures = false;
                Phase::Idle
            }
            Phase::Pressed { .. } if yank && context.line => {
                out.fire = Some(Want::Click);
                Phase::Spent
            }
            Phase::Pressed { second, .. } if !held || (button.released && !button.down) => {
                if second {
                    if context.tethers {
                        out.release = true;
                    }
                    Phase::Idle
                } else if context.tethers {
                    Phase::TapWaiting { waited_s: 0.0 }
                } else {
                    // A line with no tethers has nothing a double tap releases: the tap fires.
                    out.fire = Some(Want::Click);
                    Phase::Idle
                }
            }
            Phase::Pressed { held_s, second } => {
                let held_s = held_s + dt;
                if held_s < TAP_S {
                    Phase::Pressed { held_s, second }
                } else if context.aim_target || !context.tethers {
                    out.fire = Some(Want::Hold(1.0));
                    Phase::Firing
                } else {
                    out.tighten = Some(TIGHTEN_START);
                    Phase::Tightening { held_s: 0.0 }
                }
            }
            Phase::Firing if held => {
                out.fire = Some(Want::Hold(1.0));
                Phase::Firing
            }
            Phase::Tightening { held_s } if held => {
                let held_s = held_s + dt;
                out.tighten = Some(tighten_at(held_s));
                Phase::Tightening { held_s }
            }
            Phase::Spent if held => Phase::Spent,
            Phase::Firing | Phase::Tightening { .. } | Phase::Spent => Phase::Idle,
        };
        out
    }

    /// Whether the button is held into a gesture.
    #[cfg(test)]
    fn holding(&self) -> bool {
        matches!(
            self.phase,
            Phase::Pressed { .. }
                | Phase::Firing
                | Phase::Tightening { .. }
                | Phase::Spent
                | Phase::Passing
        )
    }

    /// Apply this tick's gestures to the tables' `desired`, whose grapple press drives
    /// `actions.fire` and whose release clicks `actions.release`: with the gestures read, the
    /// press's own writes are replaced by theirs.
    pub fn apply(
        &mut self,
        tick: GrappleTick,
        actions: GrappleActions,
        desired: &mut Vec<Desired>,
    ) -> GrappleOutput {
        let fire_actions = actions.fire;
        let out = self.step(tick);
        if !out.gestures {
            return out;
        }
        let player = |action| Target {
            map: MapKind::Player,
            action,
        };
        desired.retain(|d| {
            !(d.target.map == MapKind::Player && fire_actions.contains(&d.target.action))
        });
        if let Some(want) = out.fire {
            desired.extend(fire_actions.iter().map(|&action| Desired {
                target: player(action),
                want,
            }));
        }
        if out.release {
            desired.extend(actions.release.iter().map(|&action| Desired {
                target: player(action),
                want: Want::Click,
            }));
        }
        if let Some(value) = out.tighten {
            desired.push(Desired {
                target: player(Action::RETRACT_GRAPPLE),
                want: Want::Analog(value),
            });
        }
        out
    }
}

/// The game actions the grapple's gestures drive in a mode.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct GrappleActions {
    /// What the grapple's press drives.
    pub fire: &'static [Action],
    /// What a double tap clicks to release the tethers.
    pub release: &'static [Action],
}

/// The retract `held_s` into a tightening hold.
fn tighten_at(held_s: f32) -> f32 {
    let t = (held_s / TIGHTEN_RAMP_S.max(1e-3)).clamp(0.0, 1.0);
    TIGHTEN_START + (TIGHTEN_HELD - TIGHTEN_START) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;
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
    const TETHERS: GrappleContext = GrappleContext {
        tethers: true,
        line: false,
        aim_target: false,
    };

    fn tick(button: ButtonControl, context: GrappleContext) -> GrappleTick {
        GrappleTick {
            button,
            context,
            yank: false,
            dt: DT,
        }
    }

    /// Run `buttons`, one a tick, and return each tick's output.
    fn run(
        g: &mut GrappleButton,
        context: GrappleContext,
        buttons: &[ButtonControl],
    ) -> Vec<GrappleOutput> {
        buttons.iter().map(|&b| g.step(tick(b, context))).collect()
    }

    fn fires(out: &[GrappleOutput]) -> Vec<Want> {
        out.iter().filter_map(|o| o.fire).collect()
    }

    #[test]
    fn with_nothing_attached_the_button_passes_through() {
        let mut g = GrappleButton::new();
        let out = run(
            &mut g,
            GrappleContext::default(),
            &[PRESS, HELD, HELD, RELEASE, UP],
        );
        assert!(out.iter().all(|o| !o.gestures), "{out:?}");
        assert!(fires(&out).is_empty());
    }

    #[test]
    fn with_tethers_a_tap_fires_once_no_second_tap_follows() {
        let mut g = GrappleButton::new();
        let out = run(&mut g, TETHERS, &[PRESS, RELEASE]);
        assert!(fires(&out).is_empty(), "it waits for a second tap");
        let wait = (DOUBLE_TAP_S / DT).ceil() as usize + 1;
        let out = run(&mut g, TETHERS, &vec![UP; wait]);
        assert_eq!(fires(&out), vec![Want::Click]);
        assert!(out.iter().all(|o| !o.release));
    }

    #[test]
    fn a_double_tap_releases_every_tether_and_fires_nothing() {
        let mut g = GrappleButton::new();
        let out = run(&mut g, TETHERS, &[PRESS, RELEASE, UP, PRESS, RELEASE, UP]);
        assert_eq!(out.iter().filter(|o| o.release).count(), 1);
        let wait = (DOUBLE_TAP_S / DT).ceil() as usize + 1;
        let later = run(&mut g, TETHERS, &vec![UP; wait]);
        assert!(fires(&out).is_empty() && fires(&later).is_empty());
    }

    #[test]
    fn a_hold_on_a_target_is_the_games_tether_hold() {
        let mut g = GrappleButton::new();
        let aimed = GrappleContext {
            aim_target: true,
            ..TETHERS
        };
        let hold = (TAP_S / DT).ceil() as usize + 2;
        let mut buttons = vec![PRESS];
        buttons.extend(vec![HELD; hold]);
        let out = run(&mut g, aimed, &buttons);
        assert!(fires(&out).iter().all(|w| *w == Want::Hold(1.0)));
        assert!(!fires(&out).is_empty());
        assert!(out.iter().all(|o| o.tighten.is_none()));
        // Letting go ends the hold, which the game reads as the tether's second end.
        let out = run(&mut g, aimed, &[RELEASE, UP]);
        assert!(fires(&out).is_empty());
    }

    #[test]
    fn a_hold_with_nothing_new_under_the_aim_tightens_and_ramps() {
        let mut g = GrappleButton::new();
        let hold = (TAP_S / DT).ceil() as usize + (TIGHTEN_RAMP_S / DT).ceil() as usize + 4;
        let mut buttons = vec![PRESS];
        buttons.extend(vec![HELD; hold]);
        let out = run(&mut g, TETHERS, &buttons);
        let tighten: Vec<f32> = out.iter().filter_map(|o| o.tighten).collect();
        assert!(!tighten.is_empty());
        assert!((tighten[0] - TIGHTEN_START).abs() < 1e-6);
        assert!(tighten.windows(2).all(|w| w[1] >= w[0]), "it ramps up");
        assert!((tighten.last().unwrap() - TIGHTEN_HELD).abs() < 1e-6);
        assert!(fires(&out).is_empty(), "a tightening hold fires nothing");
        let out = run(&mut g, TETHERS, &[RELEASE]);
        assert!(out[0].tighten.is_none());
        assert!(!g.holding());
    }

    #[test]
    fn a_sharp_pull_while_held_zips_along_a_line() {
        let mut g = GrappleButton::new();
        let line = GrappleContext {
            line: true,
            ..GrappleContext::default()
        };
        run(&mut g, line, &[PRESS]);
        assert!(g.holding());
        let out = g.step(GrappleTick {
            yank: true,
            ..tick(HELD, line)
        });
        assert_eq!(out.fire, Some(Want::Click));
        // The press is spent: holding on does not fire again, and letting go fires nothing.
        let out = run(&mut g, line, &[HELD, HELD, RELEASE, UP]);
        assert!(fires(&out).is_empty());
    }

    #[test]
    fn a_tap_on_a_line_without_tethers_fires_at_once() {
        let mut g = GrappleButton::new();
        let line = GrappleContext {
            line: true,
            ..GrappleContext::default()
        };
        let out = run(&mut g, line, &[PRESS, RELEASE]);
        assert_eq!(fires(&out), vec![Want::Click]);
    }

    #[test]
    fn the_gestures_replace_the_presses_writes() {
        let mut g = GrappleButton::new();
        let fire = Desired {
            target: Target::player(Action::FIRE_GRAPPLE),
            want: Want::Hold(1.0),
        };
        let other = Desired {
            target: Target::player(Action::JUMP),
            want: Want::Hold(1.0),
        };
        let actions = GrappleActions {
            fire: &[Action::FIRE_GRAPPLE],
            release: &[Action::RELEASE_GRAPPLE, Action::CANCEL],
        };
        let mut desired = vec![fire, other];
        g.apply(tick(PRESS, TETHERS), actions, &mut desired);
        assert_eq!(
            desired,
            vec![other],
            "the press waits, so the game sees nothing yet"
        );
        // A double tap clicks the release's actions.
        let mut desired = Vec::new();
        for button in [RELEASE, UP, PRESS, RELEASE] {
            desired.clear();
            g.apply(tick(button, TETHERS), actions, &mut desired);
        }
        let click = |action| Desired {
            target: Target::player(action),
            want: Want::Click,
        };
        assert_eq!(
            desired,
            vec![click(Action::RELEASE_GRAPPLE), click(Action::CANCEL)]
        );
        // With nothing attached the press reaches the game untouched.
        let mut g = GrappleButton::new();
        let mut desired = vec![fire, other];
        g.apply(
            tick(PRESS, GrappleContext::default()),
            actions,
            &mut desired,
        );
        assert_eq!(desired, vec![fire, other]);
    }
}
