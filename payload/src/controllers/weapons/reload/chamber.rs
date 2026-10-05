//! The chamber and the action that loads it: the slide, the charging handle, the bolt, or the
//! pump. Pure.
//!
//! The game has no chambered round: a gun fires while its magazine holds rounds. The chamber is
//! the mod's: a gun that runs dry, and a bolt-action or pump-action gun after every shot (with
//! the manual action on), has an empty chamber, and the trigger only clicks until the action is
//! worked with rounds in the magazine. A tactical reload keeps the round in the chamber.
//!
//! Working the action ([`ActionWork`]) is a grab of its handle followed by a pull back along the
//! gun's axis to the pull share of its travel. A slide or a charging handle closes when it is let
//! go or pushed back home; a bolt or a pump has to be pushed back home. The pump is worked by the
//! hand on the foregrip, which holds it already, so its work starts from the hand's place when the
//! chamber empties rather than from a grab.

use glam::Vec3;

use crate::controllers::weapons::reload::config::ActionConfig;

/// The mod's chamber.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum Chamber {
    #[default]
    Chambered,
    Empty,
}

/// How a gun's action is worked.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ActionKind {
    /// A pistol's slide.
    Slide,
    /// A charging handle on the receiver.
    ChargingHandle,
    /// A bolt-action rifle's bolt.
    Bolt,
    /// A pump-action shotgun's pump, on the foregrip.
    Pump,
    /// Nothing to work: a revolver, a launcher, or a break-action.
    None,
}

impl ActionKind {
    pub const fn label(self) -> &'static str {
        match self {
            ActionKind::Slide => "slide",
            ActionKind::ChargingHandle => "charging handle",
            ActionKind::Bolt => "bolt",
            ActionKind::Pump => "pump",
            ActionKind::None => "none",
        }
    }

    /// Whether the action cycles by hand after every shot, with the manual action on.
    pub const fn manual(self) -> bool {
        matches!(self, ActionKind::Bolt | ActionKind::Pump)
    }

    /// Whether the action has to be pushed back home to close.
    const fn must_return(self) -> bool {
        matches!(self, ActionKind::Bolt | ActionKind::Pump)
    }
}

/// The chamber after a shot that left `magazine` rounds, for a gun worked by `action`.
pub fn after_shot(magazine: i32, action: ActionKind, manual_action: bool) -> Chamber {
    if action == ActionKind::None {
        return Chamber::Chambered;
    }
    if magazine <= 0 || (manual_action && action.manual()) {
        Chamber::Empty
    } else {
        Chamber::Chambered
    }
}

/// The chamber after the action is worked with `magazine` rounds in a magazine that is in.
pub fn after_work(magazine: i32, magazine_in: bool) -> Chamber {
    if magazine_in && magazine > 0 {
        Chamber::Chambered
    } else {
        Chamber::Empty
    }
}

/// Where the action's work is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum WorkStage {
    #[default]
    Idle,
    /// The handle is held, its rest at `rest` along the gun's rearward axis; `pulled` once it has
    /// reached the pull share of its travel.
    Held { rest: f32, pulled: bool },
}

/// What the work reports.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum WorkEvent {
    Grabbed,
    /// The pull reached the action's travel.
    Pulled,
    /// The action closed after a full pull: the chamber is loaded.
    Worked,
    /// The handle was let go before a full pull.
    LetGo,
}

/// One tick's inputs, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WorkInputs {
    pub kind: ActionKind,
    /// The handle's place at rest.
    pub handle: Vec3,
    /// The gun's rearward axis (from the muzzle toward the stock), unit length.
    pub rearward: Vec3,
    /// The action's travel, metres.
    pub travel_m: f32,
    /// The working hand's grip position.
    pub hand: Vec3,
    /// The working hand's grip was pressed this tick.
    pub grip_pressed: bool,
    pub grip_held: bool,
    /// For the pump: the hand holds the foregrip.
    pub on_foregrip: bool,
    /// Whether the chamber needs the action worked.
    pub needed: bool,
}

/// One gun's action work.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ActionWork {
    pub stage: WorkStage,
    /// How far the handle is back, as a share of its travel, while held.
    pub progress: f32,
    pub worked: u64,
}

impl ActionWork {
    pub const fn new() -> Self {
        Self {
            stage: WorkStage::Idle,
            progress: 0.0,
            worked: 0,
        }
    }

    /// Fold in one tick (`None` without a gun whose action can be worked).
    pub fn step(&mut self, inputs: Option<WorkInputs>, config: &ActionConfig) -> Option<WorkEvent> {
        let Some(i) = inputs.filter(|i| {
            i.kind != ActionKind::None
                && i.hand.is_finite()
                && i.handle.is_finite()
                && i.travel_m > 0.0
        }) else {
            self.reset();
            return None;
        };
        let along = (i.hand - i.handle).dot(i.rearward);
        match self.stage {
            WorkStage::Idle => {
                let grabbed = if i.kind == ActionKind::Pump {
                    i.needed && i.on_foregrip
                } else {
                    i.grip_pressed && i.hand.distance(i.handle) <= config.grab_radius_m
                };
                if !grabbed {
                    return None;
                }
                self.stage = WorkStage::Held {
                    rest: along,
                    pulled: false,
                };
                self.progress = 0.0;
                Some(WorkEvent::Grabbed)
            }
            WorkStage::Held { rest, pulled } => {
                let back = along - rest;
                let off_axis = ((i.hand - i.handle) - i.rearward * along).length();
                self.progress = (back / i.travel_m).clamp(0.0, 1.0);
                let holding = if i.kind == ActionKind::Pump {
                    i.on_foregrip
                } else {
                    i.grip_held
                };
                let returned = self.progress <= config.return_fraction;
                if !holding || off_axis > config.release_distance_m {
                    self.reset();
                    return Some(if pulled && !i.kind.must_return() {
                        self.worked += 1;
                        WorkEvent::Worked
                    } else {
                        WorkEvent::LetGo
                    });
                }
                if !pulled && self.progress >= config.pull_fraction {
                    self.stage = WorkStage::Held { rest, pulled: true };
                    return Some(WorkEvent::Pulled);
                }
                if pulled && returned {
                    self.reset();
                    self.worked += 1;
                    return Some(WorkEvent::Worked);
                }
                None
            }
        }
    }

    pub fn reset(&mut self) {
        self.stage = WorkStage::Idle;
        self.progress = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: ActionConfig = ActionConfig::new();
    const HANDLE: Vec3 = Vec3::new(0.1, -0.2, -0.3);

    fn inputs(kind: ActionKind, back: f32, pressed: bool, held: bool) -> Option<WorkInputs> {
        Some(WorkInputs {
            kind,
            handle: HANDLE,
            rearward: Vec3::Z,
            travel_m: 0.05,
            hand: HANDLE + Vec3::Z * back,
            grip_pressed: pressed,
            grip_held: held,
            on_foregrip: held,
            needed: true,
        })
    }

    #[test]
    fn a_dry_gun_empties_its_chamber_and_a_manual_action_after_every_shot() {
        assert_eq!(after_shot(0, ActionKind::Slide, true), Chamber::Empty);
        assert_eq!(after_shot(5, ActionKind::Slide, true), Chamber::Chambered);
        assert_eq!(after_shot(5, ActionKind::Bolt, true), Chamber::Empty);
        assert_eq!(after_shot(5, ActionKind::Bolt, false), Chamber::Chambered);
        assert_eq!(after_shot(0, ActionKind::None, true), Chamber::Chambered);
        assert_eq!(after_work(5, true), Chamber::Chambered);
        assert_eq!(after_work(0, true), Chamber::Empty);
        assert_eq!(after_work(5, false), Chamber::Empty);
    }

    #[test]
    fn a_slide_pulled_back_and_let_go_is_worked() {
        let mut w = ActionWork::new();
        let k = ActionKind::Slide;
        assert_eq!(
            w.step(inputs(k, 0.0, true, true), &CONFIG),
            Some(WorkEvent::Grabbed)
        );
        assert_eq!(w.step(inputs(k, 0.02, false, true), &CONFIG), None);
        assert_eq!(
            w.step(inputs(k, 0.045, false, true), &CONFIG),
            Some(WorkEvent::Pulled)
        );
        assert_eq!(
            w.step(inputs(k, 0.045, false, false), &CONFIG),
            Some(WorkEvent::Worked)
        );
        assert_eq!(w.worked, 1);
    }

    #[test]
    fn a_short_pull_or_a_grab_away_from_the_handle_does_nothing() {
        let mut w = ActionWork::new();
        let k = ActionKind::ChargingHandle;
        assert_eq!(w.step(inputs(k, 0.2, true, true), &CONFIG), None);
        w.step(inputs(k, 0.0, true, true), &CONFIG);
        w.step(inputs(k, 0.02, false, true), &CONFIG);
        assert_eq!(
            w.step(inputs(k, 0.02, false, false), &CONFIG),
            Some(WorkEvent::LetGo)
        );
        assert_eq!(w.worked, 0);
    }

    #[test]
    fn a_bolt_has_to_go_back_home() {
        let mut w = ActionWork::new();
        let k = ActionKind::Bolt;
        w.step(inputs(k, 0.0, true, true), &CONFIG);
        w.step(inputs(k, 0.05, false, true), &CONFIG);
        assert_eq!(
            w.step(inputs(k, 0.05, false, false), &CONFIG),
            Some(WorkEvent::LetGo)
        );
        w.step(inputs(k, 0.0, true, true), &CONFIG);
        w.step(inputs(k, 0.05, false, true), &CONFIG);
        assert_eq!(
            w.step(inputs(k, 0.005, false, true), &CONFIG),
            Some(WorkEvent::Worked)
        );
    }

    #[test]
    fn the_pump_starts_from_the_foregrip_hand_when_needed() {
        let mut w = ActionWork::new();
        let k = ActionKind::Pump;
        let not_needed = inputs(k, 0.03, false, true).map(|i| WorkInputs { needed: false, ..i });
        assert_eq!(w.step(not_needed, &CONFIG), None);
        assert_eq!(
            w.step(inputs(k, 0.03, false, true), &CONFIG),
            Some(WorkEvent::Grabbed)
        );
        assert_eq!(
            w.step(inputs(k, 0.075, false, true), &CONFIG),
            Some(WorkEvent::Pulled)
        );
        assert_eq!(
            w.step(inputs(k, 0.035, false, true), &CONFIG),
            Some(WorkEvent::Worked)
        );
    }
}
