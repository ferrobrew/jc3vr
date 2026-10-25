//! The two-handed insertion: a magazine (or a shell, or a rocket) taken from the pouch, carried to
//! the gun's well, caught by it, and seated. Pure.
//!
//! The hand holds the round with its top ahead of the closed fist ([`Held`]). The well is a mouth
//! on the gun's surface and the axis a round travels along into the gun ([`Well`]). The top of the
//! held round catches the well when it is within the capture radius of the mouth, turned no more
//! than the capture angle from the axis, and moving into the gun. A caught round slides only along
//! the axis; once its top is the seat depth past the mouth it seats. Pulled back out of the capture
//! it is held again. Letting go of the grip drops it, unless it is already more than half seated,
//! which seats it.

use glam::Vec3;

use crate::controllers::{frame::Hand, weapons::reload::config::ReloadConfig};

/// Where the insertion is.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub enum InsertStage {
    #[default]
    Idle,
    /// A round is in the hand, taken `elapsed_s` ago.
    Holding { elapsed_s: f32 },
    /// The round is caught by the well, its top `depth_m` past the mouth along the axis.
    Captured { elapsed_s: f32, depth_m: f32 },
}

/// What the insertion reports.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InsertEvent {
    /// A round was taken from the pouch.
    Taken,
    /// The round caught the well.
    Captured,
    /// The round left the capture without seating.
    Released,
    /// The round seated: load it.
    Seated,
    /// The round was let go, timed out, or its gun went away.
    Dropped,
}

/// The held round in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Held {
    /// The point of the round that enters first.
    pub top: Vec3,
    /// The direction the round points out of the hand, unit length.
    pub axis: Vec3,
    /// The top's velocity, metres per second.
    pub velocity: Vec3,
}

/// A gun's well in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Well {
    pub mouth: Vec3,
    /// The direction a round travels into the gun, unit length.
    pub axis: Vec3,
}

/// One tick's inputs.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct InsertInputs {
    /// The hand took a round from the pouch this tick.
    pub taken: bool,
    /// The hand's grip is held.
    pub grip_held: bool,
    pub held: Option<Held>,
    /// The well of the gun the round is for, or `None` without a gun that can take it now.
    pub well: Option<Well>,
    pub dt: f32,
}

/// The origin determines whether an actively held round has the pouch lifetime.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub(crate) enum InsertionSource {
    #[default]
    Pouch,
    RetainedMagazine,
}

/// The insertion's state and counts.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Insertion {
    pub stage: InsertStage,
    pub taken: u64,
    pub seated: u64,
    pub dropped: u64,
    /// The held top's distance from the mouth on the last tick with a round in hand, metres.
    pub distance_m: Option<f32>,
}

impl Insertion {
    pub const fn new() -> Self {
        Self {
            stage: InsertStage::Idle,
            taken: 0,
            seated: 0,
            dropped: 0,
            distance_m: None,
        }
    }

    pub fn holding(&self) -> bool {
        self.stage != InsertStage::Idle
    }

    /// Fold one tick in for a pouch round.
    #[cfg(test)]
    pub fn step(&mut self, inputs: InsertInputs, config: &ReloadConfig) -> Option<InsertEvent> {
        self.step_with_source(inputs, InsertionSource::Pouch, config)
    }

    /// Physical magazines remain held until release or tracking loss; only pouch ammunition expires.
    pub(super) fn step_with_source(
        &mut self,
        inputs: InsertInputs,
        source: InsertionSource,
        config: &ReloadConfig,
    ) -> Option<InsertEvent> {
        let dt = if inputs.dt.is_finite() {
            inputs.dt.max(0.0)
        } else {
            0.0
        };
        let event = match self.stage {
            InsertStage::Idle => {
                if inputs.taken {
                    self.stage = InsertStage::Holding { elapsed_s: 0.0 };
                    Some(InsertEvent::Taken)
                } else {
                    None
                }
            }
            InsertStage::Holding { elapsed_s } | InsertStage::Captured { elapsed_s, .. } => {
                self.step_held(inputs, elapsed_s + dt, source, config)
            }
        };
        match event {
            Some(InsertEvent::Taken) => self.taken += 1,
            Some(InsertEvent::Seated) => self.seated += 1,
            Some(InsertEvent::Dropped) => self.dropped += 1,
            _ => {}
        }
        if self.stage == InsertStage::Idle {
            self.distance_m = None;
        }
        event
    }

    fn step_held(
        &mut self,
        inputs: InsertInputs,
        elapsed_s: f32,
        source: InsertionSource,
        config: &ReloadConfig,
    ) -> Option<InsertEvent> {
        let captured = match self.stage {
            InsertStage::Captured { depth_m, .. } => Some(depth_m),
            _ => None,
        };
        let (Some(held), Some(well)) = (inputs.held, inputs.well) else {
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        };
        let offset = held.top - well.mouth;
        let depth = offset.dot(well.axis);
        let off_axis = (offset - well.axis * depth).length();
        self.distance_m = Some(offset.length()).filter(|d| d.is_finite());
        if !inputs.grip_held {
            self.stage = InsertStage::Idle;
            return Some(
                if captured.is_some() && depth >= config.seat_depth_m * 0.5 {
                    InsertEvent::Seated
                } else {
                    InsertEvent::Dropped
                },
            );
        }
        if source == InsertionSource::Pouch && captured.is_none() && elapsed_s > config.timeout_s {
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        }
        match captured {
            Some(_) => {
                let slack = config.capture_radius_m * 1.5;
                if depth >= config.seat_depth_m {
                    self.stage = InsertStage::Idle;
                    Some(InsertEvent::Seated)
                } else if off_axis > slack || depth < -slack {
                    self.stage = InsertStage::Holding { elapsed_s };
                    Some(InsertEvent::Released)
                } else {
                    self.stage = InsertStage::Captured {
                        elapsed_s,
                        depth_m: depth,
                    };
                    None
                }
            }
            None => {
                let aligned =
                    held.axis.dot(well.axis) >= config.capture_angle_deg.to_radians().cos();
                let entering = held.velocity.dot(well.axis) > 0.0;
                if offset.length() <= config.capture_radius_m && aligned && entering {
                    self.stage = InsertStage::Captured {
                        elapsed_s,
                        depth_m: depth,
                    };
                    Some(InsertEvent::Captured)
                } else {
                    self.stage = InsertStage::Holding { elapsed_s };
                    None
                }
            }
        }
    }

    /// Drop any round in hand without counting it: the reload stopped acting.
    pub fn reset(&mut self) {
        self.stage = InsertStage::Idle;
        self.distance_m = None;
    }
}

/// The held round of a hand whose grip is at `grip` pointing along `pointing` (the direction the
/// closed hand points), its top `top_m` ahead of the grip, moving at `velocity`.
pub fn held_round(grip: Vec3, pointing: Vec3, top_m: f32, velocity: Vec3) -> Option<Held> {
    let axis = pointing.try_normalize()?;
    Some(Held {
        top: grip + axis * top_m,
        axis,
        velocity,
    })
    .filter(|h| h.top.is_finite() && h.velocity.is_finite())
}

/// The hand whose gun a round from the pouch is for, from the hands holding a gun (`gun_hands`, by
/// [`Hand::index`]), the hand that would take the round (`taker`, `None` while no hand is free),
/// and the `dominant` hand: the gun in the other hand from the taker, or without a taker, the
/// dominant hand's gun and then the other hand's. `None` without a gun.
pub fn loading_hand(gun_hands: [bool; 2], taker: Option<Hand>, dominant: Hand) -> Option<Hand> {
    let first = taker.map_or(dominant, Hand::other);
    [first, first.other()]
        .into_iter()
        .find(|hand| gun_hands[hand.index()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_magazines_keep_the_grip_beyond_pouch_and_world_lifetimes() {
        let config = ReloadConfig::new();
        for source in [InsertionSource::Pouch, InsertionSource::RetainedMagazine] {
            let mut insertion = Insertion::new();
            let mut sample = inputs(true, WELL.mouth - Vec3::Y, Vec3::ZERO);
            insertion.step_with_source(sample, source, &config);
            sample.taken = false;
            sample.dt = config.timeout_s + config.loose.keep_s + 1.0;
            let event = insertion.step_with_source(sample, source, &config);
            match source {
                InsertionSource::Pouch => assert_eq!(event, Some(InsertEvent::Dropped)),
                InsertionSource::RetainedMagazine => {
                    assert_eq!(event, None);
                    assert!(insertion.holding());
                    sample.grip_held = false;
                    assert_eq!(
                        insertion.step_with_source(sample, source, &config),
                        Some(InsertEvent::Dropped)
                    );
                    sample.taken = true;
                    sample.grip_held = true;
                    insertion.step_with_source(sample, source, &config);
                    sample.taken = false;
                    sample.held = None;
                    assert_eq!(
                        insertion.step_with_source(sample, source, &config),
                        Some(InsertEvent::Dropped)
                    );
                }
            }
        }
    }

    const CONFIG: ReloadConfig = ReloadConfig::new();
    const WELL: Well = Well {
        mouth: Vec3::new(0.2, -0.3, -0.4),
        axis: Vec3::Y,
    };

    fn inputs(taken: bool, top: Vec3, velocity: Vec3) -> InsertInputs {
        InsertInputs {
            taken,
            grip_held: true,
            held: Some(Held {
                top,
                axis: Vec3::Y,
                velocity,
            }),
            well: Some(WELL),
            dt: 0.02,
        }
    }

    fn take(i: &mut Insertion) {
        assert_eq!(
            i.step(inputs(true, Vec3::ZERO, Vec3::ZERO), &CONFIG),
            Some(InsertEvent::Taken)
        );
    }

    #[test]
    fn an_upward_push_into_the_well_catches_then_seats() {
        let mut i = Insertion::new();
        take(&mut i);
        let up = Vec3::Y * 0.5;
        assert_eq!(
            i.step(inputs(false, WELL.mouth - Vec3::Y * 0.02, up), &CONFIG),
            Some(InsertEvent::Captured)
        );
        assert_eq!(
            i.step(inputs(false, WELL.mouth + Vec3::Y * 0.01, up), &CONFIG),
            None
        );
        assert_eq!(
            i.step(inputs(false, WELL.mouth + Vec3::Y * 0.03, up), &CONFIG),
            Some(InsertEvent::Seated)
        );
        assert_eq!(i.stage, InsertStage::Idle);
        assert_eq!((i.taken, i.seated, i.dropped), (1, 1, 0));
    }

    #[test]
    fn a_round_moving_away_or_crossways_does_not_catch() {
        let mut i = Insertion::new();
        take(&mut i);
        let near = WELL.mouth - Vec3::Y * 0.02;
        assert_eq!(i.step(inputs(false, near, -Vec3::Y), &CONFIG), None);
        let crossways = InsertInputs {
            held: Some(Held {
                top: near,
                axis: Vec3::X,
                velocity: Vec3::Y,
            }),
            ..inputs(false, near, Vec3::Y)
        };
        assert_eq!(i.step(crossways, &CONFIG), None);
        assert!(matches!(i.stage, InsertStage::Holding { .. }));
    }

    #[test]
    fn pulling_a_caught_round_back_out_holds_it_again() {
        let mut i = Insertion::new();
        take(&mut i);
        i.step(inputs(false, WELL.mouth, Vec3::Y), &CONFIG);
        assert_eq!(
            i.step(inputs(false, WELL.mouth - Vec3::Y * 0.2, -Vec3::Y), &CONFIG),
            Some(InsertEvent::Released)
        );
        assert!(matches!(i.stage, InsertStage::Holding { .. }));
    }

    #[test]
    fn letting_go_drops_unless_half_seated() {
        let mut i = Insertion::new();
        take(&mut i);
        let released = |top| InsertInputs {
            grip_held: false,
            ..inputs(false, top, Vec3::Y)
        };
        assert_eq!(
            i.step(released(Vec3::ZERO), &CONFIG),
            Some(InsertEvent::Dropped)
        );

        take(&mut i);
        i.step(inputs(false, WELL.mouth, Vec3::Y), &CONFIG);
        i.step(
            inputs(false, WELL.mouth + Vec3::Y * 0.015, Vec3::Y),
            &CONFIG,
        );
        assert_eq!(
            i.step(released(WELL.mouth + Vec3::Y * 0.015), &CONFIG),
            Some(InsertEvent::Seated)
        );
    }

    #[test]
    fn a_round_held_too_long_or_without_a_gun_is_dropped() {
        let mut i = Insertion::new();
        take(&mut i);
        let mut event = None;
        for _ in 0..1000 {
            event = i.step(inputs(false, Vec3::ZERO, Vec3::ZERO), &CONFIG);
            if event.is_some() {
                break;
            }
        }
        assert_eq!(event, Some(InsertEvent::Dropped));

        take(&mut i);
        let gone = InsertInputs {
            well: None,
            ..inputs(false, Vec3::ZERO, Vec3::ZERO)
        };
        assert_eq!(i.step(gone, &CONFIG), Some(InsertEvent::Dropped));
    }

    #[test]
    fn the_held_top_is_ahead_of_the_grip() {
        let held = held_round(Vec3::ZERO, Vec3::new(0.0, 0.0, -2.0), 0.07, Vec3::ZERO).unwrap();
        assert!((held.top - Vec3::new(0.0, 0.0, -0.07)).length() < 1e-6);
        assert_eq!(held.axis, Vec3::NEG_Z);
        assert!(held_round(Vec3::ZERO, Vec3::ZERO, 0.07, Vec3::ZERO).is_none());
    }

    #[test]
    fn a_round_from_the_free_hand_is_for_the_gun_in_the_other() {
        assert_eq!(
            loading_hand([false, true], Some(Hand::Left), Hand::Left),
            Some(Hand::Right)
        );
        assert_eq!(
            loading_hand([true, false], Some(Hand::Right), Hand::Right),
            Some(Hand::Left)
        );
    }

    #[test]
    fn with_both_hands_full_the_dominant_hands_gun_is_loaded_first() {
        assert_eq!(
            loading_hand([true, true], None, Hand::Right),
            Some(Hand::Right)
        );
        assert_eq!(
            loading_hand([true, false], None, Hand::Right),
            Some(Hand::Left)
        );
        assert_eq!(loading_hand([false, false], None, Hand::Right), None);
    }
}
