//! The true reload's tuning, read from the shipped `tuning/reload.toml` ([`RELOAD`]), and the
//! switch it takes from the configuration ([`ReloadConfig`]).

use glam::Vec3;
use jc3vr_core::{
    config::{Invalid, nonnegative, positive, range, vec3},
    tuning::Tuning,
};
use serde::Deserialize;

use crate::controllers::weapons::reload::{
    config::ReloadConfig,
    plan::{LoadKind, ReloadPlan},
};

jc3vr_core::tuning_file! {
    /// The true reload's tuning file.
    pub static RELOAD: ReloadTuning = "reload.toml";
}

/// The true reload as the tuning file holds it: everything short of the switch, which the
/// configuration holds.
pub(crate) type ReloadTuning = Reload<()>;

/// The true reload. Acts only under the controller scheme. `S` is the switch: `bool` as the
/// reload runs ([`Reload::of`]), and `()` in the file, which carries no switch.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reload<S = bool> {
    /// Reload by hand: the game's own reloads of the weapons the mod reloads are stopped, and a
    /// magazine is ejected, taken from the belt pouch, and seated in the well by hand, which
    /// chambers its first round.
    /// Off, the game's timed reload plays from the reload button as it does on a gamepad.
    #[serde(skip)]
    pub enabled: S,
    /// The longest press of the reload button that counts as a tap, seconds. The game holsters on
    /// a longer hold of the same button, so a hold never ejects.
    pub tap_s: f32,
    /// The longest from the end of a double tap's first tap to the end of its second, seconds
    /// ([`DoubleTapDetector`](crate::controllers::weapons::reload::eject::DoubleTapDetector)).
    pub(crate) double_tap_gap_s: f32,
    /// The magazine well of a weapon the wells table has no entry for, in the holding hand's wrist
    /// frame (-Z toward the fingers, +Y out of the back of the hand), metres: under the gun, just
    /// ahead of the grip. Given for the right hand and mirrored for the left.
    pub(crate) well_offset_m: Vec3,
    /// How far ahead of the hand's grip the top of a held magazine sits, metres, along the
    /// direction the closed hand points.
    pub(crate) held_top_m: f32,
    /// How far out of the palm a held magazine's centre line sits, metres: about half a rifle
    /// magazine's depth from its back, which lies in the palm, to its front. The grip's origin is
    /// about where Rico's palm is drawn, so a magazine centred on it sinks into the hand.
    pub(crate) held_palm_gap_m: f32,
    /// How close the top of a held magazine must come to the well's mouth to catch it, metres: a
    /// little more than a magazine's own width, since nothing stops the hand at the well as the
    /// gun would, and the hand and the well each carry a centimetre or two of tracking and
    /// placement error.
    pub(crate) capture_radius_m: f32,
    /// How far a held magazine may be turned from the well's axis and still catch, degrees.
    pub(crate) capture_angle_deg: f32,
    /// How far a caught magazine may stray from the well's axis, and how far back out of the
    /// mouth, before the well lets it go, as a multiple of the capture radius.
    pub(crate) retain_radius_scale: f32,
    /// The slowest push of the held magazine's tip into the well's mouth while the old magazine is
    /// still in, along the well into the gun and relative to it, that knocks the old one out,
    /// metres per second at the player's scale; a slower push is refused. In the 10 November 2026
    /// session the pushes that came to load were 0.3 to 0.5 m/s at the mouth and the strikes 1.9
    /// to 3.2.
    pub(crate) push_knock_mps: f32,
    /// How close the top of a held round must come to a launcher's well to catch it, metres. A
    /// launcher's well (a rocket tube's front, the UPU-210's cartridge bay) is large and sits far
    /// from the hand on the grip, so it catches from farther than a gun's.
    pub(crate) launcher_capture_radius_m: f32,
    /// How far a round held for a launcher may be turned from the well's axis and still catch,
    /// degrees.
    pub(crate) launcher_capture_angle_deg: f32,
    /// How far past the mouth a caught magazine travels to seat, metres.
    pub(crate) seat_depth_m: f32,
    /// The window of depths, as shares of the seat depth, in which a caught magazine let go of
    /// seats where it is rather than floating in: from the first to the second.
    pub(crate) release_seat_shares: (f32, f32),
    /// How long a magazine taken from the pouch stays in the hand before it is dropped, seconds.
    pub timeout_s: f32,
    /// How long after a seat's bump the gun hand's ready cue plays, seconds, for a seat that leaves
    /// the gun able to fire.
    pub(crate) ready_cue_delay_s: f32,
    /// How many pulses the ready cue plays, and the seconds between their starts: longer than the
    /// pulse itself, so the pulses stay apart.
    pub(crate) ready_cue_pulses: (u32, f32),
    /// How long an armed weapon's magazine stays empty before the game's reload refills it,
    /// milliseconds: about as long as the game's reload takes
    /// ([`crate::controllers::weapons::reload::native`]).
    pub(crate) refill_delay_ms: u64,
    /// The one-handed belt reload: push the gun's well down onto the pouch.
    pub belt: BeltTuning,
    /// The slide, charging handle, bolt, and pump.
    pub action: ActionTuning,
    /// Ejected magazines that stay in the world with their rounds.
    pub loose: LooseTuning,
    /// Grip-and-pull removal of an inserted detachable magazine.
    pub(crate) extraction: ExtractionTuning,
    /// A round let go of near the well, floating into it.
    pub float: FloatTuning,
    /// A held magazine knocking the inserted one out.
    pub knock: KnockTuning,
    /// The inserted magazine's grab zone.
    pub zones: ZoneTuning,
    /// A revolver's cylinder.
    pub revolver: RevolverTuning,
    /// The casings a revolver's cylinder dumps.
    pub(crate) casings: CasingsTuning,
    /// The insertion guide drawn at the well.
    pub guide: GuideTuning,
    /// The stop on the game's own reloads.
    pub gate: GateTuning,
}

impl Tuning for ReloadTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("tap_s", self.tap_s, 2.0)?;
        positive("double_tap_gap_s", self.double_tap_gap_s, 2.0)?;
        vec3("well_offset_m", self.well_offset_m, 0.5)?;
        nonnegative("held_top_m", self.held_top_m, 0.5)?;
        nonnegative("held_palm_gap_m", self.held_palm_gap_m, 0.2)?;
        positive("capture_radius_m", self.capture_radius_m, 0.5)?;
        range("capture_angle_deg", self.capture_angle_deg, 0.0, 90.0)?;
        range("retain_radius_scale", self.retain_radius_scale, 1.0, 10.0)?;
        positive("push_knock_mps", self.push_knock_mps, 10.0)?;
        positive(
            "launcher_capture_radius_m",
            self.launcher_capture_radius_m,
            1.0,
        )?;
        range(
            "launcher_capture_angle_deg",
            self.launcher_capture_angle_deg,
            0.0,
            90.0,
        )?;
        nonnegative("seat_depth_m", self.seat_depth_m, 0.5)?;
        let (seat_min, seat_max) = self.release_seat_shares;
        nonnegative("release_seat_shares.0", seat_min, 1.0)?;
        range("release_seat_shares.1", seat_max, seat_min.max(1.0), 10.0)?;
        positive("timeout_s", self.timeout_s, 600.0)?;
        nonnegative("ready_cue_delay_s", self.ready_cue_delay_s, 5.0)?;
        if self.ready_cue_pulses.0 > 16 {
            return Err(Invalid(format!(
                "ready_cue_pulses.0 must be at most 16, got {}",
                self.ready_cue_pulses.0
            )));
        }
        nonnegative("ready_cue_pulses.1", self.ready_cue_pulses.1, 2.0)?;
        milliseconds("refill_delay_ms", self.refill_delay_ms, 60_000)?;
        self.belt.validate()?;
        self.action.validate()?;
        self.loose.validate()?;
        self.extraction.validate()?;
        self.float.validate()?;
        self.knock.validate()?;
        self.zones.validate()?;
        self.revolver.validate()?;
        self.casings.validate()?;
        self.guide.validate()?;
        milliseconds("gate.stale_ms", self.gate.stale_ms, 10_000)
    }
}

impl Reload {
    /// The file's tuning with the switch `config` sets.
    pub fn of(config: &ReloadConfig) -> Self {
        RELOAD.get().with_switch(config.enabled)
    }

    /// The shipped tuning with the switch on.
    #[cfg(test)]
    pub(crate) fn shipped() -> Self {
        Self::of(&ReloadConfig::new())
    }
}

impl<S: Copy> Reload<S> {
    /// The tuning with a launcher's capture radius and angle in place of a gun's.
    pub(crate) fn launcher_capture(self) -> Self {
        Self {
            capture_radius_m: self.launcher_capture_radius_m,
            capture_angle_deg: self.launcher_capture_angle_deg,
            ..self
        }
    }

    /// The tuning a round for `plan`'s weapon is inserted with: a launcher's catches at the
    /// launcher capture radius and angle, and a stand-in round (a shell, or a revolver's ring of
    /// rounds) is held with its side in the palm, its axis as far out of the palm as the group's
    /// half width rather than a rifle magazine's.
    pub(crate) fn for_plan(self, plan: &ReloadPlan) -> Self {
        let tuning = if plan.launcher {
            self.launcher_capture()
        } else {
            self
        };
        Self {
            held_palm_gap_m: plan
                .round
                .map_or(tuning.held_palm_gap_m, |round| round.half_width_m()),
            // A speedloader seats as it meets the cylinder.
            seat_depth_m: if plan.load == LoadKind::Cylinder {
                0.0
            } else {
                tuning.seat_depth_m
            },
            ..tuning
        }
    }

    /// The tuning with the switch `enabled`.
    fn with_switch<T>(&self, enabled: T) -> Reload<T> {
        Reload {
            enabled,
            tap_s: self.tap_s,
            double_tap_gap_s: self.double_tap_gap_s,
            well_offset_m: self.well_offset_m,
            held_top_m: self.held_top_m,
            held_palm_gap_m: self.held_palm_gap_m,
            capture_radius_m: self.capture_radius_m,
            capture_angle_deg: self.capture_angle_deg,
            retain_radius_scale: self.retain_radius_scale,
            push_knock_mps: self.push_knock_mps,
            launcher_capture_radius_m: self.launcher_capture_radius_m,
            launcher_capture_angle_deg: self.launcher_capture_angle_deg,
            seat_depth_m: self.seat_depth_m,
            release_seat_shares: self.release_seat_shares,
            timeout_s: self.timeout_s,
            ready_cue_delay_s: self.ready_cue_delay_s,
            ready_cue_pulses: self.ready_cue_pulses,
            refill_delay_ms: self.refill_delay_ms,
            belt: self.belt,
            action: self.action,
            loose: self.loose,
            extraction: self.extraction,
            float: self.float,
            knock: self.knock,
            zones: self.zones,
            revolver: self.revolver,
            casings: self.casings,
            guide: self.guide,
            gate: self.gate,
        }
    }
}

/// Magazines in the world (see [`crate::controllers::weapons::reload::loose`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LooseTuning {
    /// The radius of a dropped magazine's grab volume, metres.
    pub radius_m: f32,
    /// How far from the player a dropped magazine may be to be picked up, metres.
    pub range_m: f32,
    /// How far behind its top a held magazine's centre is, metres: the hand carries the magazine's
    /// body there.
    pub half_length_m: f32,
    /// The fastest a held magazine is moved toward the hand, metres per second.
    pub(crate) max_carry_mps: f32,
    /// The fastest a held magazine is turned toward the hand's pose, radians per second.
    pub(crate) max_carry_turn_radps: f32,
    /// How long a dropped magazine is remembered, seconds.
    pub(crate) keep_s: f32,
}

impl LooseTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("loose.radius_m", self.radius_m, 1.0)?;
        positive("loose.range_m", self.range_m, 20.0)?;
        positive("loose.half_length_m", self.half_length_m, 0.5)?;
        positive("loose.max_carry_mps", self.max_carry_mps, 200.0)?;
        positive(
            "loose.max_carry_turn_radps",
            self.max_carry_turn_radps,
            200.0,
        )?;
        positive("loose.keep_s", self.keep_s, 3600.0)
    }
}

/// The one-handed belt reload (see [`crate::controllers::weapons::reload::belt`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeltTuning {
    /// How far the gun's insertion axis may lean from straight up, degrees: the magazine must point
    /// down at the belt, give or take a gun held at an angle in a hand lowered to the hip.
    pub max_tilt_deg: f32,
    /// The push sphere's radius around the pouch's centre, metres: wider than the pouch's grab
    /// volume, since the gun hand cannot see the magazine's base beneath the gun.
    pub radius_m: f32,
    /// How far into the push sphere the point must go, metres.
    pub depth_m: f32,
    /// How far outside the push sphere an upright gun's point may be for the push to be shown, with
    /// the ring on the pouch, metres.
    pub approach_m: f32,
    /// How long after a draw the belt reload waits, seconds, so that drawing from a hip holster
    /// beside the pouch does not reload.
    pub(crate) after_draw_s: f32,
}

impl BeltTuning {
    fn validate(&self) -> Result<(), Invalid> {
        range("belt.max_tilt_deg", self.max_tilt_deg, 0.0, 180.0)?;
        positive("belt.radius_m", self.radius_m, 1.0)?;
        range("belt.depth_m", self.depth_m, 0.0, self.radius_m)?;
        nonnegative("belt.approach_m", self.approach_m, 2.0)?;
        nonnegative("belt.after_draw_s", self.after_draw_s, 10.0)
    }
}

/// The slide, charging handle, bolt, and pump (see
/// [`crate::controllers::weapons::reload::chamber`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionTuning {
    /// How close the off-hand's grip must be to the handle to grab it, metres. A closed hand
    /// feeling along the gun for the handle lands a few centimetres off it, and while the gun
    /// waits on its action the handle is taken first within this reach
    /// ([`prefer_waiting_action`](crate::controllers::weapons::reload::chamber::prefer_waiting_action)).
    pub grab_radius_m: f32,
    /// The share of the action's travel a pull must reach, `0..=1`.
    pub(crate) pull_fraction: f32,
    /// How close to its rest a pulled action must come back to count as returned, as a share of
    /// its travel.
    pub(crate) return_fraction: f32,
    /// How far the hand may stray from the gun's axis before the grab lets go, metres.
    pub(crate) release_distance_m: f32,
    /// The travel of an action the wells table gives none for, metres.
    pub(crate) default_travel_m: f32,
    /// How long a released slide or charging handle takes to run home from the rear on its
    /// spring, seconds.
    pub(crate) spring_s: f32,
    /// How far a bolt's handle turns up to unlock it, degrees.
    pub(crate) bolt_lift_deg: f32,
    /// How far a bolt's handle is from the bolt's axis, metres.
    pub(crate) bolt_radius_m: f32,
    /// The share of the bolt's turn that counts as turned all the way, `0.5..=1`; the turn down
    /// counts once it is within the same share of the bottom.
    pub(crate) lift_fraction: f32,
}

impl ActionTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("action.grab_radius_m", self.grab_radius_m, 1.0)?;
        positive("action.pull_fraction", self.pull_fraction, 1.0)?;
        range(
            "action.return_fraction",
            self.return_fraction,
            0.0,
            self.pull_fraction,
        )?;
        positive("action.release_distance_m", self.release_distance_m, 1.0)?;
        positive("action.default_travel_m", self.default_travel_m, 0.5)?;
        positive("action.spring_s", self.spring_s, 5.0)?;
        positive("action.bolt_lift_deg", self.bolt_lift_deg, 180.0)?;
        positive("action.bolt_radius_m", self.bolt_radius_m, 0.5)?;
        range("action.lift_fraction", self.lift_fraction, 0.5, 1.0)
    }
}

/// Grip-and-pull removal of a detachable magazine, with no release-button latch (see
/// [`crate::controllers::weapons::reload::extraction`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExtractionTuning {
    /// The inserted magazine's grab radius, metres at the player's scale.
    pub grab_radius_m: f32,
    /// The outward hand travel required to detach the magazine, metres at the player's scale.
    pub pull_m: f32,
    /// How far the hand may move sideways from its acquired path, metres at the player's scale.
    pub(crate) lateral_tolerance_m: f32,
}

impl ExtractionTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("extraction.grab_radius_m", self.grab_radius_m, 1.0)?;
        positive("extraction.pull_m", self.pull_m, 1.0)?;
        positive(
            "extraction.lateral_tolerance_m",
            self.lateral_tolerance_m,
            1.0,
        )
    }
}

/// How a round let go of near the well floats into it (see
/// [`crate::controllers::weapons::reload::insert`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloatTuning {
    /// How far from the mouth the top of a round let go of may be to float in, metres: about a
    /// hand's breadth, so a magazine released while it is being brought to the well goes in, and
    /// one let go of anywhere else drops. A well whose capture radius is large floats from
    /// [`capture_radius_scale`](Self::capture_radius_scale) times that instead.
    pub radius_m: f32,
    /// The multiple of the capture radius a round floats in from when that is farther than
    /// [`radius_m`](Self::radius_m).
    pub(crate) capture_radius_scale: f32,
    /// How far a round let go of may be turned from the well's axis and float in, degrees: wider
    /// than the capture angle, as the round turns onto the axis on the way.
    pub angle_deg: f32,
    /// How fast the round travels in on average, metres per second.
    pub speed_mps: f32,
    /// The shortest and the longest the float takes, seconds.
    pub(crate) min_s: f32,
    pub(crate) max_s: f32,
    /// The share of a float spent bringing the top onto the axis, before it slides down it.
    pub(crate) approach_share: f32,
}

impl FloatTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("float.radius_m", self.radius_m, 1.0)?;
        nonnegative(
            "float.capture_radius_scale",
            self.capture_radius_scale,
            10.0,
        )?;
        range("float.angle_deg", self.angle_deg, 0.0, 90.0)?;
        positive("float.speed_mps", self.speed_mps, 20.0)?;
        positive("float.min_s", self.min_s, 5.0)?;
        range("float.max_s", self.max_s, self.min_s, 5.0)?;
        range("float.approach_share", self.approach_share, 0.01, 0.99)
    }
}

/// A held magazine knocking the inserted one out (see
/// [`crate::controllers::weapons::reload::knock`]). Lengths are in the gun model's metres.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnockTuning {
    /// The slowest closing speed of the held magazine's tip against the inserted magazine,
    /// relative to the gun, that knocks it out, metres per second at the player's scale. A hand
    /// brought to the gun to load moves at a few tenths of a metre per second, and a deliberate
    /// strike at one and a half to three, so a gentle touch does nothing. The speed is one tick's
    /// travel into the magazine, which reads 55 to 70 percent of the hand's peak as it slows into
    /// the contact: on the 14 November 2026 take strikes whose hands peaked at 1.2 to 1.8 m/s read
    /// 0.8 to 1.0 and were turned away at 1.0, while the seat attempts read 0.2 to 0.5.
    pub min_speed_mps: f32,
    /// How far from the magazine's axis the held tip touches it: a magazine's half-width plus the
    /// held tip's own, and a little of the hand's tracking error.
    pub radius_m: f32,
    /// How far into the well above the mouth the zone reaches, so a strike at the joint of the
    /// magazine and the gun counts.
    pub(crate) lip_m: f32,
    /// The share of the tip's speed the struck point is sent on at. A collision with a hand-driven
    /// body would send it on faster than the tip, but a magazine knocked at that speed leaves the
    /// view before the eye finds it; at about half, a strike of two to four metres per second
    /// drops it clear of the gun where it can be seen falling.
    pub share: f32,
    /// The most a knock adds to the magazine's speed, metres per second.
    pub(crate) max_knock_mps: f32,
    /// The most a knock adds to the magazine's spin, radians per second.
    pub(crate) max_spin_radps: f32,
    /// How many input ticks a knocked magazine's impulse waits on its body becoming readable.
    pub(crate) impulse_ticks: u32,
    /// How many input ticks the gun's magazine part stays drawn in the well after the knocked
    /// prop's impulse lands, so the magazine does not vanish while its prop is not yet drawn.
    pub(crate) linger_ticks: u32,
    /// How long the grab view shows a knock, milliseconds.
    pub(crate) flash_ms: u64,
    /// How long a published knock view is drawn without a newer one, milliseconds.
    pub(crate) view_max_age_ms: u64,
}

impl KnockTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("knock.min_speed_mps", self.min_speed_mps, 10.0)?;
        positive("knock.radius_m", self.radius_m, 0.5)?;
        nonnegative("knock.lip_m", self.lip_m, 0.5)?;
        range("knock.share", self.share, 0.0, 2.0)?;
        nonnegative("knock.max_knock_mps", self.max_knock_mps, 50.0)?;
        nonnegative("knock.max_spin_radps", self.max_spin_radps, 500.0)?;
        count("knock.impulse_ticks", self.impulse_ticks, 1000)?;
        count("knock.linger_ticks", self.linger_ticks, 1000)?;
        milliseconds("knock.flash_ms", self.flash_ms, 10_000)?;
        milliseconds("knock.view_max_age_ms", self.view_max_age_ms, 10_000)
    }
}

/// The inserted magazine's grab zone (see [`crate::controllers::weapons::reload::zones`]), in the
/// gun model's metres.
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZoneTuning {
    /// How far into the well above its mouth: the base plate of a pistol's magazine sits at the
    /// bottom of the grip, and a hand pinching it is level with the mouth.
    pub(crate) lip_m: f32,
    /// How far below the mouth: past the longest magazine, plus a hand's grip on its end.
    pub length_m: f32,
    /// How far from the axis: a magazine's half-width plus the hand's tracking and placement
    /// error, short of a long gun's pistol grip behind the well.
    pub radius_m: f32,
    /// How long a published zone view is drawn without a newer one, milliseconds.
    pub(crate) view_max_age_ms: u64,
}

impl ZoneTuning {
    fn validate(&self) -> Result<(), Invalid> {
        nonnegative("zones.lip_m", self.lip_m, 0.5)?;
        positive("zones.length_m", self.length_m, 2.0)?;
        positive("zones.radius_m", self.radius_m, 0.5)?;
        milliseconds("zones.view_max_age_ms", self.view_max_age_ms, 10_000)
    }
}

/// A revolver's cylinder's handling (see [`crate::controllers::weapons::reload::revolver`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevolverTuning {
    /// How fast the gun hand must roll about the barrel's line for a flick, degrees per second.
    pub(crate) flick_dps: f32,
    /// The shortest time between one flick and the next, seconds.
    pub(crate) flick_cooldown_s: f32,
    /// How long the cylinder takes to swing out, seconds, about as long as the clip's swing.
    pub open_s: f32,
    /// How long it takes to swing shut, seconds.
    pub(crate) close_s: f32,
    /// How far the muzzle must point above the horizon to dump the cylinder, degrees.
    pub(crate) dump_elevation_deg: f32,
    /// How many turns the cylinder spins as it closes, and over how long, seconds.
    pub(crate) spin_turns: f32,
    pub(crate) spin_s: f32,
}

impl RevolverTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("revolver.flick_dps", self.flick_dps, 5000.0)?;
        nonnegative("revolver.flick_cooldown_s", self.flick_cooldown_s, 5.0)?;
        positive("revolver.open_s", self.open_s, 5.0)?;
        positive("revolver.close_s", self.close_s, 5.0)?;
        range(
            "revolver.dump_elevation_deg",
            self.dump_elevation_deg,
            0.0,
            90.0,
        )?;
        nonnegative("revolver.spin_turns", self.spin_turns, 20.0)?;
        positive("revolver.spin_s", self.spin_s, 5.0)
    }
}

/// The casings a revolver's cylinder dumps (see [`crate::controllers::weapons::reload::casings`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CasingsTuning {
    /// How long a dumped casing is drawn falling, seconds.
    pub(crate) fall_s: f32,
    /// How fast a casing leaves its chamber, out of the cylinder's rear, metres per second.
    pub(crate) eject_mps: f32,
    /// How fast a casing tumbles, radians per second.
    pub(crate) tumble_radps: f32,
}

impl CasingsTuning {
    fn validate(&self) -> Result<(), Invalid> {
        positive("casings.fall_s", self.fall_s, 10.0)?;
        nonnegative("casings.eject_mps", self.eject_mps, 20.0)?;
        nonnegative("casings.tumble_radps", self.tumble_radps, 200.0)
    }
}

/// The insertion guide: the magazine's outline drawn at its seat in the well, and a preview of it
/// sliding in from the capture radius while the magazine is held toward the well (see
/// [`crate::controllers::weapons::reload::guide`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideTuning {
    /// How long a submitted guide stays drawn without a newer one, milliseconds.
    pub ttl_ms: u64,
    /// The outline's line width, metres at the player's scale.
    pub width_m: f32,
    /// The outline's colour while the magazine is held toward the well, linear RGB.
    pub(crate) holding_rgb: [f32; 3],
    /// The outline's colour once the well has caught the magazine, linear RGB.
    pub(crate) caught_rgb: [f32; 3],
    /// The opacity of the outline's parts hidden behind the gun, as a share of its own.
    pub occluded_alpha: f32,
    /// The seat outline's opacity while the magazine is held toward the well.
    pub(crate) seat_holding_alpha: f32,
    /// The seat outline's opacity once the well has caught the magazine.
    pub(crate) seat_caught_alpha: f32,
    /// The sliding preview's opacity at its fullest.
    pub(crate) preview_alpha: f32,
    /// How long one slide of the preview takes, seconds.
    pub(crate) preview_period_s: f32,
    /// The share of a slide's period the preview fades in over, at the capture radius.
    pub(crate) preview_fade_in_share: f32,
    /// The share of a slide's period the preview travels from the capture radius to the seat over,
    /// once faded in.
    pub(crate) preview_travel_share: f32,
    /// The share of a slide's period at which the preview has faded out, and the share it fades
    /// out over before that.
    pub(crate) preview_fade_out_end_share: f32,
    pub(crate) preview_fade_out_share: f32,
}

impl GuideTuning {
    fn validate(&self) -> Result<(), Invalid> {
        milliseconds("guide.ttl_ms", self.ttl_ms, 10_000)?;
        positive("guide.width_m", self.width_m, 0.1)?;
        for (name, rgb) in [
            ("guide.holding_rgb", self.holding_rgb),
            ("guide.caught_rgb", self.caught_rgb),
        ] {
            for (i, channel) in rgb.into_iter().enumerate() {
                range(&format!("{name}.{i}"), channel, 0.0, 1.0)?;
            }
        }
        for (name, alpha) in [
            ("guide.occluded_alpha", self.occluded_alpha),
            ("guide.seat_holding_alpha", self.seat_holding_alpha),
            ("guide.seat_caught_alpha", self.seat_caught_alpha),
            ("guide.preview_alpha", self.preview_alpha),
        ] {
            range(name, alpha, 0.0, 1.0)?;
        }
        positive("guide.preview_period_s", self.preview_period_s, 30.0)?;
        positive(
            "guide.preview_fade_in_share",
            self.preview_fade_in_share,
            1.0,
        )?;
        positive("guide.preview_travel_share", self.preview_travel_share, 1.0)?;
        range(
            "guide.preview_fade_out_end_share",
            self.preview_fade_out_end_share,
            self.preview_fade_in_share,
            1.0,
        )?;
        positive(
            "guide.preview_fade_out_share",
            self.preview_fade_out_share,
            1.0,
        )
    }
}

/// The stop on the game's own reloads (see [`crate::controllers::weapons::reload::gate`]).
#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateTuning {
    /// How long the input tick's state stands without a refresh, milliseconds. Past it the
    /// detours stop acting, so a stalled input tick hands the weapon back to the game.
    pub stale_ms: u64,
}

/// Check that a duration is above zero and at most `max_ms`.
fn milliseconds(name: &str, value: u64, max_ms: u64) -> Result<(), Invalid> {
    if value == 0 || value > max_ms {
        return Err(Invalid(format!(
            "{name} must be in [1, {max_ms}] milliseconds, got {value}"
        )));
    }
    Ok(())
}

/// Check that a count is above zero and at most `max`.
fn count(name: &str, value: u32, max: u32) -> Result<(), Invalid> {
    if value == 0 || value > max {
        return Err(Invalid(format!(
            "{name} must be in [1, {max}], got {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_loads_and_validates() {
        assert_eq!(RELOAD.get().validate(), Ok(()));
    }
}
