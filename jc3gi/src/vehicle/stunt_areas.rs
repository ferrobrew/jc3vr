#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// One stunt area of a vehicle (`SStuntArea`): a patch of the vehicle's surface, triangulated in
/// the space of one of its parts, that a character in a stunt position stands and moves on (a
/// car's roof, a boat's deck, a plane's wing). Each area names the interaction-graph node a
/// character on it occupies (`CInteractionGraph::GetFirstNodeWithStuntArea`, by the area's name
/// hash). Only its size is bound.
pub struct StuntArea {
    _field_0: [u8; 392],
}
fn _StuntArea_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x188], StuntArea>([0u8; 0x188]);
    }
    unreachable!()
}
impl StuntArea {}
impl std::convert::AsRef<StuntArea> for StuntArea {
    fn as_ref(&self) -> &StuntArea {
        self
    }
}
impl std::convert::AsMut<StuntArea> for StuntArea {
    fn as_mut(&mut self) -> &mut StuntArea {
        self
    }
}
#[repr(C, align(8))]
/// A vehicle's stunt areas (`SStuntAreas`, [`Vehicle::m_StuntAreas`](crate::vehicle::vehicle::Vehicle::m_StuntAreas)): up to 32
/// [`StuntArea`](crate::vehicle::stunt_areas::StuntArea)s, followed by their count and the welding information that joins the edges of
/// neighbouring areas. Only the count is bound.
pub struct StuntAreas {
    _field_0: [u8; 12544],
    /// How many of the leading areas are in use.
    pub m_number_of_stunt_areas: u8,
    _field_3101: [u8; 7],
}
fn _StuntAreas_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3108], StuntAreas>([0u8; 0x3108]);
    }
    unreachable!()
}
impl StuntAreas {}
impl std::convert::AsRef<StuntAreas> for StuntAreas {
    fn as_ref(&self) -> &StuntAreas {
        self
    }
}
impl std::convert::AsMut<StuntAreas> for StuntAreas {
    fn as_mut(&mut self) -> &mut StuntAreas {
        self
    }
}
pub const GetNearestStuntArea_World_ADDRESS: usize = 0x140F8D180;
/// `NStuntAreas::GetNearestStuntArea_World`: the area of `stunt_areas` nearest `position_world`.
/// An area that `position_world` projects onto (straight down its plane) wins, and
/// `out_closest_point_world` is the projection; otherwise every active area's nearest edge is
/// searched, and `out_closest_point_world` is the nearest point on it, carried into the world
/// through the area's part and the vehicle's transform at the end of the step
/// ([`Vehicle::m_WorldMatrixT1`](crate::vehicle::vehicle::Vehicle::m_WorldMatrixT1)). `out_min_distance_squared` is the search's distance measure,
/// `out_closest_point_found` whether a point was found, and `ignore_welded_edges` skips the edges
/// welded to a neighbouring area. Returns the area found, or null for a vehicle without a physics
/// body or without areas. [`Character::AutoAttachToClosestStuntAreaOnVehicle`](crate::character::character::Character::AutoAttachToClosestStuntAreaOnVehicle)
/// uses it to choose where a character attaches.
pub unsafe fn GetNearestStuntArea_World(
    vehicle: *const crate::vehicle::vehicle::Vehicle,
    stunt_areas: *const crate::vehicle::stunt_areas::StuntAreas,
    position_world: *const crate::types::math::Vector3,
    out_closest_point_world: *mut crate::types::math::Vector3,
    out_min_distance_squared: *mut f32,
    out_closest_point_found: *mut bool,
    ignore_welded_edges: bool,
) -> *const crate::vehicle::stunt_areas::StuntArea {
    unsafe {
        let f: unsafe extern "system" fn(
            vehicle: *const crate::vehicle::vehicle::Vehicle,
            stunt_areas: *const crate::vehicle::stunt_areas::StuntAreas,
            position_world: *const crate::types::math::Vector3,
            out_closest_point_world: *mut crate::types::math::Vector3,
            out_min_distance_squared: *mut f32,
            out_closest_point_found: *mut bool,
            ignore_welded_edges: bool,
        ) -> *const crate::vehicle::stunt_areas::StuntArea = ::std::mem::transmute(
            GetNearestStuntArea_World_ADDRESS,
        );
        f(
            vehicle,
            stunt_areas,
            position_world,
            out_closest_point_world,
            out_min_distance_squared,
            out_closest_point_found,
            ignore_welded_edges,
        )
    }
}
