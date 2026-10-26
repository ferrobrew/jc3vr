//! The weight of a long gun. Held in one hand, it sags, trails the hand, and sways, it kicks
//! harder, and it shoots wider. Braced on the foregrip, most of that goes, but the gun keeps a
//! weight of its own: it lags a quick swing of the hands a little on a critically damped spring and
//! settles on the aim without overshooting, and it sags a little.
//!
//! How much of each a weapon gets is its class's tuning ([`HeftClass`], from the weapon's class,
//! [`WeaponBase::m_Class`]): none for the pistols and revolvers, in either hold; one-handed, none
//! for the SMGs either, a little for the rifles, and a lot for the launchers and the other heavy
//! weapons; braced, least for the SMGs and most for the heavy weapons. The input tick publishes the
//! gun in each hand ([`observe`]), the wielded weapon and a weapon armed beside it alike, and each
//! hand's gun has a weight, a spring, and a freeze of its own; everything else reads that.
//!
//! - The pose: [`apply`] turns the holding hand's poses about the controller's grip, on the active
//!   hands every hand subsystem starts from ([`crate::controllers::hands::active_hands`]), so the
//!   IK, the late update, the gun's sight line, and the overlay all carry it, and a shot goes where
//!   the drooping gun points. The barrel is the wrist frame's fingers axis, not the controller's
//!   grip -Z, which runs up the handle. The gun pitches down about the horizontal axis across its
//!   barrel, by the sag plus the one-handed reach sag as the hand moves away from the head, scaled
//!   by how level the barrel is; held in one hand it sways in pitch about that axis and in yaw
//!   about the vertical, on two incommensurate sine waves; and the result is
//!   followed on a damped rotational spring, so the gun trails a fast turn and settles after it.
//!   The one-handed and the braced weights blend by how far the foregrip is engaged ([`pose::Weight`]).
//!   While the foregrip is held, the off-hand is turned with the gun, so the gun drives both hands
//!   and they stay on it; the braced weight's angle is capped small, so the hands never stray far
//!   from the controllers.
//! - The kick: the recoil multiplies a one-handed shot's kick by the class's kick scale
//!   ([`one_hand_kick`]); the foregrip's own two-hand kick scale takes over as the grip engages.
//! - The spread: a detour on [`WeaponBase::GetCurrentScatterValue`] multiplies the scatter the game
//!   gives the local player's shot by the class's spread scale ([`spread_scale`]).
//!
//! The off-hand's foregrip weight ([`foregrip::two_hand_grip`]) scales the one-handed weight down
//! to nothing, and the braced weight up from nothing.
//! The spring runs on wall-clock time and is advanced lazily by whichever thread reads the hands, as
//! the recoil's is.
//!
//! [`WeaponBase::m_Class`]: jc3gi::equipment::weapon::WeaponBase::m_Class
//! [`WeaponBase::GetCurrentScatterValue`]: jc3gi::equipment::weapon::WeaponBase::GetCurrentScatterValue

mod class;
mod pose;
#[cfg(test)]
mod test_support;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3};
use jc3gi::equipment::weapon::WeaponBase;
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
#[cfg(test)]
pub(crate) use test_support::TestWeight;

pub use self::class::HeftClass;
use self::pose::{Weight, clamp_angle, sway, unbraced_share};
use crate::{
    config::Config,
    controllers::{
        frame::Hand,
        hands::CockpitHand,
        scheme,
        weapons::{config::HeftConfig, foregrip, recoil::Motion},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_CURRENT_SCATTER_VALUE_BINDER)
}

/// A weapon the weight acts on, as the input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HeftTarget {
    /// The hand holding it.
    pub hand: Hand,
    pub class: HeftClass,
    /// The weapon's address.
    pub weapon: usize,
}

/// The weight's readout for one hand.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HandHeftStatus {
    pub target: Option<HeftTarget>,
    /// The unbraced share, `0..=1`: one in one hand, zero braced on the foregrip.
    pub share: f32,
    /// The hand's reach, `0..=1`, between the near and far distances.
    pub reach: f32,
    /// The sag asked for, degrees.
    pub sag_deg: f32,
    /// The sway asked for, pitch and yaw, degrees.
    pub sway_deg: Vec2,
    /// How far the shown gun trails the sagged and swayed pose, degrees.
    pub lag_deg: f32,
    /// The whole turn from the controller to the shown gun, degrees.
    pub offset_deg: f32,
}

/// The weight's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeftStatus {
    /// Each hand's gun and its weight, by [`Hand::index`].
    pub hands: [HandHeftStatus; 2],
    /// Shots whose scatter was widened, since the payload loaded.
    pub widened_shots: u64,
}

pub fn status() -> HeftStatus {
    STATE.lock().status
}

/// Publish the weapon the weight acts on in each hand for this input tick, by [`Hand::index`], or
/// `None` for a hand to let go of its weapon. A target is taken only in its own hand's place.
pub fn observe(targets: [Option<HeftTarget>; 2]) {
    let now = Instant::now();
    let mut guard = STATE.lock();
    let state = &mut *guard;
    for hand in Hand::BOTH {
        let target = targets[hand.index()].filter(|t| t.hand == hand);
        let h = &mut state.hands[hand.index()];
        if h.target.map(|(t, _)| t) != target {
            h.rest();
        }
        h.target = target.map(|t| (t, now));
        state.status.hands[hand.index()].target = target;
    }
}

/// Freeze the weight of the gun in each hand, by [`Hand::index`], or let it go again. While frozen,
/// a gun keeps the turn the weight had put on it relative to the controller's grip, without
/// lagging, sagging further, or swaying, so a well a round is being carried to, or a foregrip a
/// hand is closing on, moves rigidly with the hand. Published by the input tick.
pub fn freeze(hands: [bool; 2]) {
    let mut state = STATE.lock();
    for hand in Hand::BOTH {
        state.hands[hand.index()].freeze = hands[hand.index()];
    }
}

/// Turn each gun hand's poses by its gun's weight. `hands` are the cockpit-frame hands, already in
/// any two-handed pose, indexed by [`Hand::index`]; `head` is the head's cockpit position. The
/// off-hand of an engaged foregrip moves with the gun by the grip's weight, so the gun carries it.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Vec3, config: &HeftConfig) {
    let now = Instant::now();
    let grip = foregrip::two_hand_grip();
    let mut guard = STATE.lock();
    let state = &mut *guard;
    let dt = advance_clock(&mut state.at, now);
    let acting = config.enabled || config.braced.enabled;
    let t = epoch().elapsed().as_secs_f32();
    let mut motions = [None; 2];
    for hand in Hand::BOTH {
        let h = &mut state.hands[hand.index()];
        let target = h
            .target
            .filter(|(_, at)| acting && now.saturating_duration_since(*at) < MAX_AGE)
            .map(|(t, _)| t);
        let Some(target) = target else {
            h.rest();
            state.status.hands[hand.index()] = HandHeftStatus::default();
            continue;
        };
        let Some(held) = hands[hand.index()] else {
            h.rest();
            continue;
        };
        let share = unbraced_share(grip, hand);
        let (rotation, status) = h.weigh(&HandInputs {
            hand: &held,
            head,
            weight: &Weight::of(target.class, config, share),
            config,
            dt,
            t,
        });
        state.status.hands[hand.index()] = HandHeftStatus {
            target: Some(target),
            share,
            ..status
        };
        if rotation != Quat::IDENTITY {
            motions[hand.index()] = Some(Motion {
                pivot: held.grip.position,
                rotation,
                translation: Vec3::ZERO,
            });
        }
    }
    drop(guard);
    for hand in Hand::BOTH {
        let (Some(motion), Some(held)) = (motions[hand.index()], hands[hand.index()]) else {
            continue;
        };
        hands[hand.index()] = Some(motion.hand(held));
        if let Some(g) = grip.filter(|g| g.holder == hand)
            && let Some(off) = hands[hand.other().index()]
        {
            hands[hand.other().index()] = Some(motion.scaled(g.weight).hand(off));
        }
    }
}

/// The multiplier on a shot's kick for the gun in `hand` held in one hand: the class's kick scale
/// for a gun the weight acts on in that hand, one otherwise. The recoil blends it toward its own
/// two-hand kick scale by the foregrip's weight.
pub fn one_hand_kick(hand: Hand) -> f32 {
    let config = Config::lock_query(|c| c.controllers.weapons.heft);
    let target = STATE.lock().hands[hand.index()].target.map(|(t, _)| t);
    match target {
        Some(t) if config.enabled => t.class.tuning(&config).kick_scale,
        _ => 1.0,
    }
}

/// The multiplier on the scatter of a shot from `weapon`: the class's spread scale, scaled by the
/// unbraced share, for a weapon the weight acts on in either hand, one otherwise.
pub fn spread_scale(weapon: usize) -> f32 {
    let config = Config::lock_query(|c| c.controllers.weapons.heft);
    if !(config.enabled && config.spread) {
        return 1.0;
    }
    let target = STATE
        .lock()
        .hands
        .iter()
        .find_map(|h| h.target.map(|(t, _)| t).filter(|t| t.weapon == weapon));
    let Some(target) = target else {
        return 1.0;
    };
    let share = unbraced_share(foregrip::two_hand_grip(), target.hand);
    lerp(1.0, target.class.tuning(&config).spread_scale, share)
}

/// How long a published weapon stays in effect without a new input tick.
const MAX_AGE: Duration = Duration::from_millis(500);
/// A gap longer than this between reads puts the spring at rest on its target.
const MAX_GAP: Duration = Duration::from_millis(250);

/// One hand's weight: its gun, its spring, and its freeze.
#[derive(Debug, Copy, Clone, PartialEq)]
struct HandHeft {
    target: Option<(HeftTarget, Instant)>,
    spring: Option<pose::RotSpring>,
    /// The turn the weight last put on the gun, relative to the controller's grip.
    offset: Quat,
    /// Whether the weight is frozen ([`freeze`]), and the turn it holds while it is.
    freeze: bool,
    frozen: Option<Quat>,
}

/// What one hand's weight is worked out from.
struct HandInputs<'a> {
    hand: &'a CockpitHand,
    head: Vec3,
    weight: &'a Weight,
    config: &'a HeftConfig,
    /// The seconds since the last read.
    dt: f32,
    /// The sway's clock, seconds.
    t: f32,
}

impl HandHeft {
    const IDLE: HandHeft = HandHeft {
        target: None,
        spring: None,
        offset: Quat::IDENTITY,
        freeze: false,
        frozen: None,
    };

    /// Put the gun at rest on the controller: no spring, no turn, no freeze held.
    fn rest(&mut self) {
        self.spring = None;
        self.offset = Quat::IDENTITY;
        self.frozen = None;
    }

    /// The turn the weight puts on the gun in `inputs.hand` relative to the controller's grip, and
    /// the readout of it (its target and share left for the caller).
    fn weigh(&mut self, inputs: &HandInputs) -> (Quat, HandHeftStatus) {
        let HandInputs {
            hand: h,
            head,
            weight,
            config,
            dt,
            t,
        } = *inputs;
        let pose = pose::weighted_pose(
            h,
            head,
            weight,
            config,
            sway(t, config.sway_hz, weight.sway_deg.to_radians()),
        );
        let rotation = if self.freeze {
            // The gun keeps the turn it had on the hand, and the spring rests where the gun is
            // shown, so the gun follows on from there once the freeze ends.
            let held = *self.frozen.get_or_insert(self.offset);
            self.spring = Some(pose::RotSpring::at(held * h.grip.orientation));
            held
        } else {
            self.frozen = None;
            let shown = if weight.stiffness > 0.0 && dt <= MAX_GAP.as_secs_f32() {
                let spring = self
                    .spring
                    .get_or_insert(pose::RotSpring::at(pose.orientation));
                spring.step(pose.orientation, dt, weight.stiffness, weight.damping_ratio);
                spring.orientation
            } else {
                self.spring = Some(pose::RotSpring::at(pose.orientation));
                pose.orientation
            };
            clamp_angle(
                shown * h.grip.orientation.inverse(),
                weight.max_angle_deg.to_radians(),
            )
        };
        self.offset = rotation;
        let shown = rotation * h.grip.orientation;
        let sway_shown = pose.sway * weight.sway_share * pose.sway_reach;
        let status = HandHeftStatus {
            target: None,
            share: 0.0,
            reach: pose.reach,
            sag_deg: pose.sag.to_degrees(),
            sway_deg: Vec2::new(sway_shown.x.to_degrees(), sway_shown.y.to_degrees()),
            lag_deg: angle(shown * pose.orientation.inverse()).to_degrees(),
            offset_deg: angle(rotation).to_degrees(),
        };
        (rotation, status)
    }
}

struct State {
    hands: [HandHeft; 2],
    at: Option<Instant>,
    status: HeftStatus,
}

const IDLE_STATUS: HandHeftStatus = HandHeftStatus {
    target: None,
    share: 0.0,
    reach: 0.0,
    sag_deg: 0.0,
    sway_deg: Vec2::ZERO,
    lag_deg: 0.0,
    offset_deg: 0.0,
};

static STATE: Mutex<State> = Mutex::new(State {
    hands: [HandHeft::IDLE; 2],
    at: None,
    status: HeftStatus {
        hands: [IDLE_STATUS; 2],
        widened_shots: 0,
    },
});

/// The seconds from the clock `at` to `now`, moving the clock to `now` unless it is already past
/// it. The hands are read on several threads, each taking its time before the lock, so a reader
/// can arrive with an older time than the last one; moving the clock back would count the time
/// between the two again on the next read.
fn advance_clock(at: &mut Option<Instant>, now: Instant) -> f32 {
    let dt = at.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
    if at.is_none_or(|at| now > at) {
        *at = Some(now);
    }
    dt
}

/// The sway's clock.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// The same rotation with a non-negative scalar part, the shorter way round.
pub(super) fn shortest(q: Quat) -> Quat {
    if q.w < 0.0 { -q } else { q }
}

fn angle(q: Quat) -> f32 {
    shortest(q).to_axis_angle().1
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// The scatter the game gives a shot, multiplied by the one-handed spread for the local player's
/// weighted weapon.
#[detour(address = jc3gi::equipment::weapon::WeaponBase::GetCurrentScatterValue_ADDRESS)]
fn get_current_scatter_value(weapon: *const WeaponBase) -> f32 {
    let scatter = GET_CURRENT_SCATTER_VALUE.get().unwrap().call(weapon);
    // Every character's shots read their scatter here on the simulation's worker threads, so the
    // local player's are picked out before any lock.
    let local = unsafe { weapon.as_ref() }.is_some_and(|w| w.m_IsUserLocalPlayer);
    if !local || !scheme::is_controllers() {
        return scatter;
    }
    let scale = spread_scale(weapon as usize);
    if scale == 1.0 || !scale.is_finite() {
        return scatter;
    }
    STATE.lock().status.widened_shots += 1;
    scatter * scale
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use glam::{Quat, Vec2, Vec3};
    use jc3gi::equipment::weapon::WeaponClass;

    use super::{HeftClass, HeftTarget, advance_clock, angle, apply, clamp_angle, freeze, observe};
    use crate::controllers::{
        config::GripOffset,
        frame::{Hand, Pose},
        hands::{CockpitHand, pose as hand_pose},
        weapons::{
            config::{BracedTuning, HeftConfig, HeftTuning},
            foregrip,
            heft::pose::{RotSpring, Weight, WeightedPose, sway, unbraced_share, weighted_pose},
            regions::GunRegion,
        },
    };

    fn config() -> HeftConfig {
        HeftConfig::new()
    }

    const OFFSET: GripOffset = GripOffset::INDEX_BEST_GUESS;

    /// A right hand holding a pistol grip at `position`, the barrel pitched up by `pitch_up` and
    /// turned left by `yaw` (radians): an OpenXR grip with its -Z up the handle, tilted back from
    /// the level controller by the grip offset's pitch so that the fingers, and the barrel along
    /// them, point forward.
    fn pistol(position: Vec3, pitch_up: f32, yaw: f32) -> CockpitHand {
        let tilt = OFFSET.rotation_deg.x.to_radians();
        let orientation = Quat::from_rotation_y(yaw)
            * Quat::from_rotation_x(pitch_up)
            * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - tilt);
        let grip = Pose {
            position,
            orientation,
        };
        hand_pose::cockpit_hand(Hand::Right, grip, grip, &OFFSET, None)
    }

    fn level(position: Vec3) -> CockpitHand {
        pistol(position, 0.0, 0.0)
    }

    /// The barrel of a gun held by a grip turned to `grip`: the wrist frame's fingers axis.
    fn barrel(grip: Quat) -> Vec3 {
        grip * hand_pose::grip_to_wrist(Hand::Right, &OFFSET).orientation * Vec3::NEG_Z
    }

    /// The axis up the handle of a gun held by a grip turned to `grip`.
    fn handle(grip: Quat) -> Vec3 {
        grip * Vec3::NEG_Z
    }

    /// The one-handed pose of a gun with `tuning` at the unbraced `share`, with no braced weight.
    fn pose_of(hand: &CockpitHand, tuning: &HeftTuning, share: f32, sway: Vec2) -> WeightedPose {
        let c = config();
        let weight = Weight::blend(tuning, &BracedTuning::NONE, share, &c);
        weighted_pose(hand, Vec3::ZERO, &weight, &c, sway)
    }

    #[test]
    fn the_test_grip_points_the_barrel_forward_and_the_handle_up() {
        let hand = level(Vec3::ZERO);
        assert!(barrel(hand.grip.orientation).distance(Vec3::NEG_Z) < 1e-5);
        assert!(handle(hand.grip.orientation).y > 0.9);
    }

    const FRAME_S: f32 = 1.0 / 90.0;

    /// The spring's lag behind a target turning about +Y at `speed` radians per second for
    /// `seconds`, then the largest overshoot past the target once it stops, both radians.
    fn swing(weight: &Weight, speed: f32, seconds: f32) -> (f32, f32) {
        let mut spring = RotSpring::at(Quat::IDENTITY);
        let mut yaw = 0.0;
        let frames = (seconds / FRAME_S) as usize;
        for _ in 0..frames {
            yaw += speed * FRAME_S;
            spring.step(
                Quat::from_rotation_y(yaw),
                FRAME_S,
                weight.stiffness,
                weight.damping_ratio,
            );
        }
        let lag = spring.orientation.angle_between(Quat::from_rotation_y(yaw));
        let target = Quat::from_rotation_y(yaw);
        let mut overshoot: f32 = 0.0;
        for _ in 0..90 {
            spring.step(target, FRAME_S, weight.stiffness, weight.damping_ratio);
            // Past the target is further round in the swing's direction.
            let past = (spring.orientation * target.inverse()).to_scaled_axis().y;
            overshoot = overshoot.max(past);
        }
        assert!(spring.orientation.angle_between(target) < 1e-3);
        (lag, overshoot)
    }

    fn braced(class: HeftClass) -> Weight {
        Weight::of(class, &config(), 0.0)
    }

    #[test]
    fn weapon_classes_map_to_weight_classes() {
        let class = |c: WeaponClass, two| HeftClass::of(c as i32, two);
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_SMG, false),
            HeftClass::Smg
        );
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_PISTOL, false),
            HeftClass::Sidearm
        );
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE, true),
            HeftClass::Rifle
        );
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_RPG, true),
            HeftClass::Heavy
        );
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_MINIGUN, true),
            HeftClass::Heavy
        );
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_NONE, true),
            HeftClass::Rifle
        );
        assert_eq!(HeftClass::of(-7, false), HeftClass::Sidearm);
    }

    #[test]
    fn sidearms_carry_no_weight() {
        let c = config();
        let hand = level(Vec3::new(0.0, -0.3, -0.6));
        let pose = pose_of(&hand, &c.sidearm, 1.0, Vec2::ZERO);
        assert!(pose.orientation.angle_between(hand.grip.orientation) < 1e-5);
        assert_eq!(c.sidearm.kick_scale, 1.0);
        assert_eq!(c.sidearm.spread_scale, 1.0);
        // In either hold: a pistol braced in two hands carries nothing either.
        for share in [0.0, 0.5, 1.0] {
            let weight = Weight::of(HeftClass::Sidearm, &c, share);
            assert_eq!((weight.sag_deg, weight.stiffness), (0.0, 0.0), "{share}");
        }
    }

    #[test]
    fn a_heavy_gun_sags_down_and_more_at_reach() {
        let c = config();
        let near = pose_of(
            &level(Vec3::new(0.0, -0.2, -0.1)),
            &c.heavy,
            1.0,
            Vec2::ZERO,
        );
        let far = pose_of(
            &level(Vec3::new(0.0, -0.2, -0.7)),
            &c.heavy,
            1.0,
            Vec2::ZERO,
        );
        assert!(barrel(near.orientation).y < -0.1, "{near:?}");
        assert!(barrel(far.orientation).y < barrel(near.orientation).y);
        assert!((near.sag.to_degrees() - c.heavy.sag_deg).abs() < 0.5);
        assert!((far.sag.to_degrees() - c.heavy.sag_deg - c.heavy.reach_sag_deg).abs() < 0.5);
        // The sag pitches the barrel only: it stays in its vertical plane.
        assert!(barrel(far.orientation).x.abs() < 1e-5);
    }

    #[test]
    fn a_gun_aimed_up_or_down_or_to_the_side_still_sags_down() {
        let c = config();
        for (pitch_deg, yaw) in [(40.0f32, 0.0), (-35.0, 0.0), (25.0, 0.9), (0.0, -2.1)] {
            let hand = pistol(Vec3::new(0.1, -0.2, -0.6), pitch_deg.to_radians(), yaw);
            let before = barrel(hand.grip.orientation);
            let pose = pose_of(&hand, &c.heavy, 1.0, Vec2::ZERO);
            let after = barrel(pose.orientation);
            assert!(
                after.y < before.y - 0.05,
                "{pitch_deg} {yaw}: {before} {after}"
            );
            // Down in the barrel's own vertical plane: the heading does not change.
            let heading = |v: Vec3| Vec2::new(v.x, v.z).normalize();
            assert!(
                heading(before).distance(heading(after)) < 1e-4,
                "{pitch_deg} {yaw}"
            );
            // And the handle keeps its side: the sag does not roll the gun.
            let across = Vec3::Y.cross(before).normalize();
            let roll = |grip: Quat| handle(grip).dot(across);
            assert!(roll(hand.grip.orientation).abs() < 1e-4);
            assert!(roll(pose.orientation).abs() < 1e-4, "{pitch_deg} {yaw}");
        }
    }

    #[test]
    fn the_sway_pitches_and_yaws_without_rolling() {
        let c = config();
        let hand = pistol(Vec3::new(0.0, -0.2, -0.6), 0.2, 0.7);
        let weight = Weight::blend(&HeftTuning::NONE, &BracedTuning::NONE, 1.0, &c);
        let before = barrel(hand.grip.orientation);
        let up_before = handle(hand.grip.orientation);
        // A yaw sway swings the barrel about the vertical: its elevation and the handle's stay.
        let yawed = weighted_pose(&hand, Vec3::ZERO, &weight, &c, Vec2::new(0.0, 0.05));
        let after = barrel(yawed.orientation);
        assert!((after.y - before.y).abs() < 1e-5, "{before} {after}");
        assert!(after.angle_between(before) > 0.02);
        assert!((handle(yawed.orientation).y - up_before.y).abs() < 1e-5);
        // A pitch sway moves the barrel in its vertical plane.
        let pitched = weighted_pose(&hand, Vec3::ZERO, &weight, &c, Vec2::new(0.05, 0.0));
        let after = barrel(pitched.orientation);
        assert!((after.y - before.y).abs() > 0.02, "{before} {after}");
        let heading = |v: Vec3| Vec2::new(v.x, v.z).normalize();
        assert!(heading(before).distance(heading(after)) < 1e-4);
    }

    #[test]
    fn a_rifle_sags_less_than_a_launcher() {
        let c = config();
        let hand = level(Vec3::new(0.0, -0.2, -0.5));
        let rifle = pose_of(&hand, &c.rifle, 1.0, Vec2::ZERO);
        let heavy = pose_of(&hand, &c.heavy, 1.0, Vec2::ZERO);
        assert!(rifle.sag > 0.0 && rifle.sag < heavy.sag);
    }

    #[test]
    fn bracing_removes_the_weight() {
        let c = config();
        let hand = level(Vec3::new(0.0, -0.2, -0.6));
        let braced = pose_of(&hand, &c.heavy, 0.0, Vec2::new(0.1, 0.1));
        assert!(braced.orientation.angle_between(hand.grip.orientation) < 1e-5);
        assert_eq!(
            Weight::blend(&c.heavy, &BracedTuning::NONE, 0.0, &c).stiffness,
            0.0
        );
        let grip_on = |weight| {
            Some(foregrip::TwoHandGrip {
                holder: Hand::Right,
                weight,
                region: GunRegion::Handguard,
            })
        };
        assert_eq!(unbraced_share(grip_on(1.0), Hand::Right), 0.0);
        assert_eq!(unbraced_share(grip_on(0.25), Hand::Right), 0.75);
        assert_eq!(unbraced_share(grip_on(1.0), Hand::Left), 1.0);
        assert_eq!(unbraced_share(None, Hand::Right), 1.0);
    }

    #[test]
    fn a_vertical_barrel_does_not_sag() {
        let c = config();
        for pitch in [std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2] {
            let hand = pistol(Vec3::new(0.0, -0.2, -0.6), pitch, 0.3);
            assert!(barrel(hand.grip.orientation).y.abs() > 1.0 - 1e-5);
            let pose = pose_of(&hand, &c.heavy, 1.0, Vec2::new(0.02, 0.02));
            assert!(pose.sag.abs() < 1e-4);
            assert!(pose.orientation.is_finite());
        }
    }

    #[test]
    fn the_sway_stays_within_its_amplitude() {
        for i in 0..200 {
            let s = sway(i as f32 * 0.05, 0.35, 0.02);
            assert!(s.x.abs() <= 0.02 + 1e-6 && s.y.abs() <= 0.02 + 1e-6);
        }
        assert_eq!(sway(1.0, 0.35, 0.0), Vec2::ZERO);
    }

    #[test]
    fn the_spring_trails_a_turn_and_settles_on_it() {
        let target = Quat::from_rotation_y(0.6);
        let mut spring = RotSpring::at(Quat::IDENTITY);
        spring.step(target, 0.02, 70.0, 0.65);
        let early = spring.orientation.angle_between(target);
        assert!(early > 0.3, "{early}");
        for _ in 0..300 {
            spring.step(target, 0.01, 70.0, 0.65);
        }
        assert!(spring.orientation.angle_between(target) < 1e-3);
    }

    #[test]
    fn the_spring_survives_a_long_step_and_a_flipped_target() {
        let mut spring = RotSpring::at(Quat::IDENTITY);
        spring.step(-Quat::from_rotation_x(0.3), 0.5, 2000.0, 0.1);
        assert!(spring.orientation.is_finite());
        for _ in 0..500 {
            spring.step(-Quat::from_rotation_x(0.3), 0.01, 260.0, 0.75);
        }
        assert!(spring.orientation.angle_between(Quat::from_rotation_x(0.3)) < 1e-3);
    }

    #[test]
    fn a_critically_damped_spring_settles_without_overshoot() {
        let target = Quat::from_rotation_y(0.4);
        for stiffness in [50.0, 500.0, 5000.0] {
            let mut spring = RotSpring::at(Quat::IDENTITY);
            // The turn left to the target, about +Y; negative past it.
            let remaining =
                |spring: &RotSpring| (target * spring.orientation.inverse()).to_scaled_axis().y;
            let mut last = remaining(&spring);
            for _ in 0..180 {
                spring.step(target, FRAME_S, stiffness, 1.0);
                // Never past the target, so the remaining turn only shrinks.
                let now = remaining(&spring);
                assert!(now >= -1e-4, "{stiffness}: past the target by {now}");
                assert!(now <= last + 1e-5, "{stiffness}: {now} after {last}");
                last = now;
            }
            assert!(last.abs() < 1e-3, "{stiffness}: {last}");
        }
    }

    #[test]
    fn a_braced_gun_lags_a_quick_swing_a_little_and_settles_on_the_aim() {
        // A brisk swing of the aim, a sixth of a turn a second.
        let speed = std::f32::consts::TAU / 6.0;
        for class in [HeftClass::Smg, HeftClass::Rifle, HeftClass::Heavy] {
            let weight = braced(class);
            assert_eq!(weight.damping_ratio, 1.0);
            let (lag, overshoot) = swing(&weight, speed, 0.5);
            // The lag is felt but small: a critically damped spring trails a steady swing by
            // twice its speed over the natural frequency (a little less here, the aim moving in
            // frame-sized steps), well inside the cap.
            let expected = 2.0 * speed / weight.stiffness.sqrt();
            assert!(
                lag > expected * 0.6 && lag < expected * 1.1,
                "{class:?}: {lag} vs {expected}"
            );
            assert!(
                lag > 0.2_f32.to_radians() && lag < weight.max_angle_deg.to_radians(),
                "{class:?}: {}",
                lag.to_degrees()
            );
            assert!(overshoot < 1e-3, "{class:?} overshoots by {overshoot}");
        }
    }

    #[test]
    fn heavier_braced_guns_lag_more() {
        let speed = 1.0;
        let lag = |class| swing(&braced(class), speed, 0.5).0;
        let (smg, rifle, heavy) = (
            lag(HeftClass::Smg),
            lag(HeftClass::Rifle),
            lag(HeftClass::Heavy),
        );
        assert!(smg < rifle && rifle < heavy, "{smg} {rifle} {heavy}");
        // And sag further.
        assert!(braced(HeftClass::Smg).sag_deg < braced(HeftClass::Rifle).sag_deg);
        assert!(braced(HeftClass::Rifle).sag_deg < braced(HeftClass::Heavy).sag_deg);
    }

    #[test]
    fn the_braced_sag_is_mild() {
        let c = config();
        let hand = level(Vec3::new(0.0, -0.2, -0.6));
        for class in [HeftClass::Smg, HeftClass::Rifle, HeftClass::Heavy] {
            let weight = braced(class);
            let pose = weighted_pose(&hand, Vec3::ZERO, &weight, &c, Vec2::new(0.1, 0.1));
            let sag = pose.sag.to_degrees();
            assert!(sag > 0.0 && sag < 5.0, "{class:?}: {sag}");
            // Braced, the gun does not sway.
            let still = weighted_pose(&hand, Vec3::ZERO, &weight, &c, Vec2::ZERO);
            assert!(pose.orientation.angle_between(still.orientation) < 1e-6);
        }
    }

    #[test]
    fn the_strength_scales_the_braced_weight() {
        let rifle = config().braced.rifle;
        let double = rifle.at_strength(2.0);
        assert!((double.sag_deg - rifle.sag_deg * 2.0).abs() < 1e-5);
        // The lag doubles: the natural frequency halves.
        assert!((double.stiffness.sqrt() * 2.0 - rifle.stiffness.sqrt()).abs() < 1e-3);
        assert_eq!(rifle.at_strength(0.0), BracedTuning::NONE);
        assert_eq!(rifle.at_strength(f32::NAN), BracedTuning::NONE);
        let mut off = config();
        off.braced.enabled = false;
        let weight = Weight::of(HeftClass::Heavy, &off, 0.0);
        assert_eq!((weight.sag_deg, weight.stiffness), (0.0, 0.0));
    }

    #[test]
    fn the_weight_blends_from_one_hand_to_braced() {
        let c = config();
        let one = Weight::of(HeftClass::Rifle, &c, 1.0);
        assert_eq!(one.stiffness, c.rifle.lag_stiffness);
        assert_eq!(one.damping_ratio, c.rifle.lag_damping_ratio);
        assert_eq!(one.sag_deg, c.rifle.sag_deg);
        assert_eq!(one.max_angle_deg, c.max_angle_deg);
        let two = Weight::of(HeftClass::Rifle, &c, 0.0);
        assert_eq!(two.stiffness, c.braced.rifle.stiffness);
        assert_eq!(two.sag_deg, c.braced.rifle.sag_deg);
        assert_eq!((two.reach_sag_deg, two.sway_share), (0.0, 0.0));
        assert_eq!(two.max_angle_deg, c.braced.max_angle_deg);
        // In between, the spring stiffens and the sag lightens as the grip engages.
        let mut last = one;
        for share in [0.75, 0.5, 0.25, 0.0] {
            let w = Weight::of(HeftClass::Rifle, &c, share);
            assert!(
                w.stiffness > last.stiffness && w.sag_deg < last.sag_deg,
                "{share}"
            );
            last = w;
        }
        // A one-handed SMG stays rigid; only its braced weight springs.
        assert_eq!(Weight::of(HeftClass::Smg, &c, 1.0).stiffness, 0.0);
        assert!(Weight::of(HeftClass::Smg, &c, 0.5).stiffness > 0.0);
    }

    #[test]
    fn a_frozen_weight_keeps_its_turn_on_the_hand() {
        // The only test that drives the shared state.
        let c = config();
        let right = HeftTarget {
            hand: Hand::Right,
            class: HeftClass::Heavy,
            weapon: 1,
        };
        observe([None, Some(right)]);
        let applied = |hand: CockpitHand| {
            let mut hands = [None, Some(hand)];
            apply(&mut hands, Vec3::ZERO, &c);
            hands[1].unwrap().grip.orientation * hand.grip.orientation.inverse()
        };
        let still = pistol(Vec3::new(0.0, -0.2, -0.6), 0.0, 0.0);
        let before = applied(still);
        assert!(angle(before) > 0.05, "the heavy gun sags");
        freeze([false, true]);
        // A quick swing of the hand: frozen, the gun turns with it by the same offset, no lag.
        let swung = pistol(Vec3::new(0.0, -0.2, -0.6), 0.3, 0.8);
        let frozen = applied(swung);
        assert!(angle(frozen * before.inverse()) < 1e-4);
        freeze([false; 2]);
        let after = applied(swung);
        // Let go, the spring starts from where the gun was shown rather than jumping.
        assert!(angle(after * frozen.inverse()) < 0.05);

        // A gun in the other hand weighs on its own: it sags on its own spring while the right
        // hand's stays frozen at its turn.
        let left = HeftTarget {
            hand: Hand::Left,
            class: HeftClass::Heavy,
            weapon: 2,
        };
        observe([Some(left), Some(right)]);
        freeze([false, true]);
        let mut hands = [Some(still), Some(still)];
        apply(&mut hands, Vec3::ZERO, &c);
        let turned =
            |i: usize| hands[i].unwrap().grip.orientation * still.grip.orientation.inverse();
        assert!(angle(turned(0)) > 0.05, "the left gun sags");
        assert!(
            angle(turned(1) * after.inverse()) < 1e-4,
            "the right gun stays frozen"
        );
        // A target is taken only in its own hand's place.
        observe([Some(right), None]);
        assert_eq!(super::status().hands.map(|h| h.target), [None; 2]);
        freeze([false; 2]);
        observe([None; 2]);
    }

    #[test]
    fn a_late_reader_does_not_turn_the_clock_back() {
        let start = Instant::now();
        let later = start + Duration::from_millis(20);
        let mut at = None;
        assert_eq!(advance_clock(&mut at, later), 0.0);
        // A reader that took the time before the last one but locked after it sees no time pass,
        // and leaves the clock where it was.
        assert_eq!(advance_clock(&mut at, start), 0.0);
        assert_eq!(at, Some(later));
        let next = later + Duration::from_millis(10);
        assert!((advance_clock(&mut at, next) - 0.01).abs() < 1e-6);
        assert_eq!(at, Some(next));
    }

    #[test]
    fn the_offset_is_clamped_to_the_largest_angle() {
        let clamped = clamp_angle(Quat::from_rotation_x(1.0), 0.5);
        assert!((angle(clamped) - 0.5).abs() < 1e-5);
        let small = Quat::from_rotation_x(0.2);
        assert!(clamp_angle(small, 0.5).angle_between(small) < 1e-6);
    }
}
