#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// A vehicle seat identifier (`NVehicle::ESeat`): an index into [`Vehicle::m_Seats`](crate::vehicle::vehicle::Vehicle::m_Seats), with the
/// seat categories laid out in bands (driver, passengers, gunners, stunt positions, scripted).
pub enum Seat {
    EDriverSeat = 0isize as _,
    EPassengerSeat = 1isize as _,
    EGunnerSeat = 6isize as _,
    EStuntSeat = 8isize as _,
    EScriptedSeats = 20isize as _,
    ENoSeatIdentifier = -1isize as _,
}
fn _Seat_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], Seat>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A vehicle (`CVehicle`). Like other game objects it carries its `CGameObject` base at the same
/// offset as a character does
/// ([`Character::GAME_OBJECT_BASE_OFFSET`](crate::character::character::Character::GAME_OBJECT_BASE_OFFSET)),
/// and the vtable there serves RTTI and the transform virtuals. Only some members are bound.
pub struct Vehicle {
    _field_0: [u8; 1108],
    /// The signed steering amount the vehicle's controller last applied
    /// (`m_SteeringValues.Turn`). It is the value of the vehicle's `steering_turn` variable, which
    /// value-driven effects and hinged parts can be driven by.
    pub m_SteeringTurn: f32,
    _field_458: [u8; 32],
    /// The vehicle's world transform at the start of the current simulation step, as the engine's
    /// row-major matrix.
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    /// The vehicle's world transform at the end of the current simulation step.
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    /// The render transform: [`m_WorldMatrixT0`](crate::vehicle::vehicle::Vehicle::m_WorldMatrixT0) and
    /// [`m_WorldMatrixT1`](crate::vehicle::vehicle::Vehicle::m_WorldMatrixT1) interpolated by the render update's frame
    /// fraction. `CVehicle::UpdateRender_Parallel` hands it to
    /// [`UpdateRender_ModelMatrices`](crate::vehicle::vehicle::Vehicle::UpdateRender_ModelMatrices).
    pub m_InterpolatedWorldMatrix: crate::types::math::Matrix4,
    _field_538: [u8; 1520],
    /// The vehicle's seats, indexed by [`Seat`](crate::vehicle::vehicle::Seat).
    pub m_Seats: [crate::vehicle::vehicle::VehicleSeat; 42],
    _field_1308: [u8; 724],
    /// The seat the local player occupies, or [`Seat::ENoSeatIdentifier`](crate::vehicle::vehicle::Seat::ENoSeatIdentifier). The game's input
    /// contexts test it against [`Seat::EDriverSeat`](crate::vehicle::vehicle::Seat::EDriverSeat) to decide whether driver controls apply.
    pub m_LocalPlayerSeat: crate::vehicle::vehicle::Seat,
    _field_15e0: [u8; 13908],
    /// The world-space point the vehicle's weapons aim at, written by
    /// [`SetAimTargetPosition`](crate::vehicle::vehicle::Vehicle::SetAimTargetPosition).
    pub m_AimingTargetPosition: crate::types::math::Vector3,
    _field_4c40: [u8; 264],
    /// How long, in seconds, AI-controlled vehicle weapons keep an externally supplied aim target.
    /// [`SetAimTargetPosition`](crate::vehicle::vehicle::Vehicle::SetAimTargetPosition) sets it to `-1` when the local player
    /// drives and to `2` otherwise.
    pub m_AIVehicleWeaponAimTimeout: f32,
    _field_4d4c: [u8; 132],
    /// The vehicle's physics body (`CPfxVehicle*`), whose breakable base, with the vehicle's
    /// parts, is at offset zero. [`IsMech`](crate::vehicle::vehicle::Vehicle::IsMech) tests its strafing wheel-steering
    /// controller.
    pub m_PfxVehicle: *mut crate::vehicle::parts::PfxBreakable,
    _field_4dd8: [u8; 1480],
    /// The hash of the vehicle's type name (`HashString` of names such as
    /// `v1706_boat_na_rocketboat`), compared by [`IsRocketBoat`](crate::vehicle::vehicle::Vehicle::IsRocketBoat).
    pub m_VehicleTypeNameHash: u32,
    _field_53a4: [u8; 4],
}
fn _Vehicle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x53A8], Vehicle>([0u8; 0x53A8]);
    }
    unreachable!()
}
impl Vehicle {
    pub const SetAimTargetPosition_ADDRESS: usize = 0x140F24B00;
    /// Sets [`m_AimingTargetPosition`](crate::vehicle::vehicle::Vehicle::m_AimingTargetPosition) and refreshes
    /// [`m_AIVehicleWeaponAimTimeout`](crate::vehicle::vehicle::Vehicle::m_AIVehicleWeaponAimTimeout).
    pub unsafe fn SetAimTargetPosition(
        &mut self,
        position: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                position: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetAimTargetPosition_ADDRESS);
            f(self as *mut Self as _, position)
        }
    }
    pub const GetSeat_ADDRESS: usize = 0x140F24FB0;
    /// Returns the seat `character` occupies: the index of the seat whose graph node is the
    /// character's current interaction-graph node, or [`Seat::ENoSeatIdentifier`](crate::vehicle::vehicle::Seat::ENoSeatIdentifier).
    pub unsafe fn GetSeat(
        &self,
        character: *const crate::character::character::Character,
    ) -> crate::vehicle::vehicle::Seat {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                character: *const crate::character::character::Character,
            ) -> crate::vehicle::vehicle::Seat = ::std::mem::transmute(
                Self::GetSeat_ADDRESS,
            );
            f(self as *const Self as _, character)
        }
    }
    pub const IsMech_ADDRESS: usize = 0x140F23990;
    /// Whether the vehicle is a mech: its physics body has a strafing wheel-steering controller.
    pub unsafe fn IsMech(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsMech_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const TeleportVehicle_ADDRESS: usize = 0x140F4FC60;
    /// Moves the vehicle to `transform` directly (through the `CGameObject` transform setter),
    /// invalidates its collision caches, and detaches any player grapple wire attached to it.
    /// Seated characters follow through their attachment.
    pub unsafe fn TeleportVehicle(
        &mut self,
        transform: *const crate::types::math::Matrix4,
        p2: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                transform: *const crate::types::math::Matrix4,
                p2: f32,
            ) = ::std::mem::transmute(Self::TeleportVehicle_ADDRESS);
            f(self as *mut Self as _, transform, p2)
        }
    }
    pub const GetWeaponTurretCurrentYawAngle_ADDRESS: usize = 0x140F90FC0;
    /// The current yaw angle of the vehicle's weapon turret, read from the powered hinge controller
    /// of the turret part; `0` when the vehicle has no turret.
    pub unsafe fn GetWeaponTurretCurrentYawAngle(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetWeaponTurretCurrentYawAngle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateRender_ModelMatrices_ADDRESS: usize = 0x140F90740;
    /// Sets the model matrix of every valid, unbroken part with a model to the part's
    /// [`m_RootSpaceTransform`](crate::vehicle::parts::PartInstance::m_RootSpaceTransform) times
    /// `transform` (through `NPfxBreakable::ForEachValidRenderablePart`), then copies the root-space
    /// transforms of the parts that attached objects hang from into those attachments. A zero
    /// `dtf`, or a vehicle flagged to snap, initialises each model matrix instead of updating it.
    /// Called from
    /// `CVehicle::UpdateRender_Parallel` with
    /// [`m_InterpolatedWorldMatrix`](crate::vehicle::vehicle::Vehicle::m_InterpolatedWorldMatrix) on the parallel render update,
    /// and from `CVehicle::InitTransform`.
    pub unsafe fn UpdateRender_ModelMatrices(
        &mut self,
        transform: *const crate::types::math::Matrix4,
        dtf: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                transform: *const crate::types::math::Matrix4,
                dtf: f32,
            ) = ::std::mem::transmute(Self::UpdateRender_ModelMatrices_ADDRESS);
            f(self as *mut Self as _, transform, dtf)
        }
    }
    pub const IsRocketBoat_ADDRESS: usize = 0x140F6F8F0;
    /// Whether the vehicle is the rocket boat: [`m_VehicleTypeNameHash`](crate::vehicle::vehicle::Vehicle::m_VehicleTypeNameHash)
    /// equals the hash of `v1706_boat_na_rocketboat` (computed once and cached). The sea-vehicle
    /// input, water-contact, and player action contexts use it. IDA labels this address
    /// `CWeaponBase::MarkNextWeaponComponentForFire`, but every caller passes a vehicle; the name
    /// here is descriptive.
    pub unsafe fn IsRocketBoat(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsRocketBoat_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl Vehicle {
    /// The number of entries in [`m_Seats`](crate::vehicle::vehicle::Vehicle::m_Seats).
    pub const MAX_SEATS: u64 = 42;
}
impl std::convert::AsRef<Vehicle> for Vehicle {
    fn as_ref(&self) -> &Vehicle {
        self
    }
}
impl std::convert::AsMut<Vehicle> for Vehicle {
    fn as_mut(&mut self) -> &mut Vehicle {
        self
    }
}
#[repr(C, align(8))]
/// One seat of a vehicle (`CVehicle::CSeat`). It is interaction-graph metadata (the graph node the
/// seated character occupies, the door slot, the window part, and whether the player may use it)
/// and carries no transform: a seated character's pose comes from its own attach bone and offset.
pub struct VehicleSeat {
    /// The interaction-graph node a character occupies while in this seat.
    /// [`Vehicle::GetSeat`](crate::vehicle::vehicle::Vehicle::GetSeat) matches a character's current node against it.
    pub m_GraphNode: *mut u8,
    _field_8: [u8; 40],
}
fn _VehicleSeat_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], VehicleSeat>([0u8; 0x30]);
    }
    unreachable!()
}
impl VehicleSeat {}
impl std::convert::AsRef<VehicleSeat> for VehicleSeat {
    fn as_ref(&self) -> &VehicleSeat {
        self
    }
}
impl std::convert::AsMut<VehicleSeat> for VehicleSeat {
    fn as_mut(&mut self) -> &mut VehicleSeat {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The engine's vehicle classification (`NVehicle::EVehicleType`), stored on a vehicle as
/// `CVehicle::m_VehicleType` and returned by `CVehicle::GetVehicleType`, which is inlined at every
/// use in this build. Jetskis classify as [`EBOAT`](crate::vehicle::vehicle::VehicleType::EBOAT). The field's offset is not mapped.
pub enum VehicleType {
    ECAR = 0isize as _,
    EHELICOPTER = 1isize as _,
    EBOAT = 2isize as _,
    EAIRPLANE = 3isize as _,
    EMOTORCYCLE = 4isize as _,
    ESUB = 5isize as _,
    ETRAIN = 6isize as _,
}
fn _VehicleType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], VehicleType>([0u8; 0x4]);
    }
    unreachable!()
}
pub const NStateTask_InputVehicleExitTask_TeleportUpwards_ADDRESS: usize = 0x14081A3C0;
/// `NStateTask_InputVehicleExitTask::TeleportUpwards`, run from the vehicle-exit animation event
/// callback: probes upward from the character with a raycast and a sphere collision test for clear
/// space, forces the neutral state, and relocates the character there directly rather than through
/// the physics proxy (the placement itself is inferred from the call shape).
pub unsafe fn NStateTask_InputVehicleExitTask_TeleportUpwards(
    character: *mut crate::character::character::Character,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            character: *mut crate::character::character::Character,
        ) = ::std::mem::transmute(
            NStateTask_InputVehicleExitTask_TeleportUpwards_ADDRESS,
        );
        f(character)
    }
}
