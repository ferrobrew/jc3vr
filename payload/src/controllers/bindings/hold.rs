//! Turning the bindings' wanted inputs into effector writes with the game's hold timing. Pure.
//!
//! The game's readers tell a tap from a hold by the effector's state and its state time (the grapple
//! tethers on a held `FIRE_GRAPPLE`, nitrous and the turbo jump split `USE_VEHICLE_MOD` by hold
//! time, the holster is a held `RELOAD`). `ForceSetPressed` zeroes the state time on every call,
//! so a press repeated every tick never grows a hold time. The tracker instead keeps the hold time
//! itself and writes the whole state (see the effector's documentation in the pyxis defs): the
//! first tick of a press writes a click with no time, every held tick writes a press with the time
//! held so far, and the tick after the press ends writes a release with the final time, which the
//! next device poll turns idle on its own.

use std::collections::BTreeMap;

use jc3gi::input::input_action_map::Action;

/// The action map a write goes to.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKind {
    /// The local player's action map, through the lookup that honours its blockers (a UI screen
    /// with focus blocks it).
    Player,
    /// The local player's action map, through the lookup that ignores the blockers: the pause and
    /// commlink toggles, which the game itself reads with the blockers overridden.
    PlayerThroughBlockers,
    /// The UI action map the game's menus read.
    Ui,
}

/// An action on a map.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub map: MapKind,
    pub action: Action,
}

#[cfg(test)]
impl Target {
    pub const fn player(action: Action) -> Self {
        Self {
            map: MapKind::Player,
            action,
        }
    }

    pub const fn ui(action: Action) -> Self {
        Self {
            map: MapKind::Ui,
            action,
        }
    }
}

/// What a binding wants from an action this tick.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Want {
    /// Held down with this value, `0..=1`.
    Hold(f32),
    /// An analog half-axis with this value, `0..=1`: pressed above [`ANALOG_PRESS`], a bare value
    /// below it.
    Analog(f32),
    /// A one-tick click with no hold.
    Click,
}

/// A wanted input.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Desired {
    pub target: Target,
    pub want: Want,
}

/// The state an effector write sets.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Phase {
    /// Raise the value only, leaving the state to the device poll.
    Value,
    /// The press edge: the clicked state with no hold time.
    Clicked,
    /// Held, for this many seconds since the press edge.
    Held { seconds: f32 },
    /// Released after being held this many seconds.
    Released { seconds: f32 },
}

/// One effector write.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ActionWrite {
    pub target: Target,
    /// The value to raise the effector to (never lowered: a physical device's reading wins).
    pub value: f32,
    pub phase: Phase,
}

/// The analog value above which an analog action also reads as pressed. The game's devices press
/// at their own button threshold, which is not read; this stands in for it.
pub const ANALOG_PRESS: f32 = 0.2;

/// The hold timers of every action the bindings hold.
#[derive(Debug, Clone, Default)]
pub struct HoldTracker {
    /// The actions held as of the last tick, and for how long.
    held: BTreeMap<Target, f32>,
}

impl HoldTracker {
    pub const fn new() -> Self {
        Self {
            held: BTreeMap::new(),
        }
    }

    /// Resolve this tick's wanted inputs into writes, advancing the hold timers by `dt`. Several
    /// wants on one action merge: the largest value wins, and any hold holds it.
    pub fn resolve(&mut self, desired: &[Desired], dt: f32) -> Vec<ActionWrite> {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let mut merged: BTreeMap<Target, Merged> = BTreeMap::new();
        for d in desired {
            let m = merged.entry(d.target).or_default();
            match d.want {
                Want::Hold(v) => m.hold = Some(m.hold.map_or(v, |h| h.max(v))),
                Want::Analog(v) => m.analog = m.analog.max(v),
                Want::Click => m.click = true,
            }
        }

        let mut writes = Vec::with_capacity(merged.len() + self.held.len());
        for (&target, m) in &merged {
            let analog = sanitize(m.analog);
            let pressed = m.hold.is_some() || analog > ANALOG_PRESS;
            let value = m.hold.map_or(analog, |h| sanitize(h).max(analog));
            let previous = self.held.get(&target).copied();
            let phase = match (pressed, previous) {
                (true, Some(seconds)) => {
                    let seconds = seconds + dt;
                    self.held.insert(target, seconds);
                    Phase::Held { seconds }
                }
                (true, None) => {
                    self.held.insert(target, 0.0);
                    Phase::Clicked
                }
                (false, Some(seconds)) => {
                    self.held.remove(&target);
                    Phase::Released {
                        seconds: seconds + dt,
                    }
                }
                (false, None) if m.click => {
                    writes.push(ActionWrite {
                        target,
                        value: 1.0,
                        phase: Phase::Clicked,
                    });
                    continue;
                }
                (false, None) if value > 0.0 => Phase::Value,
                (false, None) => continue,
            };
            let value = if matches!(phase, Phase::Released { .. }) {
                0.0
            } else {
                value
            };
            writes.push(ActionWrite {
                target,
                value,
                phase,
            });
        }

        let ended: Vec<Target> = self
            .held
            .keys()
            .filter(|t| !merged.contains_key(t))
            .copied()
            .collect();
        for target in ended {
            if let Some(seconds) = self.held.remove(&target) {
                writes.push(ActionWrite {
                    target,
                    value: 0.0,
                    phase: Phase::Released {
                        seconds: seconds + dt,
                    },
                });
            }
        }
        writes
    }

    /// Forget every hold without writing its release. Used when injection stops: the device poll
    /// releases each held effector on its own, keeping the last hold time written.
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[derive(Debug, Default)]
struct Merged {
    hold: Option<f32>,
    analog: f32,
    click: bool,
}

fn sanitize(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn hold(action: Action) -> Desired {
        Desired {
            target: Target::player(action),
            want: Want::Hold(1.0),
        }
    }

    fn analog(action: Action, value: f32) -> Desired {
        Desired {
            target: Target::player(action),
            want: Want::Analog(value),
        }
    }

    fn phases(writes: &[ActionWrite]) -> Vec<(Action, Phase)> {
        writes.iter().map(|w| (w.target.action, w.phase)).collect()
    }

    #[test]
    fn a_hold_clicks_then_accumulates_then_releases() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[hold(Action::FIRE_GRAPPLE)], DT);
        assert_eq!(phases(&w), vec![(Action::FIRE_GRAPPLE, Phase::Clicked)]);
        let w = t.resolve(&[hold(Action::FIRE_GRAPPLE)], DT);
        assert_eq!(
            phases(&w),
            vec![(Action::FIRE_GRAPPLE, Phase::Held { seconds: DT })]
        );
        let w = t.resolve(&[hold(Action::FIRE_GRAPPLE)], DT);
        assert_eq!(
            phases(&w),
            vec![(Action::FIRE_GRAPPLE, Phase::Held { seconds: 2.0 * DT })]
        );
        let w = t.resolve(&[], DT);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].value, 0.0);
        assert!(matches!(
            w[0].phase,
            Phase::Released { seconds } if (seconds - 3.0 * DT).abs() < 1e-6
        ));
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn a_one_tick_tap_releases_with_a_short_time() {
        let mut t = HoldTracker::default();
        t.resolve(&[hold(Action::USE_VEHICLE_MOD)], DT);
        let w = t.resolve(&[], DT);
        assert!(matches!(w[0].phase, Phase::Released { seconds } if seconds <= DT + 1e-6));
    }

    #[test]
    fn wants_on_one_action_merge() {
        let mut t = HoldTracker::default();
        let desired = [
            analog(Action::MOVE_FORWARD, 0.4),
            analog(Action::MOVE_FORWARD, 0.7),
            Desired {
                target: Target::player(Action::CANCEL),
                want: Want::Hold(0.5),
            },
            hold(Action::CANCEL),
        ];
        let w = t.resolve(&desired, DT);
        let forward = w.iter().find(|w| w.target.action == Action::MOVE_FORWARD);
        assert_eq!(forward.map(|w| w.value), Some(0.7));
        let cancel: Vec<_> = w
            .iter()
            .filter(|w| w.target.action == Action::CANCEL)
            .collect();
        assert_eq!(cancel.len(), 1);
        assert_eq!(cancel[0].value, 1.0);
    }

    #[test]
    fn analog_presses_only_above_the_threshold() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[analog(Action::ACCELERATE, 0.1)], DT);
        assert_eq!(phases(&w), vec![(Action::ACCELERATE, Phase::Value)]);
        let w = t.resolve(&[analog(Action::ACCELERATE, 0.5)], DT);
        assert_eq!(phases(&w), vec![(Action::ACCELERATE, Phase::Clicked)]);
        let w = t.resolve(&[analog(Action::ACCELERATE, 0.1)], DT);
        assert!(matches!(w[0].phase, Phase::Released { .. }));
        // A zero analog value writes nothing at all.
        assert!(t.resolve(&[analog(Action::ACCELERATE, 0.0)], DT).is_empty());
    }

    #[test]
    fn clicks_are_not_tracked() {
        let mut t = HoldTracker::default();
        let click = Desired {
            target: Target {
                map: MapKind::PlayerThroughBlockers,
                action: Action::GUI_PAUSE,
            },
            want: Want::Click,
        };
        let w = t.resolve(&[click], DT);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].phase, Phase::Clicked);
        assert_eq!(w[0].value, 1.0);
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn the_same_action_on_two_maps_is_two_targets() {
        let mut t = HoldTracker::default();
        let desired = [
            hold(Action::GUI_OK),
            Desired {
                target: Target::ui(Action::GUI_OK),
                want: Want::Hold(1.0),
            },
        ];
        assert_eq!(t.resolve(&desired, DT).len(), 2);
    }

    #[test]
    fn clearing_forgets_without_writing() {
        let mut t = HoldTracker::default();
        t.resolve(&[hold(Action::RELOAD)], DT);
        t.clear();
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn bad_values_are_sanitized() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[analog(Action::TURN_LEFT, f32::NAN)], f32::NAN);
        assert!(w.is_empty());
        let w = t.resolve(&[analog(Action::TURN_LEFT, 3.0)], DT);
        assert_eq!(w[0].value, 1.0);
    }
}
