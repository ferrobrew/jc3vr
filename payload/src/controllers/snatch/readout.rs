//! The snatch's readout, for the debug UI and the Status page.

use glam::Vec3;

use crate::controllers::snatch::pull::Swinging;

/// The snatch's readout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SnatchStatus {
    /// Whether the input tick ran it recently.
    pub live: bool,
    /// What the snatch is doing.
    pub phase: &'static str,
    /// The target offered to the grip on the last tick.
    pub target: Option<TargetReadout>,
    /// Why nothing was offered on the last tick, when the hand or the hook could not snatch.
    pub unoffered: Option<&'static str>,
    /// Why the last snatch failed.
    pub last_refusal: Option<String>,
    /// Snatches fired, brought to the hand (or doors opened), and failed, since the payload loaded.
    pub fired: u64,
    pub taken: u64,
    pub failed: u64,
    /// Weapons a tap of the grapple tossed to the player.
    pub tosses: u64,
    /// The mass of the loose object under the grapple's aim on the last offering tick, whatever
    /// the snatch would do with it, kilograms: what sets an object above the swing's ceiling apart.
    pub aimed_mass_kg: Option<f32>,
    /// The held line while it swings its target.
    pub swing: Option<SwingReadout>,
}

/// The target offered, for the readout.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TargetReadout {
    pub kind: &'static str,
    pub distance_m: f32,
    /// A loose object's mass, kilograms.
    pub mass_kg: Option<f32>,
}

/// The held line, for the readout and the overlay.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct SwingReadout {
    /// The line's held length, metres.
    pub length_m: f32,
    /// The line's tension, and the most it holds on this target with this brace, newtons.
    pub tension_n: f32,
    pub max_tension_n: f32,
    pub mass_kg: f32,
    /// Whether the other hand braces the grapple hand.
    pub braced: bool,
    /// Whether the target is too heavy to come to the hand.
    pub tethered: bool,
    /// Where the line is anchored, and the target's centre, world space.
    pub anchor: Vec3,
    pub centre: Vec3,
}

impl SwingReadout {
    pub(super) fn new(swinging: Swinging, tethered: bool) -> Self {
        Self {
            length_m: swinging.length_m,
            tension_n: swinging.tension_n,
            max_tension_n: swinging.max_tension_n,
            mass_kg: swinging.mass_kg,
            braced: swinging.braced,
            tethered,
            anchor: swinging.anchor,
            centre: swinging.centre,
        }
    }
}
