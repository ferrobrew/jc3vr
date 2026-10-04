//! The one-handed weight of a long gun: held in one hand, it sags, trails the hand, and sways, it
//! kicks harder, and it shoots wider; bracing it on the foregrip takes all of that away.
//!
//! How much of each a weapon gets is its class's tuning ([`HeftClass`], from the weapon's class,
//! [`WeaponBase::m_Class`]): none for the sidearms, a little for the rifles, and a lot for the
//! launchers and the other heavy weapons. The input tick publishes the held weapon and its hand
//! ([`observe`]); everything else reads that.
//!
//! - The pose: [`apply`] turns the holding hand's poses about the controller's grip, on the active
//!   hands every hand subsystem starts from ([`crate::controllers::hands::active_hands`]), so the
//!   IK, the late update, the gun's sight line, and the overlay all carry it, and a shot goes where
//!   the drooping gun points. The gun pitches down about the horizontal axis across its barrel, by
//!   the class's sag plus its reach sag as the hand moves away from the head, scaled by how level
//!   the barrel is; it sways on two incommensurate sine waves; and the result is followed on a
//!   damped rotational spring, so the gun trails a fast turn of the wrist and settles after it.
//! - The kick: the recoil multiplies a one-handed shot's kick by the class's kick scale
//!   ([`one_hand_kick`]); the foregrip's own two-hand kick scale takes over as the grip engages.
//! - The spread: a detour on [`WeaponBase::GetCurrentScatterValue`] multiplies the scatter the game
//!   gives the local player's shot by the class's spread scale ([`spread_scale`]).
//!
//! The off-hand's foregrip weight ([`foregrip::two_hand_grip`]) scales all three down to nothing.
//! The spring runs on wall-clock time and is advanced lazily by whichever thread reads the hands, as
//! the recoil's is.
//!
//! [`WeaponBase::m_Class`]: jc3gi::equipment::weapon::WeaponBase::m_Class
//! [`WeaponBase::GetCurrentScatterValue`]: jc3gi::equipment::weapon::WeaponBase::GetCurrentScatterValue

use std::{
    f32::consts::TAU,
    sync::OnceLock,
    time::{Duration, Instant},
};

use detours_macro::detour;
use glam::{Quat, Vec2, Vec3};
use jc3gi::equipment::weapon::{WeaponBase, WeaponClass};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    controllers::{
        frame::{Hand, Pose},
        hands::CockpitHand,
        scheme,
        weapons::{
            config::{HeftConfig, HeftTuning},
            foregrip,
            recoil::Motion,
        },
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_CURRENT_SCATTER_VALUE_BINDER)
}

/// The weight class a weapon's one-handed tuning comes from.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeftClass {
    /// Pistols, revolvers, and SMGs.
    Sidearm,
    /// Assault rifles, shotguns, and sniper rifles.
    Rifle,
    /// Launchers, miniguns, mortars, mounted-calibre guns, and the special weapons.
    Heavy,
}

impl HeftClass {
    /// The class of a weapon of class `class` (the raw [`WeaponClass`] value, which may be out of
    /// the enum's range), held in a two-handed slot or not. A weapon whose class is unset or
    /// unknown is a rifle in a two-handed slot and a sidearm otherwise.
    pub fn of(class: i32, two_handed: bool) -> Self {
        const SIDEARMS: [WeaponClass; 3] = [
            WeaponClass::E_WEAPON_CLASS_PISTOL,
            WeaponClass::E_WEAPON_CLASS_SMG,
            WeaponClass::E_WEAPON_CLASS_REVOLVER,
        ];
        const RIFLES: [WeaponClass; 3] = [
            WeaponClass::E_WEAPON_CLASS_ASSAULT_RIFLE,
            WeaponClass::E_WEAPON_CLASS_SHOTGUN,
            WeaponClass::E_WEAPON_CLASS_SNIPER,
        ];
        const HEAVY: [WeaponClass; 7] = [
            WeaponClass::E_WEAPON_CLASS_GRENADE_LAUNCHER,
            WeaponClass::E_WEAPON_CLASS_RPG,
            WeaponClass::E_WEAPON_CLASS_MINIGUN,
            WeaponClass::E_WEAPON_CLASS_AA_GUN,
            WeaponClass::E_WEAPON_CLASS_MORTAR,
            WeaponClass::E_WEAPON_CLASS_50CAL,
            WeaponClass::E_WEAPON_CLASS_SPECIAL,
        ];
        let is = |set: &[WeaponClass]| set.iter().any(|&c| c as i32 == class);
        if is(&SIDEARMS) {
            HeftClass::Sidearm
        } else if is(&RIFLES) {
            HeftClass::Rifle
        } else if is(&HEAVY) {
            HeftClass::Heavy
        } else if two_handed {
            HeftClass::Rifle
        } else {
            HeftClass::Sidearm
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HeftClass::Sidearm => "sidearm",
            HeftClass::Rifle => "rifle",
            HeftClass::Heavy => "heavy",
        }
    }

    /// This class's tuning in `config`.
    pub fn tuning(self, config: &HeftConfig) -> HeftTuning {
        match self {
            HeftClass::Sidearm => config.sidearm,
            HeftClass::Rifle => config.rifle,
            HeftClass::Heavy => config.heavy,
        }
    }
}

/// The weapon the weight acts on, as the input tick saw it.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HeftTarget {
    /// The hand holding it.
    pub hand: Hand,
    pub class: HeftClass,
    /// The weapon's address.
    pub weapon: usize,
}

/// The weight's readout.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct HeftStatus {
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
    /// Shots whose scatter was widened, since the payload loaded.
    pub widened_shots: u64,
}

pub fn status() -> HeftStatus {
    STATE.lock().status
}

/// Publish the weapon the weight acts on for this input tick, or `None` to let go of it.
pub fn observe(target: Option<HeftTarget>) {
    let mut state = STATE.lock();
    if state.target.map(|(t, _)| t) != target {
        state.spring = None;
    }
    state.target = target.map(|t| (t, Instant::now()));
    state.status.target = target;
}

/// Turn the holding hand's poses by the weight. `hands` are the cockpit-frame hands, already in
/// any two-handed pose, indexed by [`Hand::index`]; `head` is the head's cockpit position. The
/// off-hand of a partly engaged foregrip moves with the gun by the grip's weight.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Vec3, config: &HeftConfig) {
    let now = Instant::now();
    let grip = foregrip::two_hand_grip();
    let mut state = STATE.lock();
    let dt = state
        .at
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
    state.at = Some(now);
    let target = state
        .target
        .filter(|(_, at)| config.enabled && now.saturating_duration_since(*at) < MAX_AGE)
        .map(|(t, _)| t);
    let Some(target) = target else {
        state.spring = None;
        state.status = HeftStatus {
            widened_shots: state.status.widened_shots,
            ..HeftStatus::default()
        };
        return;
    };
    let tuning = target.class.tuning(config);
    let share = unbraced_share(grip, target.hand);
    let Some(h) = hands[target.hand.index()] else {
        state.spring = None;
        return;
    };
    let t = epoch().elapsed().as_secs_f32();
    let pose = weighted_pose(
        h.grip,
        head,
        &tuning,
        config,
        share,
        sway(t, config.sway_hz, tuning.sway_deg.to_radians()),
    );
    let shown = if tuning.lag_stiffness > 0.0 && share > 0.0 && dt <= MAX_GAP.as_secs_f32() {
        let spring = state.spring.get_or_insert(RotSpring::at(pose.orientation));
        spring.step(
            pose.orientation,
            dt,
            tuning.lag_stiffness,
            tuning.lag_damping_ratio,
        );
        spring.orientation
    } else {
        state.spring = Some(RotSpring::at(pose.orientation));
        pose.orientation
    };
    let rotation = clamp_angle(
        shown * h.grip.orientation.inverse(),
        config.max_angle_deg.to_radians(),
    );
    let sway_shown = pose.sway * share * pose.sway_reach;
    state.status = HeftStatus {
        target: Some(target),
        share,
        reach: pose.reach,
        sag_deg: pose.sag.to_degrees(),
        sway_deg: Vec2::new(sway_shown.x.to_degrees(), sway_shown.y.to_degrees()),
        lag_deg: angle(shown * pose.orientation.inverse()).to_degrees(),
        offset_deg: angle(rotation).to_degrees(),
        widened_shots: state.status.widened_shots,
    };
    drop(state);
    if rotation == Quat::IDENTITY {
        return;
    }
    let motion = Motion {
        pivot: h.grip.position,
        rotation,
        translation: Vec3::ZERO,
    };
    hands[target.hand.index()] = Some(motion.hand(h));
    if let Some(g) = grip.filter(|g| g.holder == target.hand)
        && let Some(off) = hands[target.hand.other().index()]
    {
        hands[target.hand.other().index()] = Some(motion.scaled(g.weight).hand(off));
    }
}

/// The multiplier on a shot's kick for the gun in `hand` held in one hand: the class's kick scale
/// for the weight's hand, one otherwise. The recoil blends it toward its own two-hand kick scale by
/// the foregrip's weight.
pub fn one_hand_kick(hand: Hand) -> f32 {
    let config = Config::lock_query(|c| c.controllers.weapons.heft);
    let target = STATE.lock().target.map(|(t, _)| t);
    match target {
        Some(t) if config.enabled && t.hand == hand => t.class.tuning(&config).kick_scale,
        _ => 1.0,
    }
}

/// The multiplier on the scatter of a shot from `weapon`: the class's spread scale, scaled by the
/// unbraced share, for the weight's weapon, one otherwise.
pub fn spread_scale(weapon: usize) -> f32 {
    let config = Config::lock_query(|c| c.controllers.weapons.heft);
    if !(config.enabled && config.spread) {
        return 1.0;
    }
    let Some(target) = STATE.lock().target.map(|(t, _)| t) else {
        return 1.0;
    };
    if target.weapon != weapon {
        return 1.0;
    }
    let share = unbraced_share(foregrip::two_hand_grip(), target.hand);
    lerp(1.0, target.class.tuning(&config).spread_scale, share)
}

/// The weighted pose of a gun held at `grip`, before the spring.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WeightedPose {
    /// The sagged and swayed grip orientation.
    pub orientation: Quat,
    /// The sag applied, radians.
    pub sag: f32,
    /// The hand's reach, `0..=1`.
    pub reach: f32,
    /// The sway asked for at full share, pitch and yaw, radians.
    pub sway: Vec2,
    /// The share of the sway the reach lets through, `0..=1`.
    pub sway_reach: f32,
}

/// Sag and sway the grip orientation of a gun held at `grip` with the head at `head`, for the
/// unbraced `share` of the class's `tuning`. The sag turns the barrel (the grip's -Z) down about
/// the horizontal axis across it, by the sag plus the reach sag as the hand moves out, scaled by
/// how level the barrel is; `sway` (pitch and yaw, radians) turns it about the grip's own axes,
/// half of it close in and all of it at full reach.
pub fn weighted_pose(
    grip: Pose,
    head: Vec3,
    tuning: &HeftTuning,
    config: &HeftConfig,
    share: f32,
    sway: Vec2,
) -> WeightedPose {
    let share = share.clamp(0.0, 1.0);
    let reach = smoothstep(
        config.reach_near_m,
        config.reach_far_m,
        grip.position.distance(head),
    );
    let forward = grip.orientation * Vec3::NEG_Z;
    let level = Vec2::new(forward.x, forward.z).length().min(1.0);
    let sag = (tuning.sag_deg + tuning.reach_sag_deg * reach).to_radians() * level * share;
    let across = Vec3::Y.cross(forward).try_normalize();
    let droop = across.map_or(Quat::IDENTITY, |axis| Quat::from_axis_angle(axis, sag));
    let sway_reach = 0.5 + 0.5 * reach;
    let sway_scaled = sway * share * sway_reach;
    let wobble = Quat::from_rotation_y(sway_scaled.y) * Quat::from_rotation_x(sway_scaled.x);
    WeightedPose {
        orientation: (droop * grip.orientation * wobble).normalize(),
        sag: if across.is_some() { sag } else { 0.0 },
        reach,
        sway,
        sway_reach,
    }
}

/// The sway at time `t` seconds: a pitch and a yaw of amplitude `amplitude` (radians) at
/// `hz` and at an incommensurate multiple of it, so the path does not repeat.
pub fn sway(t: f32, hz: f32, amplitude: f32) -> Vec2 {
    let phase = TAU * hz.max(0.0) * t;
    Vec2::new(
        phase.sin() * amplitude,
        (phase * YAW_RATIO + YAW_PHASE).sin() * amplitude * YAW_SHARE,
    )
}

/// The share of the weight left by a two-hand `grip` on `hand`'s gun: one in one hand, zero with
/// the foregrip fully engaged.
pub fn unbraced_share(grip: Option<foregrip::TwoHandGrip>, hand: Hand) -> f32 {
    1.0 - grip
        .filter(|g| g.holder == hand)
        .map_or(0.0, |g| g.weight.clamp(0.0, 1.0))
}

/// A damped spring on an orientation, pulled toward a target orientation.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct RotSpring {
    pub orientation: Quat,
    /// The angular velocity, radians per second, as a rotation vector.
    pub velocity: Vec3,
}

impl RotSpring {
    /// At rest at `orientation`.
    pub fn at(orientation: Quat) -> Self {
        Self {
            orientation,
            velocity: Vec3::ZERO,
        }
    }

    /// Advance by `dt` seconds toward `target` with the given stiffness (per second squared) and
    /// damping ratio, in sub-steps short enough to stay stable at any stiffness the settings allow.
    pub fn step(&mut self, target: Quat, dt: f32, stiffness: f32, damping_ratio: f32) {
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let stiffness = stiffness.max(0.0);
        let damping = 2.0 * damping_ratio.max(0.0) * stiffness.sqrt();
        let steps = (dt / MAX_SUBSTEP).ceil().max(1.0) as u32;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let error = shortest(target * self.orientation.inverse()).to_scaled_axis();
            self.velocity += (stiffness * error - damping * self.velocity) * h;
            self.orientation =
                (Quat::from_scaled_axis(self.velocity * h) * self.orientation).normalize();
        }
        if !(self.orientation.is_finite() && self.velocity.is_finite()) {
            *self = Self::at(target);
        }
    }
}

/// Limit a rotation to `max` radians, keeping its axis.
pub fn clamp_angle(rotation: Quat, max: f32) -> Quat {
    let rotation = shortest(rotation);
    let (axis, angle) = rotation.to_axis_angle();
    if angle <= max.max(0.0) || !axis.is_finite() {
        rotation
    } else {
        Quat::from_axis_angle(axis, max.max(0.0))
    }
}

/// How long a published weapon stays in effect without a new input tick.
const MAX_AGE: Duration = Duration::from_millis(500);
/// A gap longer than this between reads puts the spring at rest on its target.
const MAX_GAP: Duration = Duration::from_millis(250);
/// The longest sub-step the spring integrates in one go, seconds.
const MAX_SUBSTEP: f32 = 1.0 / 480.0;
/// The yaw sway's frequency relative to the pitch sway's, its phase offset, and its share of the
/// amplitude.
const YAW_RATIO: f32 = 0.618;
const YAW_PHASE: f32 = 1.3;
const YAW_SHARE: f32 = 0.8;

struct State {
    target: Option<(HeftTarget, Instant)>,
    spring: Option<RotSpring>,
    at: Option<Instant>,
    status: HeftStatus,
}

static STATE: Mutex<State> = Mutex::new(State {
    target: None,
    spring: None,
    at: None,
    status: HeftStatus {
        target: None,
        share: 0.0,
        reach: 0.0,
        sag_deg: 0.0,
        sway_deg: Vec2::ZERO,
        lag_deg: 0.0,
        offset_deg: 0.0,
        widened_shots: 0,
    },
});

/// The sway's clock.
fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// The same rotation with a non-negative scalar part, the shorter way round.
fn shortest(q: Quat) -> Quat {
    if q.w < 0.0 { -q } else { q }
}

fn angle(q: Quat) -> f32 {
    shortest(q).to_axis_angle().1
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
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
    use super::*;

    fn config() -> HeftConfig {
        HeftConfig::new()
    }

    fn level_grip(position: Vec3) -> Pose {
        Pose {
            position,
            orientation: Quat::IDENTITY,
        }
    }

    fn forward(q: Quat) -> Vec3 {
        q * Vec3::NEG_Z
    }

    #[test]
    fn weapon_classes_map_to_weight_classes() {
        let class = |c: WeaponClass, two| HeftClass::of(c as i32, two);
        assert_eq!(
            class(WeaponClass::E_WEAPON_CLASS_SMG, false),
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
        let grip = level_grip(Vec3::new(0.0, -0.3, -0.6));
        let pose = weighted_pose(grip, Vec3::ZERO, &c.sidearm, &c, 1.0, Vec2::ZERO);
        assert!(pose.orientation.angle_between(Quat::IDENTITY) < 1e-5);
        assert_eq!(c.sidearm.kick_scale, 1.0);
        assert_eq!(c.sidearm.spread_scale, 1.0);
    }

    #[test]
    fn a_heavy_gun_sags_down_and_more_at_reach() {
        let c = config();
        let near = weighted_pose(
            level_grip(Vec3::new(0.0, -0.2, -0.1)),
            Vec3::ZERO,
            &c.heavy,
            &c,
            1.0,
            Vec2::ZERO,
        );
        let far = weighted_pose(
            level_grip(Vec3::new(0.0, -0.2, -0.7)),
            Vec3::ZERO,
            &c.heavy,
            &c,
            1.0,
            Vec2::ZERO,
        );
        assert!(forward(near.orientation).y < -0.1, "{near:?}");
        assert!(forward(far.orientation).y < forward(near.orientation).y);
        assert!((near.sag.to_degrees() - c.heavy.sag_deg).abs() < 0.5);
        assert!((far.sag.to_degrees() - c.heavy.sag_deg - c.heavy.reach_sag_deg).abs() < 0.5);
        // The sag pitches the barrel only: it stays in its vertical plane.
        assert!(forward(far.orientation).x.abs() < 1e-5);
    }

    #[test]
    fn a_rifle_sags_less_than_a_launcher() {
        let c = config();
        let grip = level_grip(Vec3::new(0.0, -0.2, -0.5));
        let rifle = weighted_pose(grip, Vec3::ZERO, &c.rifle, &c, 1.0, Vec2::ZERO);
        let heavy = weighted_pose(grip, Vec3::ZERO, &c.heavy, &c, 1.0, Vec2::ZERO);
        assert!(rifle.sag > 0.0 && rifle.sag < heavy.sag);
    }

    #[test]
    fn bracing_removes_the_weight() {
        let c = config();
        let grip = level_grip(Vec3::new(0.0, -0.2, -0.6));
        let braced = weighted_pose(grip, Vec3::ZERO, &c.heavy, &c, 0.0, Vec2::new(0.1, 0.1));
        assert!(braced.orientation.angle_between(grip.orientation) < 1e-5);
        let grip_on = |weight| {
            Some(foregrip::TwoHandGrip {
                holder: Hand::Right,
                weight,
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
        let grip = Pose {
            position: Vec3::new(0.0, -0.2, -0.6),
            orientation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        };
        let pose = weighted_pose(grip, Vec3::ZERO, &c.heavy, &c, 1.0, Vec2::ZERO);
        assert!(pose.sag.abs() < 1e-4);
        assert!(pose.orientation.is_finite());
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
    fn the_offset_is_clamped_to_the_largest_angle() {
        let clamped = clamp_angle(Quat::from_rotation_x(1.0), 0.5);
        assert!((angle(clamped) - 0.5).abs() < 1e-5);
        let small = Quat::from_rotation_x(0.2);
        assert!(clamp_angle(small, 0.5).angle_between(small) < 1e-6);
    }
}
