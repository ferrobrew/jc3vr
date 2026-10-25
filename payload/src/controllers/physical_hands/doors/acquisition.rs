//! Pure acquisition of a door panel reported by a ray on the vehicle's body.

use std::num::NonZeroUsize;

use glam::Vec3;

use crate::controllers::physical_hands::{
    doors::{engine::DoorRead, geometry::DoorReach},
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
    reach_m: f32,
    handle_share: f32,
    vehicle_origin: Vec3,
) -> Result<PanelAcquisition, PanelRefusal> {
    let door_box = door.door_box.ok_or(PanelRefusal::NoBox)?;
    let reach = door_box
        .reach(palm, reach_m)
        .ok_or(PanelRefusal::OutOfReach)?;
    let body = match door.body {
        None if door.latched => {
            if reach.handle >= handle_share.clamp(0.0, 1.0) {
                PanelBody::LatchedHandle
            } else {
                return Err(PanelRefusal::HandleRequired);
            }
        }
        Some((object, body)) if !door.latched => PanelBody::OpenPanel { object, body },
        None => return Err(PanelRefusal::NoOpenBody),
        Some(_) => return Err(PanelRefusal::LatchedWithOwnBody),
    };
    Ok(PanelAcquisition {
        reach,
        outward: door_box.outward(vehicle_origin),
        body,
    })
}

/// Apply the production panel rules and retain their exact result.
pub(crate) fn observed_panel(
    door: &DoorRead,
    palm: Vec3,
    reach_m: f32,
    handle_share: f32,
    vehicle_origin: Vec3,
    mut entry: ScanEntry,
    scan: &mut Scan,
) -> Result<PanelAcquisition, PanelRefusal> {
    let result = panel(door, palm, reach_m, handle_share, vehicle_origin);
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
        let refused = observed_panel(&door, Vec3::ZERO, 0.1, 0.7, Vec3::ZERO, entry, &mut scan);
        assert_eq!(refused, Err(PanelRefusal::HandleRequired));
        assert_eq!(
            scan.entries[0].unwrap().result,
            ScanResult::PanelRefused(PanelRefusal::HandleRequired)
        );
        let accepted = observed_panel(
            &door,
            Vec3::new(0.1, 1.0, 1.0),
            0.1,
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
            panel(&door, Vec3::ZERO, 0.1, 0.7, Vec3::ZERO),
            Err(PanelRefusal::HandleRequired)
        );
        let acquisition = panel(&door, Vec3::new(0.1, 1.0, 1.0), 0.1, 0.7, Vec3::ZERO).unwrap();
        assert_eq!(acquisition.reach.handle, 1.0);
        assert_eq!(acquisition.body, PanelBody::LatchedHandle);
    }

    #[test]
    fn an_open_panel_is_available_at_the_hinge_without_a_handle() {
        let acquisition = panel(&door(false), Vec3::ZERO, 0.1, 1.0, Vec3::ZERO).unwrap();
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
        assert!(panel(&door, Vec3::new(-0.125, 0.0, 0.0), 0.125, 1.0, Vec3::ZERO).is_ok());
        assert_eq!(
            panel(&door, Vec3::new(-0.25, 0.0, 0.0), 0.125, 1.0, Vec3::ZERO),
            Err(PanelRefusal::OutOfReach)
        );
        door.body = None;
        assert_eq!(
            panel(&door, Vec3::ZERO, 0.1, 1.0, Vec3::ZERO),
            Err(PanelRefusal::NoOpenBody)
        );
        door.door_box = None;
        assert_eq!(
            panel(&door, Vec3::ZERO, 0.1, 1.0, Vec3::ZERO),
            Err(PanelRefusal::NoBox)
        );
    }

    #[test]
    fn a_latched_panel_with_a_separate_body_is_not_reclassified() {
        let mut door = door(true);
        door.body = Some((NonZeroUsize::new(8).unwrap(), 3));
        assert_eq!(
            panel(&door, Vec3::new(0.1, 1.0, 1.0), 0.1, 0.7, Vec3::ZERO,),
            Err(PanelRefusal::LatchedWithOwnBody)
        );
    }
}
