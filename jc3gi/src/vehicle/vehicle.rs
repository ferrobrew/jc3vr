#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CDoorInfo`: one door part of a vehicle's [`VehicleDoorController`](crate::vehicle::vehicle::VehicleDoorController) and its state. A closed door
/// is merged into the vehicle's body; opening it loosens the part onto the hinge constraint of the
/// vehicle's physics prototype, as a body of its own
/// ([`PartInstance::GetPfxVehiclePartIfAny`](crate::vehicle::parts::PartInstance::GetPfxVehiclePartIfAny))
/// owned by a [`VehiclePartGameObject`](crate::vehicle::parts::VehiclePartGameObject).
///
/// Each post-simulation update, the controller acts on [`m_PendingState`](crate::vehicle::vehicle::DoorInfo::m_PendingState).
/// An opening request loosens the part
/// ([`PfxBreakable::EnablePart`](crate::vehicle::parts::PfxBreakable::EnablePart) with
/// [`PartEventReason::EPER_PartOperated`](crate::vehicle::parts::PartEventReason::EPER_PartOperated)),
/// and both requests convert the hinge to a powered one whose motor turns it toward the open or the
/// closed angle. A door that has opened stops its motor, unless it is kept open, and then swings
/// freely on its hinge. A freely swinging door whose hinge angle above its minimum
/// ([`PfxBreakable::GetHingeAngleAboveMin`](crate::vehicle::parts::PfxBreakable::GetHingeAngleAboveMin))
/// falls below a small angle while it swings shut faster than a threshold speed is merged back into
/// the body (`CPartInstance::MergePart`): a door swung shut hard latches. A door the motor closes
/// is merged once it reaches its smallest angle. Only some members are bound.
pub struct DoorInfo {
    /// The state the controller moves the door to on its next update, or
    /// [`DoorState::DOOR_CLOSED`](crate::vehicle::vehicle::DoorState::DOOR_CLOSED) when nothing is pending.
    pub m_PendingState: crate::vehicle::vehicle::DoorState,
    pub m_State: crate::vehicle::vehicle::DoorState,
    pub m_DoorSlot: crate::vehicle::vehicle::DoorSlot,
    _field_c: [u8; 4],
    /// The door's part instance in the vehicle's physics body.
    pub m_PartInstance: *mut crate::vehicle::parts::PartInstance,
    _field_18: [u8; 4],
    /// The hinge's smallest angle, radians.
    pub m_HingeMinAngle_Rad: f32,
    /// The hinge's largest angle, radians.
    pub m_HingeMaxAngle_Rad: f32,
    _field_24: [u8; 4],
    /// The smallest angle a door kept open is held at, radians.
    pub m_KeepOpenMinAngle_Rad: f32,
    /// A freely swinging door's hinge angle above its minimum on the controller's last update,
    /// degrees, from which it measures how fast the door swings shut.
    pub m_PreviousAngle_Deg: f32,
    _field_30: [u8; 12],
    pub m_Flags: u32,
    _field_40: [u8; 16],
}
fn _DoorInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], DoorInfo>([0u8; 0x50]);
    }
    unreachable!()
}
impl DoorInfo {}
impl DoorInfo {
    /// The flag of [`m_Flags`](crate::vehicle::vehicle::DoorInfo::m_Flags) an opening request that keeps the door open sets.
    pub const FLAG_KEEP_OPEN: u32 = 2;
}
impl std::convert::AsRef<DoorInfo> for DoorInfo {
    fn as_ref(&self) -> &DoorInfo {
        self
    }
}
impl std::convert::AsMut<DoorInfo> for DoorInfo {
    fn as_mut(&mut self) -> &mut DoorInfo {
        self
    }
}
#[repr(C, align(8))]
/// One node of the red-black tree behind [`VehicleDoorController`](crate::vehicle::vehicle::VehicleDoorController)'s door map
/// (`std::_Tree_nod<...>::_Node`): a slot and its doors.
pub struct DoorMapNode {
    pub m_Left: *mut crate::vehicle::vehicle::DoorMapNode,
    pub m_Parent: *mut crate::vehicle::vehicle::DoorMapNode,
    pub m_Right: *mut crate::vehicle::vehicle::DoorMapNode,
    /// The slot this node holds.
    pub m_DoorSlot: crate::vehicle::vehicle::DoorSlot,
    _field_1c: [u8; 4],
    /// The first of the slot's doors (the vector's `_Myfirst`).
    pub m_DoorsBegin: *mut crate::vehicle::vehicle::DoorInfo,
    /// One past the last of the slot's doors (`_Mylast`).
    pub m_DoorsEnd: *mut crate::vehicle::vehicle::DoorInfo,
    _field_30: [u8; 17],
    /// Whether the node is the head, the tree's sentinel, rather than a slot (`_Isnil`).
    pub m_IsNil: bool,
    _field_42: [u8; 6],
}
fn _DoorMapNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x48], DoorMapNode>([0u8; 0x48]);
    }
    unreachable!()
}
impl DoorMapNode {}
impl std::convert::AsRef<DoorMapNode> for DoorMapNode {
    fn as_ref(&self) -> &DoorMapNode {
        self
    }
}
impl std::convert::AsMut<DoorMapNode> for DoorMapNode {
    fn as_mut(&mut self) -> &mut DoorMapNode {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The door slots of a vehicle (`NVehicle::EDoorSlot`): which opening a [`DoorInfo`](crate::vehicle::vehicle::DoorInfo) is. A slot
/// may hold several door parts that open together.
pub enum DoorSlot {
    FRONT_LEFT = 0isize as _,
    FRONT_RIGHT = 1isize as _,
    REAR_LEFT = 2isize as _,
    REAR_RIGHT = 3isize as _,
    CENTER_LEFT = 4isize as _,
    CENTER_RIGHT = 5isize as _,
    TURRET_DOOR = 6isize as _,
    TRUNK = 7isize as _,
    HOOD = 8isize as _,
    REAR_DOOR = 9isize as _,
    CARGO_BAY_DOOR = 10isize as _,
    FOLDING_WING = 11isize as _,
    MISSILE_HATCH = 12isize as _,
}
fn _DoorSlot_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DoorSlot>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A door's state (`NVehicle::EDoorState`), as [`DoorInfo::m_State`](crate::vehicle::vehicle::DoorInfo::m_State) and
/// [`DoorInfo::m_PendingState`](crate::vehicle::vehicle::DoorInfo::m_PendingState) hold it.
pub enum DoorState {
    DOOR_OPEN = 0isize as _,
    DOOR_OPENING = 1isize as _,
    DOOR_CLOSING = 2isize as _,
    DOOR_MISSING = 3isize as _,
    DOOR_CLOSED = 4isize as _,
}
fn _DoorState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DoorState>([0u8; 0x4]);
    }
    unreachable!()
}
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
    _field_15e0: [u8; 136],
    /// The vehicle's doors, hood, trunk, hatches, and other opening parts, and their state.
    pub m_VehicleDoorController: crate::vehicle::vehicle::VehicleDoorController,
    _field_1690: [u8; 13732],
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
    pub const TYPE_ID_ADDRESS: usize = 0x14060F3F0;
    /// `CVehicle::TYPE_ID`: the run-time type id of `CVehicle`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const OpenDoor_ADDRESS: usize = 0x140FC7320;
    /// Asks every door of `slot` to open: makes sure the vehicle's physics body is in the world
    /// and the vehicle is in the update, then sets each [`DoorInfo`](crate::vehicle::vehicle::DoorInfo) of the slot's
    /// [`m_PendingState`](crate::vehicle::vehicle::DoorInfo::m_PendingState) to [`DoorState::DOOR_OPENING`](crate::vehicle::vehicle::DoorState::DOOR_OPENING) and clears its
    /// [`m_Flags`](crate::vehicle::vehicle::DoorInfo::m_Flags). With `keep_open`, the door is held open at least
    /// `keep_open_angle_deg` below its hinge's largest angle and reopens when pushed back
    /// ([`DoorInfo::FLAG_KEEP_OPEN`](crate::vehicle::vehicle::DoorInfo::FLAG_KEEP_OPEN)). The door controller acts on the request in the vehicle's
    /// post-simulation update (`CVehicle::UpdatePostSim_Doors`), which loosens the part onto its
    /// hinge ([`PfxBreakable::EnablePart`](crate::vehicle::parts::PfxBreakable::EnablePart)) and turns
    /// the hinge with a motor to the open angle.
    pub unsafe fn OpenDoor(
        &mut self,
        slot: crate::vehicle::vehicle::DoorSlot,
        keep_open: bool,
        keep_open_angle_deg: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::vehicle::vehicle::DoorSlot,
                keep_open: bool,
                keep_open_angle_deg: f32,
            ) = ::std::mem::transmute(Self::OpenDoor_ADDRESS);
            f(self as *mut Self as _, slot, keep_open, keep_open_angle_deg)
        }
    }
    pub const CloseDoor_ADDRESS: usize = 0x140FC72B0;
    /// Asks every door of `slot` to close, the same way [`OpenDoor`](crate::vehicle::vehicle::Vehicle::OpenDoor) asks it to
    /// open: the door controller turns the hinge's motor back to its smallest angle and merges the
    /// part into the body there.
    pub unsafe fn CloseDoor(&mut self, slot: crate::vehicle::vehicle::DoorSlot) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::vehicle::vehicle::DoorSlot,
            ) = ::std::mem::transmute(Self::CloseDoor_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
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
/// `CVehicleDoorController`: a vehicle's doors ([`DoorInfo`](crate::vehicle::vehicle::DoorInfo)), kept in a
/// `std::map<NVehicle::EDoorSlot, std::vector<CDoorInfo>>` keyed by [`DoorSlot`](crate::vehicle::vehicle::DoorSlot), and updated from
/// the vehicle's post-simulation update. Only some members are bound.
pub struct VehicleDoorController {
    _field_0: [u8; 8],
    /// The map's head node (`_Myhead`): its [`m_Parent`](crate::vehicle::vehicle::DoorMapNode::m_Parent) is the tree's
    /// root, and the tree's leaf links point back at it.
    pub m_DoorMapHead: *mut crate::vehicle::vehicle::DoorMapNode,
    _field_10: [u8; 16],
    /// The vehicle the doors belong to.
    pub m_Vehicle: *mut crate::vehicle::vehicle::Vehicle,
}
fn _VehicleDoorController_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x28], VehicleDoorController>([0u8; 0x28]);
    }
    unreachable!()
}
impl VehicleDoorController {}
impl std::convert::AsRef<VehicleDoorController> for VehicleDoorController {
    fn as_ref(&self) -> &VehicleDoorController {
        self
    }
}
impl std::convert::AsMut<VehicleDoorController> for VehicleDoorController {
    fn as_mut(&mut self) -> &mut VehicleDoorController {
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
