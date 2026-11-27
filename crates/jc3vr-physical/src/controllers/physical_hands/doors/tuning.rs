//! The vehicle doors' tuning, read from the shipped `tuning/doors.toml` ([`DOORS`]): the grip's
//! reach and unlatching pull, the box guess for a handle, the handle scan, the hinges of the doors
//! the hands opened, the palm's pressure on a door, and the tearing of a door off its hinge.
//! Distances are metres of the player's hand, scaled by the VR world scale where the code acts in
//! the world.

use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

jc3vr_core::tuning_file! {
    /// The vehicle doors' tuning file.
    pub static DOORS: DoorsTuning = "doors.toml";
}

/// The vehicle doors' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorsTuning {
    pub grip: GripTuning,
    pub handle: HandleGuessTuning,
    pub scan: ScanTuning,
    pub(crate) hinge: HingeTuning,
    pub pressure: PressureTuning,
    pub tearing: TearTuning,
}

impl Tuning for DoorsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.grip.validate()?;
        self.handle.validate()?;
        self.scan.validate()?;
        self.hinge.validate()?;
        self.pressure.validate()?;
        self.tearing.validate()
    }
}

/// How a palm takes a door and undoes its latch (see
/// [`crate::controllers::physical_hands::doors`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GripTuning {
    /// How far outside a door's box a palm still reaches it, metres.
    pub(crate) box_reach_m: f32,
    /// How far from a door's handle a palm takes the door, whatever the reach's rays hit, metres.
    pub handle_reach_m: f32,
    /// Where on a closed door its handle area starts, as a share of the way from the door's hinge
    /// to its far edge, `0..=1`: a grip nearer the hinge than this takes nothing. Used where the
    /// door has no handle to measure from.
    pub(crate) handle_share: f32,
    /// How far outside a door's box a ray hit still counts as on the door's panel, metres.
    pub(crate) panel_slack_m: f32,
    /// How far a hand gripping a closed door must pull it outward before the latch gives, metres.
    /// A palm pressing a closed door from inside opens it after the same movement.
    pub unlatch_pull_m: f32,
    /// How long an unlatched door's grip waits for the game to publish the door's loose body,
    /// seconds, before it lets go.
    pub(crate) body_wait_s: f32,
}

/// Where a door's box puts its handle when the embedded table has none (see
/// [`crate::controllers::physical_hands::doors::geometry::DoorBox::handle`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandleGuessTuning {
    /// The share of the door's run within which the part's origin counts as at one end, where the
    /// hinge is, `0..0.5`.
    pub(crate) pivot_near_end: f32,
    /// How far in from the edge across from the hinge the handle sits, as a share of the door's
    /// run.
    pub inset: f32,
}

/// The doors near the hands, offered by their handles (see
/// [`crate::controllers::physical_hands::doors::scan`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanTuning {
    /// How far from a palm the vehicles whose doors it might reach are looked for, metres: past the
    /// end of the longest vehicle's door from its middle.
    pub near_m: f32,
    /// The most vehicles a tick looks at, a bound against a crowded street.
    pub(crate) max_vehicles: usize,
    /// How far from a palm a door out of reach is still noted, metres.
    pub(crate) noted_m: f32,
    /// The most door handles a hand records for the grab view per tick.
    pub(crate) max_handles_seen: usize,
    /// The most door notes a hand keeps per tick.
    pub(crate) max_notes: usize,
    /// How far past the handle the casts that find a latched door's vehicle body reach, metres.
    /// A canopy's casts reach as far past the surface.
    pub(crate) cast_past_m: f32,
}

/// The hinges of the doors the hands opened (see
/// [`crate::controllers::physical_hands::doors::hinge`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HingeTuning {
    /// The closing speed past which a free door within the controller's latch angle latches,
    /// degrees per second. The controller's own is a tenth of a typical shove.
    pub(crate) latch_speed_deg_s: f32,
    /// How long a free door must rest within the latch angle before it latches, seconds.
    pub settle_s: f32,
    /// The free side door's damping, per second: its velocities fall by `e` each `1 / damping_per_s`
    /// seconds.
    pub damping_per_s: f32,
    /// The share of its travel above which a free lid is held where it is left, `0..=1`.
    pub(crate) strut_fraction: f32,
    /// The fastest a lid may open and still be held by the strut, radians per second; faster, it
    /// was thrown open and carries on. A supported canopy closing faster than this yields.
    pub(crate) strut_rising_rad_s: f32,
    /// The slowest a door within the latch angle may swing and still count as resting, radians per
    /// second. A yielding canopy counts as quiet below the same speed.
    pub(crate) resting_rad_s: f32,
    pub canopy: CanopyTuning,
}

/// The support that holds an aircraft canopy open where it is left, and how it yields to a palm.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CanopyTuning {
    /// How long the support yields after a palm pushes the canopy, at least, seconds.
    pub yield_s: f32,
    /// How long a yielding canopy must be quiet before the support takes it again, seconds.
    pub(crate) quiet_s: f32,
    /// The rate at which the support pulls a drifting canopy back to where it took it, per second.
    pub(crate) restore_per_s: f32,
    /// The fastest the support's pull moves the canopy, metres per second.
    pub(crate) restore_mps: f32,
    /// The fastest the support's pull turns the canopy, radians per second.
    pub(crate) restore_rad_s: f32,
}

/// An ungripped palm pressing a door: from inside a closed door to undo its latch, and on an open
/// door to push it (see [`crate::controllers::physical_hands::doors::pressure`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PressureTuning {
    /// How far from the palm the contact query's hit may be, metres: a wider follow-through than a
    /// new grip's.
    pub reach_m: f32,
    /// The longest tick a press is measured over, seconds; a longer one resets it.
    pub max_dt_s: f32,
    /// How long a missed palm probe keeps a press's progress, seconds.
    pub(crate) contact_grace_s: f32,
    /// How far the palm may move in the vehicle's frame and still be at the same contact, metres.
    pub(crate) gap_drift_m: f32,
    /// How far either side of the panel the ray that proves a retained contact reaches, metres.
    pub(crate) proof_depth_m: f32,
    /// The slowest opening a palm's pull gives the door, metres per second.
    pub(crate) min_open_mps: f32,
    /// The fastest opening a palm's pull gives the door, metres per second.
    pub(crate) max_open_mps: f32,
    /// The largest impulse an opening gives the door, newton seconds.
    pub(crate) max_impulse_ns: f32,
    /// The largest cosine between the hit's normal and the door's outward direction for the hit to
    /// be on the door's inner face; a face turned further out is the outer one.
    pub(crate) inner_face_max_cos: f32,
    /// How far the palm may draw back inward from the door between two ticks and keep its contact,
    /// metres.
    pub(crate) retreat_m: f32,
    /// The fastest a palm's push swings an open door, metres per second.
    pub(crate) max_push_mps: f32,
    /// How far in front of an open door's face the palm may be and still push it, metres.
    pub(crate) push_depth_m: f32,
}

/// Deliberate tearing of a detachable door off its hinge after an open grip settles, with a yank, a
/// hard pull, or a push past its opening limit (see
/// [`crate::controllers::physical_hands::doors::tearing`]); distances and speeds use player metres.
///
/// The lags are measured above the grip's settled lag, the lag it held while it settled: a heavy
/// door held through a joint hangs a few centimetres off the hand, and a door taken by a reach
/// sits wherever the hand stopped, so a settled grip need not be a close one.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TearTuning {
    /// How long the grip's lag must hold steady before a gesture tears, seconds.
    pub settle_s: f32,
    /// The fastest the lag may change for the grip to count as steady, metres per second.
    pub settle_speed_mps: f32,
    /// The smallest cosine between the lag's direction and the direction it settled in for the
    /// gesture to carry on; a sharper turn starts it again.
    pub(crate) direction_cos: f32,
    /// The lag a yank opens above the settled lag, metres.
    pub(crate) yank_lag_m: f32,
    /// How fast the lag must grow, and the hand pull away from the door, for a yank, metres per
    /// second: a pull a player makes on a door that does not give.
    pub(crate) yank_speed_mps: f32,
    /// The lag a hard pull holds above the settled lag, however fast it got there, metres. Short of
    /// the distance at which a hand's body lets go of what it is joined to.
    pub(crate) wrench_lag_m: f32,
    /// How long a hard pull must hold its lag, seconds.
    pub(crate) wrench_s: f32,
    /// The share of the lag along the door's free swing above which a yank or a hard pull does not
    /// count away from the hinge's limits: there the door would swing with the hand, and a lag is
    /// a heavy door catching up rather than the hinge holding it.
    pub(crate) free_swing_share: f32,
    /// How far past the opening limit a push must carry the hand, metres.
    pub(crate) push_lag_m: f32,
    /// How long a push must last, seconds.
    pub(crate) push_hold_s: f32,
    /// The grip load of a push, `0..=1`.
    pub(crate) push_load: f32,
    /// How near its opening limit the door must be for a push to count, radians.
    pub(crate) limit_margin_rad: f32,
    /// The longest tick a gesture is measured over, seconds; a longer one starts it again.
    pub max_dt_s: f32,
}

/// The largest distance any of the doors' reaches may span, metres.
const MAX_REACH_M: f32 = 10.0;

/// The largest time any of the doors' waits and windows may span, seconds.
const MAX_TIME_S: f32 = 30.0;

/// The largest count any of the doors' per-tick bounds may take.
const MAX_COUNT: usize = 1024;

impl GripTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("grip.box_reach_m", self.box_reach_m, MAX_REACH_M)?;
        positive("grip.handle_reach_m", self.handle_reach_m, MAX_REACH_M)?;
        range("grip.handle_share", self.handle_share, 0.0, 1.0)?;
        nonnegative("grip.panel_slack_m", self.panel_slack_m, MAX_REACH_M)?;
        positive("grip.unlatch_pull_m", self.unlatch_pull_m, MAX_REACH_M)?;
        nonnegative("grip.body_wait_s", self.body_wait_s, MAX_TIME_S)
    }
}

impl HandleGuessTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("handle.pivot_near_end", self.pivot_near_end, 0.0, 0.5)?;
        range("handle.inset", self.inset, 0.0, 1.0)
    }
}

impl ScanTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("scan.near_m", self.near_m, 100.0)?;
        count("scan.max_vehicles", self.max_vehicles)?;
        nonnegative("scan.noted_m", self.noted_m, MAX_REACH_M)?;
        count("scan.max_handles_seen", self.max_handles_seen)?;
        count("scan.max_notes", self.max_notes)?;
        positive("scan.cast_past_m", self.cast_past_m, MAX_REACH_M)
    }
}

impl HingeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("hinge.latch_speed_deg_s", self.latch_speed_deg_s, 10_000.0)?;
        nonnegative("hinge.settle_s", self.settle_s, MAX_TIME_S)?;
        nonnegative("hinge.damping_per_s", self.damping_per_s, 1000.0)?;
        range("hinge.strut_fraction", self.strut_fraction, 0.0, 1.0)?;
        nonnegative("hinge.strut_rising_rad_s", self.strut_rising_rad_s, 100.0)?;
        positive("hinge.resting_rad_s", self.resting_rad_s, 100.0)?;
        let canopy = &self.canopy;
        nonnegative("hinge.canopy.yield_s", canopy.yield_s, MAX_TIME_S)?;
        nonnegative("hinge.canopy.quiet_s", canopy.quiet_s, MAX_TIME_S)?;
        nonnegative("hinge.canopy.restore_per_s", canopy.restore_per_s, 1000.0)?;
        nonnegative("hinge.canopy.restore_mps", canopy.restore_mps, 100.0)?;
        nonnegative("hinge.canopy.restore_rad_s", canopy.restore_rad_s, 100.0)
    }
}

impl PressureTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("pressure.reach_m", self.reach_m, MAX_REACH_M)?;
        positive("pressure.max_dt_s", self.max_dt_s, MAX_TIME_S)?;
        nonnegative("pressure.contact_grace_s", self.contact_grace_s, MAX_TIME_S)?;
        nonnegative("pressure.gap_drift_m", self.gap_drift_m, MAX_REACH_M)?;
        positive("pressure.proof_depth_m", self.proof_depth_m, MAX_REACH_M)?;
        positive("pressure.max_open_mps", self.max_open_mps, 100.0)?;
        range(
            "pressure.min_open_mps",
            self.min_open_mps,
            0.0,
            self.max_open_mps,
        )?;
        nonnegative("pressure.max_impulse_ns", self.max_impulse_ns, 100_000.0)?;
        range(
            "pressure.inner_face_max_cos",
            self.inner_face_max_cos,
            -1.0,
            1.0,
        )?;
        nonnegative("pressure.retreat_m", self.retreat_m, MAX_REACH_M)?;
        nonnegative("pressure.max_push_mps", self.max_push_mps, 100.0)?;
        nonnegative("pressure.push_depth_m", self.push_depth_m, MAX_REACH_M)
    }
}

impl TearTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("tearing.settle_s", self.settle_s, MAX_TIME_S)?;
        nonnegative("tearing.settle_speed_mps", self.settle_speed_mps, 100.0)?;
        range("tearing.direction_cos", self.direction_cos, 0.0, 1.0)?;
        nonnegative("tearing.yank_lag_m", self.yank_lag_m, MAX_REACH_M)?;
        nonnegative("tearing.yank_speed_mps", self.yank_speed_mps, 100.0)?;
        nonnegative("tearing.wrench_lag_m", self.wrench_lag_m, MAX_REACH_M)?;
        nonnegative("tearing.wrench_s", self.wrench_s, MAX_TIME_S)?;
        range("tearing.free_swing_share", self.free_swing_share, 0.0, 1.0)?;
        nonnegative("tearing.push_lag_m", self.push_lag_m, MAX_REACH_M)?;
        nonnegative("tearing.push_hold_s", self.push_hold_s, MAX_TIME_S)?;
        range("tearing.push_load", self.push_load, 0.0, 1.0)?;
        nonnegative(
            "tearing.limit_margin_rad",
            self.limit_margin_rad,
            std::f32::consts::PI,
        )?;
        positive("tearing.max_dt_s", self.max_dt_s, MAX_TIME_S)
    }
}

/// Check that a per-tick bound admits at least one and at most [`MAX_COUNT`].
fn count(name: &str, value: usize) -> Result<(), Invalid> {
    if !(1..=MAX_COUNT).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be in [1, {MAX_COUNT}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(DOORS.get().validate(), Ok(()));
    }
}
