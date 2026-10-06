//! The weight of a long gun. Held in one hand, it sags, trails the hand, and sways, it kicks
//! harder, and it shoots wider. Braced on the foregrip, most of that goes, but the gun keeps a
//! weight of its own: it lags a quick swing of the hands a little on a critically damped spring and
//! settles on the aim without overshooting, and it sags a little.
//!
//! How much of each a weapon gets is its class's tuning ([`HeftClass`], from the weapon's class,
//! [`WeaponBase::m_Class`]): none for the pistols and revolvers, in either hold; one-handed, none
//! for the SMGs either, a little for the rifles, and a lot for the launchers and the other heavy
//! weapons; braced, least for the SMGs and most for the heavy weapons. The input tick publishes the
//! held weapon and its hand ([`observe`]); everything else reads that.
//!
//! - The pose: [`apply`] turns the holding hand's poses about the controller's grip, on the active
//!   hands every hand subsystem starts from ([`crate::controllers::hands::active_hands`]), so the
//!   IK, the late update, the gun's sight line, and the overlay all carry it, and a shot goes where
//!   the drooping gun points. The gun pitches down about the horizontal axis across its barrel, by
//!   the sag plus the one-handed reach sag as the hand moves away from the head, scaled by how level
//!   the barrel is; held in one hand it sways on two incommensurate sine waves; and the result is
//!   followed on a damped rotational spring, so the gun trails a fast turn and settles after it.
//!   The one-handed and the braced weights blend by how far the foregrip is engaged ([`Weight`]).
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
            config::{BracedHeftConfig, BracedTuning, HeftConfig, HeftTuning},
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
    /// Pistols and revolvers.
    Sidearm,
    /// SMGs: a sidearm in one hand, the lightest of the long guns braced.
    Smg,
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
        const SIDEARMS: [WeaponClass; 2] = [
            WeaponClass::E_WEAPON_CLASS_PISTOL,
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
        } else if class == WeaponClass::E_WEAPON_CLASS_SMG as i32 {
            HeftClass::Smg
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
            HeftClass::Smg => "SMG",
            HeftClass::Rifle => "rifle",
            HeftClass::Heavy => "heavy",
        }
    }

    /// This class's one-handed tuning in `config`.
    pub fn tuning(self, config: &HeftConfig) -> HeftTuning {
        match self {
            HeftClass::Sidearm | HeftClass::Smg => config.sidearm,
            HeftClass::Rifle => config.rifle,
            HeftClass::Heavy => config.heavy,
        }
    }

    /// This class's braced tuning in `config`, before its strength.
    pub fn braced(self, config: &BracedHeftConfig) -> BracedTuning {
        match self {
            HeftClass::Sidearm => BracedTuning::NONE,
            HeftClass::Smg => config.smg,
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
/// off-hand of an engaged foregrip moves with the gun by the grip's weight, so the gun carries it.
pub fn apply(hands: &mut [Option<CockpitHand>; 2], head: Vec3, config: &HeftConfig) {
    let now = Instant::now();
    let grip = foregrip::two_hand_grip();
    let mut state = STATE.lock();
    let dt = state
        .at
        .map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32());
    state.at = Some(now);
    let acting = config.enabled || config.braced.enabled;
    let target = state
        .target
        .filter(|(_, at)| acting && now.saturating_duration_since(*at) < MAX_AGE)
        .map(|(t, _)| t);
    let Some(target) = target else {
        state.spring = None;
        state.status = HeftStatus {
            widened_shots: state.status.widened_shots,
            ..HeftStatus::default()
        };
        return;
    };
    let share = unbraced_share(grip, target.hand);
    let weight = Weight::of(target.class, config, share);
    let Some(h) = hands[target.hand.index()] else {
        state.spring = None;
        return;
    };
    let t = epoch().elapsed().as_secs_f32();
    let pose = weighted_pose(
        h.grip,
        head,
        &weight,
        config,
        sway(t, config.sway_hz, weight.sway_deg.to_radians()),
    );
    let shown = if weight.stiffness > 0.0 && dt <= MAX_GAP.as_secs_f32() {
        let spring = state.spring.get_or_insert(RotSpring::at(pose.orientation));
        spring.step(pose.orientation, dt, weight.stiffness, weight.damping_ratio);
        spring.orientation
    } else {
        state.spring = Some(RotSpring::at(pose.orientation));
        pose.orientation
    };
    let rotation = clamp_angle(
        shown * h.grip.orientation.inverse(),
        weight.max_angle_deg.to_radians(),
    );
    let sway_shown = pose.sway * weight.sway_share * pose.sway_reach;
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

/// What a gun's weight asks of its pose: its one-handed and its braced tunings blended by how
/// braced it is.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Weight {
    /// The sag close in, degrees.
    pub sag_deg: f32,
    /// The further sag at full reach, degrees.
    pub reach_sag_deg: f32,
    /// The sway's amplitude, degrees.
    pub sway_deg: f32,
    /// How much of the sway shows, `0..=1`.
    pub sway_share: f32,
    /// The spring's stiffness, per second squared; zero follows rigidly.
    pub stiffness: f32,
    pub damping_ratio: f32,
    /// The furthest the gun may turn from where the hands turn it, degrees.
    pub max_angle_deg: f32,
}

impl Weight {
    /// The weight of a gun of `class` with the unbraced `share` (one in one hand, zero braced):
    /// each of the one-handed and the braced weights, where its switch in `config` is on, blended
    /// by the share. The sag and the limits blend linearly; the spring blends by its natural
    /// frequency, a rigid side counting as a very stiff spring, so that the lag shrinks smoothly as
    /// the foregrip engages.
    pub fn of(class: HeftClass, config: &HeftConfig, share: f32) -> Self {
        let one = if config.enabled {
            class.tuning(config)
        } else {
            HeftTuning::NONE
        };
        let braced = if config.braced.enabled {
            class
                .braced(&config.braced)
                .at_strength(config.braced.strength)
        } else {
            BracedTuning::NONE
        };
        Self::blend(&one, &braced, share, config)
    }

    /// The blend of the one-handed `one` and the braced `braced` by the unbraced `share`.
    pub fn blend(one: &HeftTuning, braced: &BracedTuning, share: f32, config: &HeftConfig) -> Self {
        let share = if share.is_finite() {
            share.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let (one_k, braced_k) = (one.lag_stiffness.max(0.0), braced.stiffness.max(0.0));
        let stiffness = if share >= 1.0 {
            one_k
        } else if share <= 0.0 {
            braced_k
        } else if one_k == 0.0 && braced_k == 0.0 {
            0.0
        } else {
            let omega = |k: f32| if k > 0.0 { k.sqrt() } else { RIGID_OMEGA };
            lerp(omega(braced_k), omega(one_k), share).powi(2)
        };
        Self {
            sag_deg: lerp(braced.sag_deg, one.sag_deg, share),
            reach_sag_deg: one.reach_sag_deg * share,
            sway_deg: one.sway_deg,
            sway_share: share,
            stiffness,
            damping_ratio: lerp(1.0, one.lag_damping_ratio, share),
            max_angle_deg: lerp(config.braced.max_angle_deg, config.max_angle_deg, share),
        }
    }
}

/// Sag and sway the grip orientation of a gun held at `grip` with the head at `head`, by its
/// `weight`. The sag turns the barrel (the grip's -Z) down about the horizontal axis across it, by
/// the sag plus the reach sag as the hand moves out, scaled by how level the barrel is; `sway`
/// (pitch and yaw, radians, at full share) turns it about the grip's own axes, half of it close in
/// and all of it at full reach.
pub fn weighted_pose(
    grip: Pose,
    head: Vec3,
    weight: &Weight,
    config: &HeftConfig,
    sway: Vec2,
) -> WeightedPose {
    let reach = smoothstep(
        config.reach_near_m,
        config.reach_far_m,
        grip.position.distance(head),
    );
    let forward = grip.orientation * Vec3::NEG_Z;
    let level = Vec2::new(forward.x, forward.z).length().min(1.0);
    let sag = (weight.sag_deg + weight.reach_sag_deg * reach).to_radians() * level;
    let across = Vec3::Y.cross(forward).try_normalize();
    let droop = across.map_or(Quat::IDENTITY, |axis| Quat::from_axis_angle(axis, sag));
    let sway_reach = 0.5 + 0.5 * reach;
    let sway_scaled = sway * weight.sway_share * sway_reach;
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

/// The natural frequency a rigid side of a blend counts as, radians per second: stiff enough that
/// its lag cannot be seen, soft enough for the spring's sub-steps.
const RIGID_OMEGA: f32 = 120.0;
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

    /// The one-handed pose of a gun with `tuning` at the unbraced `share`, with no braced weight.
    fn pose_of(grip: Pose, tuning: &HeftTuning, share: f32, sway: Vec2) -> WeightedPose {
        let c = config();
        let weight = Weight::blend(tuning, &BracedTuning::NONE, share, &c);
        weighted_pose(grip, Vec3::ZERO, &weight, &c, sway)
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
        let grip = level_grip(Vec3::new(0.0, -0.3, -0.6));
        let pose = pose_of(grip, &c.sidearm, 1.0, Vec2::ZERO);
        assert!(pose.orientation.angle_between(Quat::IDENTITY) < 1e-5);
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
            level_grip(Vec3::new(0.0, -0.2, -0.1)),
            &c.heavy,
            1.0,
            Vec2::ZERO,
        );
        let far = pose_of(
            level_grip(Vec3::new(0.0, -0.2, -0.7)),
            &c.heavy,
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
        let rifle = pose_of(grip, &c.rifle, 1.0, Vec2::ZERO);
        let heavy = pose_of(grip, &c.heavy, 1.0, Vec2::ZERO);
        assert!(rifle.sag > 0.0 && rifle.sag < heavy.sag);
    }

    #[test]
    fn bracing_removes_the_weight() {
        let c = config();
        let grip = level_grip(Vec3::new(0.0, -0.2, -0.6));
        let braced = pose_of(grip, &c.heavy, 0.0, Vec2::new(0.1, 0.1));
        assert!(braced.orientation.angle_between(grip.orientation) < 1e-5);
        assert_eq!(
            Weight::blend(&c.heavy, &BracedTuning::NONE, 0.0, &c).stiffness,
            0.0
        );
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
        let pose = pose_of(grip, &c.heavy, 1.0, Vec2::ZERO);
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
        let grip = level_grip(Vec3::new(0.0, -0.2, -0.6));
        for class in [HeftClass::Smg, HeftClass::Rifle, HeftClass::Heavy] {
            let weight = braced(class);
            let pose = weighted_pose(grip, Vec3::ZERO, &weight, &c, Vec2::new(0.1, 0.1));
            let sag = pose.sag.to_degrees();
            assert!(sag > 0.0 && sag < 5.0, "{class:?}: {sag}");
            // Braced, the gun does not sway.
            let still = weighted_pose(grip, Vec3::ZERO, &weight, &c, Vec2::ZERO);
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
    fn the_offset_is_clamped_to_the_largest_angle() {
        let clamped = clamp_angle(Quat::from_rotation_x(1.0), 0.5);
        assert!((angle(clamped) - 0.5).abs() < 1e-5);
        let small = Quat::from_rotation_x(0.2);
        assert!(clamp_angle(small, 0.5).angle_between(small) < 1e-6);
    }
}
