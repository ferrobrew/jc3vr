//! The two-handed insertion: a magazine (or a shell, or a rocket) taken from the pouch, carried to
//! the gun's well, caught by it, and seated. Pure.
//!
//! The hand holds the round with its top ahead of the closed fist ([`Held`]). The well is a mouth
//! on the gun's surface and the axis a round travels along into the gun ([`Well`]). The top of the
//! held round catches the well when it is within the capture radius of the mouth, turned no more
//! than the capture angle from the axis, and moving into the gun. A caught round slides only along
//! the axis; once its top is the seat depth past the mouth it seats. Pulled back out of the capture
//! it is held again. Letting go of the grip seats a round already more than half seated. A round
//! let go of near the mouth and pointing roughly into the well floats in ([`FLOAT`]): its top
//! travels from where the hand left it onto the axis and down to the seat depth, turning onto the
//! axis as it goes, and seats when it arrives. Anything else let go of drops.

use glam::{Quat, Vec3};

use crate::controllers::{frame::Hand, weapons::reload::tuning::Reload};

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
    /// The previous held top in the well's rotating frame.
    pub(super) previous_local_top: Option<Vec3>,
    /// The float of a round let go of near the well, while it travels in.
    pub(super) float: Option<Float>,
}

/// How a round let go of near the well floats into it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct FloatTuning {
    /// How far from the mouth the top of a round let go of may be to float in, metres: about a
    /// hand's breadth, so a magazine released while it is being brought to the well goes in, and
    /// one let go of anywhere else drops. A well whose capture radius is large floats from twice
    /// that instead.
    pub radius_m: f32,
    /// How far a round let go of may be turned from the well's axis and float in, degrees: wider
    /// than the capture angle, as the round turns onto the axis on the way.
    pub angle_deg: f32,
    /// How fast the round travels in on average, metres per second.
    pub speed_mps: f32,
    /// The shortest and the longest the float takes, seconds.
    pub min_s: f32,
    pub max_s: f32,
}

/// The float's fixed tuning.
pub const FLOAT: FloatTuning = FloatTuning {
    radius_m: 0.15,
    angle_deg: 70.0,
    speed_mps: 0.8,
    min_s: 0.08,
    max_s: 0.3,
};

/// A round floating into the well, in the well's frame (+Y along the axis into the gun, the mouth
/// at the origin).
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) struct Float {
    /// Where the top was let go of.
    from: Vec3,
    /// The direction the round pointed when it was let go of, unit length.
    from_axis: Vec3,
    /// How far past the mouth the top seats.
    seat_m: f32,
    elapsed_s: f32,
    duration_s: f32,
}

impl Float {
    /// The float of a round let go of with its top at `local` pointing along `axis` (both in the
    /// well's frame), or `None` when it is too far from the mouth, turned too far from the axis,
    /// or beside the gun past the mouth.
    fn start(local: Vec3, axis: Vec3, config: &Reload, tuning: &FloatTuning) -> Option<Self> {
        let axis = axis.try_normalize()?;
        let radius = tuning.radius_m.max(config.capture_radius_m * 2.0);
        let distance = local.length();
        let near = distance <= radius && local.y <= config.capture_radius_m;
        let aligned = axis.y >= tuning.angle_deg.to_radians().cos();
        if !(near && aligned && local.is_finite()) {
            return None;
        }
        let travel = (distance + config.seat_depth_m) / tuning.speed_mps.max(f32::EPSILON);
        Some(Self {
            from: local,
            from_axis: axis,
            seat_m: config.seat_depth_m,
            elapsed_s: 0.0,
            duration_s: travel.clamp(tuning.min_s, tuning.max_s.max(tuning.min_s)),
        })
    }

    fn advanced(self, dt: f32) -> Self {
        Self {
            elapsed_s: (self.elapsed_s + dt).min(self.duration_s),
            ..self
        }
    }

    fn arrived(&self) -> bool {
        self.elapsed_s >= self.duration_s
    }

    /// The top now. It first comes onto the axis without passing the mouth, then slides down the
    /// axis to the seat depth, eased in and out, so it never cuts through the gun's side.
    fn top(&self) -> Vec3 {
        let t = (self.elapsed_s / self.duration_s.max(f32::EPSILON)).clamp(0.0, 1.0);
        let approach = smoothstep((t / APPROACH_SHARE).min(1.0));
        let slide = smoothstep(((t - APPROACH_SHARE) / (1.0 - APPROACH_SHARE)).max(0.0));
        let entry = self.from.y.min(0.0);
        let lateral = Vec3::new(self.from.x, 0.0, self.from.z) * (1.0 - approach);
        let depth = if t < APPROACH_SHARE {
            self.from.y + (entry - self.from.y) * approach
        } else {
            entry + (self.seat_m - entry) * slide
        };
        lateral + Vec3::Y * depth
    }

    /// The direction the round points now: from where it pointed onto the axis.
    fn axis(&self) -> Vec3 {
        let t = (self.elapsed_s / self.duration_s.max(f32::EPSILON)).clamp(0.0, 1.0);
        Quat::from_rotation_arc(self.from_axis, Vec3::Y)
            .slerp(Quat::IDENTITY, 1.0 - smoothstep(t))
            .mul_vec3(self.from_axis)
    }
}

/// The share of a float spent bringing the top onto the axis.
const APPROACH_SHARE: f32 = 0.6;

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Insertion {
    pub const fn new() -> Self {
        Self {
            stage: InsertStage::Idle,
            taken: 0,
            seated: 0,
            dropped: 0,
            distance_m: None,
            previous_local_top: None,
            float: None,
        }
    }

    pub fn holding(&self) -> bool {
        self.stage != InsertStage::Idle
    }

    /// Fold one tick in for a pouch round.
    #[cfg(test)]
    pub fn step(&mut self, inputs: InsertInputs, config: &Reload) -> Option<InsertEvent> {
        self.step_with_source(inputs, InsertionSource::Pouch, config)
    }

    /// Physical magazines remain held until release or tracking loss; only pouch ammunition expires.
    pub(super) fn step_with_source(
        &mut self,
        inputs: InsertInputs,
        source: InsertionSource,
        config: &Reload,
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
        self.previous_local_top = if self.stage == InsertStage::Idle {
            self.distance_m = None;
            None
        } else {
            inputs
                .held
                .zip(inputs.well)
                .and_then(|(held, well)| relative_top(held, well).map(|(local, _, _)| local))
        };
        event
    }

    fn step_held(
        &mut self,
        inputs: InsertInputs,
        elapsed_s: f32,
        source: InsertionSource,
        config: &Reload,
    ) -> Option<InsertEvent> {
        if let Some(float) = self.float {
            return self.step_float(float, inputs, elapsed_s);
        }
        let captured = match self.stage {
            InsertStage::Captured { depth_m, .. } => Some(depth_m),
            _ => None,
        };
        let (Some(held), Some(well)) = (inputs.held, inputs.well) else {
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        };
        let Some((local, aligned_cos, axis)) = relative_top(held, well) else {
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        };
        let depth = local.y;
        let off_axis = Vec3::new(local.x, 0.0, local.z).length();
        let aligned = aligned_cos >= config.capture_angle_deg.to_radians().cos();
        let slack = config.capture_radius_m * 1.5;
        let retained = aligned && off_axis <= slack && depth >= -slack;
        self.distance_m = Some(local.length());
        if !inputs.grip_held {
            if captured.is_some() && retained && depth >= config.seat_depth_m * 0.5 {
                self.stage = InsertStage::Idle;
                return Some(InsertEvent::Seated);
            }
            let pointing = Quat::from_rotation_arc(axis, Vec3::Y) * held.axis;
            if let Some(float) = Float::start(local, pointing, config, &FLOAT) {
                self.float = Some(float);
                self.stage = InsertStage::Captured {
                    elapsed_s,
                    depth_m: depth,
                };
                return captured.is_none().then_some(InsertEvent::Captured);
            }
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        }
        if source == InsertionSource::Pouch && captured.is_none() && elapsed_s > config.timeout_s {
            self.stage = InsertStage::Idle;
            return Some(InsertEvent::Dropped);
        }
        match captured {
            Some(_) => {
                if !retained {
                    self.stage = InsertStage::Holding { elapsed_s };
                    Some(InsertEvent::Released)
                } else if depth >= config.seat_depth_m {
                    self.stage = InsertStage::Idle;
                    Some(InsertEvent::Seated)
                } else {
                    self.stage = InsertStage::Captured {
                        elapsed_s,
                        depth_m: depth,
                    };
                    None
                }
            }
            None => {
                let entering = self.previous_local_top.map_or_else(
                    || held.velocity.dot(axis) > 0.0,
                    |previous| local.y > previous.y + 1.0e-5,
                );
                let near = local.length() <= config.capture_radius_m
                    || self.previous_local_top.is_some_and(|previous| {
                        swept_mouth(previous, local, config.capture_radius_m)
                    });
                if near && aligned && entering {
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

    /// Move a floating round on by the tick: it seats when it arrives, and drops when its gun can
    /// no longer take it. The grip has let it go, so neither the grip nor the hand moves it.
    fn step_float(
        &mut self,
        float: Float,
        inputs: InsertInputs,
        elapsed_s: f32,
    ) -> Option<InsertEvent> {
        if inputs.well.is_none() {
            self.stage = InsertStage::Idle;
            self.float = None;
            return Some(InsertEvent::Dropped);
        }
        let dt = if inputs.dt.is_finite() {
            inputs.dt.max(0.0)
        } else {
            0.0
        };
        let float = float.advanced(dt);
        if float.arrived() {
            self.stage = InsertStage::Idle;
            self.float = None;
            return Some(InsertEvent::Seated);
        }
        self.float = Some(float);
        self.stage = InsertStage::Captured {
            elapsed_s,
            depth_m: float.top().y,
        };
        None
    }

    /// The top of the captured round, in the cockpit frame: on the well's axis at its measured
    /// depth, or where a floating round has travelled to.
    pub(super) fn captured_top(&self, well: Well) -> Option<Vec3> {
        let InsertStage::Captured { depth_m, .. } = self.stage else {
            return None;
        };
        let axis = well.axis.try_normalize()?;
        Some(match self.float {
            Some(float) => well.mouth + Quat::from_rotation_arc(Vec3::Y, axis) * float.top(),
            None => well.mouth + axis * depth_m,
        })
    }

    /// Constrain the displayed round to the well while captured, keeping its measured depth, or
    /// carry a floating round along its float.
    pub(super) fn carried(&self, held: Held, well: Option<Well>) -> Held {
        let (InsertStage::Captured { .. }, Some(well)) = (self.stage, well) else {
            return held;
        };
        let (Some(axis), Some(top)) = (well.axis.try_normalize(), self.captured_top(well)) else {
            return held;
        };
        let axis = match self.float {
            Some(float) => Quat::from_rotation_arc(Vec3::Y, axis) * float.axis(),
            None => axis,
        };
        Held {
            top,
            axis,
            velocity: held.velocity,
        }
    }

    /// Drop any round in hand without counting it: the reload stopped acting.
    pub fn reset(&mut self) {
        self.stage = InsertStage::Idle;
        self.distance_m = None;
        self.previous_local_top = None;
        self.float = None;
    }
}

fn relative_top(held: Held, well: Well) -> Option<(Vec3, f32, Vec3)> {
    if !(held.top.is_finite() && held.velocity.is_finite() && well.mouth.is_finite()) {
        return None;
    }
    let axis = well.axis.try_normalize()?;
    let pointing = held.axis.try_normalize()?;
    let local = Quat::from_rotation_arc(axis, Vec3::Y) * (held.top - well.mouth);
    local
        .is_finite()
        .then_some((local, pointing.dot(axis), axis))
}

fn swept_mouth(previous: Vec3, current: Vec3, radius: f32) -> bool {
    if previous.y >= 0.0 || current.y < 0.0 || current.y <= previous.y {
        return false;
    }
    let at_mouth = previous.lerp(current, -previous.y / (current.y - previous.y));
    at_mouth.length_squared() <= radius * radius
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
    use crate::controllers::weapons::reload::tuning::RELOAD;

    #[test]
    fn retained_magazines_keep_the_grip_beyond_pouch_and_world_lifetimes() {
        let config = RELOAD;
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

    const CONFIG: Reload = RELOAD;
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
            i.step(
                InsertInputs {
                    held: None,
                    ..inputs(true, Vec3::ZERO, Vec3::ZERO)
                },
                &CONFIG
            ),
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
    fn a_swept_push_catches_and_the_display_is_constrained_before_seating() {
        let mut insertion = Insertion::new();
        insertion.step(
            inputs(true, WELL.mouth - Vec3::Y * 0.12, Vec3::ZERO),
            &CONFIG,
        );
        let pushed = inputs(false, WELL.mouth + Vec3::Y * 0.08, Vec3::ZERO);
        assert_eq!(insertion.step(pushed, &CONFIG), Some(InsertEvent::Captured));
        let shown = insertion.carried(
            Held {
                top: Vec3::splat(5.0),
                axis: Vec3::X,
                velocity: Vec3::ZERO,
            },
            Some(WELL),
        );
        assert!(shown.top.abs_diff_eq(WELL.mouth + Vec3::Y * 0.08, 1.0e-5));
        assert_eq!(shown.axis, WELL.axis);
        assert_eq!(insertion.step(pushed, &CONFIG), Some(InsertEvent::Seated));
        assert_eq!(insertion.step(pushed, &CONFIG), None);
        assert_eq!(insertion.seated, 1);
    }

    /// Let go of a round held with its top at `top` pointing along `axis`, then step until the
    /// insertion settles: the events in order, and the displayed tops of a floating round.
    fn let_go_at(top: Vec3, axis: Vec3) -> (Vec<InsertEvent>, Vec<Vec3>) {
        let mut i = Insertion::new();
        take(&mut i);
        let held = |grip_held| InsertInputs {
            grip_held,
            held: Some(Held {
                top,
                axis,
                velocity: Vec3::ZERO,
            }),
            ..inputs(false, top, Vec3::ZERO)
        };
        i.step(held(true), &CONFIG);
        let mut events = Vec::new();
        let mut shown = Vec::new();
        for _ in 0..50 {
            if let Some(event) = i.step(held(false), &CONFIG) {
                events.push(event);
            }
            if i.float.is_some() {
                let round = i.carried(
                    Held {
                        top: Vec3::splat(9.0),
                        axis: Vec3::X,
                        velocity: Vec3::ZERO,
                    },
                    Some(WELL),
                );
                shown.push(round.top);
            }
            if i.stage == InsertStage::Idle {
                break;
            }
        }
        (events, shown)
    }

    #[test]
    fn a_round_let_go_of_near_the_well_floats_in_and_seats() {
        let top = WELL.mouth + Vec3::new(0.06, -0.08, 0.03);
        let (events, shown) = let_go_at(top, Vec3::new(0.3, 1.0, 0.0));
        assert_eq!(events, vec![InsertEvent::Captured, InsertEvent::Seated]);
        assert!(shown.len() >= 3, "the float takes a few ticks: {shown:?}");
        // It leaves from where the hand let go of it, never jumps, and never cuts through the
        // gun's side: past the mouth, it is on the axis.
        assert!(shown[0].distance(top) < 0.05);
        for pair in shown.windows(2) {
            assert!(pair[0].distance(pair[1]) < 0.05, "{pair:?}");
        }
        for p in &shown {
            let local = *p - WELL.mouth;
            if local.y > 1.0e-4 {
                assert!(
                    Vec3::new(local.x, 0.0, local.z).length() < 1.0e-4,
                    "{local:?}"
                );
            }
        }
    }

    #[test]
    fn a_round_let_go_of_away_from_the_well_or_turned_from_it_drops() {
        let far = WELL.mouth + Vec3::new(0.15, -0.15, 0.0);
        assert_eq!(let_go_at(far, Vec3::Y).0, vec![InsertEvent::Dropped]);
        let near = WELL.mouth - Vec3::Y * 0.08;
        assert_eq!(let_go_at(near, Vec3::X).0, vec![InsertEvent::Dropped]);
        // Beside the gun, past the mouth.
        let beside = WELL.mouth + Vec3::new(0.1, 0.1, 0.0);
        assert_eq!(let_go_at(beside, Vec3::Y).0, vec![InsertEvent::Dropped]);
    }

    #[test]
    fn a_floating_round_drops_when_its_gun_goes() {
        let mut i = Insertion::new();
        take(&mut i);
        let near = WELL.mouth - Vec3::Y * 0.08;
        i.step(inputs(false, near, Vec3::ZERO), &CONFIG);
        let released = InsertInputs {
            grip_held: false,
            ..inputs(false, near, Vec3::ZERO)
        };
        assert_eq!(i.step(released, &CONFIG), Some(InsertEvent::Captured));
        assert!(i.float.is_some());
        let gone = InsertInputs {
            well: None,
            ..released
        };
        assert_eq!(i.step(gone, &CONFIG), Some(InsertEvent::Dropped));
        assert_eq!((i.stage, i.float, i.seated), (InsertStage::Idle, None, 0));
    }

    #[test]
    fn the_float_turns_the_round_onto_the_axis() {
        let float = Float::start(
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            &CONFIG,
            &FLOAT,
        )
        .unwrap();
        assert!(
            float
                .axis()
                .abs_diff_eq(Vec3::new(1.0, 1.0, 0.0).normalize(), 1.0e-5)
        );
        let arrived = float.advanced(FLOAT.max_s);
        assert!(arrived.arrived());
        assert!(arrived.axis().abs_diff_eq(Vec3::Y, 1.0e-5));
        assert!(
            arrived
                .top()
                .abs_diff_eq(Vec3::Y * CONFIG.seat_depth_m, 1.0e-5)
        );
    }

    #[test]
    fn lateral_escape_cannot_seat_even_after_full_depth_or_grip_release() {
        for released in [false, true] {
            let mut insertion = Insertion::new();
            take(&mut insertion);
            insertion.step(inputs(false, WELL.mouth, Vec3::Y), &CONFIG);
            let mut escaped = inputs(false, WELL.mouth + Vec3::new(0.2, 0.1, 0.0), Vec3::Y);
            escaped.grip_held = !released;
            assert_eq!(
                insertion.step(escaped, &CONFIG),
                Some(if released {
                    InsertEvent::Dropped
                } else {
                    InsertEvent::Released
                }),
            );
            assert_eq!(insertion.seated, 0);
        }
    }

    #[test]
    fn relative_motion_accepts_a_moving_well_but_not_shared_transport() {
        let top = WELL.mouth - Vec3::Y * 0.04;
        let mut insertion = Insertion::new();
        insertion.step(inputs(true, top, Vec3::ZERO), &CONFIG);
        let mut shared = inputs(false, top + Vec3::Y, Vec3::Y * 20.0);
        shared.well = Some(Well {
            mouth: WELL.mouth + Vec3::Y,
            ..WELL
        });
        assert_eq!(insertion.step(shared, &CONFIG), None);
        let mut entering = shared;
        entering.well.as_mut().unwrap().mouth -= Vec3::Y * 0.03;
        assert_eq!(
            insertion.step(entering, &CONFIG),
            Some(InsertEvent::Captured)
        );
    }

    #[test]
    fn invalid_capture_geometry_drops_without_seating() {
        let mut insertion = Insertion::new();
        take(&mut insertion);
        insertion.step(inputs(false, WELL.mouth, Vec3::Y), &CONFIG);
        let mut invalid = inputs(false, WELL.mouth + Vec3::Y, Vec3::Y);
        invalid.well.as_mut().unwrap().axis = Vec3::ZERO;
        assert_eq!(insertion.step(invalid, &CONFIG), Some(InsertEvent::Dropped));
        assert_eq!(insertion.seated, 0);
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
