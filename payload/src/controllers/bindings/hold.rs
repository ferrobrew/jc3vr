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
//!
//! The tracker also remembers every effector it wrote on the last tick, so that a write never
//! outlives the bindings' intent. A bare click the bindings no longer want is cleared on the next
//! tick when the device poll did not move it on, since an effector left clicked reads as a fresh
//! click on every frame. When injection stops ([`HoldTracker::release_all`]) every write is undone
//! explicitly: while the poll runs, each hold is released with its time and each click cleared as
//! above; while it is disabled, every written effector is returned to idle, because nothing else
//! will.

use std::collections::{BTreeMap, BTreeSet};

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

/// Whether the game's device poll runs on an input tick.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    /// The poll ran before the bindings: it overwrote every bound effector with its device's
    /// reading and moved its state on.
    Running,
    /// The device manager is disabled, so the poll leaves every effector as it is: a written value
    /// and state persist until they are written again.
    Disabled,
}

/// The identity of the effector a write landed on: its address, compared and never dereferenced.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EffectorId(usize);

impl EffectorId {
    pub const fn from_address(address: usize) -> Self {
        Self(address)
    }
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
    /// Undo a bare click written on the last tick: when the effector it landed on is still
    /// clicked, the device poll did not move it on, and it is released with no hold time.
    Unclick { effector: EffectorId },
    /// Return the effector to idle with no value, whatever it shows. Written only while the device
    /// poll is disabled, when every pressed state is a stale write rather than a device's reading.
    Neutral,
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

/// The hold timers of every action the bindings hold, and the other effectors they wrote on the
/// last tick.
#[derive(Debug, Clone, Default)]
pub struct HoldTracker {
    /// The actions held as of the last tick, and for how long.
    held: BTreeMap<Target, f32>,
    /// The bare clicks written on the last tick, with the effector each landed on once
    /// [`landed`](Self::landed) reports it.
    clicked: BTreeMap<Target, Option<EffectorId>>,
    /// The actions whose value alone was raised on the last tick.
    valued: BTreeSet<Target>,
}

impl HoldTracker {
    pub const fn new() -> Self {
        Self {
            held: BTreeMap::new(),
            clicked: BTreeMap::new(),
            valued: BTreeSet::new(),
        }
    }

    /// Resolve this tick's wanted inputs into writes, advancing the hold timers by `dt`. Several
    /// wants on one action merge: the largest value wins, and any hold holds it. The last tick's
    /// bare clicks that are not written again come first, as [`Phase::Unclick`].
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

        let previous_clicks = std::mem::take(&mut self.clicked);
        self.valued.clear();
        let mut writes = Vec::with_capacity(merged.len() + self.held.len() + previous_clicks.len());
        writes.extend(
            previous_clicks
                .into_iter()
                .filter(|(target, _)| !merged.get(target).is_some_and(Merged::writes_a_press))
                .filter_map(|(target, effector)| Some(unclick(target, effector?))),
        );
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
                    self.clicked.insert(target, None);
                    writes.push(ActionWrite {
                        target,
                        value: 1.0,
                        phase: Phase::Clicked,
                    });
                    continue;
                }
                (false, None) if value > 0.0 => {
                    self.valued.insert(target);
                    Phase::Value
                }
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

    /// Record the effector each of this tick's bare clicks landed on, `landings` running parallel
    /// to the `writes` [`resolve`](Self::resolve) returned. A click that landed nowhere is not
    /// undone later.
    pub fn landed(&mut self, writes: &[ActionWrite], landings: &[Option<EffectorId>]) {
        for (write, &landing) in writes.iter().zip(landings) {
            if write.phase == Phase::Clicked
                && let Some(effector) = self.clicked.get_mut(&write.target)
            {
                *effector = landing;
            }
        }
    }

    /// Stop injecting while keeping the hold timers, for a tick on which the bindings are paused
    /// rather than stood down: undo the last tick's bare clicks and forget its bare values. Called
    /// only while the device poll runs, which releases the held effectors and overwrites the
    /// values; the holds resume with their times when injection does.
    pub fn pause(&mut self) -> Vec<ActionWrite> {
        self.valued.clear();
        std::mem::take(&mut self.clicked)
            .into_iter()
            .filter_map(|(target, effector)| Some(unclick(target, effector?)))
            .collect()
    }

    /// Stop injecting: forget everything, returning the writes that undo what the last tick wrote.
    /// While the poll runs, each hold is released with its time (a physical press in the same
    /// effector is kept) and each bare click undone as [`Phase::Unclick`]; the poll has already
    /// overwritten the bare values. While the poll is disabled nothing moves an effector on, so
    /// every written effector is returned to idle as [`Phase::Neutral`].
    pub fn release_all(&mut self, poll: DevicePoll) -> Vec<ActionWrite> {
        let held = std::mem::take(&mut self.held);
        let clicked = std::mem::take(&mut self.clicked);
        let valued = std::mem::take(&mut self.valued);
        match poll {
            DevicePoll::Running => held
                .into_iter()
                .map(|(target, seconds)| ActionWrite {
                    target,
                    value: 0.0,
                    phase: Phase::Released { seconds },
                })
                .chain(
                    clicked
                        .into_iter()
                        .filter_map(|(target, effector)| Some(unclick(target, effector?))),
                )
                .collect(),
            DevicePoll::Disabled => held
                .into_keys()
                .chain(clicked.into_keys())
                .chain(valued)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|target| ActionWrite {
                    target,
                    value: 0.0,
                    phase: Phase::Neutral,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Default)]
struct Merged {
    hold: Option<f32>,
    analog: f32,
    click: bool,
}

impl Merged {
    /// Whether the merged want writes a pressed state this tick, overwriting a click left from the
    /// last tick.
    fn writes_a_press(&self) -> bool {
        self.click || self.hold.is_some() || sanitize(self.analog) > ANALOG_PRESS
    }
}

fn unclick(target: Target, effector: EffectorId) -> ActionWrite {
    ActionWrite {
        target,
        value: 0.0,
        phase: Phase::Unclick { effector },
    }
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

    fn pause_click() -> Desired {
        Desired {
            target: Target {
                map: MapKind::PlayerThroughBlockers,
                action: Action::GUI_PAUSE,
            },
            want: Want::Click,
        }
    }

    const EFFECTOR: EffectorId = EffectorId::from_address(0x1000);

    #[test]
    fn a_click_is_undone_on_the_next_tick() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[pause_click()], DT);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].phase, Phase::Clicked);
        assert_eq!(w[0].value, 1.0);
        t.landed(&w, &[Some(EFFECTOR)]);
        let w = t.resolve(&[], DT);
        assert_eq!(
            phases(&w),
            vec![(Action::GUI_PAUSE, Phase::Unclick { effector: EFFECTOR })]
        );
        assert_eq!(w[0].value, 0.0);
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn a_repeated_click_is_not_undone() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[pause_click()], DT);
        t.landed(&w, &[Some(EFFECTOR)]);
        let w = t.resolve(&[pause_click()], DT);
        assert_eq!(phases(&w), vec![(Action::GUI_PAUSE, Phase::Clicked)]);
    }

    #[test]
    fn a_click_that_landed_nowhere_is_not_undone() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[pause_click()], DT);
        t.landed(&w, &[None]);
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn landings_are_recorded_only_for_bare_clicks() {
        let mut t = HoldTracker::default();
        // The first tick of a hold is also a click, but the hold's own release undoes it.
        let w = t.resolve(&[hold(Action::RELOAD), pause_click()], DT);
        t.landed(&w, &[Some(EFFECTOR); 2]);
        let w = t.resolve(&[], DT);
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(
            |w| w.target.action == Action::RELOAD && matches!(w.phase, Phase::Released { .. })
        ));
        assert!(w.iter().any(|w| w.target.action == Action::GUI_PAUSE
            && w.phase == Phase::Unclick { effector: EFFECTOR }));
    }

    #[test]
    fn releasing_while_the_poll_runs_releases_holds_and_undoes_clicks() {
        let mut t = HoldTracker::default();
        t.resolve(&[hold(Action::RELOAD)], DT);
        let w = t.resolve(
            &[
                hold(Action::RELOAD),
                pause_click(),
                analog(Action::MOVE_FORWARD, 0.1),
            ],
            DT,
        );
        let landings: Vec<_> = w.iter().map(|_| Some(EFFECTOR)).collect();
        t.landed(&w, &landings);
        let w = t.release_all(DevicePoll::Running);
        assert_eq!(
            phases(&w),
            vec![
                (Action::RELOAD, Phase::Released { seconds: DT }),
                (Action::GUI_PAUSE, Phase::Unclick { effector: EFFECTOR }),
            ]
        );
        assert!(w.iter().all(|w| w.value == 0.0));
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn releasing_while_the_poll_is_disabled_neutralises_every_write() {
        let mut t = HoldTracker::default();
        let w = t.resolve(
            &[
                hold(Action::FIRE_GRAPPLE),
                analog(Action::MOVE_FORWARD, 0.7),
                analog(Action::MOVE_LEFT, 0.1),
                pause_click(),
                Desired {
                    target: Target::ui(Action::GUI_PAUSE),
                    want: Want::Click,
                },
            ],
            DT,
        );
        // A click that landed nowhere is neutralised too: the poll does not move anything on.
        t.landed(&w, &[None; 5]);
        let w = t.release_all(DevicePoll::Disabled);
        let mut targets: Vec<_> = w.iter().map(|w| (w.target.map, w.target.action)).collect();
        targets.sort();
        assert_eq!(
            targets,
            vec![
                (MapKind::Player, Action::MOVE_FORWARD),
                (MapKind::Player, Action::MOVE_LEFT),
                (MapKind::Player, Action::FIRE_GRAPPLE),
                (MapKind::PlayerThroughBlockers, Action::GUI_PAUSE),
                (MapKind::Ui, Action::GUI_PAUSE),
            ]
        );
        assert!(
            w.iter()
                .all(|w| w.phase == Phase::Neutral && w.value == 0.0)
        );
        assert!(t.release_all(DevicePoll::Disabled).is_empty());
        assert!(t.resolve(&[], DT).is_empty());
    }

    #[test]
    fn a_release_after_a_finished_hold_writes_nothing() {
        let mut t = HoldTracker::default();
        t.resolve(&[hold(Action::RELOAD)], DT);
        t.resolve(&[], DT);
        assert!(t.release_all(DevicePoll::Running).is_empty());
        assert!(t.release_all(DevicePoll::Disabled).is_empty());
    }

    #[test]
    fn pausing_undoes_clicks_and_keeps_holds() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[hold(Action::RELOAD), pause_click()], DT);
        t.landed(&w, &[Some(EFFECTOR); 2]);
        let w = t.pause();
        assert_eq!(
            phases(&w),
            vec![(Action::GUI_PAUSE, Phase::Unclick { effector: EFFECTOR })]
        );
        let w = t.resolve(&[hold(Action::RELOAD)], DT);
        assert_eq!(
            phases(&w),
            vec![(Action::RELOAD, Phase::Held { seconds: DT })]
        );
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
    fn bad_values_are_sanitized() {
        let mut t = HoldTracker::default();
        let w = t.resolve(&[analog(Action::TURN_LEFT, f32::NAN)], f32::NAN);
        assert!(w.is_empty());
        let w = t.resolve(&[analog(Action::TURN_LEFT, 3.0)], DT);
        assert_eq!(w[0].value, 1.0);
    }
}
