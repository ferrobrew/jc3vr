//! The grab zones of a held gun for the free hand: which part of the gun the hand is at decides
//! what its grip takes.
//!
//! The handover takes the gun by its pistol grip within a generous radius, wide enough to reach the
//! bottom of a pistol's grip, where its magazine is. A hand going for the magazine there would take
//! the gun instead. So the space under the magazine well is the magazine's zone: a free hand in it
//! takes the magazine (the pull, `extraction`), whatever the handover's radius, and never the gun.
//! Only a grab at the pistol grip above the well, or elsewhere in the handover's reach, hands the
//! gun over. Targeting is otherwise the nearest target to the palm, as everywhere else.
//!
//! The zone is a cylinder down the well's axis from just inside the mouth, in the cockpit frame,
//! at the player's scale. It exists while the gun has a magazine in its own well (a wells-table
//! well, not the wrist fallback); with the magazine out there is nothing under the well to take.
//!
//! The zones of the last input tick are published for the grab view's in-world drawing
//! ([`views`]).

use std::time::{Duration, Instant};

use glam::Vec3;
use parking_lot::Mutex;

use crate::controllers::{
    frame::Hand,
    weapons::reload::plan::{LoadKind, WellFrame},
};

/// The space under a gun's magazine well, in the cockpit frame.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct MagazineZone {
    /// The well's mouth.
    pub mouth: Vec3,
    /// Out of the well, the way a magazine leaves it: a unit vector.
    pub outward: Vec3,
    /// How far above the mouth the zone starts, into the well.
    pub lip: f32,
    /// How far below the mouth it ends.
    pub length: f32,
    /// How far from the well's axis it reaches.
    pub radius: f32,
}

impl MagazineZone {
    /// The zone under a well with its mouth at `mouth` and its insertion axis `axis` (into the gun),
    /// at `world_scale`. `None` for a degenerate axis or a non-finite point.
    pub(crate) fn under(mouth: Vec3, axis: Vec3, world_scale: f32) -> Option<Self> {
        let outward = (-axis).try_normalize()?;
        let scale = world_scale
            .is_finite()
            .then_some(world_scale)
            .filter(|s| *s > 0.0)?;
        mouth.is_finite().then_some(Self {
            mouth,
            outward,
            lip: ZONE.lip_m / scale,
            length: ZONE.length_m / scale,
            radius: ZONE.radius_m / scale,
        })
    }

    /// Whether `point` is in the zone.
    pub(crate) fn contains(&self, point: Vec3) -> bool {
        self.distance(point).is_some()
    }

    /// How far `point` is from the zone's axis, when it is in the zone.
    pub(crate) fn distance(&self, point: Vec3) -> Option<f32> {
        if !point.is_finite() {
            return None;
        }
        let offset = point - self.mouth;
        let along = offset.dot(self.outward);
        let across = (offset - self.outward * along).length();
        (along >= -self.lip && along <= self.length && across <= self.radius).then_some(across)
    }

    /// The zone's ends on its axis: the top (inside the mouth) and the bottom.
    pub(crate) fn ends(&self) -> (Vec3, Vec3) {
        (
            self.mouth - self.outward * self.lip,
            self.mouth + self.outward * self.length,
        )
    }
}

/// The zone of a gun whose plan loads `load` through `well`, with its magazine `out` or not, the
/// well's mouth and axis as the tick placed them.
pub(super) fn zone_for(
    load: LoadKind,
    well: WellFrame,
    out: bool,
    placed: Option<(Vec3, Vec3)>,
    world_scale: f32,
) -> Option<MagazineZone> {
    if load != LoadKind::Magazine || out || !matches!(well, WellFrame::Gun { .. }) {
        return None;
    }
    let (mouth, axis) = placed?;
    MagazineZone::under(mouth, axis, world_scale)
}

/// One held gun's zones for the free hand, as the last input tick resolved them.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct ZoneView {
    /// The hand holding the gun; the zones are for the other hand.
    pub holder: Hand,
    pub magazine: Option<MagazineZone>,
    /// The handover's reach: its centre on the pistol grip and its radius, cockpit frame.
    pub handover: Option<(Vec3, f32)>,
    /// The free hand is in the magazine's zone, so the handover is not offered to it.
    pub at_magazine: bool,
}

/// The last input tick's zones, by the holding hand's [`Hand::index`]; none once the weapon
/// handling has stopped resolving them.
pub(crate) fn views() -> [Option<ZoneView>; 2] {
    match *VIEWS.lock() {
        Some((at, views)) if at.elapsed() <= VIEW_MAX_AGE => views,
        _ => [None; 2],
    }
}

pub(super) fn publish(views: [Option<ZoneView>; 2]) {
    *VIEWS.lock() = Some((Instant::now(), views));
}

/// The zone's size, in the gun model's metres.
struct ZoneTuning {
    /// How far into the well above its mouth: the base plate of a pistol's magazine sits at the
    /// bottom of the grip, and a hand pinching it is level with the mouth.
    lip_m: f32,
    /// How far below the mouth: past the longest magazine, plus a hand's grip on its end.
    length_m: f32,
    /// How far from the axis: a magazine's half-width plus the hand's tracking and placement
    /// error, short of a long gun's pistol grip behind the well.
    radius_m: f32,
}

const ZONE: ZoneTuning = ZoneTuning {
    lip_m: 0.015,
    length_m: 0.24,
    radius_m: 0.06,
};

/// How long a published view is drawn without a newer one.
const VIEW_MAX_AGE: Duration = Duration::from_millis(250);

static VIEWS: Mutex<Option<(Instant, [Option<ZoneView>; 2])>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;

    fn pistol() -> MagazineZone {
        // A pistol's well at the bottom of its grip, the magazine pushed up (+Y) into it.
        MagazineZone::under(Vec3::new(0.0, 1.0, 0.0), Vec3::Y, 1.0).unwrap()
    }

    #[test]
    fn a_hand_under_the_well_is_at_the_magazine() {
        let zone = pistol();
        assert!(zone.contains(Vec3::new(0.0, 0.95, 0.0)));
        assert!(zone.contains(Vec3::new(0.04, 0.9, 0.02)));
        // Pinching the base plate, level with the mouth.
        assert!(zone.contains(Vec3::new(0.0, 1.01, 0.0)));
    }

    #[test]
    fn a_hand_on_the_grip_above_the_well_is_not() {
        let zone = pistol();
        assert!(!zone.contains(Vec3::new(0.0, 1.05, 0.0)));
        assert!(!zone.contains(Vec3::new(0.0, 1.1, 0.02)));
    }

    #[test]
    fn a_hand_beside_or_far_below_the_well_is_not() {
        let zone = pistol();
        assert!(!zone.contains(Vec3::new(0.08, 0.95, 0.0)));
        assert!(!zone.contains(Vec3::new(0.0, 0.7, 0.0)));
        assert!(!zone.contains(Vec3::NAN));
    }

    #[test]
    fn the_distance_is_from_the_axis() {
        let zone = pistol();
        let d = zone.distance(Vec3::new(0.03, 0.9, 0.04)).unwrap();
        assert!((d - 0.05).abs() < 1e-5, "{d}");
    }

    #[test]
    fn the_zone_follows_the_player_scale() {
        // At twice the world scale the player is half as large against the gun.
        let zone = MagazineZone::under(Vec3::ZERO, Vec3::Y, 2.0).unwrap();
        assert!((zone.radius - ZONE.radius_m / 2.0).abs() < 1e-6);
        assert!(!zone.contains(Vec3::new(0.0, -0.2, 0.0)));
        assert!(zone.contains(Vec3::new(0.0, -0.1, 0.0)));
    }

    #[test]
    fn a_degenerate_well_has_no_zone() {
        assert!(MagazineZone::under(Vec3::ZERO, Vec3::ZERO, 1.0).is_none());
        assert!(MagazineZone::under(Vec3::ZERO, Vec3::Y, 0.0).is_none());
        assert!(MagazineZone::under(Vec3::NAN, Vec3::Y, 1.0).is_none());
    }

    #[test]
    fn only_a_seated_magazine_in_the_gun_s_own_well_has_a_zone() {
        let gun = WellFrame::Gun {
            mouth: Vec3::ZERO,
            axis: Vec3::Y,
        };
        let placed = Some((Vec3::ZERO, Vec3::Y));
        assert!(zone_for(LoadKind::Magazine, gun, false, placed, 1.0).is_some());
        assert!(zone_for(LoadKind::Magazine, gun, true, placed, 1.0).is_none());
        assert!(zone_for(LoadKind::Shell, gun, false, placed, 1.0).is_none());
        assert!(zone_for(LoadKind::Magazine, gun, false, None, 1.0).is_none());
        let wrist = WellFrame::Wrist { offset: Vec3::ZERO };
        assert!(zone_for(LoadKind::Magazine, wrist, false, placed, 1.0).is_none());
    }
}
