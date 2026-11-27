//! The hand bodies' tuning, read from the shipped `tuning/physics.toml` ([`PHYSICS`]): the bodies
//! and their drive, their hits, the held guns' bodies, the readers' staleness and the drawn
//! offset, the rig's carry, the bodies' remaking, and the drawn torso's capsules.

use std::{f32::consts::PI, time::Duration};

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    published::Window,
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The hand bodies' tuning file.
    pub static PHYSICS: PhysicsTuning = "physics.toml";
}

/// The hand bodies' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicsTuning {
    /// The hand bodies. Each hand draws where its body is, so that a hand stops at what stops its
    /// body, and holds what its grip takes by joining its body to it with a fixed constraint, so
    /// that a held object has mass and the hand's strength carries it. The world does not end a
    /// constraint whose body is destroyed, so an object broken apart inside a step leaves the
    /// joint on a dead body until the next pre-simulation step ends it.
    pub hand_bodies: HandBodyTuning,
    pub step: StepTuning,
    pub render: HandRenderTuning,
    pub rig: RigTuning,
    pub lifecycle: LifecycleTuning,
    pub drawn_body: DrawnBodyTuning,
}

impl Tuning for PhysicsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.hand_bodies.validate()?;
        self.step.validate()?;
        self.render.validate()?;
        self.rig.validate()?;
        self.lifecycle.validate()?;
        self.drawn_body.validate()
    }
}

/// The hand bodies' tuning (see [`crate::controllers::physical_hands::physics`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandBodyTuning {
    /// A hand body's mass, kilograms. The hand's strength is its force and twist caps, not its mass:
    /// a light hand meets a light prop without throwing it, and still shoves a heavy one within its
    /// strength over a few steps, while a heavy hand makes a prop between it and the world jitter.
    pub mass_kg: f32,
    /// The strongest pull of one hand at a strength of one, newtons.
    pub max_force_n: f32,
    /// The strongest twist of one hand at a strength of one, newton metres.
    pub max_torque_nm: f32,
    /// The share of a hand's position error the drive closes in one step, `0..=1`.
    pub linear_gain: f32,
    /// The share of a hand's rotation error the drive closes in one step, `0..=1`.
    pub angular_gain: f32,
    /// The share of the drive's gain a hand keeps while it presses as hard as it can into what holds
    /// it back, `0..=1`; a lighter press keeps more. It rests on a wall rather than pounding it.
    pub contact_gain: f32,
    /// The share of the drive's gains a hand joined to a ragdoll or an articulated object pulls
    /// with, `0..=1`.
    pub constrained_gain_share: f32,
    /// The fastest a hand body is driven, metres per second, so that a tracking glitch cannot fling
    /// what it touches.
    pub max_speed_mps: f32,
    /// The fastest a hand body is turned, radians per second.
    pub max_angular_speed_rps: f32,
    /// The radius of the capsule a hand body is, metres of the player's hand.
    pub radius_m: f32,
    /// Half the length of the capsule's segment along the fingers, metres of the player's hand.
    pub half_length_m: f32,
    /// How far a hand body may fall behind its controller before it is put back on it, metres.
    pub break_distance_m: f32,
    /// The mass the hands' reaction moves, kilograms: the lighter, the further a push carries the
    /// player.
    pub body_mass_kg: f32,
    /// The stiffness of the spring between a held-back hand and its controller that pushes the
    /// player, newtons per metre of lag past the deadband.
    pub push_stiffness_n_per_m: f32,
    /// How far a hand body may lag behind its controller before the lag pushes the player, metres,
    /// so that resting a hand on a wall does not drift the body.
    pub push_deadband_m: f32,
    /// The fastest push of both hands together, metres per second.
    pub max_push_speed_mps: f32,
    /// The least horizontal share of a hand's lag for it to push the player, `0..=1`: pressing down
    /// on a table or the floor does not push.
    pub min_push_horizontal: f32,
    /// How long the push is smoothed over, seconds.
    pub push_smoothing_s: f32,
    /// How far past the push deadband a hand is held back for the strongest pressure pulse, metres.
    pub pressure_full_lag_m: f32,
    /// What a hand body's contacts do: the pulse of a hit, and a character's reaction to it.
    pub hits: HandHitsTuning,
    /// A held gun as a body of its own, joined to the hand bodies that hold it.
    pub guns: HeldGunsTuning,
}

impl HandBodyTuning {
    /// The strongest pull of one hand at `strength`, newtons.
    pub fn force_n(&self, strength: f32) -> f32 {
        self.max_force_n
            * if strength.is_finite() {
                strength.max(0.0)
            } else {
                0.0
            }
    }

    /// The strongest twist of one hand at `strength`, newton metres.
    pub fn torque_nm(&self, strength: f32) -> f32 {
        self.max_torque_nm
            * if strength.is_finite() {
                strength.max(0.0)
            } else {
                0.0
            }
    }

    fn validate(&self) -> Result<(), Invalid> {
        positive("hand_bodies.mass_kg", self.mass_kg, 1000.0)?;
        nonnegative("hand_bodies.max_force_n", self.max_force_n, 1.0e6)?;
        nonnegative("hand_bodies.max_torque_nm", self.max_torque_nm, 1.0e5)?;
        range("hand_bodies.linear_gain", self.linear_gain, 0.0, 1.0)?;
        range("hand_bodies.angular_gain", self.angular_gain, 0.0, 1.0)?;
        range("hand_bodies.contact_gain", self.contact_gain, 0.0, 1.0)?;
        range(
            "hand_bodies.constrained_gain_share",
            self.constrained_gain_share,
            0.0,
            1.0,
        )?;
        nonnegative("hand_bodies.max_speed_mps", self.max_speed_mps, 1000.0)?;
        nonnegative(
            "hand_bodies.max_angular_speed_rps",
            self.max_angular_speed_rps,
            1000.0,
        )?;
        positive("hand_bodies.radius_m", self.radius_m, 1.0)?;
        nonnegative("hand_bodies.half_length_m", self.half_length_m, 1.0)?;
        positive("hand_bodies.break_distance_m", self.break_distance_m, 100.0)?;
        positive("hand_bodies.body_mass_kg", self.body_mass_kg, 10_000.0)?;
        nonnegative(
            "hand_bodies.push_stiffness_n_per_m",
            self.push_stiffness_n_per_m,
            1.0e7,
        )?;
        nonnegative("hand_bodies.push_deadband_m", self.push_deadband_m, 10.0)?;
        nonnegative(
            "hand_bodies.max_push_speed_mps",
            self.max_push_speed_mps,
            100.0,
        )?;
        range(
            "hand_bodies.min_push_horizontal",
            self.min_push_horizontal,
            0.0,
            1.0,
        )?;
        nonnegative("hand_bodies.push_smoothing_s", self.push_smoothing_s, 10.0)?;
        positive(
            "hand_bodies.pressure_full_lag_m",
            self.pressure_full_lag_m,
            10.0,
        )?;
        self.hits.validate()?;
        self.guns.validate()
    }
}

/// A hand body's hits (see [`crate::controllers::physical_hands::physics::hit`]): each contact a
/// hand body starts pulses the hand by its impulse, and a character it strikes reacts as to a
/// blow of that impulse, against its own stagger and knockdown thresholds; a blow at or above
/// [`damage_impulse_ns`](Self::damage_impulse_ns) deals damage.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandHitsTuning {
    /// The weakest contact impulse a character reacts to, newton seconds.
    pub min_impulse_ns: f32,
    /// The contact impulse that staggers a character, newton seconds: a solid punch, which lands
    /// just past the character's own stagger threshold.
    pub stagger_impulse_ns: f32,
    /// The contact impulse that knocks a character down, newton seconds: a hard swing, which
    /// reaches the character's own knockdown threshold and ragdolls it.
    pub knockdown_impulse_ns: f32,
    /// How far past the stagger threshold a solid punch lands, as a factor of it.
    pub over_stagger: f32,
    /// How far short of the stagger threshold a blow just under a solid punch stays, as a factor.
    pub under_stagger: f32,
    /// How far short of the knockdown threshold a blow just under a hard swing stays, as a factor.
    pub under_knockdown: f32,
    /// The weakest contact impulse that deals damage, newton seconds.
    pub damage_impulse_ns: f32,
    /// The damage of a blow at the knockdown impulse, as a share of the player's melee damage; a
    /// weaker or stronger blow deals in proportion.
    pub damage_scale: f32,
    /// The most damage a held prop's blow deals, in health points.
    pub max_held_damage: i16,
    /// How long a character struck by a hand ignores that hand's further contacts, seconds, so that
    /// one blow is one hit.
    pub cooldown_s: f32,
    /// How long a character a blow damaged takes no further damage from either hand, seconds.
    pub damage_cooldown_s: f32,
    /// The slowest a hand must move toward a character before the contact for it to be a blow,
    /// metres per second: a hand resting on a character or pushing it slowly strikes nothing.
    pub min_approach_speed_mps: f32,
    /// The mass of the arm a blow carries, kilograms: a blow's impulse is this at the hand's speed
    /// into the character.
    pub blow_mass_kg: f32,
    /// How far above a character's root its chest is, metres: where a blow aims.
    pub chest_height_m: f32,
    /// The share of a knockdown from which a blow follows through.
    pub follow_through_strength: f32,
    /// How long a hand's drive is softened after a blow of at least
    /// [`follow_through_strength`](Self::follow_through_strength), seconds, so that the hand
    /// follows through rather than stopping dead on the character.
    pub follow_through_s: f32,
    /// The share of the drive's gain a hand keeps while it follows through, `0..=1`.
    pub follow_through_gain: f32,
    /// The impulse of the strongest hit pulse, newton seconds: an arm of
    /// [`blow_mass_kg`](Self::blow_mass_kg) at the hand's speed into what it met.
    pub full_pulse_impulse_ns: f32,
    /// The least share of the strongest hit pulse that pulses a hand, `0..=1`.
    pub min_pulse_share: f32,
}

impl HandHitsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "hand_bodies.hits.min_impulse_ns",
            self.min_impulse_ns,
            1.0e4,
        )?;
        positive(
            "hand_bodies.hits.stagger_impulse_ns",
            self.stagger_impulse_ns,
            1.0e4,
        )?;
        positive(
            "hand_bodies.hits.knockdown_impulse_ns",
            self.knockdown_impulse_ns,
            1.0e4,
        )?;
        positive("hand_bodies.hits.over_stagger", self.over_stagger, 10.0)?;
        positive("hand_bodies.hits.under_stagger", self.under_stagger, 10.0)?;
        positive(
            "hand_bodies.hits.under_knockdown",
            self.under_knockdown,
            10.0,
        )?;
        nonnegative(
            "hand_bodies.hits.damage_impulse_ns",
            self.damage_impulse_ns,
            1.0e4,
        )?;
        nonnegative("hand_bodies.hits.damage_scale", self.damage_scale, 100.0)?;
        range(
            "hand_bodies.hits.max_held_damage",
            f32::from(self.max_held_damage),
            1.0,
            f32::from(i16::MAX),
        )?;
        nonnegative("hand_bodies.hits.cooldown_s", self.cooldown_s, 60.0)?;
        nonnegative(
            "hand_bodies.hits.damage_cooldown_s",
            self.damage_cooldown_s,
            60.0,
        )?;
        nonnegative(
            "hand_bodies.hits.min_approach_speed_mps",
            self.min_approach_speed_mps,
            100.0,
        )?;
        positive("hand_bodies.hits.blow_mass_kg", self.blow_mass_kg, 1000.0)?;
        range(
            "hand_bodies.hits.chest_height_m",
            self.chest_height_m,
            -5.0,
            5.0,
        )?;
        nonnegative(
            "hand_bodies.hits.follow_through_strength",
            self.follow_through_strength,
            100.0,
        )?;
        nonnegative(
            "hand_bodies.hits.follow_through_s",
            self.follow_through_s,
            10.0,
        )?;
        range(
            "hand_bodies.hits.follow_through_gain",
            self.follow_through_gain,
            0.0,
            1.0,
        )?;
        positive(
            "hand_bodies.hits.full_pulse_impulse_ns",
            self.full_pulse_impulse_ns,
            1.0e4,
        )?;
        range(
            "hand_bodies.hits.min_pulse_share",
            self.min_pulse_share,
            0.0,
            1.0,
        )
    }
}

/// Held guns as physics bodies (see [`crate::controllers::physical_hands::physics::guns`]): a gun
/// in a hand whose body acts is a body of the gun's own shape and mass, joined to the holding
/// hand's body (and to the bracing hand's on its foregrip), so its weight drags the hands, it
/// collides along its whole length, and it strikes what it is swung into. The gun is near-rigid in
/// the hand: its body moves with the hand's on every step, its mass is held to its class's, and
/// both the body and the drawn gun keep within a couple of degrees of the hand's grip.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldGunsTuning {
    /// The lightest a gun body is made, kilograms.
    pub min_mass_kg: f32,
    /// The heaviest a gun body of each weight class is made, kilograms. A gun's own body carries
    /// its pickup's mass, which is far heavier than the gun (a rifle's ten kilograms), and a gun
    /// that outweighs the hand holding it swings the hand rather than the other way round.
    pub max_mass_by_class_kg: GunClassValues,
    /// The mass of a gun whose own body gives none, by weight class, kilograms.
    pub fallback_mass_kg: GunClassValues,
    /// The impulse a shot gives the gun body back along its barrel, by weight class, newton
    /// seconds.
    pub recoil_impulse_ns: GunClassValues,
    /// The share of a shot's impulse that also lifts the muzzle, `0..=1`.
    pub muzzle_rise: f32,
    /// How far a gun body may stray from its place in the holding hand before it is put back
    /// there at once, metres: past what the steering back to the drawn clamps can recover, as when
    /// the gun is pressed into a wall.
    pub stray_distance_m: f32,
    /// How far a gun body may turn from its place in the holding hand before it is put back there
    /// at once, degrees.
    pub stray_turn_deg: f32,
    /// The furthest the drawn gun is carried off the hand's grip by its body's stray, metres. The
    /// body is steered back within it on every step, so the body and the drawn gun agree.
    pub drawn_max_shift_m: f32,
    /// The furthest the drawn gun is turned off the hand's grip by its body's stray, degrees. The
    /// body is steered back within it on every step, which bounds a shot's muzzle rise too.
    pub drawn_max_turn_deg: f32,
    /// The least time between two pulses of a gun body's contacts in one hand, seconds: a gun
    /// rubbing along a wall touches it on every step.
    pub contact_pulse_interval_s: f32,
    /// How far a hand body may be off its palm for a gun to be seated on it, metres.
    pub settled_shift_m: f32,
    /// How far a hand body may be turned off its palm for a gun to be seated on it, radians.
    pub settled_turn_rad: f32,
    /// The furthest a gun held by its grip sits from the palm, metres: a seat further out is a gun
    /// still on its way into the hand, or one the hand's pose put somewhere else.
    pub max_grip_seat_m: f32,
    /// The furthest from a gun's origin a hand may hold it, metres: past the longest gun's muzzle.
    pub max_carry_reach_m: f32,
    /// How far a remade brace may move the support palm without muting its contacts, metres: a palm
    /// seated a little along the gun it already holds is no placement into the world.
    pub quiet_rebrace_m: f32,
    /// When the joint between a held gun and its bracing hand is remade, and when it breaks.
    pub brace: BraceTuning,
}

impl HeldGunsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("hand_bodies.guns.min_mass_kg", self.min_mass_kg, 100.0)?;
        self.max_mass_by_class_kg
            .validate("hand_bodies.guns.max_mass_by_class_kg", true)?;
        self.fallback_mass_kg
            .validate("hand_bodies.guns.fallback_mass_kg", true)?;
        self.recoil_impulse_ns
            .validate("hand_bodies.guns.recoil_impulse_ns", false)?;
        range("hand_bodies.guns.muzzle_rise", self.muzzle_rise, 0.0, 1.0)?;
        nonnegative(
            "hand_bodies.guns.stray_distance_m",
            self.stray_distance_m,
            10.0,
        )?;
        range(
            "hand_bodies.guns.stray_turn_deg",
            self.stray_turn_deg,
            0.0,
            180.0,
        )?;
        nonnegative(
            "hand_bodies.guns.drawn_max_shift_m",
            self.drawn_max_shift_m,
            1.0,
        )?;
        range(
            "hand_bodies.guns.drawn_max_turn_deg",
            self.drawn_max_turn_deg,
            0.0,
            180.0,
        )?;
        nonnegative(
            "hand_bodies.guns.contact_pulse_interval_s",
            self.contact_pulse_interval_s,
            60.0,
        )?;
        nonnegative(
            "hand_bodies.guns.settled_shift_m",
            self.settled_shift_m,
            10.0,
        )?;
        range(
            "hand_bodies.guns.settled_turn_rad",
            self.settled_turn_rad,
            0.0,
            PI,
        )?;
        nonnegative(
            "hand_bodies.guns.max_grip_seat_m",
            self.max_grip_seat_m,
            10.0,
        )?;
        // The reach of a gun held by another part is clamped between the two.
        range(
            "hand_bodies.guns.max_carry_reach_m",
            self.max_carry_reach_m,
            self.max_grip_seat_m,
            10.0,
        )?;
        nonnegative(
            "hand_bodies.guns.quiet_rebrace_m",
            self.quiet_rebrace_m,
            10.0,
        )?;
        self.brace.validate()
    }
}

/// One value per gun weight class.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunClassValues {
    pub sidearm: f32,
    pub smg: f32,
    pub rifle: f32,
    pub heavy: f32,
}

impl GunClassValues {
    #[cfg(test)]
    pub(crate) const fn new(sidearm: f32, smg: f32, rifle: f32, heavy: f32) -> Self {
        Self {
            sidearm,
            smg,
            rifle,
            heavy,
        }
    }

    /// Check each class's value at `path`: positive when `mass`, and otherwise not negative.
    fn validate(&self, path: &str, mass: bool) -> Result<(), Invalid> {
        let check = if mass { positive } else { nonnegative };
        check(&format!("{path}.sidearm"), self.sidearm, 1000.0)?;
        check(&format!("{path}.smg"), self.smg, 1000.0)?;
        check(&format!("{path}.rifle"), self.rifle, 1000.0)?;
        check(&format!("{path}.heavy"), self.heavy, 1000.0)
    }
}

/// The joint between a held gun and its bracing hand: when it is remade, and when it breaks.
///
/// It is remade (see
/// [`brace_due`](crate::controllers::physical_hands::physics::guns::math::brace_due)) once the palm
/// frame the hand is driven to has moved or turned past [`distance_m`](Self::distance_m) or
/// [`angle_rad`](Self::angle_rad) from the frame the joint holds, and no sooner than
/// [`min_interval_s`](Self::min_interval_s) after the joint was made. A joint kept through a larger
/// change holds a stale contact as the hand slides or turns its grip around the gun; a joint
/// remade on every small change churns, since ordinary two-handed aim moves the support hand on
/// the gun all the time.
///
/// It breaks (see
/// [`brace_broken`](crate::controllers::physical_hands::physics::guns::math::brace_broken)), and the
/// two-handed hold lets go, once the tracked palm is further than
/// [`break_distance_m`](Self::break_distance_m) from where the joint holds the palm on the gun for
/// [`break_steps`](Self::break_steps) steps in a row.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BraceTuning {
    /// The palm frame's furthest move, metres.
    pub distance_m: f32,
    /// The palm frame's largest turn, radians.
    pub angle_rad: f32,
    /// The shortest a joint stands before it is remade for a moved palm, seconds.
    pub min_interval_s: f32,
    /// The furthest the tracked palm may be from its place on the gun before the brace breaks,
    /// metres.
    pub break_distance_m: f32,
    /// How many steps in a row the tracked support palm must be past the break distance before the
    /// brace breaks: a single stray sample of the tracking breaks nothing.
    pub break_steps: u8,
}

impl BraceTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("hand_bodies.guns.brace.distance_m", self.distance_m, 10.0)?;
        range("hand_bodies.guns.brace.angle_rad", self.angle_rad, 0.0, PI)?;
        nonnegative(
            "hand_bodies.guns.brace.min_interval_s",
            self.min_interval_s,
            60.0,
        )?;
        nonnegative(
            "hand_bodies.guns.brace.break_distance_m",
            self.break_distance_m,
            10.0,
        )?;
        range(
            "hand_bodies.guns.brace.break_steps",
            f32::from(self.break_steps),
            1.0,
            f32::from(u8::MAX),
        )
    }
}

/// How the tick's two halves hand the palms and the results to each other.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepTuning {
    /// How long a published pose stays current for a reader between the steps, milliseconds: the
    /// aim update runs every simulation tick while the hands act, so an older one means they have
    /// stopped.
    pub stale_after_ms: u64,
    /// How long a hand may stop acting before its body leaves the simulation, milliseconds, so
    /// that a moment's lost tracking or a flicker of the mode does not churn the world. A hand
    /// that takes a gun or carries what it grabbed leaves at once.
    pub park_after_ms: u64,
    /// The longest a published pose is carried forward by its velocity, seconds.
    pub max_lead_s: f32,
    /// The least share of a hand's strength that pulses it, `0..=1`.
    pub min_pressure_load: f32,
    /// How often the drive is written to the debug log while a hand acts, milliseconds.
    pub readout_period_ms: u64,
}

impl StepTuning {
    /// [`stale_after_ms`](Self::stale_after_ms) as a freshness window.
    pub fn stale_after(&self) -> Window {
        Window::from_millis(self.stale_after_ms)
    }

    /// [`park_after_ms`](Self::park_after_ms) as a duration.
    pub fn park_after(&self) -> Duration {
        Duration::from_millis(self.park_after_ms)
    }

    /// [`readout_period_ms`](Self::readout_period_ms) as a duration.
    pub fn readout_period(&self) -> Duration {
        Duration::from_millis(self.readout_period_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        range(
            "step.stale_after_ms",
            self.stale_after_ms as f32,
            0.0,
            10_000.0,
        )?;
        range(
            "step.park_after_ms",
            self.park_after_ms as f32,
            0.0,
            60_000.0,
        )?;
        nonnegative("step.max_lead_s", self.max_lead_s, 1.0)?;
        range("step.min_pressure_load", self.min_pressure_load, 0.0, 1.0)?;
        range(
            "step.readout_period_ms",
            self.readout_period_ms as f32,
            0.0,
            3_600_000.0,
        )
    }
}

/// How a free hand is drawn at its body against its controller.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandRenderTuning {
    /// The turn between a body and its controller's palm that is not drawn, radians: a free hand
    /// keeps the controller's own rotation.
    pub turn_deadband_rad: f32,
    /// The largest turn a hand is drawn at against its controller, radians.
    pub max_turn_rad: f32,
}

impl HandRenderTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("render.turn_deadband_rad", self.turn_deadband_rad, 0.0, PI)?;
        range("render.max_turn_rad", self.max_turn_rad, 0.0, PI)
    }
}

/// The rig that carries the hands: the motion the drive feeds forward. What counts as a jump of the
/// rig is the physics substrate's (`jc3vr_body::controllers::physics::rig`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RigTuning {
    /// The fastest carrier the drive feeds forward, metres per second: past a fall's terminal
    /// speed, so an estimate thrown by a jump of the rig cannot fling a hand.
    pub max_carrier_speed_mps: f32,
}

impl RigTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative(
            "rig.max_carrier_speed_mps",
            self.max_carrier_speed_mps,
            1000.0,
        )
    }
}

/// When the bodies ignore their contacts after a move, and when one is remade.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleTuning {
    /// How many steps a body put somewhere, rather than moved there, ignores its contacts, at the
    /// game's tick ([`contact::mute_for`] scales it at another sim rate).
    ///
    /// [`contact::mute_for`]: crate::controllers::physical_hands::physics::contact::mute_for
    pub placed_mute_steps: u64,
    /// How many steps a body the rig carried ignores its contacts, at the game's tick
    /// ([`contact::mute_for`] scales it at another sim rate).
    ///
    /// [`contact::mute_for`]: crate::controllers::physical_hands::physics::contact::mute_for
    pub rig_mute_steps: u64,
    /// The relative change in a body's size or mass that remakes it.
    pub spec_tolerance: f32,
}

impl LifecycleTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "lifecycle.placed_mute_steps",
            self.placed_mute_steps as f32,
            0.0,
            1000.0,
        )?;
        range(
            "lifecycle.rig_mute_steps",
            self.rig_mute_steps as f32,
            0.0,
            1000.0,
        )?;
        nonnegative("lifecycle.spec_tolerance", self.spec_tolerance, 1.0)
    }
}

/// The drawn torso's capsules (see [`crate::controllers::physical_hands::physics::drawn_body`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawnBodyTuning {
    /// How long after it was last asked about a resting body is forgotten, milliseconds: its hold
    /// ended.
    pub forget_after_ms: u64,
    /// How near a held body's box must come to a capsule to count as still resting on it, metres:
    /// the broad phase's box already reaches a little past the body.
    pub reach_margin_m: f32,
    /// How much of the keep-out's radius a capsule takes: slimmer than the drawn body, so a hand
    /// reaching for a holster or a pouch meets nothing first.
    pub radius_share: f32,
    /// The thinnest a capsule is made, metres.
    pub min_radius_m: f32,
    /// The capsules' mass at creation, kilograms; keyframed, they move as driven whatever it is.
    pub mass_kg: f32,
    /// How far off its target a capsule is put there rather than flown, metres.
    pub snap_m: f32,
}

impl DrawnBodyTuning {
    /// [`forget_after_ms`](Self::forget_after_ms) as a duration.
    pub fn forget_after(&self) -> Duration {
        Duration::from_millis(self.forget_after_ms)
    }

    fn validate(&self) -> Result<(), Invalid> {
        range(
            "drawn_body.forget_after_ms",
            self.forget_after_ms as f32,
            0.0,
            60_000.0,
        )?;
        nonnegative("drawn_body.reach_margin_m", self.reach_margin_m, 1.0)?;
        nonnegative("drawn_body.radius_share", self.radius_share, 2.0)?;
        positive("drawn_body.min_radius_m", self.min_radius_m, 1.0)?;
        positive("drawn_body.mass_kg", self.mass_kg, 10_000.0)?;
        nonnegative("drawn_body.snap_m", self.snap_m, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PHYSICS.get().validate(), Ok(()));
    }
}
