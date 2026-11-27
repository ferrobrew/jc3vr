//! The context the weapon handling's two tick halves share: one settings snapshot and one mode
//! table for the tick, taken as the begin half starts, and what the begin half read of the guns in
//! the hands for the finish half to act on.

use glam::Vec3;
use jc3vr_body::controllers::mode::Mode;

use crate::controllers::weapons::{
    capabilities::Capabilities,
    config::WeaponSettings,
    engine::HeldWeapon,
    foregrip::GripGeometry,
    regions::GunRegion,
    state::{OffTouch, STATE},
};

/// One input tick's settings and mode table, and the begin half's reading of the guns.
#[derive(Clone)]
pub(super) struct TickContext {
    /// The settings, read once for the tick.
    pub(super) settings: WeaponSettings,
    /// What the weapon handling does in the tick's mode.
    pub(super) modes: Capabilities,
    /// The tick's wielded weapon and its two-handed geometry.
    pub(super) weapon: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// The tick's gun held in one hand while the other is free, wielded or armed, and its
    /// two-handed geometry when it is a long gun ([`crate::controllers::weapons::engine::lone_gun`]).
    pub(super) lone: Option<(HeldWeapon, Option<GripGeometry>)>,
    /// Where the free hand touches the lone long gun on this tick.
    pub(super) touch: Option<OffTouch>,
    /// Where the bracing hand has slid to along the gun on this tick, in the holding wrist's frame.
    pub(super) slide: Option<(Vec3, GunRegion)>,
}

impl TickContext {
    /// The context for a tick in `mode` before anything in it is read.
    pub(super) fn read(mode: Mode) -> Self {
        Self {
            settings: WeaponSettings::read(),
            modes: Capabilities::of(mode),
            weapon: None,
            lone: None,
            touch: None,
            slide: None,
        }
    }

    /// The context the begin half handed over, or, when it stood down before it read the hands,
    /// one read now for the current mode with nothing in the hands.
    pub(super) fn handed_over() -> Self {
        STATE
            .lock()
            .tick
            .clone()
            .unwrap_or_else(|| Self::read(jc3vr_body::controllers::mode::current().mode))
    }
}
