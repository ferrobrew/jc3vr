//! The C4 throw's and the press's fixed tuning.

use glam::Vec3;
use jc3vr_body::controllers::hands::held::HoldPlacement;

/// The C4 tuning.
pub const C4: C4Tuning = C4Tuning::new();

/// The throw of a charge from the C4 pouch (see [`crate::controllers::weapons::c4`]); the pouch
/// itself is placed with the others
/// ([`POUCHES`](crate::controllers::weapons::volumes::POUCHES)).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct C4Tuning {
    /// Where the held charge's in-hand model sits in the hand ([`HoldPlacement`]): its top face flat
    /// on the palm, its sticking face out.
    pub hold: HoldPlacement,
    /// The middle of the charge in its model's frame, metres: where the surface probes start.
    pub centre_m: Vec3,
    /// Pressing a held charge onto a surface sticks it there.
    pub stick: StickTuning,
}

impl C4Tuning {
    const fn new() -> Self {
        Self {
            hold: HOLD,
            centre_m: Vec3::new(0.0, CHARGE_TOP_M / 2.0, 0.0),
            stick: StickTuning::new(),
        }
    }
}

/// The charge's hold, derived from the C4 weapon's in-hand model: a flat square brick lying in the
/// model's XZ plane, centred on its origin there, whose origin is at the sticking (bottom) face and
/// whose top face is [`CHARGE_TOP_M`] up the model's +Y. The top face lies on the palm, so the
/// model's +Y points into the palm (the right grip's +X) and the sticking face looks out of the hand,
/// at a surface the charge is pressed onto. The brick's sides run up the finger tube (the model's
/// +Z along the grip's -Z) and along the hand; it is centred on the middle of the palm.
pub const HOLD: HoldPlacement = HoldPlacement {
    offset_m: Vec3::new(-(CHARGE_TOP_M + 0.003), 0.01, 0.0),
    axes: [Vec3::Y, Vec3::X, Vec3::NEG_Z],
};

/// How far the in-hand model's top face is from its origin along its +Y, metres.
pub const CHARGE_TOP_M: f32 = 0.029;

/// Pressing a held charge onto a surface sticks it there.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct StickTuning {
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
            contact_m: 0.04,
            standoff_m: 0.03,
            push_mps: 1.5,
            after_prime_s: 0.3,
            preview_m: 0.3,
        }
    }
}
