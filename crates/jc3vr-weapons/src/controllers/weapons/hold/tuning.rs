//! The weapon handling's hold tuning, read from the shipped `tuning/hold.toml` ([`HOLD`]): the
//! hold's slides, reaches, and windows, the takes from characters and racks, the grip targets'
//! volumes and marks, the sockets' attraction, the body's grab volumes, the gun's regions, the
//! dual-wield split's waits, the guns' bodies, the pouches' models, and the pouch returns.

use glam::Vec3;
use jc3vr_body::tuning::Millis;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::weapons::{
    docks::attract::AttractionTuning, pouch_models::ModelPlacement, pouch_return::PressTuning,
    volumes::VolumesTuning,
};

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
    pub regions: RegionsTuning,
    pub dual_wield: DualWieldTuning,
    pub(crate) gun_bodies: GunBodiesTuning,
    pub pouch_models: PouchModelsTuning,
    pub(crate) pouch_return: PouchReturnTuning,
}

impl Tuning for HoldFileTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.hold.validate()?;
        self.begin.validate()?;
        self.docks.validate()?;
        self.volumes.validate()?;
        self.regions.validate()?;
        self.dual_wield.validate()?;
        self.gun_bodies
            .pinned_lately_ms
            .validate("gun_bodies.pinned_lately_ms", 5_000)?;
        self.pouch_models.validate()?;
        self.pouch_return.validate()
    }
}

/// The hold's slides, reaches, and windows (see [`crate::controllers::weapons::hold`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldTuning {
    /// How long a draw off a socket, a dock onto one, or a take from the world slides the gun
    /// between where it was and the hand, seconds.
    pub snap_s: f32,
    /// The radius around the held gun's pistol grip for the free hand to take it, in player-scale
    /// metres. A touch on another gun region keeps that region's action.
    pub(crate) handle_radius_m: f32,
    /// The farthest a free hand may be from a gun's grip or handguard to take it, in player-scale
    /// metres. The volume around the touched region otherwise reaches as far as the hand does,
    /// which would take a gun from any distance the palm ray meets it; beyond this reach the grab
    /// is refused, and within it the gun slides into the hand over [`HoldTuning::snap_s`].
    pub(crate) handover_max_m: f32,
    /// The reach from the nearest point of a gun's grip-to-grip segment on a rack or in a locker,
    /// in player-scale metres. Acquisition and completion use the same boundary.
    pub(crate) take_radius_m: f32,
    /// The reach from the nearest point of a gun's grip-to-grip segment in a living character's
    /// hands, in player-scale metres: wider than a rack's, since the character moves and turns
    /// while the hand closes, and a hand within it takes the gun rather than the character holding
    /// it. Acquisition and completion use the same boundary.
    pub(crate) steal_radius_m: f32,
    /// How long a weapon let go of beside a socket keeps trying to dock there before it is left
    /// to fall, seconds of the hand staying near (see
    /// [`crate::controllers::weapons::hold::dock::ReleaseIntent`]).
    pub(crate) release_intent_s: f32,
    /// How long a [`crate::controllers::weapons::hold::button_pickup::grant`] lets the use-item
    /// input through: long enough for the injected click to reach the weapon handling a tick or two
    /// later, short enough that a button press after it does not ride along.
    pub(crate) use_item_grant_ms: Millis,
    /// How long a hand's aim target stays current for the native armed-weapon update.
    pub(crate) target_max_age_ms: Millis,
    pub steal: StealTuning,
    pub swap: SwapTuning,
    pub rack: RackTuning,
}

impl HoldTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("hold.snap_s", self.snap_s, 2.0)?;
        positive("hold.handle_radius_m", self.handle_radius_m, 1.0)?;
        positive("hold.handover_max_m", self.handover_max_m, 2.0)?;
        positive("hold.take_radius_m", self.take_radius_m, 2.0)?;
        positive("hold.steal_radius_m", self.steal_radius_m, 2.0)?;
        positive("hold.release_intent_s", self.release_intent_s, 5.0)?;
        self.use_item_grant_ms
            .validate("hold.use_item_grant_ms", 5_000)?;
        self.target_max_age_ms
            .validate("hold.target_max_age_ms", 5_000)?;
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
    pub(crate) notice_range_m: f32,
    /// How long a noted character stays a target without being noted again.
    pub(crate) seen_for_ms: Millis,
    /// The stagger a take puts on the character it takes from, as a multiple of the character's
    /// stumble threshold (see [`crate::controllers::weapons::hold::world::stagger_impulse`]).
    pub(crate) over_stumble: f32,
    /// The most the stagger may be, as a share of the character's knockdown threshold.
    pub(crate) under_ragdoll: f32,
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
    pub(crate) eject_mps: f32,
    /// The upward share of an ejected weapon's direction against its outward one.
    pub(crate) eject_lift: f32,
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
    pub(crate) restock_after_ms: Millis,
    /// How long a rack's copy is waited for.
    pub copy_wait_ms: Millis,
    /// How long a copy that was never collected, or that never came, keeps the rack's weapon
    /// hidden before it is shown again.
    pub(crate) uncollected_ms: Millis,
    pub paired: PairedRackTuning,
}

impl RackTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.restock_after_ms
            .validate("hold.rack.restock_after_ms", 10_000)?;
        self.copy_wait_ms
            .validate("hold.rack.copy_wait_ms", 30_000)?;
        self.uncollected_ms
            .validate("hold.rack.uncollected_ms", 60_000)?;
        self.paired.validate()
    }
}

/// The racks that hold a pair of pistols.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedRackTuning {
    /// How long a paired source stays known without being seen again.
    pub(crate) source_grace_ms: Millis,
    /// How long a paired member's copy is waited for.
    pub copy_wait_ms: Millis,
    /// How far the second pistol of a finite source is drawn from the first, metres: a pistol's
    /// thickness, a little more.
    pub(crate) stack_m: f32,
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
    pub(crate) gun_volume_margin_m: f32,
    /// How deep in a volume on a gun the hand is, as a share of its radius.
    pub(crate) gun_volume_depth: f32,
    pub(crate) marks: MarksTuning,
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
pub(crate) struct MarksTuning {
    /// The smallest mark, metres.
    pub(crate) min_m: f32,
    /// The largest mark, metres.
    pub max_m: f32,
    /// The mark where a held charge would stick, metres.
    pub(crate) charge_m: f32,
    /// The mark on a braced foregrip, metres.
    pub(crate) foregrip_m: f32,
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
    pub(crate) attachments_max_age_ms: Millis,
    /// The longest grip offset from a socket's bone, metres.
    pub(crate) max_grip_offset_m: f32,
    pub attraction: AttractionTuning,
}

impl DocksTuning {
    fn validate(&self) -> Result<(), Invalid> {
        self.attachments_max_age_ms
            .validate("docks.attachments_max_age_ms", 5_000)?;
        positive("docks.max_grip_offset_m", self.max_grip_offset_m, 5.0)?;
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

/// Where a hand touches a gun, and what it holds there (see
/// [`crate::controllers::weapons::regions`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionsTuning {
    /// How far outside the gun's model box a hand still touches the gun, metres.
    pub reach_m: f32,
    /// How far from the pistol grip a hand still holds the grip, metres.
    pub(crate) grip_radius_m: f32,
    /// How long a bracing hand takes to slide onto the grip.
    pub(crate) grip_slide_ms: Millis,
    pub shape: GunShapeTuning,
}

impl RegionsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("regions.reach_m", self.reach_m, 1.0)?;
        positive("regions.grip_radius_m", self.grip_radius_m, 1.0)?;
        self.grip_slide_ms
            .validate("regions.grip_slide_ms", 5_000)?;
        self.shape.validate()
    }
}

/// The regions of a gun's shape, in the gun's frame (see
/// [`crate::controllers::weapons::regions::shape::classify`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GunShapeTuning {
    /// The bottom of the pistol grip, from the attach point: down and a little back, metres.
    pub(crate) grip_bottom_m: Vec3,
    /// How far along the bore either side of the grip the grip's hand reaches, metres: the
    /// receiver's length the grip hand covers.
    pub(crate) grip_reach_m: f32,
    /// The radius of the action's handle, metres.
    pub(crate) action_radius_m: f32,
    /// How far a magazine hangs from its well, metres.
    pub(crate) magazine_length_m: f32,
    /// How thick a magazine is, metres.
    pub(crate) magazine_radius_m: f32,
    /// How far past the foregrip the handguard reaches, metres.
    pub(crate) past_foregrip_m: f32,
    /// The share of the way to the muzzle the handguard covers on a gun without a foregrip.
    pub(crate) handguard_share: f32,
    /// The diameter the fingers wrap on a barrel, metres.
    pub(crate) barrel_diameter_m: f32,
    /// The diameter the fingers wrap on a stock, metres.
    pub(crate) stock_diameter_m: f32,
}

impl GunShapeTuning {
    fn validate(&self) -> Result<(), Invalid> {
        vec3("regions.shape.grip_bottom_m", self.grip_bottom_m, 1.0)?;
        nonnegative("regions.shape.grip_reach_m", self.grip_reach_m, 1.0)?;
        positive("regions.shape.action_radius_m", self.action_radius_m, 1.0)?;
        nonnegative(
            "regions.shape.magazine_length_m",
            self.magazine_length_m,
            1.0,
        )?;
        positive(
            "regions.shape.magazine_radius_m",
            self.magazine_radius_m,
            1.0,
        )?;
        nonnegative("regions.shape.past_foregrip_m", self.past_foregrip_m, 2.0)?;
        range(
            "regions.shape.handguard_share",
            self.handguard_share,
            0.0,
            1.0,
        )?;
        positive(
            "regions.shape.barrel_diameter_m",
            self.barrel_diameter_m,
            0.5,
        )?;
        positive("regions.shape.stock_diameter_m", self.stock_diameter_m, 0.5)
    }
}

/// The dual-wield split's waits (see [`crate::controllers::weapons::dual_wield::single::grant`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DualWieldTuning {
    /// How long a twin is waited for once asked, seconds: the respawn's own wait.
    pub(crate) twin_wait_s: f32,
    /// How long a rack's copy is expected after the game's pickup of a rack with unlimited stock,
    /// seconds.
    pub(crate) rack_copy_cover_s: f32,
    /// How long a respawn's cover waits for the slot's pistol to come back after a reset, seconds.
    pub(crate) respawn_cover_s: f32,
}

impl DualWieldTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("dual_wield.twin_wait_s", self.twin_wait_s, 120.0)?;
        positive(
            "dual_wield.rack_copy_cover_s",
            self.rack_copy_cover_s,
            120.0,
        )?;
        positive("dual_wield.respawn_cover_s", self.respawn_cover_s, 120.0)
    }
}

/// The guns' own bodies (see [`crate::controllers::weapons::gun_bodies`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GunBodiesTuning {
    /// How lately the rigid grip must have pinned a gun for it to count as in the hand.
    pub(crate) pinned_lately_ms: Millis,
}

/// Where the pouches' models sit on their pouches (see
/// [`crate::controllers::weapons::pouch_models`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PouchModelsTuning {
    /// The grenade's, the magazine's, and the detonator's models: at the pouch's centre, square to
    /// the girdle.
    pub model: ModelPlacement,
    /// The C4 charge's model: at the pouch's centre, pitched down a quarter turn, so that the
    /// charge, whose in-hand model lies face up, lies flat on the chest with its face forward
    /// rather than standing out of it.
    pub charge: ModelPlacement,
}

impl PouchModelsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        for (path, placement) in [
            ("pouch_models.model", &self.model),
            ("pouch_models.charge", &self.charge),
        ] {
            vec3(&format!("{path}.offset_m"), placement.offset_m, 1.0)?;
            vec3(
                &format!("{path}.rotation_deg"),
                placement.rotation_deg,
                360.0,
            )?;
        }
        Ok(())
    }
}

/// How a press onto a chest pouch puts its item back (see
/// [`crate::controllers::weapons::pouch_return`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PouchReturnTuning {
    /// A grenade's press. Its reach rises above the pouch: a hand bringing a grenade back stops
    /// high, and a grenade let go at shoulder height, above the pouch, falls rather than
    /// returning to it.
    pub grenade: PressTuning,
    /// A held charge's return, near the pouch's surface without a deep push into the chest. The
    /// larger leave margin keeps a draw or a hand resting in the return volume from returning
    /// immediately.
    pub c4: PressTuning,
}

impl PouchReturnTuning {
    fn validate(&self) -> Result<(), Invalid> {
        for (path, press) in [
            ("pouch_return.grenade", &self.grenade),
            ("pouch_return.c4", &self.c4),
        ] {
            nonnegative(&format!("{path}.leave_margin_m"), press.leave_margin_m, 0.5)?;
            nonnegative(&format!("{path}.depth_m"), press.depth_m, 0.5)?;
            nonnegative(
                &format!("{path}.return_margin_m"),
                press.return_margin_m,
                0.5,
            )?;
            nonnegative(&format!("{path}.rise_m"), press.rise_m, 0.5)?;
        }
        Ok(())
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
