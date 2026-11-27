//! The procedural body's tuning (`docs/mod/body/procedural-body.md`), read from the shipped
//! `tuning/procedural.toml` ([`PROCEDURAL`]): the hand-over's blends and the flight's hold, the
//! torso's limits and shares, the chest's clearance of the camera, the arms' keep-out, the elbow
//! hints, the clavicles, the square stance, the hips' follower, the in-air debounce, the hit
//! reaction's hold, the wingsuit's arms, the arms' twist, and the relaxed legs' swing.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::tuning::Millis;

jc3vr_core::tuning_file! {
    /// The procedural body's tuning file.
    pub static PROCEDURAL: ProceduralTuning = "procedural.toml";
}

/// The procedural body's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralTuning {
    pub blend: BlendTuning,
    pub torso: TorsoTuning,
    pub(crate) chest_clearance: ChestClearanceTuning,
    pub keep_out: KeepOutTuning,
    pub elbow_hint: ElbowHintTuning,
    pub clavicles: ClavicleTuning,
    pub stance: StanceTuning,
    pub hips: HipsTuning,
    pub in_air: InAirTuning,
    pub(crate) hit_reaction: HitReactionTuning,
    pub(crate) wings: WingsTuning,
    pub twist: TwistTuning,
    pub(crate) leg_swing: LegSwingTuning,
}

impl Tuning for ProceduralTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.blend.validate()?;
        self.torso.validate()?;
        self.chest_clearance.validate()?;
        self.keep_out.validate()?;
        self.elbow_hint.validate()?;
        self.clavicles.validate()?;
        self.stance.validate()?;
        positive("hips.smoothing_s", self.hips.smoothing_s, 10.0)?;
        self.in_air.validate()?;
        self.hit_reaction
            .hold_ms
            .validate("hit_reaction.hold_ms", 10_000)?;
        self.wings.validate()?;
        self.twist.validate()?;
        self.leg_swing.validate()
    }
}

/// How a region of the body hands over between the animation and the procedural pose.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendTuning {
    /// How long a region of the body takes to hand over from the animation to the procedural pose,
    /// seconds: the smoothing time of its spring.
    pub(crate) in_s: f32,
    /// How long a region takes to hand back to the animation, seconds; shorter, since the game's
    /// authored motion (a vehicle entry, a climb, a fall) is under way when it does.
    pub(crate) out_s: f32,
    /// The longest step the body takes, seconds: a hitch is not a long blend.
    pub max_step_s: f32,
    /// How long a flight keeps the torso procedural after a jump or the parachute, seconds of the
    /// simulation's time.
    pub(crate) flight_hold_s: f32,
}

impl BlendTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("blend.in_s", self.in_s, 10.0)?;
        positive("blend.out_s", self.out_s, 10.0)?;
        positive("blend.max_step_s", self.max_step_s, 1.0)?;
        nonnegative("blend.flight_hold_s", self.flight_hold_s, 30.0)
    }
}

/// The torso's limits and the shares the spine's bones and the neck take.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TorsoTuning {
    /// How far the chest turns toward the player's torso, at most, degrees.
    pub(crate) chest_turn_limit_deg: f32,
    /// How far the spine leans from upright, at most, degrees.
    pub(crate) lean_limit_deg: f32,
    /// The share of a gait clip's forward lean the torso takes out on the ground before following
    /// the player's head.
    pub(crate) delean: f32,
    /// How long the de-lean takes to ease in or out across a change of case, seconds.
    pub(crate) delean_blend_s: f32,
    /// The spine's tilt from the vertical within which it takes the whole of the torso's turn and
    /// the look down, radians.
    pub(crate) upright_full_rad: f32,
    /// The spine's tilt from the vertical past which it takes none of them, radians.
    pub(crate) upright_none_rad: f32,
    /// The share of the head's look down the chest pitches by.
    pub(crate) chest_pitch_share: f32,
    /// The furthest the chest pitches forward for a look down, radians.
    pub(crate) max_chest_pitch_rad: f32,
    /// The share of the head's turn against the chest the neck takes.
    pub(crate) neck_share: f32,
    /// Each spine bone's share of the chest's pitch, from the base: the upper back takes it.
    pub(crate) pitch_shares: [f32; 4],
    /// Each spine bone's share of the swing toward the neck target, from the base.
    pub(crate) swing_shares: [f32; 4],
    /// Each spine bone's share of the chest's twist, from the base: more of it higher up, as in a
    /// real spine.
    pub(crate) twist_shares: [f32; 4],
}

impl TorsoTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "torso.chest_turn_limit_deg",
            self.chest_turn_limit_deg,
            0.0,
            180.0,
        )?;
        range("torso.lean_limit_deg", self.lean_limit_deg, 0.0, 180.0)?;
        range("torso.delean", self.delean, 0.0, 1.0)?;
        positive("torso.delean_blend_s", self.delean_blend_s, 10.0)?;
        range(
            "torso.upright_full_rad",
            self.upright_full_rad,
            0.0,
            std::f32::consts::FRAC_PI_2,
        )?;
        // The share eases from the first to the second, so the second must lie beyond it.
        range(
            "torso.upright_none_rad",
            self.upright_none_rad,
            self.upright_full_rad + 1e-3,
            std::f32::consts::FRAC_PI_2,
        )?;
        range("torso.chest_pitch_share", self.chest_pitch_share, 0.0, 1.0)?;
        range(
            "torso.max_chest_pitch_rad",
            self.max_chest_pitch_rad,
            0.0,
            std::f32::consts::FRAC_PI_2,
        )?;
        range("torso.neck_share", self.neck_share, 0.0, 1.0)?;
        for (name, shares) in [
            ("torso.pitch_shares", self.pitch_shares),
            ("torso.swing_shares", self.swing_shares),
            ("torso.twist_shares", self.twist_shares),
        ] {
            for (i, share) in shares.into_iter().enumerate() {
                range(&format!("{name}[{i}]"), share, 0.0, 1.0)?;
            }
        }
        Ok(())
    }
}

/// How the chest is kept out of the camera's near field.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChestClearanceTuning {
    /// A point of the chest higher than this below the camera's eyes is kept out of the near
    /// field, metres.
    pub below_m: f32,
    /// How far behind the eyes, along the view's level forward, the chest is kept, metres.
    pub back_m: f32,
    /// The furthest the clearance swings the spine back in all, radians. A camera far inside the
    /// chest would otherwise fold the spine over backwards to clear it.
    pub(crate) limit_rad: f32,
}

impl ChestClearanceTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("chest_clearance.below_m", self.below_m, 1.0)?;
        nonnegative("chest_clearance.back_m", self.back_m, 1.0)?;
        range(
            "chest_clearance.limit_rad",
            self.limit_rad,
            0.0,
            std::f32::consts::FRAC_PI_2,
        )
    }
}

/// The torso the wrist and elbow targets are kept out of.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeepOutTuning {
    /// The chest capsule's radius, metres.
    pub(crate) chest_radius_m: f32,
    /// The abdomen capsule's radius, metres.
    pub(crate) abdomen_radius_m: f32,
    /// How far outside the torso a pushed wrist or elbow lands, metres: the hand's half-thickness.
    pub(crate) hand_margin_m: f32,
}

impl KeepOutTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("keep_out.chest_radius_m", self.chest_radius_m, 0.5)?;
        nonnegative("keep_out.abdomen_radius_m", self.abdomen_radius_m, 0.5)?;
        nonnegative("keep_out.hand_margin_m", self.hand_margin_m, 0.2)
    }
}

/// The elbow hints: where an arm's elbow naturally sits for a wrist target
/// ([`crate::controllers::body::procedural::solver::arms::elbow_hint`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElbowHintTuning {
    /// The elbow hint's reach: how strongly the solve pulls the elbow toward the hint.
    pub reach: f32,
    /// How upright the written chest must be for the elbow hints, as
    /// [`upright_share`](crate::controllers::body::procedural::solver::anchored::upright_share)
    /// measures it: the hints point down and back from the chest, which only holds near upright.
    pub upright: f32,
    /// The elbow pole's lean down.
    pub(crate) pole_down: f32,
    /// The elbow pole's lean out to the arm's side.
    pub(crate) pole_out: f32,
    /// The elbow pole's lean back.
    pub(crate) pole_back: f32,
    /// How much further out a reach up or across lifts the elbow, on top of
    /// [`pole_out`](Self::pole_out).
    pub(crate) pole_reach_out: f32,
    /// The share of the pole left across the reach below which the fallback pole (straight back)
    /// takes over, fully at none.
    pub(crate) pole_fallback_below: f32,
}

impl ElbowHintTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("elbow_hint.reach", self.reach, 0.0, 1.0)?;
        range("elbow_hint.upright", self.upright, 0.0, 1.0)?;
        nonnegative("elbow_hint.pole_down", self.pole_down, 10.0)?;
        nonnegative("elbow_hint.pole_out", self.pole_out, 10.0)?;
        nonnegative("elbow_hint.pole_back", self.pole_back, 10.0)?;
        nonnegative("elbow_hint.pole_reach_out", self.pole_reach_out, 10.0)?;
        // A divisor.
        positive(
            "elbow_hint.pole_fallback_below",
            self.pole_fallback_below,
            1.0,
        )
    }
}

/// The clavicles' reach with the hands
/// ([`crate::controllers::body::procedural::solver::clavicles`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClavicleTuning {
    /// How far a clavicle lifts for a hand overhead, at most, degrees.
    pub elevation_deg: f32,
    /// How far a clavicle drops for a hand reaching far down, at most, degrees.
    pub(crate) depression_deg: f32,
    /// How far a clavicle swings forward for a hand reaching far ahead or across, at most,
    /// degrees.
    pub(crate) protraction_deg: f32,
    /// The share of the arm's length at which a reach ahead or down starts to take the clavicle;
    /// it takes it fully at the arm's full length.
    pub(crate) reach_start: f32,
    /// The share of the arm's length from which a hand overhead starts to lift the clavicle, by
    /// how straight the arm is.
    pub(crate) lift_extension_start: f32,
    /// How steeply up the hand must be from the shoulder before it lifts it: the sine of the angle
    /// above the horizontal.
    pub(crate) lift_start: f32,
    /// How steeply down the hand must be from the shoulder before it lowers it: the sine of the
    /// angle below the horizontal.
    pub(crate) lower_start: f32,
    /// How much a reach across the body counts against a reach ahead: less, since the chest is in
    /// the way.
    pub(crate) across_share: f32,
    /// How quickly the clavicles follow the hands, seconds: the time constant of their follower.
    pub(crate) follow_s: f32,
}

impl ClavicleTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("clavicles.elevation_deg", self.elevation_deg, 0.0, 90.0)?;
        range("clavicles.depression_deg", self.depression_deg, 0.0, 90.0)?;
        range("clavicles.protraction_deg", self.protraction_deg, 0.0, 90.0)?;
        // Each start must leave room below the full reach it eases toward.
        range("clavicles.reach_start", self.reach_start, 0.0, 0.99)?;
        range(
            "clavicles.lift_extension_start",
            self.lift_extension_start,
            0.0,
            0.99,
        )?;
        range("clavicles.lift_start", self.lift_start, 0.0, 0.99)?;
        range("clavicles.lower_start", self.lower_start, 0.0, 0.99)?;
        range("clavicles.across_share", self.across_share, 0.0, 1.0)?;
        positive("clavicles.follow_s", self.follow_s, 10.0)
    }
}

/// Rico's feet squared under him while he stands idle on foot
/// ([`crate::controllers::body::procedural::stance`]). The IK body's steps land on the same
/// stance.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StanceTuning {
    /// How far apart the ankles stand, metres.
    pub width_m: f32,
    /// How far each foot turns out from the facing, degrees.
    pub toe_out_deg: f32,
    /// How long the stance takes to blend in once Rico stands still, and out once he moves,
    /// seconds: the time constant of its follower.
    pub blend_s: f32,
}

impl StanceTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("stance.width_m", self.width_m, 1.0)?;
        range("stance.toe_out_deg", self.toe_out_deg, -45.0, 45.0)?;
        positive("stance.blend_s", self.blend_s, 10.0)
    }
}

/// The follower of the clip's hips ([`crate::controllers::body::procedural::hips`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HipsTuning {
    /// The follower's time constant, seconds: long against a stride, so the walk's swing of the
    /// hips averages out, and short against standing still.
    pub smoothing_s: f32,
}

/// When the body counts as in the air off its support
/// ([`crate::controllers::body::procedural::state`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InAirTuning {
    /// How long the proxy must be off its support, with no jump and no fall detected, before the
    /// body counts as in the air, seconds: a stair or a bump lifts it for a tick without a fall.
    pub(crate) debounce_s: f32,
    /// How long the proxy must be off its support while the movement task still runs, seconds: the
    /// proxy loses its support for a few ticks as a walk comes to a stop, and drops a few
    /// centimetres onto the ground, without leaving it.
    pub(crate) debounce_on_foot_s: f32,
}

impl InAirTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("in_air.debounce_s", self.debounce_s, 2.0)?;
        nonnegative("in_air.debounce_on_foot_s", self.debounce_on_foot_s, 2.0)
    }
}

/// The hit reactions ([`crate::controllers::body::procedural::signals`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HitReactionTuning {
    /// How long a hit reaction (a stumble or a knock-back) holds the body on the animation after
    /// its act: the game keeps no state for the reaction's length.
    pub(crate) hold_ms: Millis,
}

/// The wingsuit's arms ([`crate::controllers::body::procedural::wings`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WingsTuning {
    /// The least the player's reach is scaled by onto Rico's arm.
    pub(crate) arm_scale_min: f32,
    /// The most the player's reach is scaled by onto Rico's arm.
    pub(crate) arm_scale_max: f32,
    /// The wrists spread at least this far apart across the ground give the torso's heading,
    /// metres.
    pub(crate) spread_m: f32,
    /// The follower of the head in flight, when the wrists are not spread.
    pub follow: WingFollowTuning,
}

impl WingsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("wings.arm_scale_min", self.arm_scale_min, 1.0)?;
        range(
            "wings.arm_scale_max",
            self.arm_scale_max,
            self.arm_scale_min,
            10.0,
        )?;
        positive("wings.spread_m", self.spread_m, 3.0)?;
        let follow = &self.follow;
        range("wings.follow.turn_deg", follow.turn_deg, 0.0, 180.0)?;
        range(
            "wings.follow.sustain_deg",
            follow.sustain_deg,
            0.0,
            follow.turn_deg,
        )?;
        nonnegative("wings.follow.sustain_s", follow.sustain_s, 30.0)?;
        positive("wings.follow.slew_deg_s", follow.slew_deg_s, 1000.0)
    }
}

/// The follower of the head in flight: no turn for a look alone, however far, a slow turn after a
/// look held to one side, and a slow settle under a steady head. The rest of the follower is the
/// standing yaw follow's ([`body_rig::yaw::YawTuning`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WingFollowTuning {
    /// [`body_rig::yaw::YawTuning::turn_deg`].
    pub turn_deg: f32,
    /// [`body_rig::yaw::YawTuning::sustain_deg`].
    pub sustain_deg: f32,
    /// [`body_rig::yaw::YawTuning::sustain_s`].
    pub sustain_s: f32,
    /// [`body_rig::yaw::YawTuning::slew_deg_s`].
    pub slew_deg_s: f32,
}

/// The arms' twist ([`crate::controllers::body::procedural::solver::twist`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TwistTuning {
    /// How far a hand turns from its forearm before the upper arm takes a share, radians: about the
    /// forearm's own range of pronation from the reference.
    pub(crate) forearm_limit_rad: f32,
    /// The share of a hand's turn past [`forearm_limit_rad`](Self::forearm_limit_rad) the upper arm
    /// takes.
    pub(crate) upper_arm_share: f32,
}

impl TwistTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range(
            "twist.forearm_limit_rad",
            self.forearm_limit_rad,
            0.0,
            std::f32::consts::PI,
        )?;
        range("twist.upper_arm_share", self.upper_arm_share, 0.0, 1.0)
    }
}

/// The relaxed legs' swing on the grapple's line: a damped pendulum hung from the root, which lags
/// the root's acceleration and settles back under it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LegSwingTuning {
    /// The pendulum's length, metres: with gravity, its period.
    pub length_m: f32,
    /// The pendulum's damping, per second.
    pub damping: f32,
    /// The most of the root's horizontal acceleration the pendulum feels, metres per second
    /// squared.
    pub(crate) max_acceleration: f32,
    /// The fastest the pendulum swings, offset per second.
    pub(crate) max_rate: f32,
    /// The furthest the pendulum swings from straight down, as an offset across the unit down.
    pub(crate) max_offset: f32,
    /// The longest integration step, seconds: a tick is split into steps no longer than this.
    pub(crate) substep_s: f32,
    /// A root moved further than this in one tick has been carried, not swung, and the pendulum
    /// starts again under it, metres.
    pub jump_m: f32,
}

impl LegSwingTuning {
    fn validate(&self) -> Result<(), Invalid> {
        // Divisors.
        positive("leg_swing.length_m", self.length_m, 10.0)?;
        positive("leg_swing.substep_s", self.substep_s, 0.1)?;
        nonnegative("leg_swing.damping", self.damping, 100.0)?;
        nonnegative("leg_swing.max_acceleration", self.max_acceleration, 1000.0)?;
        nonnegative("leg_swing.max_rate", self.max_rate, 100.0)?;
        nonnegative("leg_swing.max_offset", self.max_offset, 10.0)?;
        positive("leg_swing.jump_m", self.jump_m, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(PROCEDURAL.get().validate(), Ok(()));
    }
}
