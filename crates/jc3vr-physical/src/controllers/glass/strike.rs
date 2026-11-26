//! The pure side of breaking glass: what strikes, how fast it moved between two ticks, the
//! segment its sweep is cast along, how much damage a strike does, and whether the pane shatters.

use glam::Vec3;
use jc3vr_input::frame::Hand;

use crate::controllers::glass::tuning::StrikeTuning;

/// What a strike is made with.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StrikerKind {
    /// The palm of a hand that holds nothing.
    Palm,
    /// The muzzle end of a gun a hand holds.
    Weapon,
}

/// A hand's palm or the gun in it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Striker {
    pub hand: Hand,
    pub kind: StrikerKind,
}

impl Striker {
    /// Every striker, in [`Striker::index`] order.
    pub const ALL: [Striker; STRIKERS] = [
        Striker::new(Hand::Left, StrikerKind::Palm),
        Striker::new(Hand::Left, StrikerKind::Weapon),
        Striker::new(Hand::Right, StrikerKind::Palm),
        Striker::new(Hand::Right, StrikerKind::Weapon),
    ];

    pub const fn new(hand: Hand, kind: StrikerKind) -> Self {
        Self { hand, kind }
    }

    /// The striker's slot in per-striker arrays.
    pub const fn index(self) -> usize {
        let kind = match self.kind {
            StrikerKind::Palm => 0,
            StrikerKind::Weapon => 1,
        };
        self.hand.index() * 2 + kind
    }

    pub const fn label(self) -> &'static str {
        match (self.hand, self.kind) {
            (Hand::Left, StrikerKind::Palm) => "left palm",
            (Hand::Left, StrikerKind::Weapon) => "left gun",
            (Hand::Right, StrikerKind::Palm) => "right palm",
            (Hand::Right, StrikerKind::Weapon) => "right gun",
        }
    }
}

/// The number of strikers: a palm and a gun per hand.
pub const STRIKERS: usize = 4;

/// Where a striker was at a tick, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Sample {
    /// Cockpit metres.
    pub at: Vec3,
    /// Seconds on the tick clock.
    pub time_s: f64,
}

/// A striker's move between two ticks.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Swing {
    /// Where it was, cockpit metres.
    pub from: Vec3,
    /// Where it is, cockpit metres.
    pub to: Vec3,
    /// How fast it moved, world metres per second.
    pub speed_mps: f32,
}

/// The move from `prev` to `cur`, at `world_scale` world metres per cockpit metre. `None` when the
/// samples are not in order or further apart than `max_gap_s`, so that a striker that was lost for
/// a while does not sweep across the gap.
pub fn swing(prev: Sample, cur: Sample, world_scale: f32, max_gap_s: f32) -> Option<Swing> {
    let dt = cur.time_s - prev.time_s;
    if !(dt > 0.0 && dt <= f64::from(max_gap_s)) {
        return None;
    }
    let scale = if world_scale.is_finite() && world_scale > 0.0 {
        world_scale
    } else {
        1.0
    };
    let speed_mps = ((cur.at - prev.at).length() * scale) / dt as f32;
    speed_mps.is_finite().then_some(Swing {
        from: prev.at,
        to: cur.at,
        speed_mps,
    })
}

/// The segment a sweep is cast along, world space.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Sweep {
    pub origin: Vec3,
    /// A unit vector.
    pub direction: Vec3,
    /// Metres.
    pub length: f32,
}

/// The sweep from `from` to `to`, world space, reaching `lead_m` past `to` so that a striker that
/// stops at the glass's face still meets it. `None` for a move too short to give a direction.
pub fn sweep(from: Vec3, to: Vec3, lead_m: f32) -> Option<Sweep> {
    let travel = to - from;
    let length = travel.length();
    if !(length.is_finite() && length > MIN_TRAVEL_M) {
        return None;
    }
    Some(Sweep {
        origin: from,
        direction: travel / length,
        length: length + lead_m.max(0.0),
    })
}

/// The damage a strike at `speed_mps` does to the pane at the point it meets, or `None` for a
/// strike too slow to count: from [`StrikeTuning::min_damage`] at
/// [`StrikeTuning::min_speed_mps`] up to [`StrikeTuning::max_damage`] at
/// [`StrikeTuning::full_speed_mps`] and beyond.
pub fn hit_damage(speed_mps: f32, tuning: &StrikeTuning) -> Option<f32> {
    if !(speed_mps.is_finite() && speed_mps >= tuning.min_speed_mps) {
        return None;
    }
    let span = tuning.full_speed_mps - tuning.min_speed_mps;
    let t = if span > 0.0 {
        ((speed_mps - tuning.min_speed_mps) / span).clamp(0.0, 1.0)
    } else {
        1.0
    };
    Some(tuning.min_damage + (tuning.max_damage - tuning.min_damage) * t)
}

/// The total strength of a pane's damage points, each counted up to one, as the window shader
/// draws them.
pub fn point_total(strengths: impl IntoIterator<Item = f32>) -> f32 {
    strengths
        .into_iter()
        .map(|s| {
            if s.is_finite() {
                s.clamp(0.0, 1.0)
            } else {
                0.0
            }
        })
        .sum()
}

/// A pane's damage after a strike, counting the damage the game's point list could not take.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Accumulated {
    /// The pane's damage: its points' total and what the strikes added past it.
    pub total: f32,
    /// The strikes' damage the point list did not take (a point already at full strength takes no
    /// more), so far.
    pub excess: f32,
}

/// The pane's damage after a strike of `damage` raised its points' total from `before` to `after`,
/// with `excess` of earlier strikes' damage not taken. Repeated blows on one spot saturate a
/// single point, so the damage they add past it is kept here, and they still bring the pane down.
pub fn accumulate(before: f32, after: f32, damage: f32, excess: f32) -> Accumulated {
    let taken = (after - before).max(0.0);
    let excess = excess.max(0.0) + (damage - taken).max(0.0);
    Accumulated {
        total: after + excess,
        excess,
    }
}

/// Why a pane shatters.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ShatterCause {
    /// One strike at [`StrikeTuning::shatter_speed_mps`] or faster.
    HardHit,
    /// The pane's damage reached [`StrikeTuning::shatter_total`].
    Accumulated,
}

impl ShatterCause {
    pub const fn label(self) -> &'static str {
        match self {
            ShatterCause::HardHit => "a hard hit",
            ShatterCause::Accumulated => "accumulated damage",
        }
    }
}

/// What a strike does to a pane.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It adds damage at the point it meets.
    Damage,
    /// It brings the whole pane down.
    Shatter(ShatterCause),
}

/// What a strike at `speed_mps` that leaves the pane at `total` damage does.
pub fn outcome(speed_mps: f32, total: f32, tuning: &StrikeTuning) -> Outcome {
    if speed_mps >= tuning.shatter_speed_mps {
        Outcome::Shatter(ShatterCause::HardHit)
    } else if total >= tuning.shatter_total {
        Outcome::Shatter(ShatterCause::Accumulated)
    } else {
        Outcome::Damage
    }
}

/// `world`, a world point, in the space of the row-major rigid transform `matrix` (rotation rows,
/// translation in the fourth row): the inverse the engine applies to a bullet's hit before it adds
/// window damage (`CMatrix4f::MultiplyReverse`).
pub fn world_to_local(matrix: &[f32; 16], world: Vec3) -> Vec3 {
    let offset = world - Vec3::new(matrix[12], matrix[13], matrix[14]);
    let row = |i: usize| Vec3::new(matrix[i * 4], matrix[i * 4 + 1], matrix[i * 4 + 2]);
    Vec3::new(offset.dot(row(0)), offset.dot(row(1)), offset.dot(row(2)))
}

/// The cooldown that keeps one swing from striking a pane on every tick it spends in it.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Limiter {
    last: [Option<f64>; STRIKERS],
}

impl Limiter {
    pub const fn new() -> Self {
        Self {
            last: [None; STRIKERS],
        }
    }

    /// Whether `striker` may strike at `now_s`.
    pub fn ready(&self, striker: Striker, now_s: f64, cooldown_s: f32) -> bool {
        self.last[striker.index()].is_none_or(|last| now_s - last >= f64::from(cooldown_s.max(0.0)))
    }

    /// Note that `striker` struck at `now_s`.
    pub fn note(&mut self, striker: Striker, now_s: f64) {
        self.last[striker.index()] = Some(now_s);
    }
}

/// The shortest move a sweep is cast for, metres.
const MIN_TRAVEL_M: f32 = 1e-4;

#[cfg(test)]
mod tests {
    use glam::Quat;
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::*;
    use crate::controllers::glass::tuning::GLASS;

    fn tuning() -> &'static StrikeTuning {
        &GLASS.get().strike
    }

    fn sample(at: Vec3, time_s: f64) -> Sample {
        Sample { at, time_s }
    }

    #[test]
    fn every_striker_has_its_own_slot() {
        for (i, striker) in Striker::ALL.into_iter().enumerate() {
            assert_eq!(striker.index(), i);
        }
    }

    #[test]
    fn a_swing_measures_world_speed() {
        let s = swing(
            sample(Vec3::ZERO, 1.0),
            sample(Vec3::new(0.1, 0.0, 0.0), 1.05),
            1.5,
            0.1,
        )
        .unwrap();
        assert!((s.speed_mps - 3.0).abs() < 1e-3, "{}", s.speed_mps);
        assert_eq!(s.from, Vec3::ZERO);
    }

    #[test_case(1.0, 1.0 ; "no time passed")]
    #[test_case(1.0, 0.9 ; "out of order")]
    #[test_case(1.0, 1.5 ; "a gap past the limit")]
    fn a_swing_needs_close_ordered_samples(prev: f64, cur: f64) {
        assert_eq!(
            swing(sample(Vec3::ZERO, prev), sample(Vec3::X, cur), 1.0, 0.1),
            None
        );
    }

    #[test]
    fn a_sweep_reaches_past_the_striker() {
        let s = sweep(Vec3::ZERO, Vec3::new(0.0, 0.0, -0.2), 0.05).unwrap();
        assert_eq!(s.direction, Vec3::NEG_Z);
        assert!((s.length - 0.25).abs() < 1e-6);
        assert_eq!(sweep(Vec3::ONE, Vec3::ONE, 0.05), None);
    }

    #[test]
    fn a_resting_hand_does_no_damage() {
        assert_eq!(hit_damage(0.3, tuning()), None);
        assert_eq!(hit_damage(f32::NAN, tuning()), None);
    }

    #[test]
    fn damage_grows_with_speed_and_stops_at_its_ceiling() {
        let low = hit_damage(tuning().min_speed_mps, tuning()).unwrap();
        let mid = hit_damage(
            (tuning().min_speed_mps + tuning().full_speed_mps) / 2.0,
            tuning(),
        )
        .unwrap();
        let full = hit_damage(tuning().full_speed_mps, tuning()).unwrap();
        let past = hit_damage(tuning().full_speed_mps * 3.0, tuning()).unwrap();
        assert_eq!(low, tuning().min_damage);
        assert!(low < mid && mid < full, "{low} {mid} {full}");
        assert_eq!(full, tuning().max_damage);
        assert_eq!(past, tuning().max_damage);
    }

    #[test]
    fn a_degenerate_speed_span_gives_full_damage() {
        let tuning = StrikeTuning {
            full_speed_mps: tuning().min_speed_mps,
            ..*tuning()
        };
        assert_eq!(
            hit_damage(tuning.min_speed_mps, &tuning),
            Some(tuning.max_damage)
        );
    }

    #[test]
    fn the_point_total_counts_each_point_up_to_one() {
        assert_eq!(point_total([0.2, 1.7, f32::NAN, -0.5]), 1.2);
    }

    #[test]
    fn blows_on_a_saturated_point_still_add_up() {
        // The point is already full: the list takes nothing, so the whole blow is excess.
        let first = accumulate(1.0, 1.0, 0.4, 0.0);
        assert_eq!(first.excess, 0.4);
        assert_eq!(first.total, 1.4);
        let second = accumulate(1.0, 1.0, 0.4, first.excess);
        assert!((second.total - 1.8).abs() < 1e-6);
        // A blow the list takes in full adds nothing to the excess.
        let fresh = accumulate(0.0, 0.3, 0.3, 0.0);
        assert_eq!(fresh.excess, 0.0);
        assert_eq!(fresh.total, 0.3);
    }

    #[test]
    fn repeated_punches_bring_the_pane_down() {
        let speed = tuning().min_speed_mps + 1.0;
        let damage = hit_damage(speed, tuning()).unwrap();
        let (mut points, mut excess) = (0.0_f32, 0.0);
        let mut blows = 0;
        loop {
            blows += 1;
            let after = (points + damage).min(1.0);
            let acc = accumulate(points, after, damage, excess);
            points = after;
            excess = acc.excess;
            if outcome(speed, acc.total, tuning()) != Outcome::Damage {
                break;
            }
            assert!(blows < 50, "the pane never fell");
        }
        assert!(
            blows > 2,
            "a light punch should not shatter at once ({blows})"
        );
    }

    #[test]
    fn a_hard_hit_shatters_at_once() {
        assert_eq!(
            outcome(tuning().shatter_speed_mps, 0.0, tuning()),
            Outcome::Shatter(ShatterCause::HardHit)
        );
        assert_eq!(
            outcome(tuning().min_speed_mps, tuning().shatter_total, tuning()),
            Outcome::Shatter(ShatterCause::Accumulated)
        );
        assert_eq!(
            outcome(tuning().min_speed_mps, 0.0, tuning()),
            Outcome::Damage
        );
    }

    #[test]
    fn world_to_local_inverts_a_rigid_transform() {
        let rotation = Quat::from_rotation_y(0.8) * Quat::from_rotation_x(-0.3);
        let translation = Vec3::new(4.0, -2.0, 7.5);
        // Row-major with row vectors: the rows are the rotated basis vectors.
        let x = rotation * Vec3::X;
        let y = rotation * Vec3::Y;
        let z = rotation * Vec3::Z;
        let matrix = [
            x.x,
            x.y,
            x.z,
            0.0,
            y.x,
            y.y,
            y.z,
            0.0,
            z.x,
            z.y,
            z.z,
            0.0,
            translation.x,
            translation.y,
            translation.z,
            1.0,
        ];
        let local = Vec3::new(0.3, -0.1, 1.2);
        let world = translation + local.x * x + local.y * y + local.z * z;
        assert!(world_to_local(&matrix, world).abs_diff_eq(local, 1e-5));
    }

    #[test]
    fn the_cooldown_holds_one_striker_only() {
        let mut limiter = Limiter::new();
        let palm = Striker::new(Hand::Left, StrikerKind::Palm);
        let gun = Striker::new(Hand::Right, StrikerKind::Weapon);
        assert!(limiter.ready(palm, 0.0, 0.2));
        limiter.note(palm, 0.0);
        assert!(!limiter.ready(palm, 0.1, 0.2));
        assert!(limiter.ready(gun, 0.1, 0.2));
        assert!(limiter.ready(palm, 0.25, 0.2));
    }
}
