#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `SHashString` as the ADF resources store it: a pointer to the string and its `hashlittle`
/// hash, both resolved when the resource is loaded.
pub struct AdfHashString {
    /// The NUL-terminated string.
    pub m_string: *const u8,
    /// The `hashlittle` hash of [`m_string`](crate::vehicle::parts::AdfHashString::m_string).
    pub m_hash: u32,
    _field_c: [u8; 4],
}
fn _AdfHashString_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], AdfHashString>([0u8; 0x10]);
    }
    unreachable!()
}
impl AdfHashString {}
impl std::convert::AsRef<AdfHashString> for AdfHashString {
    fn as_ref(&self) -> &AdfHashString {
        self
    }
}
impl std::convert::AsMut<AdfHashString> for AdfHashString {
    fn as_mut(&mut self) -> &mut AdfHashString {
        self
    }
}
#[repr(C, align(8))]
/// `SPart`: one entry of a vehicle's `VehicleParts` ADF resource (the `<model>_vehicle_parts.vmodc`
/// in the vehicle's entity), describing a breakable, separately modelled piece of the vehicle such
/// as a door, a wheel, a mirror, or the steering wheel. Only the leading members are bound.
///
/// A part's mesh is authored in its own space, whose origin is the pivot that
/// [`m_parent_space_offset`](crate::vehicle::parts::Part::m_parent_space_offset) places in the parent part's space. Most
/// parts, the steering wheel among them, have an identity offset under the body, so their mesh is
/// authored in the vehicle's own space (+X right, +Y up, -Z forward). A hinged part, such as a
/// motorcycle's or jetski's steering fork, has its hinge pivot as its offset.
pub struct Part {
    /// The part's name, `<model>_<piece>`, such as `modernsuv_steeringwheel`,
    /// `atv_upper_steering`, or `modernmini_mirror_l`.
    pub m_name: crate::vehicle::parts::AdfHashString,
    /// The name of the part's damage type, such as `Land_Steering_Wheel`. The damage type is chosen
    /// for the damage the part takes, not for what the part is: some steering wheels use a generic
    /// type, and some mirrors use the steering wheel's.
    pub m_damage_type: crate::vehicle::parts::AdfHashString,
    /// The name of the part's behaviour, `None` for most parts. A hinged part's behaviour names its
    /// entry in the vehicle-common hinged-part properties: `SteeringFork` is a motorcycle's, ATV's,
    /// or jetski's handlebars, turned by the steering. The steering wheels of a few cars carry
    /// `SteeringWheel`, but none of them is listed as a hinged part, so it drives nothing.
    pub m_behavior: crate::vehicle::parts::AdfHashString,
    /// The name of the part's model.
    pub m_model: crate::vehicle::parts::AdfHashString,
    /// The name of the model the part swaps to when damaged.
    pub m_model_swap: crate::vehicle::parts::AdfHashString,
    /// The part's index in the vehicle's part array.
    pub m_part_index: i8,
    /// The index of the part this one hangs from.
    pub m_parent_part_index: i8,
    _field_52: [u8; 2],
    /// The part's pivot in its parent part's space, as the engine's row-major matrix.
    pub m_parent_space_offset: crate::types::math::Matrix4,
    _field_94: [u8; 4],
}
fn _Part_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x98], Part>([0u8; 0x98]);
    }
    unreachable!()
}
impl Part {}
impl std::convert::AsRef<Part> for Part {
    fn as_ref(&self) -> &Part {
        self
    }
}
impl std::convert::AsMut<Part> for Part {
    fn as_mut(&mut self) -> &mut Part {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Why a breakable body's part changed (`NPfxBreakable::EPartEventReason`), passed to
/// [`PfxBreakable::EnablePart`](crate::vehicle::parts::PfxBreakable::EnablePart) and on to the part's loosened and broken-off events.
pub enum PartEventReason {
    EPER_PartDamaged = 0isize as _,
    EPER_PartOperated = 1isize as _,
    EPER_PartInitialState = 2isize as _,
}
fn _PartEventReason_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PartEventReason>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `CPartInstance`: the live state of one [`Part`](crate::vehicle::parts::Part) of a breakable physics body
/// ([`PfxBreakable`](crate::vehicle::parts::PfxBreakable)). The instances are stored contiguously, indexed by the part index. Only some
/// members are bound.
pub struct PartInstance {
    /// The part this one hangs from, or null for the root part.
    pub m_ParentPart: *mut crate::vehicle::parts::PartInstance,
    _field_8: [u8; 24],
    /// The part's index in its body's part instances, the same as its [`Part::m_part_index`](crate::vehicle::parts::Part::m_part_index).
    pub m_PartInstanceIndex: u32,
    _field_24: [u8; 12],
    /// The owner the part reports damage to (`IDamagePartOwner*`). For a vehicle's part this is
    /// the vehicle's damage-part-owner interface; the vehicle's render update only draws parts
    /// whose owner it is.
    pub m_DamagePartOwner: *mut ::std::ffi::c_void,
    _field_38: [u8; 14],
    /// Whether the part has broken off the body. A part also counts as broken off when any ancestor
    /// has ([`BrokeOffOrAncestorBrokeOff`](crate::vehicle::parts::PartInstance::BrokeOffOrAncestorBrokeOff)).
    pub m_BrokeOff: bool,
    _field_47: [u8; 121],
    /// The part's transform from its own space into the body's root space, as the engine's
    /// row-major matrix. It is the parent space offset composed with the parent's root-space
    /// transform, further moved by a hinge's angle, a loosened part's swing, or deformation.
    /// [`Vehicle::UpdateRender_ModelMatrices`](crate::vehicle::vehicle::Vehicle::UpdateRender_ModelMatrices)
    /// draws the part at this transform times the vehicle's render transform.
    /// `CPartInstance::SetRootSpaceTransform_NoNotification` writes it without notifying the part's
    /// listeners.
    pub m_RootSpaceTransform: crate::types::math::Matrix4,
    _field_100: [u8; 24],
    /// The part's model instance, null for a part without a model.
    pub m_PartModelInstance: *mut crate::graphics_engine::model_instance::ModelInstance,
    _field_120: [u8; 32],
    /// The part's [`Part`](crate::vehicle::parts::Part) entry in the vehicle's parts resource.
    pub m_PartProperties: *const crate::vehicle::parts::Part,
    _field_148: [u8; 72],
}
fn _PartInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x190], PartInstance>([0u8; 0x190]);
    }
    unreachable!()
}
impl PartInstance {
    pub const BrokeOffOrAncestorBrokeOff_ADDRESS: usize = 0x1401D0670;
    /// Whether the part or any of its ancestors has broken off ([`m_BrokeOff`](crate::vehicle::parts::PartInstance::m_BrokeOff)),
    /// walking [`m_ParentPart`](crate::vehicle::parts::PartInstance::m_ParentPart) to the root.
    pub unsafe fn BrokeOffOrAncestorBrokeOff(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::BrokeOffOrAncestorBrokeOff_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsConstraintActive_ADDRESS: usize = 0x1401DF050;
    /// Whether the part hangs from its parent by its own constraint: the part's constraint id is
    /// valid and the world's constraint is enabled. A loosened part, such as an open door, does; a
    /// part merged into the body, or one that broke off, does not.
    pub unsafe fn IsConstraintActive(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsConstraintActive_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetPfxVehiclePartIfAny_ADDRESS: usize = 0x1401F0810;
    /// The part's own body, a `CPfxVehiclePart` (a [`PfxRigidBody`](crate::physics::PfxRigidBody)), when
    /// the part has been loosened or broken off into one; null while the part is merged into its
    /// body's own rigid body. It reads the part's world-collision rigid body and keeps it only when
    /// its type test says it is a vehicle part.
    pub unsafe fn GetPfxVehiclePartIfAny(
        &mut self,
    ) -> *mut crate::physics::PfxRigidBody {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::physics::PfxRigidBody = ::std::mem::transmute(
                Self::GetPfxVehiclePartIfAny_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<PartInstance> for PartInstance {
    fn as_ref(&self) -> &PartInstance {
        self
    }
}
impl std::convert::AsMut<PartInstance> for PartInstance {
    fn as_mut(&mut self) -> &mut PartInstance {
        self
    }
}
#[repr(C, align(8))]
/// `CPfxBreakable`: a rigid body made of [`PartInstance`](crate::vehicle::parts::PartInstance)s that can deform, loosen, and break
/// apart. A vehicle's physics body (`CPfxVehicle`) derives from it with the breakable at offset
/// zero ([`Vehicle::m_PfxVehicle`](crate::vehicle::vehicle::Vehicle::m_PfxVehicle)). Only some members
/// are bound.
pub struct PfxBreakable {
    _field_0: [u8; 288],
    /// The part instances, one per entry of the parts resource, indexed by part index.
    pub m_PartInstances: *mut crate::vehicle::parts::PartInstance,
    /// The indices of the valid part instances, [`m_NumberOfValidParts`](crate::vehicle::parts::PfxBreakable::m_NumberOfValidParts)
    /// of them. The render and update passes over the parts walk this list.
    pub m_ValidPartIndices: *mut u32,
    /// The number of entries in [`m_ValidPartIndices`](crate::vehicle::parts::PfxBreakable::m_ValidPartIndices).
    pub m_NumberOfValidParts: u32,
    _field_134: [u8; 4],
}
fn _PfxBreakable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x138], PfxBreakable>([0u8; 0x138]);
    }
    unreachable!()
}
impl PfxBreakable {
    pub const GetPartByIndex_ADDRESS: usize = 0x140237A60;
    /// The part instance at `index`, or null when `index` is not below the parts resource's part
    /// count.
    pub unsafe fn GetPartByIndex(
        &mut self,
        index: u32,
    ) -> *mut crate::vehicle::parts::PartInstance {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u32,
            ) -> *mut crate::vehicle::parts::PartInstance = ::std::mem::transmute(
                Self::GetPartByIndex_ADDRESS,
            );
            f(self as *mut Self as _, index)
        }
    }
    pub const EnablePart_ADDRESS: usize = 0x140237DE0;
    /// Loosens the merged part `part` into a body of its own (`CPartInstance::EnablePart`): the
    /// part's owner spawns its physical part (for a vehicle, `CVehicle::SpawnPhysicalPart`, which
    /// makes a [`VehiclePartGameObject`](crate::vehicle::parts::VehiclePartGameObject) and its body), and with `keep_constraint` the part's
    /// constraint from the physics prototype is cloned to hang it from its parent, such as a door
    /// on its hinge; otherwise the part breaks off. A part already hanging by its constraint
    /// ([`PartInstance::IsConstraintActive`](crate::vehicle::parts::PartInstance::IsConstraintActive)) is left alone with `keep_constraint`, and broken off
    /// without it. Runs synchronously on the calling thread, outside the physics step.
    pub unsafe fn EnablePart(
        &mut self,
        part: *mut crate::vehicle::parts::PartInstance,
        reason: crate::vehicle::parts::PartEventReason,
        keep_constraint: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                part: *mut crate::vehicle::parts::PartInstance,
                reason: crate::vehicle::parts::PartEventReason,
                keep_constraint: bool,
            ) = ::std::mem::transmute(Self::EnablePart_ADDRESS);
            f(self as *mut Self as _, part, reason, keep_constraint)
        }
    }
    pub const GetHingeAngleAboveMin_ADDRESS: usize = 0x14024C170;
    /// The hinge angle of the part at index `index` above the hinge's smallest angle, radians, from
    /// the world's constraint of a part that hangs by an enabled hinge; zero for a part without one.
    pub unsafe fn GetHingeAngleAboveMin(&mut self, index: i32) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, index: i32) -> f32 = ::std::mem::transmute(
                Self::GetHingeAngleAboveMin_ADDRESS,
            );
            f(self as *mut Self as _, index)
        }
    }
}
impl std::convert::AsRef<PfxBreakable> for PfxBreakable {
    fn as_ref(&self) -> &PfxBreakable {
        self
    }
}
impl std::convert::AsMut<PfxBreakable> for PfxBreakable {
    fn as_mut(&mut self) -> &mut PfxBreakable {
        self
    }
}
#[repr(C, align(8))]
/// `CVehiclePartGameObject`: the rigid object (`CRigidObject`) a vehicle spawns for a part that
/// loosens or breaks off (`CVehicle::SpawnPhysicalPart`), such as an open door: it owns the part's
/// own body and draws the part. Only some members are bound.
pub struct VehiclePartGameObject {}
impl VehiclePartGameObject {
    pub const TYPE_ID_ADDRESS: usize = 0x140733650;
    /// `CVehiclePartGameObject::TYPE_ID`: the run-time type id of `CVehiclePartGameObject`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const GetVehicle_ADDRESS: usize = 0x140C544F0;
    /// The vehicle the part came from, through the object's weak reference to it; null once the
    /// vehicle is gone.
    pub unsafe fn GetVehicle(&mut self) -> *mut crate::vehicle::vehicle::Vehicle {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::vehicle::vehicle::Vehicle = ::std::mem::transmute(
                Self::GetVehicle_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<VehiclePartGameObject> for VehiclePartGameObject {
    fn as_ref(&self) -> &VehiclePartGameObject {
        self
    }
}
impl std::convert::AsMut<VehiclePartGameObject> for VehiclePartGameObject {
    fn as_mut(&mut self) -> &mut VehiclePartGameObject {
        self
    }
}
