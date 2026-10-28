#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A grapple attachment fixed to a character bone (`CCharacterBoneAttachedPointProxy`). Its
/// visual getters multiply the local attachment transform by the bone and character transforms.
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
/// A grapple attachment on a physics game object (`CGenericObjectPointProxy`).
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
