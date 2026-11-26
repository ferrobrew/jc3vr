//! The reload's zones on a held gun, as the weapon handling resolved them on its last input tick:
//! where the free hand takes the magazine out or the gun over, and where a held magazine knocks
//! the inserted one out. The grab view draws them; the weapon handling owns them and hands them
//! over through [`HeldSource::reload_views`](crate::controllers::held::HeldSource::reload_views).

use glam::Vec3;
use jc3vr_input::frame::Hand;

/// The reload's zones on the held guns, for the grab view.
#[derive(Debug, Copy, Clone, PartialEq, Default)]
pub struct ReloadViews {
    /// The grab zones of each gun held in one hand for the other, by the holding hand's
    /// [`Hand::index`].
    pub zones: [Option<GunZones>; 2],
    /// The knock zone of the gun whose magazine a held pouch magazine can knock out.
    pub knock: Option<KnockZone>,
}

/// A zone along an axis, cockpit frame: a capsule from `top` to `bottom` of `radius`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ZoneAxis {
    /// The end inside the magazine well's mouth.
    pub top: Vec3,
    /// The end below the mouth: the zone's bottom, or the inserted magazine's base.
    pub bottom: Vec3,
    pub radius: f32,
}

/// One held gun's grab zones for the free hand.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct GunZones {
    /// The hand holding the gun; the zones are for the other hand.
    pub holder: Hand,
    /// The space under the gun's magazine well, where the free hand takes the magazine out.
    pub magazine: Option<ZoneAxis>,
    /// The handover's reach: its centre on the pistol grip and its radius, cockpit frame.
    pub handover: Option<(Vec3, f32)>,
    /// The free hand is in the magazine's zone, so the handover is not offered to it.
    pub at_magazine: bool,
}

/// One held gun's knock zone: the inserted magazine a held one can knock out.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct KnockZone {
    /// The hand holding the gun; the held magazine is in the other.
    pub holder: Hand,
    /// The inserted magazine, from inside the well's mouth to its base.
    pub zone: ZoneAxis,
    /// The held magazine's tip, cockpit frame.
    pub tip: Option<Vec3>,
    /// Whether the tip's last move would have knocked the magazine out had it met it.
    pub fast: bool,
    /// Whether the magazine was knocked out recently enough for the zone to flash.
    pub flashing: bool,
}
