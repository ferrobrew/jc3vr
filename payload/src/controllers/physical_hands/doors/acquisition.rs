//! Pure acquisition of a door panel reported by a ray on the vehicle's body.

use std::num::NonZeroUsize;

use glam::Vec3;
use jc3gi::vehicle::vehicle::DoorSlot;

use crate::controllers::physical_hands::{
    doors::{
        engine::DoorRead,
        geometry::{DoorKind, DoorReach, door_handle},
        scan::kind_of,
        table,
    },
    pickup::diagnostics::scan::{Scan, ScanEntry, ScanResult},
};

/// The body offered by a reached panel.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PanelBody {
    LatchedHandle,
    OpenPanel { object: NonZeroUsize, body: u32 },
}

/// A panel's geometry, outward direction, and body a hand can take there.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PanelAcquisition {
    pub reach: DoorReach,
    pub outward: Vec3,
    pub body: PanelBody,
}

/// How near a palm must be to a door to reach it.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct PanelReach {
    /// How far outside the door's box the palm still reaches the door, metres.
    pub box_m: f32,
    /// How far from a closed door's handle the palm takes the door, metres. The door scan offers a
    /// handle at the same distance, so a closed door is taken alike whichever finds it.
    pub handle_m: f32,
}

impl PanelReach {
    /// One reach for both the box and the handle.
    pub(crate) const fn uniform(reach_m: f32) -> Self {
        Self {
            box_m: reach_m,
            handle_m: reach_m,
        }
    }
}

/// Why a vehicle-reported panel offers no door grip.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum PanelRefusal {
    NoBox,
    OutOfReach,
    /// A closed door reached away from its handle where a hand more likely holds on to the
    /// vehicle: a hood or a trunk, the door's inner side, or its half by the hinge.
    AwayFromHandle,
    NoOpenBody,
    LatchedWithOwnBody,
}

/// Apply the closed-handle and open-panel rules without reading the engine.
pub(crate) fn panel(
    door: &DoorRead,
    palm: Vec3,
    radii: PanelReach,
    handle_share: f32,
    vehicle_origin: Vec3,
) -> Result<PanelAcquisition, PanelRefusal> {
    let door_box = door.door_box.ok_or(PanelRefusal::NoBox)?;
    let in_box = |m: f32| door_box.reach(palm, m).is_some();
    let mut reach = door_box
        .reach(palm, radii.box_m.max(radii.handle_m))
        .ok_or(PanelRefusal::OutOfReach)?;
    let kind = kind_of(door.slot);
    let handle = door_handle(&door_box, kind, door.part_hash.and_then(table::lookup));
    let outward = handle.map_or_else(|| door_box.outward(vehicle_origin), |h| h.normal);
    // A closed door is taken away from its handle as an open one is, where the hand puts its
    // force, but only where a hand reaching for it more likely means to open it than to hold on
    // to the vehicle: on the outer face of a door other than a hood or a trunk, in the half
    // away from the hinge. A hand braced on a hood, on the door's inner side, or by its hinge
    // holds the vehicle instead. Within the handle's reach the hand closes on the handle itself.
    let at_handle = handle.map_or_else(
        || in_box(radii.box_m) && reach.handle >= handle_share.clamp(0.0, 1.0),
        |handle| palm.distance(handle.point) <= radii.handle_m.max(0.0),
    );
    let body = match door.body {
        None if door.latched => {
            if !at_handle {
                if !in_box(radii.box_m) {
                    return Err(PanelRefusal::OutOfReach);
                }
                let outer_face = (palm - reach.point).dot(outward) >= -OUTER_FACE_SLACK_M;
                let lid = kind == DoorKind::Front || door.slot == DoorSlot::TRUNK;
                if lid || !outer_face || reach.handle < AWAY_FROM_HINGE {
                    return Err(PanelRefusal::AwayFromHandle);
                }
            }
            PanelBody::LatchedHandle
        }
        Some((object, body)) if !door.latched => {
            if !in_box(radii.box_m) {
                return Err(PanelRefusal::OutOfReach);
            }
            PanelBody::OpenPanel { object, body }
        }
        None => return Err(PanelRefusal::NoOpenBody),
        Some(_) => return Err(PanelRefusal::LatchedWithOwnBody),
    };
    if door.latched
        && at_handle
        && let Some(handle) = handle
    {
        reach.point = handle.point;
        reach.distance = palm.distance(handle.point);
        reach.handle = 1.0;
    }
    Ok(PanelAcquisition {
        reach,
        outward,
        body,
    })
}

/// How far inside a closed door's outer face the palm may sit and still be on it, metres: the
/// door's box runs through the panel's thickness.
const OUTER_FACE_SLACK_M: f32 = 0.02;

/// The least share of the run from the hinge to the far edge at which a closed door is taken away
/// from its handle: nearer the hinge, a pull barely swings it.
const AWAY_FROM_HINGE: f32 = 0.5;

/// Apply the production panel rules and retain their exact result.
pub(crate) fn observed_panel(
    door: &DoorRead,
    palm: Vec3,
    reach: PanelReach,
    handle_share: f32,
    vehicle_origin: Vec3,
    mut entry: ScanEntry,
    scan: &mut Scan,
) -> Result<PanelAcquisition, PanelRefusal> {
    let result = panel(door, palm, reach, handle_share, vehicle_origin);
    match result {
        Ok(acquired) => {
            entry.point = Some(acquired.reach.point);
            entry.distance = Some(acquired.reach.distance);
            entry.result = ScanResult::Offered;
        }
        Err(refusal) => entry.result = ScanResult::PanelRefused(refusal),
    }
    scan.push(entry);
    result
}

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::controllers::physical_hands::doors::geometry::DoorBox;

    fn door(latched: bool) -> DoorRead {
        DoorRead {
            slot: DoorSlot::FRONT_LEFT,
            part_index: 1,
            latched,
            door_box: DoorBox::new(
                Mat4::IDENTITY,
                Mat4::IDENTITY,
                Vec3::ZERO,
                Vec3::new(0.1, 1.0, 1.0),
            ),
            body: (!latched).then_some((NonZeroUsize::new(8).unwrap(), 3)),
            part_hash: None,
        }
    }

    fn handle_point(door: &DoorRead) -> Vec3 {
        door_handle(&door.door_box.unwrap(), kind_of(door.slot), None)
            .unwrap()
            .point
    }

    #[test]
    fn a_closed_door_is_taken_on_its_outer_face_away_from_the_hinge() {
        let closed = door(true);
        let corner = panel(
            &closed,
            Vec3::new(0.1, 1.0, 1.0),
            PanelReach::uniform(0.1),
            0.7,
            Vec3::ZERO,
        )
        .unwrap();
        assert_eq!(corner.body, PanelBody::LatchedHandle);
        assert!(
            corner.reach.point.distance(Vec3::new(0.1, 1.0, 1.0)) < 1e-4,
            "the hand takes the door where it reached it: {}",
            corner.reach.point
        );
        assert_eq!(
            panel(
                &closed,
                Vec3::new(1.0, 3.0, 3.0),
                PanelReach::uniform(0.1),
                0.7,
                Vec3::ZERO
            ),
            Err(PanelRefusal::OutOfReach)
        );
    }

    /// Points across a door's outer face, its box running through `0.1` along x.
    fn outer_face() -> impl Iterator<Item = Vec3> {
        (0..8)
            .flat_map(|i| (0..8).map(move |j| (i, j)))
            .map(|(i, j)| Vec3::new(0.1, i as f32 / 7.0, j as f32 / 7.0))
    }

    #[test]
    fn a_closed_door_away_from_its_handle_is_left_to_the_vehicle_hold_where_a_hand_holds_on() {
        let closed = door(true);
        let door_box = closed.door_box.unwrap();
        let reach = PanelReach::uniform(0.1);
        let handle = handle_point(&closed);
        let away = |p: &Vec3| p.distance(handle) > 0.3;
        let hinge = outer_face()
            .filter(away)
            .find(|&p| door_box.reach(p, 0.1).unwrap().handle < 0.2)
            .unwrap();
        assert_eq!(
            panel(&closed, hinge, reach, 0.7, Vec3::ZERO),
            Err(PanelRefusal::AwayFromHandle),
            "by the hinge, at {hinge}"
        );
        let far = outer_face()
            .filter(away)
            .find(|&p| door_box.reach(p, 0.1).unwrap().handle > 0.8)
            .unwrap();
        assert_eq!(
            panel(&closed, far, reach, 0.7, Vec3::ZERO).map(|a| a.body),
            Ok(PanelBody::LatchedHandle),
            "on the outer face away from the hinge, at {far}"
        );
        // The same point reached from the door's inner side, the vehicle's.
        let inner = far - Vec3::X * 0.18;
        assert_eq!(
            panel(&closed, inner, reach, 0.7, Vec3::ZERO),
            Err(PanelRefusal::AwayFromHandle),
            "on the inner side, at {inner}"
        );
        let mut hood = door(true);
        hood.slot = DoorSlot::HOOD;
        let mut trunk = door(true);
        trunk.slot = DoorSlot::TRUNK;
        for lid in [hood, trunk] {
            let handle = handle_point(&lid);
            let away = outer_face().find(|p| p.distance(handle) > 0.3).unwrap();
            assert_eq!(
                panel(&lid, away, reach, 0.7, Vec3::ZERO),
                Err(PanelRefusal::AwayFromHandle),
                "{:?} at {away}",
                lid.slot
            );
            assert!(panel(&lid, handle, reach, 0.7, Vec3::ZERO).is_ok());
        }
    }

    #[test]
    fn observed_panel_retains_the_exact_production_acquisition_result() {
        let door = door(true);
        let mut scan = Scan::new();
        let entry = ScanEntry {
            branch:
                crate::controllers::physical_hands::pickup::diagnostics::scan::Branch::DoorPanel,
            object: NonZeroUsize::new(8),
            body: Some(3),
            door: Some((NonZeroUsize::new(8).unwrap(), door.slot, door.part_index)),
            point: None,
            distance: None,
            mass_kg: None,
            result: ScanResult::Offered,
        };
        let refused = observed_panel(
            &door,
            Vec3::new(1.0, 3.0, 3.0),
            PanelReach::uniform(0.1),
            0.7,
            Vec3::ZERO,
            entry,
            &mut scan,
        );
        assert_eq!(refused, Err(PanelRefusal::OutOfReach));
        assert_eq!(
            scan.entries[0].unwrap().result,
            ScanResult::PanelRefused(PanelRefusal::OutOfReach)
        );
        let accepted = observed_panel(
            &door,
            handle_point(&door),
            PanelReach::uniform(0.1),
            0.7,
            Vec3::ZERO,
            entry,
            &mut scan,
        )
        .unwrap();
        assert_eq!(scan.entries[1].unwrap().result, ScanResult::Offered);
        assert_eq!(scan.entries[1].unwrap().point, Some(accepted.reach.point));
        assert_eq!(scan.entries[1].unwrap().door, entry.door);
    }

    #[test]
    fn a_closed_panel_at_its_handle_is_taken_by_the_handle() {
        let door = door(true);
        let acquisition = panel(
            &door,
            handle_point(&door),
            PanelReach::uniform(0.1),
            0.7,
            Vec3::ZERO,
        )
        .unwrap();
        assert_eq!(acquisition.reach.handle, 1.0);
        assert_eq!(acquisition.body, PanelBody::LatchedHandle);
    }

    #[test]
    fn a_closed_door_near_its_handle_closes_on_the_handle() {
        let closed = door(true);
        let near_handle = handle_point(&closed) + Vec3::Y * 0.1;
        let reach = PanelReach {
            box_m: 0.06,
            handle_m: 0.15,
        };
        let acquisition = panel(&closed, near_handle, reach, 0.4, Vec3::ZERO).unwrap();
        assert_eq!(acquisition.body, PanelBody::LatchedHandle);
        assert_eq!(acquisition.reach.point, handle_point(&closed));
        // An open door is still reached only within the box reach.
        let open = door(false);
        let outside = Vec3::new(-0.1, 0.5, 0.5);
        assert_eq!(
            panel(&open, outside, reach, 0.4, Vec3::ZERO),
            Err(PanelRefusal::OutOfReach)
        );
    }

    #[test]
    fn an_open_panel_is_available_at_the_hinge_without_a_handle() {
        let acquisition = panel(
            &door(false),
            Vec3::ZERO,
            PanelReach::uniform(0.1),
            1.0,
            Vec3::ZERO,
        )
        .unwrap();
        assert_eq!(acquisition.reach.handle, 0.0);
        assert_eq!(
            acquisition.body,
            PanelBody::OpenPanel {
                object: NonZeroUsize::new(8).unwrap(),
                body: 3,
            }
        );
        assert!(acquisition.outward.is_finite());
        assert_ne!(acquisition.outward, Vec3::ZERO);
    }

    #[test]
    fn an_open_panel_still_requires_reach_and_its_own_body() {
        let mut door = door(false);
        assert!(
            panel(
                &door,
                Vec3::new(-0.125, 0.0, 0.0),
                PanelReach::uniform(0.125),
                1.0,
                Vec3::ZERO
            )
            .is_ok()
        );
        assert_eq!(
            panel(
                &door,
                Vec3::new(-0.25, 0.0, 0.0),
                PanelReach::uniform(0.125),
                1.0,
                Vec3::ZERO
            ),
            Err(PanelRefusal::OutOfReach)
        );
        door.body = None;
        assert_eq!(
            panel(&door, Vec3::ZERO, PanelReach::uniform(0.1), 1.0, Vec3::ZERO),
            Err(PanelRefusal::NoOpenBody)
        );
        door.door_box = None;
        assert_eq!(
            panel(&door, Vec3::ZERO, PanelReach::uniform(0.1), 1.0, Vec3::ZERO),
            Err(PanelRefusal::NoBox)
        );
    }

    #[test]
    fn a_latched_panel_with_a_separate_body_is_not_reclassified() {
        let mut door = door(true);
        door.body = Some((NonZeroUsize::new(8).unwrap(), 3));
        assert_eq!(
            panel(
                &door,
                Vec3::new(0.1, 1.0, 1.0),
                PanelReach::uniform(0.1),
                0.7,
                Vec3::ZERO,
            ),
            Err(PanelRefusal::LatchedWithOwnBody)
        );
    }
}
