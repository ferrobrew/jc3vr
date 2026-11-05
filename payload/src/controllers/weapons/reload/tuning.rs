//! The true reload's fixed tuning, and the feature switches it takes from the configuration
//! ([`ReloadConfig`]).

use glam::Vec3;

use crate::controllers::weapons::reload::{
    config::{ExtractionConfig, ReloadConfig},
    plan::ReloadPlan,
};

/// The true reload's tuning with its default switches.
pub const RELOAD: Reload = Reload::new();

/// The true reload. Acts only under the controller scheme.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Reload {
    /// Reload by hand: the game's own reloads of the weapons the mod reloads are stopped, and a
    /// magazine is ejected, taken from the belt pouch, seated in the well, and chambered by hand.
    /// Off, the game's timed reload plays from the reload button as it does on a gamepad.
    pub enabled: bool,
    /// The longest press of the reload button that counts as a tap, seconds. The game holsters on
    /// a longer hold of the same button, so a hold never ejects.
    pub tap_s: f32,
    /// Eject a gun's magazine with a sharp flick of the hand holding it.
    pub flick_eject: bool,
    /// The angular speed of the gun hand that counts as a flick, degrees per second.
    pub flick_speed_dps: f32,
    /// The shortest time between two flick ejects, seconds.
    pub flick_cooldown_s: f32,
    /// The magazine well of a weapon the wells table has no entry for, in the holding hand's wrist
    /// frame (-Z toward the fingers, +Y out of the back of the hand), metres: under the gun, just
    /// ahead of the grip. Given for the right hand and mirrored for the left.
    pub well_offset_m: Vec3,
    /// How far ahead of the hand's grip the top of a held magazine sits, metres, along the
    /// direction the closed hand points.
    pub held_top_m: f32,
    /// How far out of the palm a held magazine's centre line sits, metres: about half a rifle
    /// magazine's depth from its back, which lies in the palm, to its front. The grip's origin is
    /// about where Rico's palm is drawn, so a magazine centred on it sinks into the hand.
    pub held_palm_gap_m: f32,
    /// How close the top of a held magazine must come to the well's mouth to catch it, metres: a
    /// little more than a magazine's own width, since nothing stops the hand at the well as the
    /// gun would, and the hand and the well each carry a centimetre or two of tracking and
    /// placement error.
    pub capture_radius_m: f32,
    /// How far a held magazine may be turned from the well's axis and still catch, degrees.
    pub capture_angle_deg: f32,
    /// How close the top of a held round must come to a launcher's well to catch it, metres. A
    /// launcher's well (a rocket tube's front, the UPU-210's cartridge bay) is large and sits far
    /// from the hand on the grip, so it catches from farther than a gun's.
    pub launcher_capture_radius_m: f32,
    /// How far a round held for a launcher may be turned from the well's axis and still catch,
    /// degrees.
    pub launcher_capture_angle_deg: f32,
    /// How far past the mouth a caught magazine travels to seat, metres.
    pub seat_depth_m: f32,
    /// How long a magazine taken from the pouch stays in the hand before it is dropped, seconds.
    pub timeout_s: f32,
    /// How long after a seat's bump the gun hand's ready cue plays, seconds, for a seat that leaves
    /// the gun able to fire.
    pub ready_cue_delay_s: f32,
    /// The one-handed belt reload: push the gun's well down onto the pouch.
    pub belt: BeltTuning,
    /// The slide, charging handle, bolt, and pump.
    pub action: ActionTuning,
    /// Ejected magazines that stay in the world with their rounds.
    pub loose: LooseTuning,
    /// Grip-and-pull removal of an inserted detachable magazine.
    pub extraction: ExtractionConfig,
}

impl Reload {
    const fn new() -> Self {
        Self {
            enabled: true,
            tap_s: 0.3,
            flick_eject: false,
            flick_speed_dps: 900.0,
            flick_cooldown_s: 0.6,
            well_offset_m: Vec3::new(0.0, -0.08, -0.06),
            held_top_m: 0.07,
            held_palm_gap_m: 0.03,
            capture_radius_m: 0.08,
            capture_angle_deg: 50.0,
            launcher_capture_radius_m: 0.12,
            launcher_capture_angle_deg: 75.0,
            seat_depth_m: 0.025,
            timeout_s: 8.0,
            ready_cue_delay_s: 0.15,
            belt: BeltTuning::new(),
            action: ActionTuning::new(),
            loose: LooseTuning::new(),
            extraction: ExtractionConfig::new(),
        }
    }

    /// The fixed tuning with the switches `config` sets.
    pub const fn of(config: &ReloadConfig) -> Self {
        Self {
            enabled: config.enabled,
            flick_eject: config.flick_eject,
            belt: BeltTuning {
                enabled: config.belt.enabled,
                ..RELOAD.belt
            },
            action: ActionTuning {
                physical: config.action.physical,
                ..RELOAD.action
            },
            loose: LooseTuning {
                enabled: config.loose.enabled,
                ..RELOAD.loose
            },
            extraction: config.extraction,
            ..RELOAD
        }
    }

    /// The tuning a round for `plan`'s weapon is inserted with: a launcher's catches at the
    /// launcher capture radius and angle.
    pub fn for_plan(self, plan: &ReloadPlan) -> Self {
        if plan.launcher {
            self.launcher_capture()
        } else {
            self
        }
    }

    /// The tuning with a launcher's capture radius and angle in place of a gun's.
    pub const fn launcher_capture(self) -> Self {
        Self {
            capture_radius_m: self.launcher_capture_radius_m,
            capture_angle_deg: self.launcher_capture_angle_deg,
            ..self
        }
    }
}

/// Magazines in the world (see [`crate::controllers::weapons::reload::loose`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LooseTuning {
    /// An ejected magazine keeps the rounds it held as an object in the world, and a dropped
    /// magazine that fits the held gun can be picked up and seated with its rounds.
    pub enabled: bool,
    /// The radius of a dropped magazine's grab volume, metres.
    pub radius_m: f32,
    /// How far from the player a dropped magazine may be to be picked up, metres.
    pub range_m: f32,
    /// How far behind its top a held magazine's centre is, metres: the hand carries the magazine's
    /// body there.
    pub half_length_m: f32,
    /// The fastest a held magazine is moved toward the hand, metres per second.
    pub max_carry_mps: f32,
    /// How long a dropped magazine is remembered, seconds.
    pub keep_s: f32,
}

impl LooseTuning {
    const fn new() -> Self {
        Self {
            enabled: true,
            radius_m: 0.1,
            range_m: 2.0,
            half_length_m: 0.06,
            max_carry_mps: 20.0,
            keep_s: 300.0,
        }
    }
}

/// The one-handed belt reload (see [`crate::controllers::weapons::reload::belt`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct BeltTuning {
    pub enabled: bool,
    /// How far the gun's insertion axis may lean from straight up, degrees: the well must face the
    /// belt.
    pub max_tilt_deg: f32,
    /// How fast the well must be moving down as it enters the pouch, metres per second: a push,
    /// not a hover.
    pub min_push_speed_mps: f32,
    /// How far into the pouch the well must be pushed, metres.
    pub depth_m: f32,
    /// How long after a draw the belt reload waits, seconds, so that drawing from a hip holster
    /// beside the pouch does not reload.
    pub after_draw_s: f32,
}

impl BeltTuning {
    const fn new() -> Self {
        Self {
            enabled: true,
            max_tilt_deg: 45.0,
            min_push_speed_mps: 0.3,
            depth_m: 0.03,
            after_draw_s: 0.8,
        }
    }
}

/// The slide, charging handle, bolt, and pump (see
/// [`crate::controllers::weapons::reload::chamber`]).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ActionTuning {
    /// The action is a part that follows the hand through its travel: a slide or a charging handle
    /// runs home on its spring when let go, a slide catches at the rear on an empty magazine, a
    /// pump stays where it is left, and a bolt turns up before it slides. Off, the action is a
    /// gesture: a pull to the pull share and a release or a push home.
    pub physical: bool,
    /// How close the off-hand's grip must be to the handle to grab it, metres. A closed hand
    /// feeling along the gun for the handle lands a few centimetres off it, and while the gun
    /// waits on its action the handle is taken first within this reach
    /// ([`prefer_waiting_action`](crate::controllers::weapons::reload::chamber::prefer_waiting_action)).
    pub grab_radius_m: f32,
    /// The share of the action's travel a pull must reach, `0..=1`.
    pub pull_fraction: f32,
    /// How close to its rest a pulled action must come back to count as returned, as a share of
    /// its travel.
    pub return_fraction: f32,
    /// How far the hand may stray from the gun's axis before the grab lets go, metres.
    pub release_distance_m: f32,
    /// The travel of an action the wells table gives none for, metres.
    pub default_travel_m: f32,
    /// How long a released slide or charging handle takes to run home from the rear on its
    /// spring, seconds.
    pub spring_s: f32,
    /// How far a bolt's handle turns up to unlock it, degrees.
    pub bolt_lift_deg: f32,
    /// How far a bolt's handle is from the bolt's axis, metres.
    pub bolt_radius_m: f32,
    /// The share of the bolt's turn that counts as turned all the way, `0.5..=1`; the turn down
    /// counts once it is within the same share of the bottom.
    pub lift_fraction: f32,
}

impl ActionTuning {
    const fn new() -> Self {
        Self {
            grab_radius_m: 0.105,
            pull_fraction: 0.8,
            return_fraction: 0.3,
            release_distance_m: 0.15,
            default_travel_m: 0.06,
            physical: true,
            spring_s: 0.05,
            bolt_lift_deg: 60.0,
            bolt_radius_m: 0.03,
            lift_fraction: 0.85,
        }
    }
}
