//! The grapple snatch's tuning, read from the shipped `tuning/snatch.toml` ([`SNATCH`]), and the
//! settings one snatch tick reads.

use jc3vr_body::controllers::overlay::config::OverlayConfig;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::{snatch::landing::LandingTuning, yank::YankConfig};

jc3vr_core::tuning_file! {
    /// The grapple snatch's tuning file.
    pub static SNATCH: SnatchTuning = "snatch.toml";
}

/// The settings one snatch tick reads: the fields of the groups it consults. The bindings fill it
/// from the snapshot of the settings they take for the tick.
#[derive(Copy, Clone)]
pub struct SnatchSettings {
    /// The world-space overlay, which draws the armed snatch's latch.
    pub overlay: OverlayConfig,
}

/// The yank that fires the snatch, measured along the grapple hand's aim at the grip edge
/// ([`reach_along`](crate::controllers::yank::reach_along)), from the tuning's
/// [`yank`](SnatchTuning::yank).
pub(crate) fn snatch_yank() -> YankConfig {
    let yank = &SNATCH.get().yank;
    YankConfig {
        min_distance_m: yank.min_distance_m,
        min_speed_mps: yank.min_speed_mps,
        window_s: yank.window_s,
        cooldown_s: yank.cooldown_s,
    }
}

/// The yank that fires the snatch (see [`YankConfig`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnatchYankTuning {
    /// The shortest pull back along the aim that counts, metres.
    pub min_distance_m: f32,
    /// The slowest average speed of the pull that counts, metres per second.
    pub min_speed_mps: f32,
    /// How far back the pull is measured from, seconds.
    pub window_s: f32,
    /// How long after a yank before another can count, seconds.
    pub cooldown_s: f32,
}

/// The held line's tuning: the swing of a heavy object, and of a weapon or a light object while B
/// is held (`controllers::snatch::sling`).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwingTuning {
    /// The heaviest loose object the snatch offers, kilograms. One a hand can carry is pulled into
    /// the hand; one heavier, up to this, is swung on the line and never comes to the hand.
    pub(crate) ceiling_kg: f32,
    /// The line's stiffness: the pull per metre it is stretched past its held length, per second
    /// squared, so the same stretch asks the same acceleration of any mass, up to its strength.
    pub(crate) stiffness_per_s2: f32,
    /// The line's damping of the target's speed away from the hand, per second.
    pub damping_per_s: f32,
    /// The most tension the line holds on a target of
    /// [`reference_mass_kg`](Self::reference_mass_kg), newtons.
    pub(crate) strength_n: f32,
    /// How the line's strength grows with the target's mass, `0..=1`: the strength is
    /// [`strength_n`](Self::strength_n) times the mass over
    /// [`reference_mass_kg`](Self::reference_mass_kg) to this power (`line_strength_n`). At 1 every
    /// mass swings alike; at 0 the line pulls every mass equally hard, so a heavy one barely moves.
    pub(crate) mass_scaling: f32,
    /// The line's strength while the other hand braces the grapple hand's wrist, as a multiple of
    /// one hand's.
    pub(crate) brace_factor: f32,
    /// The length the line reels in to once the hook bites, metres: a target bitten farther off
    /// is drawn in to it and then swung around the hand. A target bitten nearer keeps its length.
    pub(crate) swing_length_m: f32,
    /// How fast the line reels in to [`swing_length_m`](Self::swing_length_m), metres per second.
    /// The line cannot push, so a target reeled in keeps its speed once the reel stops and coasts
    /// on toward the hand until friction, gravity, or a swing takes it; zero keeps the length the
    /// hook bit at.
    pub(crate) reel_mps: f32,
    /// The mass the line's [`strength_n`](Self::strength_n) is given at, kilograms.
    pub(crate) reference_mass_kg: f32,
    /// How far inside the target the reel may take the line in, metres: at the default stiffness,
    /// a pull of about the strength of any line up to a few hundred kilograms.
    pub(crate) reel_lead_m: f32,
    /// How far the target may be stretched past the held length before the line breaks, metres.
    pub(crate) max_stretch_m: f32,
    /// The line's anchor moving faster than this is a jump of the hand's tracking, not a swing,
    /// metres per second: the step is refused.
    pub(crate) max_anchor_speed_mps: f32,
}

/// A ragdoll's line: reeled in hard toward the grapple hand, not swung.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RagdollYankTuning {
    /// The length the line reels in to, metres: about an arm's reach from the grapple hand.
    pub length_m: f32,
    /// How fast the line reels in, metres per second: a yank, not the swing's draw.
    pub mps: f32,
    /// How fast the reel slows toward its length, per second: the reel's speed is what is left of
    /// the line times this, between the swing's reel and [`mps`](Self::mps), so a body bitten far
    /// off comes at the yank's speed and eases in over its last two metres.
    pub(crate) ease_per_s: f32,
}

/// The armed snatch's marker and the held line's drawing ([`crate::controllers::snatch`]'s latch).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LatchTuning {
    /// The line's width, metres.
    pub(crate) line_width_m: f32,
    /// How much wider the held line is at its strength, metres.
    pub(crate) strained_width_m: f32,
    /// The line's opacity, `0..=1`, fainter than the dot so that it does not hide the target.
    pub(crate) line_alpha: f32,
    /// The dot's apparent diameter, radians.
    pub(crate) dot_rad: f32,
    /// The ring's apparent diameter, radians.
    pub(crate) ring_rad: f32,
    /// The opacity of the marker where the scene hides it, `0..=1`.
    pub(crate) occluded_alpha: f32,
}

/// A gun tossed to the player's feet when no hand can catch it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TossTuning {
    /// Where the gun lands, metres in front of the player toward where it came from.
    pub(crate) land_m: f32,
    /// The toss's speed over the ground, metres per second.
    pub speed_mps: f32,
    /// The toss's shortest and longest flight, seconds.
    pub(crate) min_flight_s: f32,
    pub(crate) max_flight_s: f32,
}

/// The grapple snatch's tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnatchTuning {
    /// How near the grapple hand's wrist the other hand's wrist must be for its grip to brace it,
    /// metres.
    pub(crate) brace_take_m: f32,
    /// How far the bracing hand's wrist may drift from the grapple hand's before the brace breaks,
    /// metres.
    pub(crate) brace_break_m: f32,
    /// The farthest a target may be, metres. The hook's line caps it.
    pub range_m: f32,
    /// How far off the grapple's ray a gun in a character's hands or holster, or a weapon lying in
    /// the world, may be and still be chosen, degrees.
    pub(crate) cone_deg: f32,
    /// How much wider the cone is for a weapon lying in the world at the range, degrees, in
    /// proportion to its distance ([`floor_cone`](crate::controllers::snatch::rules::floor_cone)).
    pub(crate) floor_cone_widen_deg: f32,
    /// The speed the line yanks the target toward the hand at while it is farther than
    /// [`approach_m`](Self::approach_m), metres per second.
    pub(crate) line_speed_mps: f32,
    /// The speed the line asks of the target at the hand, metres per second: over the last
    /// [`approach_m`](Self::approach_m) the line's speed falls in proportion to the distance from
    /// [`line_speed_mps`](Self::line_speed_mps) to this, so the target still arrives hard but
    /// slower than it flew ([`line_speed`](crate::controllers::snatch::rules::line_speed)).
    pub(crate) arrival_speed_mps: f32,
    /// The hardest the line accelerates the target, metres per second squared: the line's pull is
    /// the target's mass times this, so a heavy target comes as hard as a light one, and a target
    /// a step away reaches its speed within a tick or two.
    pub(crate) line_accel_mps2: f32,
    /// How near the hand the final approach starts, metres. Within it the line's speed falls
    /// toward [`arrival_speed_mps`](Self::arrival_speed_mps), its arc fades to nothing, and it
    /// pulls at the target's centre rather than at the hook, so the spin the off-centre pull gave
    /// the target does not carry the centre off the line when it goes slack.
    pub approach_m: f32,
    /// How high the line aims above the hand, as a share of the distance still to go, so a far
    /// target clears the ground on its way. It fades to nothing over the final approach.
    pub arc: f32,
    /// How long before the target would reach the hand the line goes slack, seconds: the slack
    /// radius is the target's speed times this, kept between
    /// [`slack_min_m`](Self::slack_min_m) and [`slack_max_m`](Self::slack_max_m)
    /// ([`slack_radius`](crate::controllers::snatch::rules::slack_radius)). The target then flies
    /// on by its own momentum for about two ticks, wherever the hand goes.
    pub(crate) coast_s: f32,
    pub(crate) slack_min_m: f32,
    pub(crate) slack_max_m: f32,
    /// How near a hand's palm the target's centre must pass for the hand to catch it, metres:
    /// the hand's own reach around the target, with no pull toward the hand.
    pub(crate) catch_zone_m: f32,
    /// The mass at which a target's catch zone reaches
    /// [`heavy_catch_zone_m`](Self::heavy_catch_zone_m) and the line's speed falls to
    /// [`heavy_speed_share`](Self::heavy_speed_share), kilograms. A target at or below half of it
    /// keeps the light values. A heavy target's centre cannot come as near the palm as a light
    /// one's, so the hand catches it from further out, and it needs less speed to arrive without
    /// passing the hand.
    pub(crate) heavy_mass_kg: f32,
    /// The catch zone for a heavy target, metres.
    pub(crate) heavy_catch_zone_m: f32,
    /// The line's speed for a heavy target as a share of [`line_speed_mps`](Self::line_speed_mps),
    /// `0..=1`.
    pub(crate) heavy_speed_share: f32,
    /// How long after the target was last within a hand's catch zone the hand's grip may still
    /// close on it and catch it, seconds.
    pub(crate) catch_grace_s: f32,
    /// How long a slack target may fly before it counts as missed, seconds.
    pub(crate) max_slack_s: f32,
    /// The fastest the line's pull at the hook may spin the target, radians per second.
    pub(crate) max_spin_rps: f32,
    /// How little the target may move over [`stall_window_s`](Self::stall_window_s) before the
    /// pull counts as snagged, metres.
    pub(crate) stall_distance_m: f32,
    pub(crate) stall_window_s: f32,
    /// How long the hook may fly before it counts as having missed, seconds.
    pub(crate) attach_timeout_s: f32,
    /// How long a gun a character lets go of may take to lie in the world before the snatch gives
    /// up on it, seconds.
    pub(crate) free_timeout_s: f32,
    /// How near the grapple's aim must hit a door's handle area, metres.
    pub(crate) door_reach_m: f32,
    /// The fastest a vehicle may move for the line to pull its door open, metres per second.
    pub(crate) door_max_vehicle_speed_mps: f32,
    /// How hard the line swings a door outward once it comes open: the door's mass times this,
    /// metres per second.
    pub(crate) door_swing_mps: f32,
    /// How a snatched gun is steered into the pose the catching hand will hold it in over its
    /// final approach.
    pub landing: LandingTuning,
    /// How long a target aimed at just before the grip closes is still offered to it, seconds:
    /// covers a short aim miss at the grip edge without offering an old target indefinitely.
    pub(crate) retain_s: f32,
    /// The extra angular tolerance while retaining a recent aimed identity across a grip press,
    /// degrees.
    pub(crate) retain_cone_deg: f32,
    /// How far short of a gun on a character its line of sight may be blocked, metres: the line
    /// ends inside the character.
    pub(crate) sight_tolerance_m: f32,
    /// How near the attach point the engine's attachment proxy's visual position may be for the
    /// hook to count as on the selected body, metres.
    pub(crate) attach_tolerance_m: f32,
    /// How far past the hook's end the impact cast reaches, metres: the end sits on the target's
    /// surface or inside it.
    pub(crate) impact_overshoot_m: f32,
    /// How long the watch follows a caught weapon, milliseconds.
    pub(crate) watch_ms: u64,
    /// How many times the stall window ([`stall_window_s`](Self::stall_window_s)) a placed charge
    /// has before it counts as snagged.
    pub(crate) charge_stall_scale: f32,
    /// The yank that fires the snatch.
    pub yank: SnatchYankTuning,
    /// The held line.
    pub swing: SwingTuning,
    /// A ragdoll's line.
    pub ragdoll: RagdollYankTuning,
    /// A gun tossed to the player's feet.
    pub(crate) toss: TossTuning,
    /// The armed snatch's marker.
    pub(crate) latch: LatchTuning,
}

impl Tuning for SnatchTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("brace_take_m", self.brace_take_m, 1.0)?;
        range("brace_break_m", self.brace_break_m, self.brace_take_m, 2.0)?;
        positive("range_m", self.range_m, 1_000.0)?;
        nonnegative("cone_deg", self.cone_deg, 90.0)?;
        nonnegative("floor_cone_widen_deg", self.floor_cone_widen_deg, 90.0)?;
        positive("line_speed_mps", self.line_speed_mps, 1_000.0)?;
        range(
            "arrival_speed_mps",
            self.arrival_speed_mps,
            0.0,
            self.line_speed_mps,
        )?;
        nonnegative("line_accel_mps2", self.line_accel_mps2, 10_000.0)?;
        nonnegative("approach_m", self.approach_m, 100.0)?;
        nonnegative("arc", self.arc, 1.0)?;
        nonnegative("coast_s", self.coast_s, 10.0)?;
        nonnegative("slack_max_m", self.slack_max_m, 100.0)?;
        range("slack_min_m", self.slack_min_m, 0.0, self.slack_max_m)?;
        nonnegative("catch_zone_m", self.catch_zone_m, 10.0)?;
        nonnegative("heavy_mass_kg", self.heavy_mass_kg, 10_000.0)?;
        nonnegative("heavy_catch_zone_m", self.heavy_catch_zone_m, 10.0)?;
        range("heavy_speed_share", self.heavy_speed_share, 0.0, 1.0)?;
        nonnegative("catch_grace_s", self.catch_grace_s, 10.0)?;
        nonnegative("max_slack_s", self.max_slack_s, 10.0)?;
        nonnegative("max_spin_rps", self.max_spin_rps, 1_000.0)?;
        nonnegative("stall_distance_m", self.stall_distance_m, 100.0)?;
        nonnegative("stall_window_s", self.stall_window_s, 60.0)?;
        nonnegative("attach_timeout_s", self.attach_timeout_s, 60.0)?;
        nonnegative("free_timeout_s", self.free_timeout_s, 60.0)?;
        nonnegative("door_reach_m", self.door_reach_m, 10.0)?;
        nonnegative(
            "door_max_vehicle_speed_mps",
            self.door_max_vehicle_speed_mps,
            1_000.0,
        )?;
        nonnegative("door_swing_mps", self.door_swing_mps, 1_000.0)?;
        nonnegative("retain_s", self.retain_s, 10.0)?;
        nonnegative("retain_cone_deg", self.retain_cone_deg, 90.0)?;
        nonnegative("sight_tolerance_m", self.sight_tolerance_m, 10.0)?;
        nonnegative("attach_tolerance_m", self.attach_tolerance_m, 10.0)?;
        nonnegative("impact_overshoot_m", self.impact_overshoot_m, 100.0)?;
        nonnegative("watch_ms", self.watch_ms as f32, 60_000.0)?;
        positive("charge_stall_scale", self.charge_stall_scale, 100.0)?;

        let landing = &self.landing;
        nonnegative("landing.ramp_m", landing.ramp_m, 100.0)?;
        nonnegative("landing.ramp_ticks", landing.ramp_ticks, 1_000.0)?;
        positive("landing.max_t_go_s", landing.max_t_go_s, 10.0)?;
        range(
            "landing.min_t_go_s",
            landing.min_t_go_s,
            f32::EPSILON,
            landing.max_t_go_s,
        )?;
        nonnegative("landing.max_spin_rps", landing.max_spin_rps, 1_000.0)?;
        nonnegative(
            "landing.max_angular_accel",
            landing.max_angular_accel,
            100_000.0,
        )?;
        nonnegative("landing.lateral_window_m", landing.lateral_window_m, 10.0)?;
        nonnegative("landing.hysteresis_m", landing.hysteresis_m, 10.0)?;
        nonnegative("landing.max_extrapolate_s", landing.max_extrapolate_s, 10.0)?;
        nonnegative("landing.settled_deg", landing.settled_deg, 180.0)?;
        nonnegative("landing.settled_m", landing.settled_m, 10.0)?;
        nonnegative("landing.full_slide_deg", landing.full_slide_deg, 180.0)?;

        let yank = &self.yank;
        nonnegative("yank.min_distance_m", yank.min_distance_m, 2.0)?;
        nonnegative("yank.min_speed_mps", yank.min_speed_mps, 100.0)?;
        positive("yank.window_s", yank.window_s, 10.0)?;
        nonnegative("yank.cooldown_s", yank.cooldown_s, 10.0)?;

        let swing = &self.swing;
        nonnegative("swing.ceiling_kg", swing.ceiling_kg, 100_000.0)?;
        nonnegative("swing.stiffness_per_s2", swing.stiffness_per_s2, 100_000.0)?;
        nonnegative("swing.damping_per_s", swing.damping_per_s, 10_000.0)?;
        nonnegative("swing.strength_n", swing.strength_n, 1_000_000.0)?;
        range("swing.mass_scaling", swing.mass_scaling, 0.0, 1.0)?;
        range("swing.brace_factor", swing.brace_factor, 1.0, 10.0)?;
        nonnegative("swing.swing_length_m", swing.swing_length_m, 100.0)?;
        nonnegative("swing.reel_mps", swing.reel_mps, 100.0)?;
        positive("swing.reference_mass_kg", swing.reference_mass_kg, 10_000.0)?;
        nonnegative("swing.reel_lead_m", swing.reel_lead_m, 10.0)?;
        nonnegative("swing.max_stretch_m", swing.max_stretch_m, 100.0)?;
        positive(
            "swing.max_anchor_speed_mps",
            swing.max_anchor_speed_mps,
            1_000.0,
        )?;

        let ragdoll = &self.ragdoll;
        nonnegative("ragdoll.length_m", ragdoll.length_m, 100.0)?;
        nonnegative("ragdoll.mps", ragdoll.mps, 100.0)?;
        nonnegative("ragdoll.ease_per_s", ragdoll.ease_per_s, 100.0)?;

        let latch = &self.latch;
        positive("latch.line_width_m", latch.line_width_m, 1.0)?;
        nonnegative("latch.strained_width_m", latch.strained_width_m, 1.0)?;
        range("latch.line_alpha", latch.line_alpha, 0.0, 1.0)?;
        positive("latch.dot_rad", latch.dot_rad, 1.0)?;
        positive("latch.ring_rad", latch.ring_rad, 1.0)?;
        range("latch.occluded_alpha", latch.occluded_alpha, 0.0, 1.0)?;

        let toss = &self.toss;
        nonnegative("toss.land_m", toss.land_m, 100.0)?;
        positive("toss.speed_mps", toss.speed_mps, 100.0)?;
        positive("toss.max_flight_s", toss.max_flight_s, 10.0)?;
        range(
            "toss.min_flight_s",
            toss.min_flight_s,
            0.0,
            toss.max_flight_s,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controllers::yank::YankDetector;

    const DT: f32 = 1.0 / 30.0;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(SNATCH.get().validate(), Ok(()));
    }

    fn yanks(distances: &[f32]) -> usize {
        let mut detector = YankDetector::new();
        distances
            .iter()
            .filter(|&&d| detector.step(DT, Some(d), true, &snatch_yank()).is_some())
            .count()
    }

    #[test]
    fn the_snatch_fires_on_a_sharp_pull_back_along_the_aim() {
        assert_eq!(yanks(&[0.6, 0.6, 0.52, 0.42, 0.34, 0.33]), 1);
    }

    #[test]
    fn a_deliberate_moderate_pull_fires_it() {
        // About 1.2 m/s over 0.16 m, after a pause at the grip.
        assert_eq!(yanks(&[0.6, 0.6, 0.6, 0.56, 0.52, 0.48, 0.44, 0.44]), 1);
    }

    #[test]
    fn a_hand_held_still_or_drifting_does_not_fire_it() {
        assert_eq!(yanks(&[0.6; 30]), 0, "aiming, still");
        let drift: Vec<f32> = (0..40).map(|i| 0.6 - i as f32 * 0.005).collect();
        assert_eq!(yanks(&drift), 0, "drifting in at 0.15 m/s");
        assert_eq!(yanks(&[0.4, 0.5, 0.6, 0.7]), 0, "a reach out");
    }
}
