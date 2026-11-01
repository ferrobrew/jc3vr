//! The C4 throw's, the press's, and the wrist detonator's fixed tuning, and the switches they take
//! from the configuration ([`C4Config`]).

use glam::Vec3;

use crate::controllers::weapons::config::C4Config;

/// The C4 tuning with its default switches.
pub const C4: C4Tuning = C4Tuning::new();

/// The throw of a charge from the C4 pouch (see [`crate::controllers::weapons::c4`]); the pouch
/// itself is placed with the others
/// ([`POUCHES`](crate::controllers::weapons::volumes::POUCHES)).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct C4Tuning {
    /// Where the held charge sits in the throwing controller's grip frame, metres, given for the
    /// right hand and mirrored for the left.
    pub hold_offset_m: Vec3,
    /// The detonator button on the off-hand's wrist.
    pub detonator: DetonatorTuning,
    /// Pressing a held charge onto a surface sticks it there.
    pub stick: StickTuning,
}

impl C4Tuning {
    const fn new() -> Self {
        Self {
            hold_offset_m: Vec3::ZERO,
            detonator: DetonatorTuning::new(),
            stick: StickTuning::new(),
        }
    }

    /// The fixed tuning with the switches `config` sets.
    pub const fn of(config: &C4Config) -> Self {
        Self {
            detonator: DetonatorTuning {
                overlay: config.detonator.overlay,
                ..C4.detonator
            },
            stick: StickTuning {
                enabled: config.stick.enabled,
                ..C4.stick
            },
            ..C4
        }
    }
}

/// The detonator button on the inside of the off-hand's wrist, pressed by the dominant hand's
/// index fingertip (see [`crate::controllers::weapons::c4::press`]). The button's frame is the
/// off-hand's grip frame, given for the left hand (the off-hand of a right-handed player) and
/// mirrored for the right.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DetonatorTuning {
    /// The centre of the button's face, metres. The grip's origin is the centre of the closed
    /// fist, -Z runs out of the front of the fist and +Z back toward the wrist, and +X on the left
    /// hand points out of the palm.
    pub offset_m: Vec3,
    /// The direction out of the button's face, which the button is drawn facing. Normalised on
    /// use.
    pub normal: Vec3,
    /// The drawn button's radius, metres. The touch sphere is drawn around it as a ring.
    pub radius_m: f32,
    /// How close to the button's centre the fingertip must come to touch it, metres.
    pub touch_radius_m: f32,
    /// How far from the button's centre a fingertip must move away before the button presses
    /// again, metres. More than `touch_radius_m`, so that a fingertip resting on the button does
    /// not press it twice.
    pub release_radius_m: f32,
    /// How close to the button's centre the fingertip comes before the button shows it
    /// approaching, metres: the fingertip's dot appears, a ring closes in on the button as the
    /// fingertip nears, and the pressing hand ticks once on the way in.
    pub approach_radius_m: f32,
    /// How far behind the face's plane a fingertip may be and still touch, metres: further behind,
    /// it has come through the wrist, which a real finger cannot.
    pub max_behind_m: f32,
    /// How fast the fingertip must be closing on the button as it touches it, metres per second,
    /// for the touch to press at once.
    pub min_entry_speed_mps: f32,
    /// How long a slower fingertip must keep touching to press, seconds.
    pub dwell_s: f32,
    /// Where the index fingertip is in the dominant controller's aim frame while the finger points
    /// (off the trigger), metres, given for the right hand and mirrored for the left. Used when the
    /// runtime reports no tracked hand skeleton.
    pub fingertip_pointing_m: Vec3,
    /// Where the index fingertip is in the dominant controller's aim frame while it rests on the
    /// trigger, metres, given for the right hand and mirrored for the left.
    pub fingertip_resting_m: Vec3,
    /// Draw the button on the wrist, facing out of it: dim with no charges placed, lit with a pip
    /// beside it for each placed charge, and flashing on a press and on the detonation.
    pub overlay: bool,
}

impl DetonatorTuning {
    const fn new() -> Self {
        Self {
            offset_m: Vec3::new(0.03, 0.0, 0.08),
            normal: Vec3::X,
            radius_m: 0.02,
            touch_radius_m: 0.03,
            release_radius_m: 0.05,
            approach_radius_m: 0.1,
            max_behind_m: 0.015,
            min_entry_speed_mps: 0.25,
            dwell_s: 0.15,
            fingertip_pointing_m: Vec3::new(0.0, -0.02, -0.04),
            fingertip_resting_m: Vec3::new(0.0, -0.04, 0.02),
            overlay: true,
        }
    }
}

/// Pressing a held charge onto a surface sticks it there.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StickTuning {
    /// Stick a held charge to the surface it is pressed onto.
    pub enabled: bool,
    /// How far from the held charge's centre a surface counts as touching it, metres: about half
    /// the charge's thickness.
    pub contact_m: f32,
    /// How far off the surface the charge is placed before it is pushed onto it, metres.
    pub standoff_m: f32,
    /// How fast the charge is pushed onto the surface, metres per second, so that the charge's own
    /// collision sticks it there.
    pub push_mps: f32,
    /// How long after a charge comes out of the pouch before it sticks, seconds, so that the body
    /// the pouch is on is not mistaken for a surface as the hand leaves it.
    pub after_prime_s: f32,
    /// How far from the held charge a surface is previewed, where the charge would stick if pressed
    /// on, metres.
    pub preview_m: f32,
}

impl StickTuning {
    const fn new() -> Self {
        Self {
            enabled: true,
            contact_m: 0.04,
            standoff_m: 0.03,
            push_mps: 1.5,
            after_prime_s: 0.3,
            preview_m: 0.3,
        }
    }
}
