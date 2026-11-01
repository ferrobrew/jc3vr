//! Pure acquisition of a door panel reported by a ray on the vehicle's body.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::physical_hands::{
    doors::{
        engine::DoorRead,
        geometry::{DoorReach, door_handle},
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
    HandleRequired,
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
    let handle = door_handle(
        &door_box,
        kind_of(door.slot),
        door.part_hash.and_then(table::lookup),
    );
    let body = match door.body {
        None if door.latched => {
            let at_handle = handle.map_or_else(
                || in_box(radii.box_m) && reach.handle >= handle_share.clamp(0.0, 1.0),
                |handle| palm.distance(handle.point) <= radii.handle_m.max(0.0),
            );
            if !at_handle {
                return Err(PanelRefusal::HandleRequired);
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
        && let Some(handle) = handle
    {
        reach.point = handle.point;
        reach.distance = palm.distance(handle.point);
        reach.handle = 1.0;
    }
    Ok(PanelAcquisition {
        reach,
        outward: handle.map_or_else(|| door_box.outward(vehicle_origin), |h| h.normal),
        body,
    })
}

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
    use jc3gi::vehicle::vehicle::DoorSlot;

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
    fn a_far_panel_corner_is_not_a_closed_handle_candidate() {
        let closed = door(true);
        assert_eq!(
            panel(
                &closed,
                Vec3::new(0.1, 1.0, 1.0),
                PanelReach::uniform(0.1),
                0.7,
                Vec3::ZERO
            ),
            Err(PanelRefusal::HandleRequired)
        );
        let open = door(false);
        assert!(
            panel(
                &open,
                Vec3::new(0.1, 1.0, 1.0),
                PanelReach::uniform(0.1),
                0.7,
                Vec3::ZERO
            )
            .is_ok()
        );
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
            Vec3::ZERO,
            PanelReach::uniform(0.1),
            0.7,
            Vec3::ZERO,
            entry,
            &mut scan,
        );
        assert_eq!(refused, Err(PanelRefusal::HandleRequired));
        assert_eq!(
            scan.entries[0].unwrap().result,
            ScanResult::PanelRefused(PanelRefusal::HandleRequired)
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
    fn a_closed_panel_requires_the_handle_area() {
        let door = door(true);
        assert_eq!(
            panel(&door, Vec3::ZERO, PanelReach::uniform(0.1), 0.7, Vec3::ZERO),
            Err(PanelRefusal::HandleRequired)
        );
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
    fn a_closed_door_is_taken_within_the_handle_reach_not_the_box_reach() {
        let closed = door(true);
        let near_handle = handle_point(&closed) + Vec3::Y * 0.1;
        assert_eq!(
            panel(
                &closed,
                near_handle,
                PanelReach::uniform(0.06),
                0.4,
                Vec3::ZERO
            ),
            Err(PanelRefusal::HandleRequired)
        );
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
