//! The tuning of what the hands do to characters and with the vehicles they ride, read from the
//! shipped `tuning/characters.toml` ([`CHARACTERS`]): the grips on characters
//! ([`crate::controllers::physical_hands::characters`]), the stunt positions
//! ([`crate::controllers::physical_hands::stunt`]), and the held objects' impacts
//! ([`crate::controllers::physical_hands::impacts`]).

use std::time::Duration;

use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::physical_hands::stunt::solve::Tuning as TrackerTuning;

jc3vr_core::tuning_file! {
    /// The characters' tuning file.
    pub static CHARACTERS: CharactersTuning = "characters.toml";
}

/// The characters' tuning file's root.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharactersTuning {
    pub characters: CharacterGripTuning,
    pub stunt: StuntTuning,
    pub impacts: ImpactsTuning,
}

/// A hand's grip on a character (see [`crate::controllers::physical_hands::characters`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterGripTuning {
    /// How far a hand gripping a character must pull away from its grab point to yank, metres of
    /// the player's hand.
    pub yank_m: f32,
    /// How hard a yank throws a character the player is hostile to, as a share of the knockdown
    /// blow ([`HandHitsTuning::knockdown_impulse_ns`]); one and above knocks the character down.
    ///
    /// [`HandHitsTuning::knockdown_impulse_ns`]: crate::controllers::physical_hands::physics::HandHitsTuning::knockdown_impulse_ns
    pub yank_strength: f32,
    /// How long after a yank the hand looks for the fallen character's ragdoll to hold, seconds of
    /// game time: the fall takes as long as the game's clock says, so slow motion gives it longer.
    pub regrip_s: f32,
    /// How long a hand waits for the ragdoll after a topple before retrying the ragdoll act, game
    /// seconds: the hit reaction's ragdoll is under way well before this when it comes.
    pub ragdoll_retry_after_s: f32,
    /// The slowest hand that shoves a character it lets go of, metres per second.
    pub shove_min_speed_mps: f32,
    /// How hard a grip throws a standing character into their ragdoll, as a share of the knockdown
    /// blow: just past it, so that the character falls where the hand holds them rather than
    /// flying off.
    pub topple_strength: f32,
    /// How much harder a hand pulls a character's ragdoll than a loose object, as a multiple of
    /// the hand's strength. The hand drags the whole body by one of its bones, and the rest hangs
    /// from that bone by the ragdoll's own constraints, so the pull must move the whole body's mass
    /// rather than the bone's. Scoped to ragdolls: every other hold keeps the hand's own strength.
    pub ragdoll_carry_strength: f32,
    /// How fast a held ragdoll's motion within itself dies down, per second.
    pub held_ragdoll_damping_per_s: f32,
    /// How far open a door beside the hand must stand for a hand to reach in, degrees.
    pub hijack_min_open_deg: f32,
    /// How far from the palm an open door may be for the hand to reach in through it, metres of
    /// the player's hand.
    pub hijack_door_reach_m: f32,
}

/// The stunt positions (see [`crate::controllers::physical_hands::stunt`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StuntTuning {
    pub tracker: TrackerTuning,
    /// How long the last input tick's reading of the stunt position stays current, milliseconds.
    pub reading_stale_after_ms: u64,
    /// How long the last input tick's observation of who moves the player stays current,
    /// milliseconds.
    pub observation_stale_after_ms: u64,
    /// The longest tick the tracker counts, seconds, so that a hitch or a pause does not count as
    /// a held grip or an open one.
    pub max_tick_s: f32,
    /// The least share of the hold pulse that pulses a hand.
    pub min_strain: f32,
    /// The diameter of the bar the fingers close round on a helicopter's skid, metres.
    pub skid_diameter_m: f32,
    /// The helicopter's speed of the strongest hold pulse, metres per second.
    pub hold_full_speed_mps: f32,
}

impl StuntTuning {
    /// [`reading_stale_after_ms`](Self::reading_stale_after_ms) as a duration.
    pub fn reading_stale_after(&self) -> Duration {
        Duration::from_millis(self.reading_stale_after_ms)
    }

    /// [`observation_stale_after_ms`](Self::observation_stale_after_ms) as a duration.
    pub fn observation_stale_after(&self) -> Duration {
        Duration::from_millis(self.observation_stale_after_ms)
    }
}

/// The held objects' impacts (see [`crate::controllers::physical_hands::impacts`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactsTuning {
    /// How long the holders the pickup published, and a contact queued for delivery, stay
    /// current, milliseconds.
    pub stale_after_ms: u64,
    /// How long a contact keeps obstructing a held object's drive, milliseconds.
    pub contact_freshness_ms: u64,
    /// The most contacts queued for delivery; more are dropped.
    pub max_queued: usize,
    /// The most recent contacts kept for the obstruction; the oldest goes first.
    pub max_recent_contacts: usize,
    /// The most native hit conversions kept; the oldest goes first.
    pub max_native_hits: usize,
    /// The most recently struck pairs kept against a repeat; the oldest goes first.
    pub max_struck_pairs: usize,
    /// The most contacts whose participants are kept alive for delivery; more are not observed.
    pub max_retained: usize,
    pub strike: StrikeTuning,
    pub object_damage: ObjectDamageTuning,
    pub feedback: FeedbackTuning,
}

impl ImpactsTuning {
    /// [`stale_after_ms`](Self::stale_after_ms) as a duration.
    pub fn stale_after(&self) -> Duration {
        Duration::from_millis(self.stale_after_ms)
    }

    /// [`contact_freshness_ms`](Self::contact_freshness_ms) as a duration.
    pub fn contact_freshness(&self) -> Duration {
        Duration::from_millis(self.contact_freshness_ms)
    }
}

/// An intentional strike with a held object: the bounded momentum it deals.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrikeTuning {
    /// The slowest the held object's contact point may close on the target, by its own motion and
    /// relative to the target's, to strike, metres per second.
    pub min_speed_mps: f32,
    /// The held object's mass is counted within this range, kilograms.
    pub min_mass_kg: f32,
    pub max_mass_kg: f32,
    /// The fastest closing speed counted, metres per second.
    pub max_speed_mps: f32,
    /// The strongest strike, newton seconds.
    pub max_impulse_ns: f32,
    /// The weakest strike dealt, newton seconds.
    pub min_impulse_ns: f32,
    /// How long one pair of bodies is not struck again, seconds.
    pub repeat_s: f64,
}

/// The native impact damage a strike deals.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectDamageTuning {
    /// The weakest strike that deals damage, newton seconds.
    pub min_impulse_ns: f32,
    /// The health points dealt per newton second.
    pub health_per_ns: f32,
    /// The most health points one strike deals.
    pub max_health: f32,
}

/// The haptic and sound feedback of a held object's contact.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackTuning {
    /// The slowest contact point speed into the contact that gives feedback, metres per second.
    pub min_speed_mps: f32,
    /// The fastest contact point speed counted, metres per second.
    pub max_speed_mps: f32,
    /// The held object's mass is counted within this range, kilograms.
    pub min_mass_kg: f32,
    pub max_mass_kg: f32,
    /// The momentum of the strongest feedback, newton seconds.
    pub full_momentum_ns: f32,
    /// The weakest share of the strongest feedback that is given.
    pub min_share: f32,
    /// How long a hand's feedback waits before the next, seconds.
    pub repeat_s: f64,
}

impl Tuning for CharactersTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.characters.validate()?;
        self.stunt.validate()?;
        self.impacts.validate()
    }
}

impl CharacterGripTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("characters.yank_m", self.yank_m, 5.0)?;
        nonnegative("characters.yank_strength", self.yank_strength, 100.0)?;
        nonnegative("characters.regrip_s", self.regrip_s, 60.0)?;
        nonnegative(
            "characters.ragdoll_retry_after_s",
            self.ragdoll_retry_after_s,
            60.0,
        )?;
        nonnegative(
            "characters.shove_min_speed_mps",
            self.shove_min_speed_mps,
            100.0,
        )?;
        nonnegative("characters.topple_strength", self.topple_strength, 100.0)?;
        nonnegative(
            "characters.ragdoll_carry_strength",
            self.ragdoll_carry_strength,
            100.0,
        )?;
        nonnegative(
            "characters.held_ragdoll_damping_per_s",
            self.held_ragdoll_damping_per_s,
            1000.0,
        )?;
        range(
            "characters.hijack_min_open_deg",
            self.hijack_min_open_deg,
            0.0,
            180.0,
        )?;
        nonnegative(
            "characters.hijack_door_reach_m",
            self.hijack_door_reach_m,
            10.0,
        )
    }
}

impl StuntTuning {
    fn validate(&self) -> Result<(), Invalid> {
        let tracker = &self.tracker;
        nonnegative("stunt.tracker.roof_offer_m", tracker.roof_offer_m, 10.0)?;
        nonnegative(
            "stunt.tracker.release_grace_s",
            tracker.release_grace_s,
            10.0,
        )?;
        nonnegative("stunt.tracker.retry_s", tracker.retry_s, 60.0)?;
        nonnegative(
            "stunt.tracker.let_go_timeout_s",
            tracker.let_go_timeout_s,
            60.0,
        )?;
        milliseconds("stunt.reading_stale_after_ms", self.reading_stale_after_ms)?;
        milliseconds(
            "stunt.observation_stale_after_ms",
            self.observation_stale_after_ms,
        )?;
        positive("stunt.max_tick_s", self.max_tick_s, 10.0)?;
        range("stunt.min_strain", self.min_strain, 0.0, 1.0)?;
        positive("stunt.skid_diameter_m", self.skid_diameter_m, 1.0)?;
        positive(
            "stunt.hold_full_speed_mps",
            self.hold_full_speed_mps,
            1000.0,
        )
    }
}

impl ImpactsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        milliseconds("impacts.stale_after_ms", self.stale_after_ms)?;
        milliseconds("impacts.contact_freshness_ms", self.contact_freshness_ms)?;
        count("impacts.max_queued", self.max_queued)?;
        count("impacts.max_recent_contacts", self.max_recent_contacts)?;
        count("impacts.max_native_hits", self.max_native_hits)?;
        count("impacts.max_struck_pairs", self.max_struck_pairs)?;
        count("impacts.max_retained", self.max_retained)?;

        let strike = &self.strike;
        nonnegative("impacts.strike.min_speed_mps", strike.min_speed_mps, 100.0)?;
        positive("impacts.strike.max_mass_kg", strike.max_mass_kg, 1.0e6)?;
        positive(
            "impacts.strike.min_mass_kg",
            strike.min_mass_kg,
            strike.max_mass_kg,
        )?;
        nonnegative("impacts.strike.max_speed_mps", strike.max_speed_mps, 1000.0)?;
        positive(
            "impacts.strike.max_impulse_ns",
            strike.max_impulse_ns,
            1.0e6,
        )?;
        nonnegative(
            "impacts.strike.min_impulse_ns",
            strike.min_impulse_ns,
            strike.max_impulse_ns,
        )?;
        seconds("impacts.strike.repeat_s", strike.repeat_s)?;

        let damage = &self.object_damage;
        nonnegative(
            "impacts.object_damage.min_impulse_ns",
            damage.min_impulse_ns,
            1.0e6,
        )?;
        nonnegative(
            "impacts.object_damage.health_per_ns",
            damage.health_per_ns,
            1000.0,
        )?;
        range(
            "impacts.object_damage.max_health",
            damage.max_health,
            1.0,
            f32::from(i16::MAX),
        )?;

        let feedback = &self.feedback;
        nonnegative(
            "impacts.feedback.max_speed_mps",
            feedback.max_speed_mps,
            1000.0,
        )?;
        nonnegative(
            "impacts.feedback.min_speed_mps",
            feedback.min_speed_mps,
            feedback.max_speed_mps,
        )?;
        positive("impacts.feedback.max_mass_kg", feedback.max_mass_kg, 1.0e6)?;
        positive(
            "impacts.feedback.min_mass_kg",
            feedback.min_mass_kg,
            feedback.max_mass_kg,
        )?;
        positive(
            "impacts.feedback.full_momentum_ns",
            feedback.full_momentum_ns,
            1.0e6,
        )?;
        range("impacts.feedback.min_share", feedback.min_share, 0.0, 1.0)?;
        seconds("impacts.feedback.repeat_s", feedback.repeat_s)
    }
}

/// The longest staleness window, milliseconds.
const MAX_MILLISECONDS: u64 = 60_000;

/// Check that the window `value` named `name` is in `1..=MAX_MILLISECONDS` milliseconds.
fn milliseconds(name: &str, value: u64) -> Result<(), Invalid> {
    if value == 0 || value > MAX_MILLISECONDS {
        return Err(Invalid(format!(
            "{name} must be in [1, {MAX_MILLISECONDS}], got {value}"
        )));
    }
    Ok(())
}

/// The largest buffer, entries.
const MAX_COUNT: usize = 4096;

/// Check that the buffer size `value` named `name` is in `1..=MAX_COUNT`.
fn count(name: &str, value: usize) -> Result<(), Invalid> {
    if value == 0 || value > MAX_COUNT {
        return Err(Invalid(format!(
            "{name} must be in [1, {MAX_COUNT}], got {value}"
        )));
    }
    Ok(())
}

/// Check that the time `value` named `name`, in seconds on the impacts' `f64` clock, is in
/// `0..=60`.
fn seconds(name: &str, value: f64) -> Result<(), Invalid> {
    if !value.is_finite() || !(0.0..=60.0).contains(&value) {
        return Err(Invalid(format!(
            "{name} must be finite and in [0, 60], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(CHARACTERS.get().validate(), Ok(()));
    }
}
