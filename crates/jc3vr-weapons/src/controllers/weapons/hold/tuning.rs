//! The weapon handling's hold tuning, read from the shipped `tuning/hold.toml` ([`HOLD`]): the
//! hold's slides, reaches, and throw cap, the takes from characters and racks, the grip targets'
//! volumes and marks, the sockets' attraction, and the body's grab volumes.

use jc3vr_body::tuning::Millis;
use jc3vr_core::{
    config::engine::{Invalid, nonnegative, positive, range},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::weapons::{docks::attract::AttractionTuning, volumes::VolumesTuning};

jc3vr_core::tuning_file! {
    /// The hold tuning file.
    pub static HOLD: HoldFileTuning = "hold.toml";
}

/// The hold tuning file's root, by subsystem.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldFileTuning {
    pub hold: HoldTuning,
    pub begin: BeginTuning,
    pub docks: DocksTuning,
    pub volumes: VolumesTuning,
}

impl Tuning for HoldFileTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.hold.validate()?;
        self.begin.validate()?;
        self.docks.validate()?;
        self.volumes.validate()
    }
}

/// The hold's slides, reaches, and windows (see [`crate::controllers::weapons::hold`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldTuning {
    /// How long a draw off a socket, a dock onto one, or a take from the world slides the gun
    /// between where it was and the hand, seconds.
    pub snap_s: f32,
    /// The fastest a let-go gun leaves the hand, metres per second, against tracking glitches.
    pub max_throw_mps: f32,
    /// The radius around the held gun's pistol grip for the free hand to take it, in player-scale
    /// metres. A touch on another gun region keeps that region's action.
    pub handle_radius_m: f32,
    /// The farthest a free hand may be from a gun's grip or handguard to take it, in player-scale
    /// metres. The volume around the touched region otherwise reaches as far as the hand does,
    /// which would take a gun from any distance the palm ray meets it; beyond this reach the grab
    /// is refused, and within it the gun slides into the hand over [`HoldTuning::snap_s`].
    pub handover_max_m: f32,
    /// The reach from the nearest point of a gun's grip-to-grip segment on a rack or in a locker,
    /// in player-scale metres. Acquisition and completion use the same boundary.
    pub take_radius_m: f32,
    /// The reach from the nearest point of a gun's grip-to-grip segment in a living character's
    /// hands, in player-scale metres: wider than a rack's, since the character moves and turns
    /// while the hand closes, and a hand within it takes the gun rather than the character holding
    /// it. Acquisition and completion use the same boundary.
    pub steal_radius_m: f32,
    /// How long a weapon let go of beside a socket keeps trying to dock there before it is left
    /// to fall, seconds of the hand staying near (see
    /// [`crate::controllers::weapons::hold::dock::ReleaseIntent`]).
    pub release_intent_s: f32,
    /// How long a [`crate::controllers::weapons::hold::button_pickup::grant`] lets the use-item
    /// input through: long enough for the injected click to reach the weapon handling a tick or two
    /// later, short enough that a button press after it does not ride along.
    pub use_item_grant_ms: Millis,
    /// How long a hand's aim target stays current for the native armed-weapon update.
    pub target_max_age_ms: Millis,
    /// The oldest a release's frames, its controller publication, and its sampled point may be for
    /// the release to carry their motion, seconds.
    pub release_max_age_s: f32,
    /// The fastest carried motion a release keeps, metres per second: past the game's fastest jets.
    pub max_carried_mps: f32,
    pub steal: StealTuning,
    pub swap: SwapTuning,
    pub rack: RackTuning,
}

impl HoldTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("hold.snap_s", self.snap_s, 2.0)?;
        positive("hold.max_throw_mps", self.max_throw_mps, 100.0)?;
        positive("hold.handle_radius_m", self.handle_radius_m, 1.0)?;
        positive("hold.handover_max_m", self.handover_max_m, 2.0)?;
        positive("hold.take_radius_m", self.take_radius_m, 2.0)?;
        positive("hold.steal_radius_m", self.steal_radius_m, 2.0)?;
        positive("hold.release_intent_s", self.release_intent_s, 5.0)?;
        self.use_item_grant_ms
            .validate("hold.use_item_grant_ms", 5_000)?;
        self.target_max_age_ms
            .validate("hold.target_max_age_ms", 5_000)?;
        range("hold.release_max_age_s", self.release_max_age_s, 0.001, 1.0)?;
        positive("hold.max_carried_mps", self.max_carried_mps, 1_000.0)?;
        self.steal.validate()?;
        self.swap.validate()?;
        self.rack.validate()
    }
}

/// The takes from characters' hands and holsters (see
/// [`crate::controllers::weapons::hold::steal`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StealTuning {
    /// How far from the player a character's gun is noted, metres, short of the grapple's snatch
    /// widening it.
    pub notice_range_m: f32,
    /// How long a noted character stays a target without being noted again.
    pub seen_for_ms: Millis,
    /// The stagger a take puts on the character it takes from, as a multiple of the character's
    /// stumble threshold (see [`crate::controllers::weapons::hold::world::stagger_impulse`]).
    pub over_stumble: f32,
    /// The most the stagger may be, as a share of the character's knockdown threshold.
    pub under_ragdoll: f32,
}

impl StealTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("hold.steal.notice_range_m", self.notice_range_m, 100.0)?;
        self.seen_for_ms
            .validate("hold.steal.seen_for_ms", 10_000)?;
        range("hold.steal.over_stumble", self.over_stumble, 1.0, 10.0)?;
        range("hold.steal.under_ragdoll", self.under_ragdoll, 0.0, 1.0)
    }
}

/// A weapon ejected from a socket a swap fills (see
/// [`crate::controllers::weapons::hold::swap`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapTuning {
    /// How fast an ejected weapon leaves its socket, metres per second: enough to clear the body
    /// before it lands, slow enough to stay at the player's feet.
    pub eject_mps: f32,
    /// The upward share of an ejected weapon's direction against its outward one.
    pub eject_lift: f32,
}

impl SwapTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("hold.swap.eject_mps", self.eject_mps, 20.0)?;
        nonnegative("hold.swap.eject_lift", self.eject_lift, 10.0)
    }
}

/// The takes from racks (see [`crate::controllers::weapons::hold::rack`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RackTuning {
    /// How long a rack's weapon stays hidden after its copy left it.
    pub restock_after_ms: Millis,
    /// How long a rack's copy is waited for.
    pub copy_wait_ms: Millis,
    pub paired: PairedRackTuning,
}

impl RackTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.restock_after_ms
            .validate("hold.rack.restock_after_ms", 10_000)?;
        self.copy_wait_ms
            .validate("hold.rack.copy_wait_ms", 30_000)?;
        self.paired.validate()
    }
}

/// The racks that hold a pair of pistols.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedRackTuning {
    /// How long a paired source stays known without being seen again.
    pub source_grace_ms: Millis,
    /// How long a paired member's copy is waited for.
    pub copy_wait_ms: Millis,
    /// How far the second pistol of a finite source is drawn from the first, metres: a pistol's
    /// thickness, a little more.
    pub stack_m: f32,
}

impl PairedRackTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.source_grace_ms
            .validate("hold.rack.paired.source_grace_ms", 30_000)?;
        self.copy_wait_ms
            .validate("hold.rack.paired.copy_wait_ms", 30_000)?;
        nonnegative("hold.rack.paired.stack_m", self.stack_m, 0.5)
    }
}

/// The grip targets the begin tick places on the held gun, and their marks.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginTuning {
    /// How far past the hand a volume on a gun reaches, metres.
    pub gun_volume_margin_m: f32,
    /// How deep in a volume on a gun the hand is, as a share of its radius.
    pub gun_volume_depth: f32,
    pub marks: MarksTuning,
}

impl BeginTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("begin.gun_volume_margin_m", self.gun_volume_margin_m, 0.5)?;
        range("begin.gun_volume_depth", self.gun_volume_depth, 0.01, 1.0)?;
        self.marks.validate()
    }
}

/// The marks' sizes. A target reached within a radius is marked by the reach's diameter, kept
/// within [`MarksTuning::min_m`] and [`MarksTuning::max_m`], so the mark shows the spot without
/// covering what is around it.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarksTuning {
    /// The smallest mark, metres.
    pub min_m: f32,
    /// The largest mark, metres.
    pub max_m: f32,
    /// The mark where a held charge would stick, metres.
    pub charge_m: f32,
    /// The mark on a braced foregrip, metres.
    pub foregrip_m: f32,
}

impl MarksTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("begin.marks.min_m", self.min_m, 1.0)?;
        range("begin.marks.max_m", self.max_m, self.min_m, 1.0)?;
        positive("begin.marks.charge_m", self.charge_m, 1.0)?;
        positive("begin.marks.foregrip_m", self.foregrip_m, 1.0)
    }
}

/// The sockets (see [`crate::controllers::weapons::docks`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocksTuning {
    /// How long the sockets' attachment offers stay current after the input observations stop,
    /// for the mirror and the pouch models.
    pub attachments_max_age_ms: Millis,
    pub attraction: AttractionTuning,
}

impl DocksTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.attachments_max_age_ms
            .validate("docks.attachments_max_age_ms", 5_000)?;
        let a = &self.attraction;
        positive("docks.attraction.shoulder_m", a.shoulder_m, 2.0)?;
        positive("docks.attraction.hip_m", a.hip_m, 2.0)?;
        nonnegative("docks.attraction.back_front_m", a.back_front_m, 1.0)?;
        nonnegative("docks.attraction.back_floor_m", a.back_floor_m, 2.0)?;
        nonnegative("docks.attraction.hip_front_m", a.hip_front_m, 1.0)?;
        positive(
            "docks.attraction.back_max_speed_mps",
            a.back_max_speed_mps,
            50.0,
        )?;
        positive(
            "docks.attraction.hip_max_speed_mps",
            a.hip_max_speed_mps,
            50.0,
        )?;
        nonnegative("docks.attraction.speed_window_s", a.speed_window_s, 1.0)?;
        positive("docks.attraction.travel_mps", a.travel_mps, 50.0)?;
        positive("docks.attraction.max_travel_s", a.max_travel_s, 5.0)?;
        nonnegative("docks.attraction.exit_margin_m", a.exit_margin_m, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(HOLD.get().validate(), Ok(()));
    }
}
