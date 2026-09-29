//! The magazine reload gesture: the plan of record's "Reloads"
//! (`docs/mod/input/controllers-and-roomscale.md`), behind
//! [`WeaponsConfig::reload_gesture`](crate::controllers::config::WeaponsConfig::reload_gesture).
//!
//! The off-hand takes a magazine with a grip press at its pouch on the belt, then brings it to the
//! held weapon's magazine well, a sphere in the holding hand's wrist frame just ahead of the grip.
//! Reaching the well starts the game's own reload, as a tap of the reload button does. The button
//! still reloads, and the reload plays under the action-runs hand tier as it does from the button.
//! A magazine not inserted within the timeout is dropped. The gesture is pure ([`MagazineGesture`]);
//! the weapon handling places the pouch and the well and injects the reload.

use glam::Vec3;

use crate::controllers::{
    frame::{Hand, Pose},
    weapons::config::MagazineConfig,
};

/// Where the gesture is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum MagazineStage {
    #[default]
    Idle,
    /// A magazine is in the off-hand, taken `elapsed_s` ago.
    Holding { elapsed_s: f32 },
}

/// What the gesture reports.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GestureEvent {
    /// A magazine was taken from the pouch.
    Taken,
    /// The magazine reached the well: reload.
    Inserted,
    /// The magazine was dropped: the timeout passed or the weapon went away.
    Dropped,
}

/// One tick's inputs, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GestureInputs {
    /// The off-hand grabbed the magazine pouch this tick.
    pub taken: bool,
    /// The off-hand's grip position.
    pub hand: Option<Vec3>,
    /// The held weapon's magazine well, or `None` without a weapon the gesture can reload.
    pub well: Option<Vec3>,
    pub dt: f32,
}

/// The gesture's state and counts.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct MagazineGesture {
    pub stage: MagazineStage,
    pub taken: u64,
    pub inserted: u64,
    pub dropped: u64,
    /// The off-hand's distance from the well on the last tick with a magazine in hand, metres.
    pub distance_m: Option<f32>,
}

impl MagazineGesture {
    pub const fn new() -> Self {
        Self {
            stage: MagazineStage::Idle,
            taken: 0,
            inserted: 0,
            dropped: 0,
            distance_m: None,
        }
    }

    /// Fold one tick in.
    pub fn step(&mut self, inputs: GestureInputs, config: &MagazineConfig) -> Option<GestureEvent> {
        let event = match self.stage {
            MagazineStage::Idle if inputs.taken && inputs.well.is_some() => {
                self.stage = MagazineStage::Holding { elapsed_s: 0.0 };
                Some(GestureEvent::Taken)
            }
            MagazineStage::Idle => None,
            MagazineStage::Holding { elapsed_s } => {
                let elapsed_s = if inputs.taken {
                    0.0
                } else {
                    elapsed_s + inputs.dt.max(0.0)
                };
                let distance = inputs
                    .hand
                    .zip(inputs.well)
                    .map(|(hand, well)| hand.distance(well))
                    .filter(|d| d.is_finite());
                self.distance_m = distance;
                if inputs.well.is_none() || elapsed_s > config.timeout_s {
                    self.stage = MagazineStage::Idle;
                    Some(GestureEvent::Dropped)
                } else if distance.is_some_and(|d| d <= config.well_radius_m) {
                    self.stage = MagazineStage::Idle;
                    Some(GestureEvent::Inserted)
                } else {
                    self.stage = MagazineStage::Holding { elapsed_s };
                    None
                }
            }
        };
        match event {
            Some(GestureEvent::Taken) => self.taken += 1,
            Some(GestureEvent::Inserted) => self.inserted += 1,
            Some(GestureEvent::Dropped) => self.dropped += 1,
            None => {}
        }
        if self.stage == MagazineStage::Idle {
            self.distance_m = None;
        }
        event
    }

    /// Drop any magazine in hand without counting it: the gesture stopped acting.
    pub fn reset(&mut self) {
        self.stage = MagazineStage::Idle;
        self.distance_m = None;
    }
}

/// The magazine well of a weapon held by `holder` with its wrist at `wrist`: the configured offset,
/// given for the right hand and mirrored for the left, in the wrist frame.
pub fn well_point(wrist: Pose, holder: Hand, offset: Vec3) -> Vec3 {
    let local = match holder {
        Hand::Left => Vec3::new(-offset.x, offset.y, offset.z),
        Hand::Right => offset,
    };
    wrist.position + wrist.orientation * local
}

#[cfg(test)]
mod tests {
    use glam::Quat;

    use super::*;

    const CONFIG: MagazineConfig = MagazineConfig::new();
    const WELL: Vec3 = Vec3::new(0.2, -0.3, -0.4);

    fn inputs(taken: bool, hand: Vec3) -> GestureInputs {
        GestureInputs {
            taken,
            hand: Some(hand),
            well: Some(WELL),
            dt: 0.1,
        }
    }

    #[test]
    fn pouch_then_well_reloads() {
        let mut g = MagazineGesture::new();
        assert_eq!(
            g.step(inputs(true, Vec3::ZERO), &CONFIG),
            Some(GestureEvent::Taken)
        );
        assert_eq!(g.step(inputs(false, Vec3::ZERO), &CONFIG), None);
        assert!(g.distance_m.is_some());
        assert_eq!(
            g.step(inputs(false, WELL + Vec3::X * 0.02), &CONFIG),
            Some(GestureEvent::Inserted)
        );
        assert_eq!(g.stage, MagazineStage::Idle);
        assert_eq!((g.taken, g.inserted, g.dropped), (1, 1, 0));
    }

    #[test]
    fn the_well_without_a_magazine_does_nothing() {
        let mut g = MagazineGesture::new();
        assert_eq!(g.step(inputs(false, WELL), &CONFIG), None);
        assert_eq!(g.stage, MagazineStage::Idle);
    }

    #[test]
    fn a_magazine_held_too_long_is_dropped() {
        let mut g = MagazineGesture::new();
        g.step(inputs(true, Vec3::ZERO), &CONFIG);
        let mut event = None;
        for _ in 0..100 {
            event = g.step(inputs(false, Vec3::ZERO), &CONFIG);
            if event.is_some() {
                break;
            }
        }
        assert_eq!(event, Some(GestureEvent::Dropped));
        assert_eq!(g.dropped, 1);
    }

    #[test]
    fn losing_the_weapon_drops_the_magazine_and_no_weapon_takes_none() {
        let mut g = MagazineGesture::new();
        let none = GestureInputs {
            well: None,
            ..inputs(true, Vec3::ZERO)
        };
        assert_eq!(g.step(none, &CONFIG), None);
        g.step(inputs(true, Vec3::ZERO), &CONFIG);
        assert_eq!(
            g.step(
                GestureInputs {
                    well: None,
                    ..inputs(false, WELL)
                },
                &CONFIG
            ),
            Some(GestureEvent::Dropped)
        );
    }

    #[test]
    fn the_well_mirrors_for_a_left_hand_holder() {
        let wrist = Pose {
            position: Vec3::new(1.0, 0.0, 0.0),
            orientation: Quat::IDENTITY,
        };
        let offset = Vec3::new(0.03, -0.08, -0.06);
        assert_eq!(
            well_point(wrist, Hand::Right, offset),
            wrist.position + offset
        );
        assert_eq!(
            well_point(wrist, Hand::Left, offset),
            wrist.position + Vec3::new(-0.03, -0.08, -0.06)
        );
    }
}
