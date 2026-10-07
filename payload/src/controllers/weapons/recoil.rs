//! The recoil offset: the plan of record's "Recoil and haptics"
//! (`docs/mod/input/controllers-and-roomscale.md`).
//!
//! Each hand that holds a gun carries an offset between its controller pose and the pose the hand,
//! the gun, and the gun's sight line are placed at: a pitch and a yaw of the barrel about the
//! controller's grip, and a shift back along the barrel ([`offset_motion`]). Every shot kicks the offset of the hand whose gun fired, by the
//! recoil the game adds for that shot ([`WeaponBase::m_RecoilPitch`] and
//! [`WeaponBase::m_RecoilYaw`], read across the shot by [`crate::controllers::weapons::fire`]), and
//! a damped spring returns it to zero. The offset is applied to the active hands
//! ([`crate::controllers::hands::active_hands`]), so the IK targets, the render-time late update,
//! the gun's sight line, and the overlay all carry it: the aim follows the kicked gun, and a player
//! who holds against the climb keeps the gun on target. The game's own scatter stays on top.
//!
//! While the off-hand holds a two-handed weapon's foregrip
//! ([`foregrip::two_hand_grip`]), the kick is reduced, the spring stiffens, and the off-hand moves
//! with the gun. Held in one hand, a long gun's kick is raised by its class's one-handed scale
//! ([`crate::controllers::weapons::heft`]).
//!
//! The spring runs on wall-clock time and is advanced lazily by whichever thread reads it: the
//! kicks arrive on the simulation thread, and the hands are read on the simulation, input, and
//! render threads.
//!
//! [`WeaponBase::m_RecoilPitch`]: jc3gi::equipment::weapon::WeaponBase::m_RecoilPitch
//! [`WeaponBase::m_RecoilYaw`]: jc3gi::equipment::weapon::WeaponBase::m_RecoilYaw

use std::time::{Duration, Instant};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::controllers::{
    frame::{Hand, Pose},
    hands::CockpitHand,
    weapons::{config::RecoilConfig, foregrip},
};

/// A damped spring per component, pulled toward zero.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct Spring {
    pub position: Vec3,
    pub velocity: Vec3,
}

impl Spring {
    /// Advance by `dt` seconds with the given stiffness (per second squared) and damping ratio, in
    /// sub-steps short enough to stay stable at any stiffness the settings allow.
    pub fn step(&mut self, dt: f32, stiffness: f32, damping_ratio: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let stiffness = stiffness.max(0.0);
        let damping = 2.0 * damping_ratio.max(0.0) * stiffness.sqrt();
        let steps = (dt / MAX_SUBSTEP).ceil().max(1.0) as u32;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let acceleration = -stiffness * self.position - damping * self.velocity;
            self.velocity += acceleration * h;
            self.position += self.velocity * h;
        }
        if !(self.position.is_finite() && self.velocity.is_finite()) {
            *self = Spring::default();
        }
    }

    /// Kick the spring so that, critically damped from rest, it would peak at `kick`: a velocity
    /// impulse of `kick × ω × e`, with `ω` the spring's natural frequency.
    pub fn kick(&mut self, kick: Vec3, stiffness: f32) {
        let omega = stiffness.max(0.0).sqrt();
        self.velocity += kick * omega * std::f32::consts::E;
    }

    /// Whether the spring is close enough to rest to drop.
    pub fn settled(&self) -> bool {
        self.position.length_squared() < SETTLE_EPS && self.velocity.length_squared() < SETTLE_EPS
    }
}

/// The kick of one shot as an offset: `(pitch, yaw, back)`, radians, radians, and metres.
/// `one_hand_scale` multiplies the kick of a gun held in one hand
/// ([`heft::one_hand_kick`](crate::controllers::weapons::heft::one_hand_kick)); the two-hand kick
/// scale takes over from it as the foregrip's `two_hand_weight` rises.
pub fn kick_offset(
    pitch: f32,
    yaw: f32,
    config: &RecoilConfig,
    two_hand_weight: f32,
    one_hand_scale: f32,
) -> Vec3 {
    let scale =
        config.kick_scale * lerp(one_hand_scale, config.two_hand_kick_scale, two_hand_weight);
    let (pitch, yaw) = (pitch * scale, yaw * scale);
    let kick = Vec3::new(pitch, yaw, pitch.abs() * config.kick_back_m_per_rad);
    if kick.is_finite() { kick } else { Vec3::ZERO }
}

/// The spring's stiffness under a two-hand grip of `weight`.
pub fn stiffness(config: &RecoilConfig, two_hand_weight: f32) -> f32 {
    config.spring_stiffness * lerp(1.0, config.two_hand_stiffness_scale, two_hand_weight)
}

/// Limit an offset's rotation to `max_angle_deg` on each axis.
pub fn clamp_offset(offset: Vec3, max_angle_deg: f32) -> Vec3 {
    let max = max_angle_deg.to_radians().max(0.0);
    Vec3::new(
        offset.x.clamp(-max, max),
        offset.y.clamp(-max, max),
        offset.z,
    )
}

/// The rigid motion an offset applies about the grip of `hand`: the pivot, the world rotation, and
/// the world translation. The gun's barrel is the wrist frame's fingers axis (its -Z), along which
/// the hand holds it; the controller's grip -Z runs up the handle. Pitch turns the barrel up about
/// the axis across it (the grip's palm axis, +X), yaw turns it about the axis up the handle, square
/// to the barrel (positive to the left), and the shift is back along the barrel.
pub fn offset_motion(hand: &CockpitHand, offset: Vec3) -> Motion {
    let grip = hand.grip;
    let barrel = (hand.wrist.orientation * Vec3::NEG_Z).normalize();
    let across = (grip.orientation * Vec3::X)
        .reject_from(barrel)
        .try_normalize()
        .unwrap_or_else(|| barrel.any_orthonormal_vector());
    let up = across.cross(barrel);
    let rotation = Quat::from_axis_angle(up, offset.y) * Quat::from_axis_angle(across, offset.x);
    Motion {
        pivot: grip.position,
        rotation: rotation.normalize(),
        translation: -barrel * offset.z,
    }
}

/// A rigid motion about a pivot.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Motion {
    pub pivot: Vec3,
    pub rotation: Quat,
    pub translation: Vec3,
}

impl Motion {
    /// This motion at `weight`, `0` doing nothing and `1` all of it.
    pub fn scaled(self, weight: f32) -> Motion {
        let weight = weight.clamp(0.0, 1.0);
        Motion {
            pivot: self.pivot,
            rotation: Quat::IDENTITY.slerp(self.rotation, weight),
            translation: self.translation * weight,
        }
    }

    pub fn pose(&self, p: Pose) -> Pose {
        Pose {
            position: self.pivot + self.rotation * (p.position - self.pivot) + self.translation,
            orientation: (self.rotation * p.orientation).normalize(),
        }
    }

    pub fn hand(&self, h: CockpitHand) -> CockpitHand {
        CockpitHand {
            wrist: self.pose(h.wrist),
            grip: self.pose(h.grip),
            aim: self.pose(h.aim),
        }
    }
}

/// Move the hands by their recoil offsets. `hands` are the cockpit-frame hands, already in any
/// two-handed pose, indexed by [`Hand::index`]. The off-hand of an engaged two-hand grip moves with
/// the holding hand.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], config: &RecoilConfig) {
    if !config.enabled {
        return;
    }
    let grip = foregrip::two_hand_grip();
    let offsets = advance(Instant::now(), config, grip);
    for hand in Hand::BOTH {
        let offset = clamp_offset(offsets[hand.index()], config.max_angle_deg);
        if offset == Vec3::ZERO {
            continue;
        }
        let Some(h) = hands[hand.index()] else {
            continue;
        };
        let motion = offset_motion(&h, offset);
        hands[hand.index()] = Some(motion.hand(h));
        if let Some(g) = grip.filter(|g| g.holder == hand)
            && let Some(off) = hands[hand.other().index()]
        {
            hands[hand.other().index()] = Some(motion.scaled(g.weight).hand(off));
        }
    }
}

/// Kick `hand`'s offset by a shot's recoil, `pitch` and `yaw` radians, with `one_hand_scale` the
/// one-handed multiplier ([`kick_offset`]). Returns the kick applied.
pub fn kick(hand: Hand, pitch: f32, yaw: f32, config: &RecoilConfig, one_hand_scale: f32) -> Vec3 {
    if !config.enabled {
        return Vec3::ZERO;
    }
    let grip = foregrip::two_hand_grip();
    let now = Instant::now();
    advance(now, config, grip);
    let weight = grip_weight(grip, hand);
    let kick = kick_offset(pitch, yaw, config, weight, one_hand_scale);
    let mut state = STATE.lock();
    let h = &mut state.hands[hand.index()];
    h.spring.kick(kick, stiffness(config, weight));
    h.kicks += 1;
    h.last_kick = kick;
    kick
}

/// Drop every offset at once, for when the controllers stop acting.
pub fn clear() {
    let mut state = STATE.lock();
    for h in &mut state.hands {
        h.spring = Spring::default();
    }
}

/// The recoil's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct RecoilStatus {
    /// Each hand's current offset: pitch and yaw in degrees and the shift back in metres.
    pub offsets: [Vec3; 2],
    /// Each hand's last kick, in the same units.
    pub last_kicks: [Vec3; 2],
    pub kicks: [u64; 2],
}

pub fn status() -> RecoilStatus {
    let state = STATE.lock();
    let degrees = |v: Vec3| Vec3::new(v.x.to_degrees(), v.y.to_degrees(), v.z);
    RecoilStatus {
        offsets: state.hands.map(|h| degrees(h.spring.position)),
        last_kicks: state.hands.map(|h| degrees(h.last_kick)),
        kicks: state.hands.map(|h| h.kicks),
    }
}

/// The longest sub-step the spring integrates in one go, seconds.
const MAX_SUBSTEP: f32 = 1.0 / 480.0;
/// A gap longer than this between reads settles the springs at once.
const MAX_GAP: Duration = Duration::from_millis(250);
const SETTLE_EPS: f32 = 1e-10;

#[derive(Debug, Copy, Clone, PartialEq)]
struct HandRecoil {
    spring: Spring,
    last_kick: Vec3,
    kicks: u64,
}

struct State {
    hands: [HandRecoil; 2],
    at: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    hands: [HandRecoil {
        spring: Spring {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
        },
        last_kick: Vec3::ZERO,
        kicks: 0,
    }; 2],
    at: None,
});

/// Advance the springs to `now` and return their offsets.
fn advance(now: Instant, config: &RecoilConfig, grip: Option<foregrip::TwoHandGrip>) -> [Vec3; 2] {
    let mut state = STATE.lock();
    let dt = state
        .at
        .map_or(Duration::ZERO, |at| now.saturating_duration_since(at));
    if state.at.is_none_or(|at| now > at) {
        state.at = Some(now);
    }
    for hand in Hand::BOTH {
        let h = &mut state.hands[hand.index()];
        if dt > MAX_GAP {
            h.spring = Spring::default();
        } else {
            let weight = grip_weight(grip, hand);
            h.spring.step(
                dt.as_secs_f32(),
                stiffness(config, weight),
                config.spring_damping_ratio,
            );
        }
        if h.spring.settled() {
            h.spring = Spring::default();
        }
    }
    state.hands.map(|h| h.spring.position)
}

fn grip_weight(grip: Option<foregrip::TwoHandGrip>, hand: Hand) -> f32 {
    grip.filter(|g| g.holder == hand).map_or(0.0, |g| g.weight)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::controllers::{config::GripOffset, hands::pose as hand_pose};

    fn config() -> RecoilConfig {
        RecoilConfig::new()
    }

    #[test]
    fn a_kick_peaks_near_its_size_and_returns_to_rest() {
        let config = RecoilConfig {
            spring_damping_ratio: 1.0,
            ..config()
        };
        let mut spring = Spring::default();
        let kick = Vec3::new(0.05, 0.0, 0.0);
        spring.kick(kick, config.spring_stiffness);
        let mut peak = 0.0f32;
        for _ in 0..600 {
            spring.step(1.0 / 600.0, config.spring_stiffness, 1.0);
            peak = peak.max(spring.position.x);
        }
        assert!((peak - 0.05).abs() < 0.005, "peak {peak}");
        assert!(spring.position.x.abs() < 0.002);
        for _ in 0..10 {
            spring.step(0.1, config.spring_stiffness, 1.0);
        }
        assert!(spring.settled());
    }

    #[test]
    fn a_stiffer_spring_returns_sooner() {
        let time_to_settle = |stiffness: f32| {
            let mut spring = Spring::default();
            spring.kick(Vec3::X * 0.05, stiffness);
            let mut t = 0.0;
            while t < 0.05 || spring.position.x.abs() > 0.001 {
                spring.step(0.001, stiffness, 1.0);
                t += 0.001;
            }
            t
        };
        assert!(time_to_settle(800.0) < time_to_settle(400.0));
    }

    #[test]
    fn a_large_step_stays_stable() {
        let mut spring = Spring::default();
        spring.kick(Vec3::ONE, 2000.0);
        spring.step(0.2, 2000.0 * 5.0, 0.1);
        assert!(spring.position.is_finite());
        assert!(spring.position.length() < 10.0);
    }

    #[test]
    fn the_two_hand_grip_reduces_the_kick_and_stiffens_the_spring() {
        let c = config();
        let one = kick_offset(0.04, 0.01, &c, 0.0, 1.0);
        let two = kick_offset(0.04, 0.01, &c, 1.0, 1.0);
        assert!((two.x - one.x * c.two_hand_kick_scale).abs() < 1e-6);
        assert!(one.z > 0.0 && two.z < one.z);
        // A heavy gun kicks harder in one hand, and braced it kicks as any braced gun does.
        let heavy = kick_offset(0.04, 0.01, &c, 0.0, 1.8);
        assert!((heavy.x - one.x * 1.8).abs() < 1e-6);
        let braced = kick_offset(0.04, 0.01, &c, 1.0, 1.8);
        assert!((braced.x - two.x).abs() < 1e-6);
        assert_eq!(
            stiffness(&c, 1.0),
            c.spring_stiffness * c.two_hand_stiffness_scale
        );
        assert_eq!(stiffness(&c, 0.0), c.spring_stiffness);
    }

    const OFFSET: GripOffset = GripOffset::INDEX_BEST_GUESS;

    /// `hand` holding a pistol grip at `position`, the barrel pitched up by `pitch_up` and turned
    /// left by `yaw` (radians): an OpenXR grip with its -Z up the handle, tilted back from the level
    /// controller by the grip offset's pitch so that the fingers, and the barrel along them, point
    /// forward.
    fn pistol(hand: Hand, position: Vec3, pitch_up: f32, yaw: f32) -> CockpitHand {
        let tilt = OFFSET.rotation_deg.x.to_radians();
        let grip = Pose {
            position,
            orientation: Quat::from_rotation_y(yaw)
                * Quat::from_rotation_x(pitch_up)
                * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - tilt),
        };
        hand_pose::cockpit_hand(hand, grip, grip, &OFFSET, None)
    }

    /// The barrel of a held gun: the wrist frame's fingers axis.
    fn barrel(h: &CockpitHand) -> Vec3 {
        h.wrist.orientation * Vec3::NEG_Z
    }

    fn handle(h: &CockpitHand) -> Vec3 {
        h.grip.orientation * Vec3::NEG_Z
    }

    fn heading(v: Vec3) -> Vec2 {
        Vec2::new(v.x, v.z).normalize()
    }

    #[test]
    fn a_pitch_kick_raises_the_barrel_and_pushes_the_hand_back_along_it() {
        for hand in Hand::BOTH {
            for (pitch, yaw) in [(0.0, 0.0), (0.4, 0.9), (-0.3, -2.0)] {
                let held = pistol(hand, Vec3::new(0.2, -0.3, -0.4), pitch, yaw);
                let moved = offset_motion(&held, Vec3::new(0.1, 0.0, 0.02)).hand(held);
                let (before, after) = (barrel(&held), barrel(&moved));
                assert!(after.y > before.y + 0.05, "{hand:?} {pitch} {yaw}");
                // Up in the barrel's own vertical plane, without rolling the handle off it.
                assert!(heading(before).distance(heading(after)) < 1e-4);
                let across = Vec3::Y.cross(before).normalize();
                assert!(handle(&moved).dot(across).abs() < 1e-4);
                // Back along the barrel, not down the handle.
                let shift = moved.grip.position - held.grip.position;
                assert!(shift.dot(-before) > 0.019, "{hand:?} {shift}");
                assert!(shift.normalize().dot(-before) > 1.0 - 1e-5);
            }
        }
        let held = pistol(Hand::Right, Vec3::ZERO, 0.0, 0.0);
        let none = offset_motion(&held, Vec3::ZERO).hand(held);
        assert!(none.aim.position.distance(held.aim.position) < 1e-6);
    }

    #[test]
    fn a_yaw_kick_swings_the_barrel_about_the_handle_without_rolling() {
        let held = pistol(Hand::Right, Vec3::new(0.2, -0.3, -0.4), 0.0, 0.5);
        let moved = offset_motion(&held, Vec3::new(0.0, 0.1, 0.0)).hand(held);
        let (before, after) = (barrel(&held), barrel(&moved));
        // The barrel turns left in the horizontal plane about the upright axis through the handle,
        // not about the barrel: the palm axis stays level, so the gun does not roll.
        assert!((after.y - before.y).abs() < 1e-5);
        assert!(
            heading(before).perp_dot(heading(after)) < -0.09,
            "{before} {after}"
        );
        let palm = |h: &CockpitHand| h.grip.orientation * Vec3::X;
        assert!(palm(&held).y.abs() < 1e-5);
        assert!(palm(&moved).y.abs() < 1e-5, "{}", palm(&moved));
        assert!((handle(&moved).y - handle(&held).y).abs() < 1e-5);
    }

    #[test]
    fn a_scaled_motion_goes_part_of_the_way() {
        let held = pistol(Hand::Right, Vec3::ZERO, 0.0, 0.0);
        let motion = offset_motion(&held, Vec3::new(0.2, 0.0, 0.1));
        let half = motion.scaled(0.5);
        assert!((half.translation.length() - 0.05).abs() < 1e-6);
        let (_, angle) = half.rotation.to_axis_angle();
        assert!((angle - 0.1).abs() < 1e-4);
        assert_eq!(motion.scaled(0.0).rotation, Quat::IDENTITY);
    }

    #[test]
    fn the_offset_is_clamped_to_the_largest_angle() {
        let clamped = clamp_offset(Vec3::new(1.0, -1.0, 0.3), 10.0);
        assert!((clamped.x - 10f32.to_radians()).abs() < 1e-6);
        assert!((clamped.y + 10f32.to_radians()).abs() < 1e-6);
        assert_eq!(clamped.z, 0.3);
    }
}
