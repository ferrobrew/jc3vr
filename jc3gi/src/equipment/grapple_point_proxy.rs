#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// What a grapple attachment proxy is attached to (`CGrapplePointProxy::EAttachmentObjectInfo`),
/// stored in [`GrapplePointProxy::m_AttachmentObjectInfo`](crate::equipment::grapple_point_proxy::GrapplePointProxy::m_AttachmentObjectInfo). The reel task picks its arrival test by
/// it ([`NStateTask_ReelIn::Update`](crate::equipment::grappling_hook::NStateTask_ReelIn::Update)).
pub enum AttachmentObjectInfo {
    AOI_Invalid = 0isize as _,
    AOI_Character = 1isize as _,
    AOI_Vehicle = 2isize as _,
    AOI_Static = 3isize as _,
    AOI_Generic = 4isize as _,
}
fn _AttachmentObjectInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AttachmentObjectInfo>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A grapple attachment fixed to a character bone (`CCharacterBoneAttachedPointProxy`), a
/// [`GrapplePointProxy`](crate::equipment::grapple_point_proxy::GrapplePointProxy). Its visual getters multiply the local attachment transform by the bone
/// and character transforms. A proxy built for a bone is virtual only: its constraint hint
/// ([`GetConstraintHint`](crate::equipment::grapple_point_proxy::CharacterBoneAttachedPointProxy::GetConstraintHint)) permits no wire constraint until the
/// attachment is physicalized, and the device end of every wire
/// ([`GrapplingHook::AttachLocalWireEndToDevice`](crate::equipment::grappling_hook::GrapplingHook::AttachLocalWireEndToDevice))
/// is such a proxy, so the active wire never holds a physics constraint.
pub struct CharacterBoneAttachedPointProxy {}
impl CharacterBoneAttachedPointProxy {
    pub const GetVisualPositionAndNormalWS_ADDRESS: usize = 0x1408D8180;
    /// Writes the attachment's world position and normal, or returns false without a character.
    pub unsafe fn GetVisualPositionAndNormalWS(
        &self,
        position: *mut crate::types::math::Vector3,
        normal: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                position: *mut crate::types::math::Vector3,
                normal: *mut crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(
                Self::GetVisualPositionAndNormalWS_ADDRESS,
            );
            f(self as *const Self as _, position, normal)
        }
    }
    pub const GetVisualTransformWS_ADDRESS: usize = 0x1408D8280;
    /// Writes the attachment's world transform, or returns false without a character.
    pub unsafe fn GetVisualTransformWS(
        &self,
        transform: *mut crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                transform: *mut crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::GetVisualTransformWS_ADDRESS);
            f(self as *const Self as _, transform)
        }
    }
    pub const GetConstraintHint_ADDRESS: usize = 0x1408CA4D0;
    /// No constraint for a proxy that is not live, has no character, or is virtual only; an active
    /// (physical) constraint otherwise.
    pub unsafe fn GetConstraintHint(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetConstraintHint_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<CharacterBoneAttachedPointProxy>
for CharacterBoneAttachedPointProxy {
    fn as_ref(&self) -> &CharacterBoneAttachedPointProxy {
        self
    }
}
impl std::convert::AsMut<CharacterBoneAttachedPointProxy>
for CharacterBoneAttachedPointProxy {
    fn as_mut(&mut self) -> &mut CharacterBoneAttachedPointProxy {
        self
    }
}
#[repr(C, align(8))]
/// A grapple attachment on a physics game object (`CGenericObjectPointProxy`), a
/// [`GrapplePointProxy`](crate::equipment::grapple_point_proxy::GrapplePointProxy).
pub struct GenericObjectPointProxy {}
impl GenericObjectPointProxy {
    pub const GetConstraintHint_ADDRESS: usize = 0x1409247D0;
    /// Returns no constraint for an invalid proxy or a weapon whose grappling mode reels without
    /// attaching. Rigid objects request a virtual or physical constraint according to their motion
    /// type; other valid objects request a physical constraint.
    pub unsafe fn GetConstraintHint(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetConstraintHint_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<GenericObjectPointProxy> for GenericObjectPointProxy {
    fn as_ref(&self) -> &GenericObjectPointProxy {
        self
    }
}
impl std::convert::AsMut<GenericObjectPointProxy> for GenericObjectPointProxy {
    fn as_mut(&mut self) -> &mut GenericObjectPointProxy {
        self
    }
}
#[repr(C, align(8))]
/// The base of every grapple attachment (`CGrapplePointProxy`): what a wire end
/// ([`WireEnd::m_PointProxy`](crate::equipment::wire_end::WireEnd::m_PointProxy)) is attached to. Its
/// virtual interface answers for the attached thing whatever its kind: where the attach point is,
/// how it moves, the body behind it, and its mass. Only some members are bound.
pub struct GrapplePointProxy {
    vftable: *const crate::equipment::grapple_point_proxy::GrapplePointProxyVftable,
    _field_8: [u8; 4],
    /// What the proxy is attached to; see [`AttachmentObjectInfo`](crate::equipment::grapple_point_proxy::AttachmentObjectInfo).
    pub m_AttachmentObjectInfo: crate::equipment::grapple_point_proxy::AttachmentObjectInfo,
}
fn _GrapplePointProxy_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], GrapplePointProxy>([0u8; 0x10]);
    }
    unreachable!()
}
impl GrapplePointProxy {
    pub fn vftable(
        &self,
    ) -> *const crate::equipment::grapple_point_proxy::GrapplePointProxyVftable {
        self.vftable
            as *const crate::equipment::grapple_point_proxy::GrapplePointProxyVftable
    }
    /// Writes the attach point's world position and normal, or returns false when the
    /// attached thing is gone.
    pub unsafe fn GetVisualPositionAndNormalWS(
        &self,
        position: *mut crate::types::math::Vector3,
        normal: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).GetVisualPositionAndNormalWS).read();
            f(self as *const Self as _, position, normal)
        }
    }
    /// Whether the proxy and the thing it is attached to are live.
    pub unsafe fn IsOk(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsOk).read();
            f(self as *const Self as _)
        }
    }
    /// The attached object's mass, kilograms: its physics instance's mass for a dynamic
    /// object, a vehicle's own mass for a vehicle, and a very large mass for an object that is
    /// not dynamic.
    pub unsafe fn GetObjectMass(&self) -> f32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetObjectMass).read();
            f(self as *const Self as _)
        }
    }
    /// Writes the attach point's world velocity: for a physics object, its body's velocity at
    /// the point (the linear velocity plus the angular velocity's part at the point), and zero
    /// when the object is gone. The reel blends the character's carried velocity toward it
    /// ([`NStateTask_ReelIn::UpdateReelVelocity`](crate::equipment::grappling_hook::NStateTask_ReelIn::UpdateReelVelocity)).
    pub unsafe fn GetObjectAttachPointVelocity(
        &self,
        velocity: *mut crate::types::math::Vector3,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).GetObjectAttachPointVelocity).read();
            f(self as *const Self as _, velocity)
        }
    }
    /// Writes the id of the body a wire constraint at this end would join, or the invalid id
    /// ([`PfxCharacterInstance::INVALID_BODY_ID`](crate::physics::PfxCharacterInstance::INVALID_BODY_ID))
    /// without one: a physics object's own body while it is dynamic, a
    /// character's proxy body or the ragdoll bone it is attached to.
    /// [`GrapplingHookWire::CreateConstraint`](crate::equipment::grappling_hook::GrapplingHookWire::CreateConstraint)
    /// reads both ends' bodies through it.
    pub unsafe fn GetRigidBodyForConstraint(&self, result: *mut u32) -> *mut u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetRigidBodyForConstraint).read();
            f(self as *const Self as _, result)
        }
    }
    /// The attached game object (its `CGameObject` subobject), or null.
    pub unsafe fn GetGameObject(&self) -> *mut crate::game::GameObject {
        unsafe {
            let f = (&raw const (*self.vftable()).GetGameObject).read();
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<GrapplePointProxy> for GrapplePointProxy {
    fn as_ref(&self) -> &GrapplePointProxy {
        self
    }
}
impl std::convert::AsMut<GrapplePointProxy> for GrapplePointProxy {
    fn as_mut(&mut self) -> &mut GrapplePointProxy {
        self
    }
}
#[repr(C, align(8))]
pub struct GrapplePointProxyVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    /// Writes the attach point's world position and normal, or returns false when the
    /// attached thing is gone.
    pub GetVisualPositionAndNormalWS: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
        position: *mut crate::types::math::Vector3,
        normal: *mut crate::types::math::Vector3,
    ) -> bool,
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    /// Whether the proxy and the thing it is attached to are live.
    pub IsOk: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ) -> bool,
    /// The attached object's mass, kilograms: its physics instance's mass for a dynamic
    /// object, a vehicle's own mass for a vehicle, and a very large mass for an object that is
    /// not dynamic.
    pub GetObjectMass: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ) -> f32,
    /// Writes the attach point's world velocity: for a physics object, its body's velocity at
    /// the point (the linear velocity plus the angular velocity's part at the point), and zero
    /// when the object is gone. The reel blends the character's carried velocity toward it
    /// ([`NStateTask_ReelIn::UpdateReelVelocity`](crate::equipment::grappling_hook::NStateTask_ReelIn::UpdateReelVelocity)).
    pub GetObjectAttachPointVelocity: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
        velocity: *mut crate::types::math::Vector3,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    /// Writes the id of the body a wire constraint at this end would join, or the invalid id
    /// ([`PfxCharacterInstance::INVALID_BODY_ID`](crate::physics::PfxCharacterInstance::INVALID_BODY_ID))
    /// without one: a physics object's own body while it is dynamic, a
    /// character's proxy body or the ragdoll bone it is attached to.
    /// [`GrapplingHookWire::CreateConstraint`](crate::equipment::grappling_hook::GrapplingHookWire::CreateConstraint)
    /// reads both ends' bodies through it.
    pub GetRigidBodyForConstraint: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
        result: *mut u32,
    ) -> *mut u32,
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ),
    /// The attached game object (its `CGameObject` subobject), or null.
    pub GetGameObject: unsafe extern "system" fn(
        this: *const crate::equipment::grapple_point_proxy::GrapplePointProxy,
    ) -> *mut crate::game::GameObject,
}
fn _GrapplePointProxyVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xB8], GrapplePointProxyVftable>([0u8; 0xB8]);
    }
    unreachable!()
}
impl GrapplePointProxyVftable {}
impl std::convert::AsRef<GrapplePointProxyVftable> for GrapplePointProxyVftable {
    fn as_ref(&self) -> &GrapplePointProxyVftable {
        self
    }
}
impl std::convert::AsMut<GrapplePointProxyVftable> for GrapplePointProxyVftable {
    fn as_mut(&mut self) -> &mut GrapplePointProxyVftable {
        self
    }
}
