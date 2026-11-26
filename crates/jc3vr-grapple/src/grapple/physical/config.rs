//! The physical grapple's tuning, read from the grapple's tuning file
//! ([`GRAPPLE`](crate::grapple::tuning::GRAPPLE)). Every part acts only under the controller
//! scheme with a tracked grapple hand, so none has a switch.

use serde::Deserialize;

/// The lock's tuning ([`crate::grapple::physical::lock`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockTuning {
    /// The line's pull per metre it is stretched past its held length, per second squared, for
    /// the character and the target's reduced mass.
    pub stiffness_per_s2: f32,
    /// The line's damping of the character's speed away from the hook's point, per second.
    pub damping_per_s: f32,
    /// The most the line accelerates the character, in multiples of gravity: its strength.
    pub max_accel_g: f32,
    /// How far past its held length the line stretches before it breaks, metres.
    pub break_stretch_m: f32,
    /// How fast the grapple hand pulling back takes line in, at most, metres per second.
    pub reel_in_mps: f32,
    /// The shortest the line is held at, metres: the hand takes no line in past it.
    pub min_length_m: f32,
    /// The share of the line's pull the target feels back, `0..=1`.
    pub reaction: f32,
    /// The hang's line, from the hook to the character's root, metres: about a metre and a half of
    /// line above the raised grapple hand.
    pub hang_length_m: f32,
    /// The deceleration toward the hook beyond which the coast is braked, in multiples of gravity:
    /// about what the line holds, so a coast that would otherwise carry in too far stops at the
    /// shortest length the line bears instead.
    pub coast_brake_g: f32,
    /// The share of the line's strength a coast leaves the swing to turn on the length it
    /// reaches, `0..=1`: the rest is the margin for the pump and the hook's motion.
    pub coast_strength_share: f32,
    /// How much line the hand must take in during a hang's pay-out to end it, metres: a deliberate
    /// pull, past a hand's drift back along the aim. The tally leaks at this many metres per
    /// second, so a drift back and forth does not add up.
    pub pay_out_take_in_m: f32,
    /// How fast a hang pays line out to its length, or takes it in, metres per second.
    pub pay_out_mps: f32,
    /// How long a hang's swing across the line under a moving hook takes to die away, seconds: the
    /// time constant of its damping.
    pub hang_swing_damping_s: f32,
    /// The hook speed above which a hang counts as hung from a moving body, metres per second.
    pub moving_hook_mps: f32,
    /// How long after a release its velocity is handed back while the character is slower than
    /// it, seconds: past the reel's cancel and the state change after it.
    pub handback_for_s: f32,
    /// How long after a release a character carrying its velocity counts as having kept it,
    /// seconds: a few input ticks, past the world update in which the reel's cancel scales it.
    pub handback_settle_s: f32,
    /// The share of the release's velocity, carried on under gravity, at which the character
    /// counts as carrying it.
    pub handback_carried: f32,
    /// How long after Y under a helicopter its pull up to the door lasts, milliseconds, for the
    /// log of its arrival and the bindings' hold on the wingsuit: the pull and the door-side
    /// arrival's clip.
    pub entering_ms: u64,
    /// How long an arrival's ask for a hang stands, milliseconds: the arrival asks again on every
    /// reel tick it arrives, so an ask older than a few ticks belongs to a reel that has ended.
    pub hang_wanted_ms: u64,
    /// How long after a refused hang the reel's arrival is left to the game, milliseconds, so that
    /// the reel ends.
    pub hang_refused_ms: u64,
    /// The least level share of the reel's direction that faces the character along it rather
    /// than keeping its own facing.
    pub level_min: f32,
    /// The grapple hand pumping a held line's swing.
    pub pump: LockPumpTuning,
}

/// The grapple hand's pump of a held line's swing (`Pump` in the lock's model): the hand's speed
/// across the line, past the hang swing's deadzone ([`SwingTuning`]), pushes the character along
/// the swing.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockPumpTuning {
    /// The pump's acceleration at full strength on a taut line, in multiples of gravity.
    pub accel_g: f32,
    /// How far past its held length the line must be stretched for the pump's full strength,
    /// metres: a slack line pumps nothing.
    pub taut_m: f32,
    /// A hand's step against the head in one tick longer than this is a tracking jump, not a
    /// pump, metres.
    pub max_step_m: f32,
    /// A hand's speed against the head faster than this is a tracking jump, not a pump, metres
    /// per second.
    pub max_speed_mps: f32,
    /// How long a pump sample lasts without a fresh one before the pump stops, milliseconds.
    pub stale_ms: u64,
    /// How old the latest controller frame may be for the hand to pump, milliseconds.
    pub frame_stale_ms: u64,
}

/// The physical grapple's pull, reel, tether retract, and hang swing.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalGrappleTuning {
    /// How the grapple hand's pull is measured, for the reel and the tether retract.
    pub pull: PullTuning,
    /// The reel driven by the pull.
    pub reel: ReelPullTuning,
    /// The tether retract driven by the pull.
    pub tether: TetherTuning,
    /// The hang swing pumped by the grapple hand's motion.
    pub swing: SwingTuning,
    /// How long the physical grapple's readout counts as live after its last tick, milliseconds.
    pub status_max_age_ms: u64,
}

/// How the grapple hand's pull toward the body is measured
/// ([`crate::grapple::physical::pull`]). The hand's distance from the body point is the yank's
/// ([`grapple_yank`](jc3vr_body::controllers::intent::yank::grapple_yank)).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullTuning {
    /// How long a pull's speed is remembered after the hand stops, seconds: the time constant of
    /// the peak-held speed's decay.
    pub speed_hold_s: f32,
    /// How fast the pull's reference point follows a hand held still, metres per second: a hand
    /// held back stops pulling once the reference catches up with it.
    pub relax_mps: f32,
}

/// The reel by pulling ([`crate::grapple::physical::reel`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReelPullTuning {
    /// The reel's scale with no pull.
    pub min_scale: f32,
    /// The reel's scale at a pull of [`full_speed_mps`](Self::full_speed_mps) or faster.
    pub max_scale: f32,
    /// The pull speed at which the scale reaches [`max_scale`](Self::max_scale), metres per second.
    pub full_speed_mps: f32,
    /// How long a yank during the reel boosts it, seconds. The boost applies only when the game's
    /// own retract boost would: the retract reel-in boost upgrade is purchased.
    pub boost_s: f32,
    /// How long the hand's last ask of the reel stands, milliseconds: the reel task runs on its
    /// own cadence, and an older ask belongs to a pull that has stopped.
    pub drive_max_age_ms: u64,
}

/// The tether retract by pulling ([`crate::grapple::physical::tether`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TetherTuning {
    /// The pull below which nothing retracts, metres.
    pub deadzone_m: f32,
    /// The pull that retracts at full strength, metres.
    pub full_m: f32,
}

/// The hang swing's pump ([`crate::grapple::physical::swing`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwingTuning {
    /// The hand speed below which the hand does not pump, metres per second.
    pub deadzone_mps: f32,
    /// The hand speed that pumps at full strength, metres per second.
    pub full_mps: f32,
    /// The full-strength pump per hang tick, as a multiple of the game's own continuous stick
    /// swing at full deflection. Negative pumps the body the way the hand moves.
    pub scale: f32,
    /// The most the hold and the pump together push per hang tick, as a multiple of the game's
    /// own continuous stick swing at full deflection.
    pub max_scale: f32,
    /// The time constant of the hand's smoothed velocity, seconds.
    pub smoothing_s: f32,
}
