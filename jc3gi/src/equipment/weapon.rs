#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A kind of ammunition (`CAmmunition::EAmmunitionType`). A weapon's kind is
/// [`WeaponBase::m_AmmunitionType`](crate::equipment::weapon::WeaponBase::m_AmmunitionType), set from the weapon's properties, and the holder's reserve of
/// each kind is an entry of
/// [`Inventory::m_Ammunition`](crate::character::inventory::Inventory::m_Ammunition), so every weapon of
/// a kind draws on the same reserve.
pub enum AmmunitionType {
    AMMO_PISTOL = 0isize as _,
    AMMO_SHOTGUN = 1isize as _,
    AMMO_SMG = 2isize as _,
    AMMO_ASSAULT_RIFLE = 3isize as _,
    AMMO_SNIPER = 4isize as _,
    AMMO_ROCKETS = 5isize as _,
    AMMO_EXPLOSIVES = 6isize as _,
    AMMO_HAND_GRENADE = 7isize as _,
    AMMO_GRENADE_LAUNCHER = 8isize as _,
    AMMO_SUNGUN = 9isize as _,
    AMMO_MISSILE_STRIKE = 10isize as _,
    AMMO_UNLIMITED = 11isize as _,
    AMMO_BEACON = 12isize as _,
    AMMO_NO_AMMO = 13isize as _,
    AMMO_FLARE = 14isize as _,
    AMMO_EMP = 15isize as _,
    AMMO_POWERCORE = 16isize as _,
    NOF_AMMO_TYPES = 17isize as _,
    AMMO_INVALID = 18isize as _,
}
fn _AmmunitionType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AmmunitionType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `CBarrelRecoilWeaponComponent`: a model part that recoils with each shot, such as a pistol's
/// slide or a rifle's ejection port. It is a [`ModelAttachementWeaponComponent`](crate::equipment::weapon::ModelAttachementWeaponComponent) (its kind is
/// [`E_WEAPON_COMPONENT_BARREL_RECOIL`](crate::equipment::weapon::WeaponComponentType::E_WEAPON_COMPONENT_BARREL_RECOIL)
/// and it passes the model-attachment type test), so the same pointer reads as one; only the
/// members it adds are mapped here, at their offsets from the start of the component.
pub struct BarrelRecoilWeaponComponent {
    _field_0: [u8; 472],
    /// The fire point of the gun ([`WeaponData::m_InternalFirePositions`](crate::equipment::weapon::WeaponData::m_InternalFirePositions)) whose time since its
    /// last shot drives the recoil.
    pub m_InternalFirePositionIndex: i32,
    /// The recoil travel, in metres, along the part's own forward axis.
    pub m_RecoilMaxLength: f32,
    /// The seconds the part takes to travel back after a shot.
    pub m_RecoilFireTime: f32,
    /// The seconds the part takes to return after travelling back.
    pub m_RecoilRecoverTime: f32,
    /// Whether the part stays fully back while the magazine is empty (a slide that locks back),
    /// for a player's wielded or dual-wielded weapon.
    pub m_MaxRecoilOnEmptyAmmo: bool,
    _field_1e9: [u8; 7],
}
fn _BarrelRecoilWeaponComponent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F0], BarrelRecoilWeaponComponent>([0u8; 0x1F0]);
    }
    unreachable!()
}
impl BarrelRecoilWeaponComponent {
    pub const TYPE_ID_ADDRESS: usize = 0x140913270;
    /// `CBarrelRecoilWeaponComponent::TYPE_ID`: the run-time type id of the class, for
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x14093E060;
    /// Places the part (its override of the weapon component's attachment placement): runs
    /// [`ModelAttachementWeaponComponent::UpdateAttachmentTransforms`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdateAttachmentTransforms), then moves
    /// [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix) along the part's own
    /// forward axis by a fraction of [`m_RecoilMaxLength`](crate::equipment::weapon::BarrelRecoilWeaponComponent::m_RecoilMaxLength). The fraction
    /// comes from the time since the fire point
    /// [`m_InternalFirePositionIndex`](crate::equipment::weapon::BarrelRecoilWeaponComponent::m_InternalFirePositionIndex) last fired: it rises to
    /// full over [`m_RecoilFireTime`](crate::equipment::weapon::BarrelRecoilWeaponComponent::m_RecoilFireTime) and falls back over
    /// [`m_RecoilRecoverTime`](crate::equipment::weapon::BarrelRecoilWeaponComponent::m_RecoilRecoverTime). With
    /// [`m_MaxRecoilOnEmptyAmmo`](crate::equipment::weapon::BarrelRecoilWeaponComponent::m_MaxRecoilOnEmptyAmmo), for a player's wielded or
    /// dual-wielded weapon, it is full while the magazine is empty, and on a multi-gun weapon also
    /// on the gun that will not fire the last round. It refreshes the saved matrix as the base
    /// does.
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        snow_factor: f32,
        water_factor: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                snow_factor: f32,
                water_factor: f32,
                transform_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(self as *mut Self as _, snow_factor, water_factor, transform_type)
        }
    }
}
impl BarrelRecoilWeaponComponent {
    /// The class's vtable, the value of the vtable pointer of a barrel recoil part. A consumer
    /// holding a model part can tell a barrel recoil part by it, or by
    /// [`m_Type`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_Type).
    pub const VFTABLE: u64 = 5406350344;
}
impl std::convert::AsRef<BarrelRecoilWeaponComponent> for BarrelRecoilWeaponComponent {
    fn as_ref(&self) -> &BarrelRecoilWeaponComponent {
        self
    }
}
impl std::convert::AsMut<BarrelRecoilWeaponComponent> for BarrelRecoilWeaponComponent {
    fn as_mut(&mut self) -> &mut BarrelRecoilWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
/// `CBarrelSpinWeaponComponent`: a model part that spins while the weapon fires, such as a
/// machine gun's barrel or ammunition belt. It is a [`ModelAttachementWeaponComponent`](crate::equipment::weapon::ModelAttachementWeaponComponent) (its
/// kind is [`E_WEAPON_COMPONENT_BARREL_SPIN`](crate::equipment::weapon::WeaponComponentType::E_WEAPON_COMPONENT_BARREL_SPIN)
/// and it passes the model-attachment type test). Its own members are unmapped.
pub struct BarrelSpinWeaponComponent {}
impl BarrelSpinWeaponComponent {
    pub const TYPE_ID_ADDRESS: usize = 0x140913350;
    /// `CBarrelSpinWeaponComponent::TYPE_ID`: the run-time type id of the class, for
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
}
impl BarrelSpinWeaponComponent {
    /// The class's vtable, the value of the vtable pointer of a barrel spin part.
    pub const VFTABLE: u64 = 5406350904;
}
impl std::convert::AsRef<BarrelSpinWeaponComponent> for BarrelSpinWeaponComponent {
    fn as_ref(&self) -> &BarrelSpinWeaponComponent {
        self
    }
}
impl std::convert::AsMut<BarrelSpinWeaponComponent> for BarrelSpinWeaponComponent {
    fn as_mut(&mut self) -> &mut BarrelSpinWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
/// `CBulletWeaponBase`: the base of the fast-projectile weapons. Its layout is unmapped.
pub struct BulletWeaponBase {}
impl BulletWeaponBase {
    pub const Fire_Projectile_ADDRESS: usize = 0x140927550;
    /// The bullet weapons' [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) override: spawns the bullet projectile
    /// and then calls [`WeaponBase::Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase). It is the only caller of
    /// [`NWeaponUtil::Trajectory_GetAngleToHitPosition`](crate::equipment::weapon::NWeaponUtil::Trajectory_GetAngleToHitPosition).
    pub unsafe fn Fire_Projectile(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Projectile_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const GetTuning_ADDRESS: usize = 0x1409147D0;
    /// Returns the bullet weapon's tuning (`SBulletWeaponTunings`), which selects explosive and
    /// water behaviour for its projectiles.
    pub unsafe fn GetTuning(&self) -> *const ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const ::std::ffi::c_void = ::std::mem::transmute(
                Self::GetTuning_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<BulletWeaponBase> for BulletWeaponBase {
    fn as_ref(&self) -> &BulletWeaponBase {
        self
    }
}
impl std::convert::AsMut<BulletWeaponBase> for BulletWeaponBase {
    fn as_mut(&mut self) -> &mut BulletWeaponBase {
        self
    }
}
#[repr(C, align(8))]
/// `CDoActWeaponComponent`: a weapon component that plays an act when its weapon starts or stops
/// firing or is equipped, such as the pump shotgun's pump. Each act goes to the component's
/// skeletal object when it has one; otherwise, with
/// [`m_SendActToWeapon`](crate::equipment::weapon::DoActWeaponComponent::m_SendActToWeapon), to every gun's own animated model
/// ([`WeaponData::DoAct`](crate::equipment::weapon::WeaponData::DoAct)); otherwise to the holder
/// ([`Character::QueueAct`](crate::character::character::Character::QueueAct)). The acts are queued and
/// gate nothing: no state of the act blocks the weapon's next shot. Only some members are mapped,
/// at their offsets from the start of the component.
pub struct DoActWeaponComponent {
    _field_0: [u8; 208],
    /// The weapon the component belongs to (`CWeaponComponentBase::m_Owner`).
    pub m_Owner: *mut crate::equipment::weapon::WeaponBase,
    _field_d8: [u8; 24],
    /// Whether the component is enabled (`CWeaponComponentBase::m_Enabled`). The weapon calls the
    /// fire hooks of enabled components only.
    pub m_Enabled: bool,
    _field_f1: [u8; 7],
    /// The act (an event id) played at fire start, from the component's properties.
    pub m_FireStartAct: u32,
    /// The act played at fire stop.
    pub m_FireStopAct: u32,
    /// The act played when the weapon is equipped.
    pub m_EquipAct: u32,
    /// The gun whose fire plays the acts, or
    /// [`ANY_WEAPON_DATA_INDEX`](crate::equipment::weapon::DoActWeaponComponent::ANY_WEAPON_DATA_INDEX).
    pub m_WeaponDataIndex: i32,
    /// Whether the fire acts play only when the gun's fire-point cursor
    /// ([`WeaponData::m_InternalFirePositionCurrentIndex`](crate::equipment::weapon::WeaponData::m_InternalFirePositionCurrentIndex)) is even.
    pub m_ActivateOnEvenInternalFirePoint: bool,
    /// Whether the fire acts play only when the gun's fire-point cursor is odd.
    pub m_ActivateOnOddInternalFirePoint: bool,
    /// Whether the acts go to the guns' own animated models rather than the holder.
    pub m_SendActToWeapon: bool,
    /// Whether the fire acts play only while the owner's magazine holds more than the round being
    /// fired.
    pub m_OnlyIfAmmoLeft: bool,
    _field_10c: [u8; 4],
}
fn _DoActWeaponComponent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x110], DoActWeaponComponent>([0u8; 0x110]);
    }
    unreachable!()
}
impl DoActWeaponComponent {
    pub const TYPE_ID_ADDRESS: usize = 0x140913620;
    /// `CDoActWeaponComponent::TYPE_ID`: the run-time type id of the class, for
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const Fire_Start_ADDRESS: usize = 0x140928A30;
    /// Plays [`m_FireStartAct`](crate::equipment::weapon::DoActWeaponComponent::m_FireStartAct) when
    /// [`ShouldActActivate`](crate::equipment::weapon::DoActWeaponComponent::ShouldActActivate) allows it for `weapon_data` (the component's
    /// fire-start virtual, which [`WeaponBase::Fire_StartBase`](crate::equipment::weapon::WeaponBase::Fire_StartBase) calls for each shot).
    pub unsafe fn Fire_Start(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Start_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const Fire_Stop_ADDRESS: usize = 0x140928BF0;
    /// Plays [`m_FireStopAct`](crate::equipment::weapon::DoActWeaponComponent::m_FireStopAct) when
    /// [`ShouldActActivate`](crate::equipment::weapon::DoActWeaponComponent::ShouldActActivate) allows it for `weapon_data` (the component's
    /// fire-stop virtual).
    pub unsafe fn Fire_Stop(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Stop_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const OnEquip_ADDRESS: usize = 0x140928DB0;
    /// Plays [`m_EquipAct`](crate::equipment::weapon::DoActWeaponComponent::m_EquipAct) when the weapon is equipped by `character`.
    pub unsafe fn OnEquip(
        &mut self,
        character: *mut crate::character::character::Character,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
            ) = ::std::mem::transmute(Self::OnEquip_ADDRESS);
            f(self as *mut Self as _, character)
        }
    }
    pub const ShouldActActivate_ADDRESS: usize = 0x1408BF200;
    /// Whether a fire act plays for `weapon_data`: false under
    /// [`m_OnlyIfAmmoLeft`](crate::equipment::weapon::DoActWeaponComponent::m_OnlyIfAmmoLeft) with the owner's magazine at one round or
    /// fewer; otherwise, with a [`m_WeaponDataIndex`](crate::equipment::weapon::DoActWeaponComponent::m_WeaponDataIndex), whether the gun is
    /// that one; otherwise the fire-point parity filters, and true without them.
    pub unsafe fn ShouldActActivate(
        &self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) -> bool = ::std::mem::transmute(Self::ShouldActActivate_ADDRESS);
            f(self as *const Self as _, weapon_data)
        }
    }
    pub const SetProperties_ADDRESS: usize = 0x140913700;
    /// Reads the component's properties: the three acts (resolved through the event-id symbol
    /// table), the gun index, the parity and routing flags, and the skeletal object.
    pub unsafe fn SetProperties(
        &mut self,
        properties: *const ::std::ffi::c_void,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                properties: *const ::std::ffi::c_void,
                context: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::SetProperties_ADDRESS);
            f(self as *mut Self as _, properties, context)
        }
    }
}
impl DoActWeaponComponent {
    /// The [`m_WeaponDataIndex`](crate::equipment::weapon::DoActWeaponComponent::m_WeaponDataIndex) that matches every gun.
    pub const ANY_WEAPON_DATA_INDEX: i32 = -1;
}
impl std::convert::AsRef<DoActWeaponComponent> for DoActWeaponComponent {
    fn as_ref(&self) -> &DoActWeaponComponent {
        self
    }
}
impl std::convert::AsMut<DoActWeaponComponent> for DoActWeaponComponent {
    fn as_mut(&mut self) -> &mut DoActWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
/// One muzzle fire point of a weapon model (`CWeaponData::SInternalFireInformation`): the muzzle
/// transform relative to the weapon, the muzzle-flash effect bound to it, and the time since it
/// last fired.
pub struct InternalFireInformation {
    /// The fire point's transform in weapon space. The shot origin is this matrix composed with
    /// the weapon's world matrix.
    pub m_Position: crate::types::math::Matrix4,
    /// The attached fire effect played at this fire point.
    pub m_FireEffect: *mut ::std::ffi::c_void,
    /// Seconds since this fire point last fired; reset to zero when the fire point is used.
    pub m_TimeSinceLastFire: f32,
    _field_4c: [u8; 4],
}
fn _InternalFireInformation_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], InternalFireInformation>([0u8; 0x50]);
    }
    unreachable!()
}
impl InternalFireInformation {}
impl std::convert::AsRef<InternalFireInformation> for InternalFireInformation {
    fn as_ref(&self) -> &InternalFireInformation {
        self
    }
}
impl std::convert::AsMut<InternalFireInformation> for InternalFireInformation {
    fn as_mut(&mut self) -> &mut InternalFireInformation {
        self
    }
}
#[repr(C, align(8))]
/// `CModelAttachementWeaponComponent`: a weapon component that draws a separate model part of a
/// gun, such as a magazine, at a fixed offset from the gun or on one of the holder's bones. The
/// barrel recoil and barrel spin components (`CBarrelRecoilWeaponComponent`,
/// `CBarrelSpinWeaponComponent`) derive from it: their attachment placement runs this one's and
/// then moves the part, and their vtables share its render update.
///
/// The part keeps its own pair of cached world matrices, like a gun's
/// ([`WeaponData::m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) and [`WeaponData::m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1)):
/// [`UpdateAttachmentTransforms`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdateAttachmentTransforms) places it in the simulation,
/// and [`UpdateRender`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdateRender) interpolates between the two and pushes the result to
/// its own model instance. Only some members are mapped, at their offsets from the start of the
/// component.
pub struct ModelAttachementWeaponComponent {
    vftable: *const crate::equipment::weapon::ModelAttachementWeaponComponentVftable,
    _field_8: [u8; 200],
    /// The weapon the component belongs to (`CWeaponComponentBase::m_Owner`).
    pub m_Owner: *mut crate::equipment::weapon::WeaponBase,
    /// The component's kind (`CWeaponComponentBase::m_Type`):
    /// [`E_WEAPON_COMPONENT_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponComponentType::E_WEAPON_COMPONENT_MODEL_ATTACHEMENT)
    /// for this class, and the derived kind for the barrel recoil and barrel spin parts.
    pub m_Type: crate::equipment::weapon::WeaponComponentType,
    _field_dc: [u8; 20],
    /// Whether the part is enabled (`CWeaponComponentBase::m_Enabled`), and so shown; written by
    /// [`Enable`](crate::equipment::weapon::ModelAttachementWeaponComponent::Enable).
    pub m_Enabled: bool,
    _field_f1: [u8; 7],
    /// The index into the owner's [`m_lWeaponData`](crate::equipment::weapon::WeaponBase::m_lWeaponData) of the gun the part
    /// belongs to.
    pub m_WeaponDataIndex: u32,
    /// The hash of the part's name (such as [`MAGAZINE_NAME_HASH`](crate::equipment::weapon::ModelAttachementWeaponComponent::MAGAZINE_NAME_HASH)),
    /// from the component's properties. The magazine drop and the enable-model message find a
    /// part by it.
    pub m_NameHash: u32,
    /// Whether the next placement skips the render interpolation: when set,
    /// [`UpdateAttachmentTransforms`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdateAttachmentTransforms) copies
    /// [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix) into
    /// [`m_WorldMatrixSaved`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrixSaved) as well. Set through
    /// [`WeaponBase::SkipRenderInterpolationForOneFrame`](crate::equipment::weapon::WeaponBase::SkipRenderInterpolationForOneFrame) and cleared by
    /// [`UpdatePostFinalizePose`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdatePostFinalizePose).
    pub m_SkipRenderInterpolation: bool,
    _field_101: [u8; 3],
    /// The snow cover written by the attachment placement, which the render update hands the
    /// model's environmental metadata.
    pub m_SnowFactor: f32,
    /// The wetness written by the attachment placement, which the render update hands the model's
    /// environmental metadata while the snow cover is zero.
    pub m_WaterFactor: f32,
    /// The holder's bone (an `NBone::ESafeBoneIndex`) the part rides instead of its gun, or
    /// [`NO_CHARACTER_BONE`](crate::equipment::weapon::ModelAttachementWeaponComponent::NO_CHARACTER_BONE). Set by
    /// [`AttachToCharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::AttachToCharacterBone), which the animation's
    /// model-attachment bone task (`NWeaponModelAttachementSetBoneTask`) calls inside the holder's
    /// pose-finalization pass, and reset by
    /// [`UpdatePostFinalizePose`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdatePostFinalizePose) at the end of that pass, so it is
    /// set only between the task and the end of the pass that placed the part.
    pub m_CharacterBone: u32,
    /// The part's transform relative to its gun.
    pub m_Offset: crate::types::math::Matrix4,
    /// The part's world matrix as of the current placement:
    /// [`m_Offset`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_Offset) composed with the gun's
    /// [`GetWorldMatrixT1`](crate::equipment::weapon::WeaponData::GetWorldMatrixT1) and orthonormalized, or the holder's bone
    /// while [`m_CharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_CharacterBone) names one.
    pub m_WorldMatrix: crate::types::math::Matrix4,
    /// The part's world matrix as of the previous placement, which the render update interpolates
    /// from toward [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix).
    pub m_WorldMatrixSaved: crate::types::math::Matrix4,
    /// The part's model instance (an `NModelSystem::CModelInstance`), or null.
    pub m_BaseModelInstance: *mut ::std::ffi::c_void,
}
fn _ModelAttachementWeaponComponent_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x1D8],
            ModelAttachementWeaponComponent,
        >([0u8; 0x1D8]);
    }
    unreachable!()
}
impl ModelAttachementWeaponComponent {
    pub fn vftable(
        &self,
    ) -> *const crate::equipment::weapon::ModelAttachementWeaponComponentVftable {
        self.vftable
            as *const crate::equipment::weapon::ModelAttachementWeaponComponentVftable
    }
    pub const TYPE_ID_ADDRESS: usize = 0x140851310;
    /// `CModelAttachementWeaponComponent::TYPE_ID`: the run-time type id of the class, for
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x14092EB80;
    /// Places the part (the component's override of the `UpdateAttachmentTransforms` weapon
    /// component virtual, which the weapon's attachment placement calls for each component). It
    /// stores `snow_factor` and `water_factor`; for the advancing placement (`transform_type` 2)
    /// it first copies [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix) into
    /// [`m_WorldMatrixSaved`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrixSaved). It then writes
    /// [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix) from the gun's world matrix and
    /// [`m_Offset`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_Offset), or from the holder's bone while
    /// [`m_CharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_CharacterBone) names one. While
    /// [`m_SkipRenderInterpolation`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_SkipRenderInterpolation) is set, and for the initial
    /// placement (`transform_type` 0), it copies the new matrix into
    /// [`m_WorldMatrixSaved`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrixSaved) as well.
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        snow_factor: f32,
        water_factor: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                snow_factor: f32,
                water_factor: f32,
                transform_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(self as *mut Self as _, snow_factor, water_factor, transform_type)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1408E09F0;
    /// The render-phase update (the `CGameObject::UpdateRender` virtual), called for each of a
    /// weapon's components from the weapon's render update ([`WeaponBase::UpdateRender`](crate::equipment::weapon::WeaponBase::UpdateRender)). It
    /// interpolates [`m_WorldMatrixSaved`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrixSaved) toward
    /// [`m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix) by the render context's interpolation factor (the
    /// engine's `CTransform::Lerp`), scales the result by the gun's holstered scale while the
    /// owner is [`E_WS_HOLSTERED`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED), pushes it to
    /// [`m_BaseModelInstance`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_BaseModelInstance) with
    /// [`ModelInstance::UpdateMatrix`](crate::graphics_engine::model_instance::ModelInstance::UpdateMatrix),
    /// and hands the model the snow and wetness. The disassembler mislabels it
    /// `NInteractionGraphExitTasks::NAnimationDrivenOnlyMotion::Update`, and puts this function's
    /// name on a function that reads a gun's ([`WeaponData`](crate::equipment::weapon::WeaponData)) fields instead.
    pub unsafe fn UpdateRender(
        &mut self,
        context: *const crate::game::GameObjectRenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::game::GameObjectRenderContext,
            ) = ::std::mem::transmute(Self::UpdateRender_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const AttachToCharacterBone_ADDRESS: usize = 0x1408BFDA0;
    /// Rides the part on the holder's bone `bone` (an `NBone::ESafeBoneIndex`) for the rest of the
    /// pose-finalization pass: sets [`m_CharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_CharacterBone).
    pub unsafe fn AttachToCharacterBone(&mut self, bone: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, bone: u32) = ::std::mem::transmute(
                Self::AttachToCharacterBone_ADDRESS,
            );
            f(self as *mut Self as _, bone)
        }
    }
    pub const UpdatePostFinalizePose_ADDRESS: usize = 0x1408BFD80;
    /// The component's post-pose-finalization hook, run from [`WeaponBase::UpdatePostFinalizePose`](crate::equipment::weapon::WeaponBase::UpdatePostFinalizePose)
    /// for an enabled component: clears [`m_SkipRenderInterpolation`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_SkipRenderInterpolation)
    /// and resets [`m_CharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_CharacterBone) to
    /// [`NO_CHARACTER_BONE`](crate::equipment::weapon::ModelAttachementWeaponComponent::NO_CHARACTER_BONE). The disassembler leaves it unnamed.
    pub unsafe fn UpdatePostFinalizePose(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdatePostFinalizePose_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    /// The component base's [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType).
    pub unsafe fn IsType(&self, type_id: *const u32) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsType).read();
            f(self as *const Self as _, type_id)
        }
    }
    /// Enables or disables the part: stores [`m_Enabled`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_Enabled) and, when the
    /// value changes, shows or hides the part's model through
    /// [`SetVisible`](crate::equipment::weapon::ModelAttachementWeaponComponent::SetVisible). [`WeaponData::ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) hides the
    /// magazine part with it, and the
    /// [`TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT)
    /// message shows it again.
    pub unsafe fn Enable(&mut self, enable: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).Enable).read();
            f(self as *mut Self as _, enable)
        }
    }
    /// Shows or hides the part's model.
    pub unsafe fn SetVisible(&mut self, visible: bool, p2: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetVisible).read();
            f(self as *mut Self as _, visible, p2)
        }
    }
}
impl ModelAttachementWeaponComponent {
    /// The [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) of the UPU-210's `model.cartridge_empty` part: the
    /// hash of the name.
    pub const CARTRIDGE_EMPTY_NAME_HASH: u32 = 854643837;
    /// The [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) of the UPU-210's `model.cartridge_full` part: the
    /// hash of the name.
    pub const CARTRIDGE_FULL_NAME_HASH: u32 = 3065889217;
    /// The [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) of the `model.magazine` part, a gun's separate
    /// magazine model: the hash of the name. The reload clips' throw-away-magazine and
    /// enable-model messages carry it; the weapon code does not.
    pub const MAGAZINE_NAME_HASH: u32 = 2122392104;
    /// The value of [`m_CharacterBone`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_CharacterBone) while the part rides its gun rather
    /// than a bone of the holder.
    pub const NO_CHARACTER_BONE: u32 = 4294967295;
    /// The [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) of the `model.rocket_pod` part, the Hydra's rocket
    /// pod: the hash of the name.
    pub const ROCKET_POD_NAME_HASH: u32 = 3997502386;
    /// The [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) of the `model.rpg` part, the rocket model of a rocket
    /// launcher and the shell of the mortar: the hash of the name.
    pub const RPG_NAME_HASH: u32 = 3475302103;
    /// The class's vtable, the value of the vtable pointer of a component of exactly this class.
    /// A barrel recoil part carries [`BarrelRecoilWeaponComponent::VFTABLE`](crate::equipment::weapon::BarrelRecoilWeaponComponent::VFTABLE) and a barrel spin
    /// part [`BarrelSpinWeaponComponent::VFTABLE`](crate::equipment::weapon::BarrelSpinWeaponComponent::VFTABLE) instead.
    pub const VFTABLE: u64 = 5406335208;
}
impl std::convert::AsRef<ModelAttachementWeaponComponent>
for ModelAttachementWeaponComponent {
    fn as_ref(&self) -> &ModelAttachementWeaponComponent {
        self
    }
}
impl std::convert::AsMut<ModelAttachementWeaponComponent>
for ModelAttachementWeaponComponent {
    fn as_mut(&mut self) -> &mut ModelAttachementWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
pub struct ModelAttachementWeaponComponentVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    /// The component base's [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType).
    pub IsType: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::ModelAttachementWeaponComponent,
        type_id: *const u32,
    ) -> bool,
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_25: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_26: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_27: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_28: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_29: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_30: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_31: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_33: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_34: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_35: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_36: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    /// Enables or disables the part: stores [`m_Enabled`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_Enabled) and, when the
    /// value changes, shows or hides the part's model through
    /// [`SetVisible`](crate::equipment::weapon::ModelAttachementWeaponComponent::SetVisible). [`WeaponData::ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) hides the
    /// magazine part with it, and the
    /// [`TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT)
    /// message shows it again.
    pub Enable: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
        enable: bool,
    ),
    _vfunc_39: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_40: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_41: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_42: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_43: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_44: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_45: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_46: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_47: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_48: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_49: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_50: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_51: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_52: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_53: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    _vfunc_54: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    ),
    /// Shows or hides the part's model.
    pub SetVisible: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
        visible: bool,
        p2: bool,
    ),
}
fn _ModelAttachementWeaponComponentVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<
            [u8; 0x1C0],
            ModelAttachementWeaponComponentVftable,
        >([0u8; 0x1C0]);
    }
    unreachable!()
}
impl ModelAttachementWeaponComponentVftable {}
impl std::convert::AsRef<ModelAttachementWeaponComponentVftable>
for ModelAttachementWeaponComponentVftable {
    fn as_ref(&self) -> &ModelAttachementWeaponComponentVftable {
        self
    }
}
impl std::convert::AsMut<ModelAttachementWeaponComponentVftable>
for ModelAttachementWeaponComponentVftable {
    fn as_mut(&mut self) -> &mut ModelAttachementWeaponComponentVftable {
        self
    }
}
#[repr(C, align(8))]
/// The weapon ballistics helpers (`NWeaponUtil`).
pub struct NWeaponUtil {}
impl NWeaponUtil {
    pub const Trajectory_GetAngleToHitPosition_ADDRESS: usize = 0x140959D70;
    /// Solves the launch pitch that carries a projectile of speed `velocity` from `start_pos` to
    /// `end_pos` under `gravity_factor` times world gravity, writing the high and low solutions.
    /// Returns whether a solution exists. Only the bullet weapons' fire path calls it.
    pub unsafe fn Trajectory_GetAngleToHitPosition(
        start_pos: *const crate::types::math::Vector3,
        end_pos: *const crate::types::math::Vector3,
        velocity: f32,
        high_angle: *mut f32,
        low_angle: *mut f32,
        gravity_factor: f32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                start_pos: *const crate::types::math::Vector3,
                end_pos: *const crate::types::math::Vector3,
                velocity: f32,
                high_angle: *mut f32,
                low_angle: *mut f32,
                gravity_factor: f32,
            ) -> bool = ::std::mem::transmute(
                Self::Trajectory_GetAngleToHitPosition_ADDRESS,
            );
            f(start_pos, end_pos, velocity, high_angle, low_angle, gravity_factor)
        }
    }
    pub const Trajectory_GetVelocityToHitPosition_ADDRESS: usize = 0x140959E90;
    /// Solves the launch velocity that carries a projectile from `start_pos` to `end_pos` in
    /// `time` seconds under `gravity_factor` times world gravity.
    pub unsafe fn Trajectory_GetVelocityToHitPosition(
        result: *mut crate::types::math::Vector3,
        start_pos: *const crate::types::math::Vector3,
        end_pos: *const crate::types::math::Vector3,
        time: f32,
        gravity_factor: f32,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                result: *mut crate::types::math::Vector3,
                start_pos: *const crate::types::math::Vector3,
                end_pos: *const crate::types::math::Vector3,
                time: f32,
                gravity_factor: f32,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::Trajectory_GetVelocityToHitPosition_ADDRESS,
            );
            f(result, start_pos, end_pos, time, gravity_factor)
        }
    }
}
impl std::convert::AsRef<NWeaponUtil> for NWeaponUtil {
    fn as_ref(&self) -> &NWeaponUtil {
        self
    }
}
impl std::convert::AsMut<NWeaponUtil> for NWeaponUtil {
    fn as_mut(&mut self) -> &mut NWeaponUtil {
        self
    }
}
#[repr(C, align(8))]
/// `CPlantedExplosiveWeapon`: the C4 weapon, a [`WeaponBase`](crate::equipment::weapon::WeaponBase) held in the
/// [`E_SLOT_PLANTED_EXPLOSIVE`](crate::character::inventory::WeaponSlot::E_SLOT_PLANTED_EXPLOSIVE) slot.
/// It owns the charges the character has placed and their remote detonation.
///
/// Placing a charge runs through the planting mode, which equipping the weapon enters
/// ([`m_IsInPlantingMode`](crate::equipment::weapon::PlantedExplosiveWeapon::m_IsInPlantingMode)): an animation track message primes a charge
/// into [`m_PrimedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_PrimedExplosive) with
/// [`PrimeExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::PrimeExplosive), the fire input places it at the aimed surface
/// (`CPlantedExplosive::PlaceExplosiveAtPosition`, outside a parachute) and queues the throw act,
/// and a later track message calls [`Fire_Projectile`](crate::equipment::weapon::PlantedExplosiveWeapon::Fire_Projectile), which makes the
/// charge live. The weapon refuses to place a charge once
/// [`NumActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::NumActiveExplosives) reaches
/// [`MaxActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::MaxActiveExplosives); placed charges do not consume an inventory
/// stock.
///
/// Detonation does not need the planting mode: the inventory runs the weapon's
/// [`UpdateFromUserPostSim_Serial`](crate::equipment::weapon::PlantedExplosiveWeapon::UpdateFromUserPostSim_Serial) every frame whatever is
/// wielded, and it starts a detonation from the character's detonate input. Only some members
/// are mapped, at their offsets from the start of the weapon.
pub struct PlantedExplosiveWeapon {
    _field_0: [u8; 1848],
    /// The primed charge: the live [`PlantedExplosive`](crate::equipment::explosive::PlantedExplosive)
    /// spawned by [`PrimeExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::PrimeExplosive) and not yet placed, or null. `px` is
    /// the charge's `CGameObject` subobject
    /// ([`Explosive::GAME_OBJECT_BASE_OFFSET`](crate::equipment::explosive::Explosive::GAME_OBJECT_BASE_OFFSET)).
    /// [`Fire_Projectile`](crate::equipment::weapon::PlantedExplosiveWeapon::Fire_Projectile) moves it to
    /// [`m_LastPlantedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_LastPlantedExplosive) and clears it;
    /// [`DespawnPrimedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::DespawnPrimedExplosive) removes the charge and clears it.
    pub m_PrimedExplosive: crate::types::shared_ptr::WeakPtrGameObject,
    /// The charge [`Fire_Projectile`](crate::equipment::weapon::PlantedExplosiveWeapon::Fire_Projectile) made live most recently.
    pub m_LastPlantedExplosive: crate::types::shared_ptr::WeakPtrGameObject,
    /// The most charges that may be live at once, at each explosives-capacity upgrade level;
    /// [`MaxActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::MaxActiveExplosives) picks the entry for the player's
    /// upgrades.
    pub m_MaxActiveExplosivesAtLevel: [i32; 3],
    _field_764: [u8; 164],
    /// Whether the weapon is in the planting mode: set when the weapon is equipped, and cleared by
    /// the weapon's `StopUsing`, which also despawns a primed charge.
    pub m_IsInPlantingMode: bool,
    _field_809: [u8; 2],
    /// Set when [`UpdateFromUserPostSim_Serial`](crate::equipment::weapon::PlantedExplosiveWeapon::UpdateFromUserPostSim_Serial) starts a
    /// detonation, and cleared by it once the character's detonate input
    /// ([`E_INPUT_DETONATE_EXPLOSIVES`](crate::character::character::WeaponInput::E_INPUT_DETONATE_EXPLOSIVES))
    /// is zero, so a held input detonates once.
    pub m_AlreadyDetonatedOnThisInput: bool,
    _field_80c: [u8; 4],
}
fn _PlantedExplosiveWeapon_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x810], PlantedExplosiveWeapon>([0u8; 0x810]);
    }
    unreachable!()
}
impl PlantedExplosiveWeapon {
    pub const UpdateFromUserPostSim_Serial_ADDRESS: usize = 0x14098EBE0;
    /// The weapon's serial per-frame update from its user (its override of
    /// [`WeaponBase::UpdateFromUserPostSim_Serial`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Serial)). The inventory runs it for the planted
    /// explosive slot's weapon every frame, wielded or not
    /// ([`Inventory::UpdateWeapons_Serial`](crate::character::inventory::Inventory::UpdateWeapons_Serial)).
    ///
    /// Among the rest, it reads the holder's detonate input
    /// ([`Character::m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs) at
    /// [`E_INPUT_DETONATE_EXPLOSIVES`](crate::character::character::WeaponInput::E_INPUT_DETONATE_EXPLOSIVES))
    /// directly rather than through `input`. While the input is positive, the inventory allows
    /// triggering explosives
    /// ([`Inventory::GetAnimationAllowsTriggeringExplosives`](crate::character::inventory::Inventory::GetAnimationAllowsTriggeringExplosives)),
    /// the character is not already detonating
    /// ([`Character::IsDetonatingExplosives`](crate::character::character::Character::IsDetonatingExplosives)),
    /// and [`m_AlreadyDetonatedOnThisInput`](crate::equipment::weapon::PlantedExplosiveWeapon::m_AlreadyDetonatedOnThisInput) is clear, it
    /// queues the upper-body detonate act when the character can take it (cancelling a reload when
    /// it cannot), adds heat when charges are live, and sets
    /// [`m_AlreadyDetonatedOnThisInput`](crate::equipment::weapon::PlantedExplosiveWeapon::m_AlreadyDetonatedOnThisInput). The act's animation
    /// sends the track message that schedules the live charges for detonation. Outside the
    /// planting mode it neither primes nor despawns a charge.
    pub unsafe fn UpdateFromUserPostSim_Serial(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_Serial_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const PrimeExplosive_ADDRESS: usize = 0x140990A40;
    /// Primes a charge: puts the weapon in the
    /// [`E_WS_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_WIELDING) state when it is not (adding its models to
    /// the world) and hides it, then allocates a
    /// [`PlantedExplosive`](crate::equipment::explosive::PlantedExplosive) from the ammunition pool at
    /// the weapon's shot matrix ([`WeaponBase::GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix)), sets up its trail effect, adds it
    /// to the update, and stores it in [`m_PrimedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_PrimedExplosive). Nothing is
    /// spawned when the shot matrix is unavailable or the pool is full. It consumes nothing and
    /// does not check the planting mode; the planting mode's track message is its caller.
    pub unsafe fn PrimeExplosive(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::PrimeExplosive_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const Fire_Projectile_ADDRESS: usize = 0x1409A01E0;
    /// Makes the primed charge live (its override of [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile)). A charge
    /// whose parent is a vehicle with an active bavarium shield is removed instead. Otherwise it
    /// calls [`PlantedExplosive::Activate`](crate::equipment::explosive::PlantedExplosive::Activate); under
    /// a deployed parachute ([`Character::IsUsingParachute`](crate::character::character::Character::IsUsingParachute))
    /// it also moves the charge to the right-hand bone (the transform setter and
    /// [`Explosive::SetPhysicsTransform`](crate::equipment::explosive::Explosive::SetPhysicsTransform)),
    /// drops it ([`PlantedExplosive::DropExplosive`](crate::equipment::explosive::PlantedExplosive::DropExplosive)),
    /// gives it the character's
    /// [`m_MeasuredWorldVelocity`](crate::character::character::Character::m_MeasuredWorldVelocity) with
    /// one metre per second downward ([`PlantedExplosive::SetVelocity`](crate::equipment::explosive::PlantedExplosive::SetVelocity)),
    /// and plays its trail ([`Explosive::PlayTrailEffect`](crate::equipment::explosive::Explosive::PlayTrailEffect)).
    /// It then adds heat, runs [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase), adds the
    /// charge to the live list (which the charge's detonation timer and HUD hint come from, and
    /// which ignores a charge beyond [`MaxActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::MaxActiveExplosives)), adds it
    /// to the update, and moves [`m_PrimedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_PrimedExplosive) to
    /// [`m_LastPlantedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_LastPlantedExplosive).
    pub unsafe fn Fire_Projectile(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Projectile_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const DespawnPrimedExplosive_ADDRESS: usize = 0x140980EA0;
    /// Removes the primed charge, if any, and clears
    /// [`m_PrimedExplosive`](crate::equipment::weapon::PlantedExplosiveWeapon::m_PrimedExplosive).
    pub unsafe fn DespawnPrimedExplosive(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DespawnPrimedExplosive_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x14099FF80;
    /// Places the weapon's attachments (its override of the `UpdateAttachmentTransforms` virtual):
    /// after the base placement, while the weapon has a user, it puts the primed charge's
    /// in-hand model on the user's right-hand bone
    /// ([`PlantedExplosive::SetInactiveModelMatrix`](crate::equipment::explosive::PlantedExplosive::SetInactiveModelMatrix)).
    /// The inventory's attachment pass calls it for every slot weapon
    /// ([`Inventory::UpdateAttachmentTransforms`](crate::character::inventory::Inventory::UpdateAttachmentTransforms)).
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        p2: f32,
        p3: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                p2: f32,
                p3: f32,
                transform_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(self as *mut Self as _, p2, p3, transform_type)
        }
    }
    pub const NumActiveExplosives_ADDRESS: usize = 0x140957F10;
    /// How many placed charges are live.
    pub unsafe fn NumActiveExplosives(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::NumActiveExplosives_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const MaxActiveExplosives_ADDRESS: usize = 0x140980F70;
    /// The most charges that may be live at once: for the player, the entry of
    /// [`m_MaxActiveExplosivesAtLevel`](crate::equipment::weapon::PlantedExplosiveWeapon::m_MaxActiveExplosivesAtLevel) for the highest
    /// explosives-capacity upgrade bought and enabled; for any other holder, the first entry.
    pub unsafe fn MaxActiveExplosives(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::MaxActiveExplosives_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const CanPlantExplosive_ADDRESS: usize = 0x1409902D0;
    /// Whether another charge may be placed:
    /// [`NumActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::NumActiveExplosives) is below
    /// [`MaxActiveExplosives`](crate::equipment::weapon::PlantedExplosiveWeapon::MaxActiveExplosives).
    pub unsafe fn CanPlantExplosive(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::CanPlantExplosive_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<PlantedExplosiveWeapon> for PlantedExplosiveWeapon {
    fn as_ref(&self) -> &PlantedExplosiveWeapon {
        self
    }
}
impl std::convert::AsMut<PlantedExplosiveWeapon> for PlantedExplosiveWeapon {
    fn as_mut(&mut self) -> &mut PlantedExplosiveWeapon {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a weapon orients its projectiles (`CWeaponBase::EProjectileFireDirectionMode`), read by
/// [`WeaponBase::GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix) from [`WeaponBase::m_ProjectileTempFireDirection`](crate::equipment::weapon::WeaponBase::m_ProjectileTempFireDirection), falling back to
/// [`WeaponBase::m_ProjectileFireDirection`](crate::equipment::weapon::WeaponBase::m_ProjectileFireDirection) while the temporary mode is unset.
pub enum ProjectileFireDirectionMode {
    E_PROJECTILE_FIRE_DIRECTION_UNSET = -1isize as _,
    E_PROJECTILE_FIRE_DIRECTION_AIM_POS = 0isize as _,
    E_PROJECTILE_FIRE_DIRECTION_BARREL = 1isize as _,
}
fn _ProjectileFireDirectionMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ProjectileFireDirectionMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `CRPGWeapon`: a rocket launcher, also the base of the mortar. Its loaded rocket is a model part
/// ([`m_RPGModel`](crate::equipment::weapon::RPGWeapon::m_RPGModel)) that the reload and the per-tick update show and hide by
/// [`m_AttachedState`](crate::equipment::weapon::RPGWeapon::m_AttachedState). Only some members are mapped, at their offsets
/// from the start of the weapon.
pub struct RPGWeapon {
    _field_0: [u8; 1872],
    /// The rocket model part, or null.
    pub m_RPGModel: *mut crate::equipment::weapon::ModelAttachementWeaponComponent,
    _field_758: [u8; 5],
    /// Whether the rocket model hides once the launcher is empty: the per-tick update hides a
    /// loaded rocket while the magazine is empty.
    pub m_RPGHideModelWhenOutOfAmmo: bool,
    _field_75e: [u8; 2],
    /// Where the rocket model is.
    pub m_AttachedState: crate::equipment::weapon::RocketAttachState,
    _field_764: [u8; 4],
}
fn _RPGWeapon_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x768], RPGWeapon>([0u8; 0x768]);
    }
    unreachable!()
}
impl RPGWeapon {
    pub const Reload_ADDRESS: usize = 0x140976AB0;
    /// The [`WeaponBase::Reload`](crate::equipment::weapon::WeaponBase::Reload) override: runs [`WeaponBase::ReloadBase`](crate::equipment::weapon::WeaponBase::ReloadBase) and, when it reloaded,
    /// puts the rocket in the launcher
    /// ([`E_ATTACHED_TO_WEAPON`](crate::equipment::weapon::RocketAttachState::E_ATTACHED_TO_WEAPON)) when the magazine is
    /// not empty, or otherwise hides and disables the rocket model and leaves it
    /// [`E_ATTACHED_TO_NONE`](crate::equipment::weapon::RocketAttachState::E_ATTACHED_TO_NONE).
    pub unsafe fn Reload(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::Reload_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const Fire_Start_ADDRESS: usize = 0x14099CB20;
    /// The [`WeaponBase::Fire_Start`](crate::equipment::weapon::WeaponBase::Fire_Start) override: fires through the missile weapon's fire start
    /// while the rocket is loaded, and otherwise asks the holder to reload
    /// ([`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)).
    pub unsafe fn Fire_Start(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Start_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const CheckIfReloadShouldBeTriggered_ADDRESS: usize = 0x14096D3F0;
    /// The [`WeaponBase::CheckIfReloadShouldBeTriggered`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggered) override: for a holder that is not a
    /// player, with the launcher wielded or dual-wielded and the rocket not loaded, it asks the
    /// holder to reload; otherwise it runs
    /// [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase).
    pub unsafe fn CheckIfReloadShouldBeTriggered(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        should_try_to_fire: *mut bool,
        was_trigger_pulled: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                should_try_to_fire: *mut bool,
                was_trigger_pulled: bool,
            ) = ::std::mem::transmute(Self::CheckIfReloadShouldBeTriggered_ADDRESS);
            f(
                self as *mut Self as _,
                weapon_data,
                should_try_to_fire,
                was_trigger_pulled,
            )
        }
    }
    pub const UpdateFromUserPostSim_Parallel_ADDRESS: usize = 0x1409A8410;
    /// The [`WeaponBase::UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel) override. After the missile weapon's
    /// update, a launcher with rounds in the magazine and the rocket not loaded shows and enables
    /// the rocket model and marks it loaded, so the rocket reappears on any refill; an empty
    /// launcher with [`m_RPGHideModelWhenOutOfAmmo`](crate::equipment::weapon::RPGWeapon::m_RPGHideModelWhenOutOfAmmo) hides
    /// its loaded rocket. A player's power-core launcher (`w210_powercore`) that runs out of
    /// ammunition without unlimited ammunition is dropped from the inventory.
    pub unsafe fn UpdateFromUserPostSim_Parallel(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
}
impl std::convert::AsRef<RPGWeapon> for RPGWeapon {
    fn as_ref(&self) -> &RPGWeapon {
        self
    }
}
impl std::convert::AsMut<RPGWeapon> for RPGWeapon {
    fn as_mut(&mut self) -> &mut RPGWeapon {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a weapon reloads (`CWeaponBase::EReloadType`), stored on the weapon as
/// [`WeaponBase::m_ReloadType`](crate::equipment::weapon::WeaponBase::m_ReloadType) and set from the weapon's `reload_type` property.
pub enum ReloadType {
    E_RELOAD_MAGAZINE = 0isize as _,
    E_RELOAD_BULLET = 1isize as _,
    E_RELOAD_NO_ANIMATION = 2isize as _,
}
fn _ReloadType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ReloadType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Where a rocket launcher's rocket model is (`CRPGWeapon::ERocketAttachState`), stored as
/// [`RPGWeapon::m_AttachedState`](crate::equipment::weapon::RPGWeapon::m_AttachedState).
pub enum RocketAttachState {
    E_ATTACHED_TO_NONE = 0isize as _,
    E_ATTACHED_TO_HAND = 1isize as _,
    E_ATTACHED_TO_WEAPON = 2isize as _,
}
fn _RocketAttachState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RocketAttachState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `CSniperWeapon`: a precision-aim rifle, a bullet weapon. A bolt-action rifle
/// ([`m_BoltAction`](crate::equipment::weapon::SniperWeapon::m_BoltAction)) locks after each shot
/// ([`m_HasFiredThisInput`](crate::equipment::weapon::SniperWeapon::m_HasFiredThisInput)) until the holder has cycled the bolt: the
/// rifle's per-tick update ([`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::SniperWeapon::UpdateFromUserPostSim_Parallel))
/// queues the bolt act once the trigger is released, the bolt state of the animation graph raises
/// [`SB_IS_RELOADING`](crate::state::StateBitFlag::SB_IS_RELOADING), and the update clears the lock while
/// the holder reloads. Only some members are mapped, at their offsets from the start of the
/// weapon.
pub struct SniperWeapon {
    _field_0: [u8; 1640],
    /// Whether the rifle is a bolt action, from the weapon's `BoltAction` property.
    pub m_BoltAction: bool,
    /// Whether the bolt cycle takes the local player out of precision aim and back, from the
    /// weapon's properties.
    pub m_BoltActionAutoZoomOut: bool,
    /// The bolt lock: set by [`Fire_Projectile`](crate::equipment::weapon::SniperWeapon::Fire_Projectile) after each shot of a bolt
    /// action for a player user ([`WeaponBase::m_IsUserPlayer`](crate::equipment::weapon::WeaponBase::m_IsUserPlayer)), and cleared only by
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::SniperWeapon::UpdateFromUserPostSim_Parallel) while the holder
    /// reloads ([`Character::IsReloading`](crate::character::character::Character::IsReloading)) or has
    /// no ammunition left at all. While it is set, [`Fire_Start`](crate::equipment::weapon::SniperWeapon::Fire_Start) and
    /// [`CheckIfReloadShouldBeTriggered`](crate::equipment::weapon::SniperWeapon::CheckIfReloadShouldBeTriggered) do nothing, so
    /// the rifle neither fires nor reloads on its own.
    pub m_HasFiredThisInput: bool,
    /// Whether the bolt cycle or a reload took the local player out of precision aim, to be
    /// restored once it ends.
    pub m_ShouldgoBackToPrecisionAim: bool,
    /// The delay before the bolt cycle while the holder stays in precision aim: loaded by
    /// [`Fire_Projectile`](crate::equipment::weapon::SniperWeapon::Fire_Projectile) at each shot and counted down by the per-tick
    /// update.
    pub m_BoltActionDelayer: f32,
}
fn _SniperWeapon_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x670], SniperWeapon>([0u8; 0x670]);
    }
    unreachable!()
}
impl SniperWeapon {
    pub const Fire_Projectile_ADDRESS: usize = 0x140950950;
    /// The [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) override: fires the bullet
    /// ([`BulletWeaponBase::Fire_Projectile`](crate::equipment::weapon::BulletWeaponBase::Fire_Projectile)) and then, for a bolt action with a player user,
    /// sets the bolt lock ([`m_HasFiredThisInput`](crate::equipment::weapon::SniperWeapon::m_HasFiredThisInput)) and loads
    /// [`m_BoltActionDelayer`](crate::equipment::weapon::SniperWeapon::m_BoltActionDelayer).
    pub unsafe fn Fire_Projectile(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Projectile_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const Fire_Start_ADDRESS: usize = 0x14096D490;
    /// The [`WeaponBase::Fire_Start`](crate::equipment::weapon::WeaponBase::Fire_Start) override: does nothing while the bolt lock
    /// ([`m_HasFiredThisInput`](crate::equipment::weapon::SniperWeapon::m_HasFiredThisInput)) is set, and otherwise runs the base
    /// fire start.
    pub unsafe fn Fire_Start(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_Start_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const CheckIfReloadShouldBeTriggered_ADDRESS: usize = 0x14096D4B0;
    /// The [`WeaponBase::CheckIfReloadShouldBeTriggered`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggered) override: does nothing while the bolt
    /// lock ([`m_HasFiredThisInput`](crate::equipment::weapon::SniperWeapon::m_HasFiredThisInput)) is set, and otherwise runs
    /// [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase).
    pub unsafe fn CheckIfReloadShouldBeTriggered(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        should_try_to_fire: *mut bool,
        was_trigger_pulled: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                should_try_to_fire: *mut bool,
                was_trigger_pulled: bool,
            ) = ::std::mem::transmute(Self::CheckIfReloadShouldBeTriggered_ADDRESS);
            f(
                self as *mut Self as _,
                weapon_data,
                should_try_to_fire,
                was_trigger_pulled,
            )
        }
    }
    pub const StartReloadSequence_ADDRESS: usize = 0x14097F030;
    /// The [`WeaponBase::StartReloadSequence`](crate::equipment::weapon::WeaponBase::StartReloadSequence) override: runs the base, and for the local player
    /// in precision aim leaves precision aim and sets
    /// [`m_ShouldgoBackToPrecisionAim`](crate::equipment::weapon::SniperWeapon::m_ShouldgoBackToPrecisionAim).
    pub unsafe fn StartReloadSequence(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::StartReloadSequence_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateFromUserPostSim_Parallel_ADDRESS: usize = 0x1409A6F20;
    /// The rifle's per-tick update from its user (its override of
    /// [`WeaponBase::UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel)). After the base update, while the holder is
    /// not reloading and has rounds in the magazine or the reserve, a set bolt lock on a bolt
    /// action waits for the trigger to be released and, in precision aim, for
    /// [`m_BoltActionDelayer`](crate::equipment::weapon::SniperWeapon::m_BoltActionDelayer) to run out. Then, with rounds left in the
    /// magazine, it optionally leaves precision aim
    /// ([`m_BoltActionAutoZoomOut`](crate::equipment::weapon::SniperWeapon::m_BoltActionAutoZoomOut)), plays each gun's bolt-action
    /// effect, and queues the bolt act (`UB_ACT_SNIPER_BOLT_ACTION`) through
    /// [`Character::QueueAct`](crate::character::character::Character::QueueAct), on every tick until the
    /// bolt state's [`SB_IS_RELOADING`](crate::state::StateBitFlag::SB_IS_RELOADING) rises; with an empty
    /// magazine it calls
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)
    /// instead. With no lock it returns the player to precision aim when
    /// [`m_ShouldgoBackToPrecisionAim`](crate::equipment::weapon::SniperWeapon::m_ShouldgoBackToPrecisionAim) asks. While the holder
    /// reloads or has no ammunition at all it clears the lock.
    pub unsafe fn UpdateFromUserPostSim_Parallel(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const SetProperties_ADDRESS: usize = 0x140957650;
    /// Reads the rifle's properties after the bullet weapon's: the bolt-action and auto-zoom-out
    /// flags. `game_object` is the weapon's `CGameObject` subobject
    /// ([`WeaponBase::GAME_OBJECT_BASE_OFFSET`](crate::equipment::weapon::WeaponBase::GAME_OBJECT_BASE_OFFSET)), not the start of the weapon. The disassembler
    /// mislabels it `CAntiAirWeapon::SetProperties`.
    pub unsafe fn SetProperties(
        game_object: *mut ::std::ffi::c_void,
        properties: *const ::std::ffi::c_void,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                game_object: *mut ::std::ffi::c_void,
                properties: *const ::std::ffi::c_void,
                context: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::SetProperties_ADDRESS);
            f(game_object, properties, context)
        }
    }
    pub const SetAimMode_ADDRESS: usize = 0x14097F120;
    /// The [`WeaponBase::SetAimMode`](crate::equipment::weapon::WeaponBase::SetAimMode) override: in [`AIM_PRECISION`](crate::aim::aim::AimMode::AIM_PRECISION) it
    /// shows the HUD sniper overlay and hides the holder, and in
    /// [`AIM_NORMAL`](crate::aim::aim::AimMode::AIM_NORMAL) it reverses both.
    pub unsafe fn SetAimMode(&mut self, aim_mode: crate::aim::aim::AimMode) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                aim_mode: crate::aim::aim::AimMode,
            ) = ::std::mem::transmute(Self::SetAimMode_ADDRESS);
            f(self as *mut Self as _, aim_mode)
        }
    }
}
impl std::convert::AsRef<SniperWeapon> for SniperWeapon {
    fn as_ref(&self) -> &SniperWeapon {
        self
    }
}
impl std::convert::AsMut<SniperWeapon> for SniperWeapon {
    fn as_mut(&mut self) -> &mut SniperWeapon {
        self
    }
}
#[repr(C, align(8))]
/// `CThrownGrenadeWeapon`: the hand grenade weapon, a [`WeaponBase`](crate::equipment::weapon::WeaponBase) held in the grenade slot. A
/// throw primes a grenade (spawning it live and consuming one grenade), keeps it at the weapon's
/// attachment while held, and releases it through [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile), which launches
/// it with [`Grenade::PropelToward`](crate::equipment::explosive::Grenade::PropelToward). The throw
/// animation's track messages drive both steps: one raises [`m_PrimeGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimeGrenade)
/// and a later one raises [`WeaponBase::m_ForceFireProjectile`](crate::equipment::weapon::WeaponBase::m_ForceFireProjectile). Only some members are mapped, at
/// their offsets from the start of the weapon.
pub struct ThrownGrenadeWeapon {
    _field_0: [u8; 1800],
    /// The primed grenade: the live [`Grenade`](crate::equipment::explosive::Grenade) spawned by
    /// [`PrimeGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::PrimeGrenade) and not yet thrown, or null. `px` is the grenade's
    /// `CGameObject` subobject
    /// ([`Explosive::GAME_OBJECT_BASE_OFFSET`](crate::equipment::explosive::Explosive::GAME_OBJECT_BASE_OFFSET)).
    /// [`Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) clears it when it launches the grenade.
    pub m_PrimedGrenade: crate::types::shared_ptr::WeakPtrGameObject,
    _field_718: [u8; 177],
    /// A pending prime request: [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::ThrownGrenadeWeapon::UpdateFromUserPostSim_Parallel)
    /// calls [`PrimeGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::PrimeGrenade) after the base weapon update when it is set, and
    /// clears it.
    pub m_PrimeGrenade: bool,
    _field_7ca: [u8; 6],
}
fn _ThrownGrenadeWeapon_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x7D0], ThrownGrenadeWeapon>([0u8; 0x7D0]);
    }
    unreachable!()
}
impl ThrownGrenadeWeapon {
    pub const UpdateFromUserPostSim_Parallel_ADDRESS: usize = 0x1409A7770;
    /// The weapon's per-tick update from its user (its override of
    /// [`WeaponBase::UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel)): runs
    /// [`UpdateFromUserPostSim_ParallelBase`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase),
    /// which consumes [`WeaponBase::m_ForceFireProjectile`](crate::equipment::weapon::WeaponBase::m_ForceFireProjectile) among the fire inputs, and then services
    /// [`m_PrimeGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimeGrenade). A throw released and a prime requested in the same
    /// call therefore release nothing and prime afterwards.
    pub unsafe fn UpdateFromUserPostSim_Parallel(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const PrimeGrenade_ADDRESS: usize = 0x140989470;
    /// Primes a grenade (`CreatePrimed<CGrenade>`): consumes one grenade, allocates a
    /// [`Grenade`](crate::equipment::explosive::Grenade) from the ammunition pool at the weapon's world
    /// matrix (returning null when the pool is full), plays the priming effect, stores it in
    /// [`m_PrimedGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimedGrenade), and adds it to the update. The grenade's
    /// detonation timer runs from this point
    /// ([`Explosive::m_DetonationTimer`](crate::equipment::explosive::Explosive::m_DetonationTimer)), so a
    /// grenade held for longer than its fuse explodes before it is thrown.
    pub unsafe fn PrimeGrenade(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> *mut ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
                Self::PrimeGrenade_ADDRESS,
            );
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const CancelPrimedGrenade_ADDRESS: usize = 0x14097FA50;
    /// Removes the primed grenade, if any, clears [`m_PrimedGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimedGrenade), and
    /// returns one grenade to the user's inventory. The disassembler mislabels it
    /// `rtti_cast<CThrownGrenadeWeapon,CWeaponBase>`; the function it labels
    /// `CThrownGrenadeWeapon::CancelPrimedGrenade` clears a hash table.
    pub unsafe fn CancelPrimedGrenade(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CancelPrimedGrenade_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x14099EB70;
    /// Places the weapon's attachments (its override of the `UpdateAttachmentTransforms` virtual):
    /// after the base placement it sets the primed grenade's transform to the weapon's world
    /// matrix, so a held grenade rides the weapon's attach bone.
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        p2: f32,
        p3: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                p2: f32,
                p3: f32,
                transform_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(self as *mut Self as _, p2, p3, transform_type)
        }
    }
}
impl std::convert::AsRef<ThrownGrenadeWeapon> for ThrownGrenadeWeapon {
    fn as_ref(&self) -> &ThrownGrenadeWeapon {
        self
    }
}
impl std::convert::AsMut<ThrownGrenadeWeapon> for ThrownGrenadeWeapon {
    fn as_mut(&mut self) -> &mut ThrownGrenadeWeapon {
        self
    }
}
#[repr(C, align(8))]
/// `CTurretWeaponComponent`: a weapon component that rotates a turret toward its weapon's target.
/// Its layout is unmapped.
pub struct TurretWeaponComponent {}
impl TurretWeaponComponent {
    pub const UpdateTurretAiming_ADDRESS: usize = 0x14093FEC0;
    /// The per-tick turret update, called from the component's pre-simulation update: selects the
    /// turret's target (faction and heat checks, characters, vehicles, and rigid objects) and
    /// rotates the turret toward the owning weapon's aim target at a rate-limited speed, so the
    /// barrel lags the aim point.
    pub unsafe fn UpdateTurretAiming(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateTurretAiming_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<TurretWeaponComponent> for TurretWeaponComponent {
    fn as_ref(&self) -> &TurretWeaponComponent {
        self
    }
}
impl std::convert::AsMut<TurretWeaponComponent> for TurretWeaponComponent {
    fn as_mut(&mut self) -> &mut TurretWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
/// `CWallPenetrationWeaponComponent`: the weapon component that detects a held weapon's barrel
/// penetrating a wall or a character. Only the owner is bound.
pub struct WallPenetrationWeaponComponent {
    _field_0: [u8; 208],
    /// The weapon the component belongs to (`CWeaponComponentBase::m_Owner`).
    pub m_Owner: *mut crate::equipment::weapon::WeaponBase,
}
fn _WallPenetrationWeaponComponent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xD8], WallPenetrationWeaponComponent>([0u8; 0xD8]);
    }
    unreachable!()
}
impl WallPenetrationWeaponComponent {
    pub const UpdateWeapon_Serial_ADDRESS: usize = 0x14092F930;
    /// The component's serial weapon update. It acts only while the owner's
    /// [`m_State`](crate::equipment::weapon::WeaponBase::m_State) is [`E_WS_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_WIELDING) or
    /// [`E_WS_DUAL_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_DUAL_WIELDING) and the owner is used by a player,
    /// and it is the only writer of the owner's
    /// [`m_IsWeaponPenetratingWallIfInAim`](crate::equipment::weapon::WeaponBase::m_IsWeaponPenetratingWallIfInAim),
    /// [`m_IsWeaponPenetratingCharacterIfInAim`](crate::equipment::weapon::WeaponBase::m_IsWeaponPenetratingCharacterIfInAim),
    /// and [`m_IsLeanOverFirePossible`](crate::equipment::weapon::WeaponBase::m_IsLeanOverFirePossible). While the holder
    /// is reeled in, drawing, holstering, or switching weapons, or is in a full-body pickup, it
    /// clears both penetration flags and sets the lean-over and
    /// [`m_AimingWeaponIsPossible`](crate::equipment::weapon::WeaponBase::m_AimingWeaponIsPossible) flags. Otherwise it
    /// collides a box, built from the holder's shoulders or hips toward the barrel, against the
    /// world. A character hit sets the character flag and clears the wall flag. A world hit sets
    /// the wall flag, clears lean-over, and clears `m_AimingWeaponIsPossible` unless the component
    /// allows aiming while penetrating (`m_CanAimWhenPenetrating`). A clear result lasting long
    /// enough clears both penetration flags and sets lean-over and `m_AimingWeaponIsPossible`
    /// again. The box follows the holder's animated pose, not the weapon's own transform.
    pub unsafe fn UpdateWeapon_Serial(
        &mut self,
        dt: f32,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        p3: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                p3: f32,
            ) = ::std::mem::transmute(Self::UpdateWeapon_Serial_ADDRESS);
            f(self as *mut Self as _, dt, weapon_data, p3)
        }
    }
}
impl std::convert::AsRef<WallPenetrationWeaponComponent>
for WallPenetrationWeaponComponent {
    fn as_ref(&self) -> &WallPenetrationWeaponComponent {
        self
    }
}
impl std::convert::AsMut<WallPenetrationWeaponComponent>
for WallPenetrationWeaponComponent {
    fn as_mut(&mut self) -> &mut WallPenetrationWeaponComponent {
        self
    }
}
#[repr(C, align(8))]
/// The animation-graph condition `C_IS_TRYING_TO_FIRE` (`CWeapoIsTryingToFireCondition`, the name
/// is misspelt in the game). Its layout is unmapped.
pub struct WeapoIsTryingToFireCondition {}
impl WeapoIsTryingToFireCondition {
    pub const Evaluate_ADDRESS: usize = 0x1407F3FF0;
    /// Evaluates the condition for the character owning `instance` (a
    /// `NAnimationSystem::CStateMachineInstance`). True when the character holds a selected weapon
    /// in a wielding, dual-wielding, or mounted state (or is planting an explosive) and a fire input
    /// is present that the animation allows: the fire input with shooting allowed, the
    /// precision-aim input with precision aim possible and allowed, or the plant input while
    /// planting.
    pub unsafe fn Evaluate(&self, instance: *const ::std::ffi::c_void) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                instance: *const ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::Evaluate_ADDRESS);
            f(self as *const Self as _, instance)
        }
    }
}
impl std::convert::AsRef<WeapoIsTryingToFireCondition> for WeapoIsTryingToFireCondition {
    fn as_ref(&self) -> &WeapoIsTryingToFireCondition {
        self
    }
}
impl std::convert::AsMut<WeapoIsTryingToFireCondition> for WeapoIsTryingToFireCondition {
    fn as_mut(&mut self) -> &mut WeapoIsTryingToFireCondition {
        self
    }
}
#[repr(C, align(8))]
/// The base class of every weapon (`CWeaponBase`, a `CPhysicsGameObject`): fire input and timing,
/// ammunition, recoil, heat, aim target, tuning, and the list of physical guns
/// ([`WeaponData`](crate::equipment::weapon::WeaponData)) it drives. The weapon embeds a `CGameObject` subobject after its vtable
/// pointer.
pub struct WeaponBase {
    vftable: *const crate::equipment::weapon::WeaponBaseVftable,
    _field_8: [u8; 96],
    /// A weak reference to the weapon itself (`m_WeakThis` of its `CGameObject` subobject),
    /// referring to that subobject. The spawn system's per-frame despawn ranking
    /// ([`SpawnSystem::Update`](crate::spawn_system::SpawnSystem::Update)) reads the strong count of its
    /// control block, and the weapon hands out references to itself from it (the HUD's pickup
    /// display, the pickup hint).
    pub m_WeakThis: crate::types::shared_ptr::WeakPtrGameObject,
    _field_78: [u8; 224],
    /// Whether the current shot is the first of a trigger pull; first shots use the tuning's
    /// first-shot recoil scale.
    pub m_IsFirstShot: bool,
    /// Whether the weapon is firing.
    pub m_IsFiring: bool,
    /// Whether the fire input currently requests a shot, after trigger sensitivity, semi-automatic
    /// gating, and forced fire.
    pub m_IsTryingToFire: bool,
    /// Whether the trigger input is past the tuning's pull threshold (with hysteresis once pulled).
    pub m_IsTriggerPulled: bool,
    /// Whether firing is cancelled until the trigger is released.
    pub m_IsFiringCancelled: bool,
    /// Whether the trigger input is above zero.
    pub m_IsFeatheringTrigger: bool,
    /// Whether the current trigger pull has already fired; semi-automatic weapons fire once per
    /// pull.
    pub m_FiredThisInput: bool,
    _field_15f: [u8; 1],
    /// The index into [`m_lWeaponData`](crate::equipment::weapon::WeaponBase::m_lWeaponData) of the gun that fires next, which
    /// [`GetFireFromPositionBase`](crate::equipment::weapon::WeaponBase::GetFireFromPositionBase) reads to pick the muzzle. The
    /// tail of [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase) (`MarkNextWeaponComponentForFire`,
    /// inlined in this build) advances it modulo the gun count after each shot.
    pub m_NextWeaponToFire: u8,
    /// The index of the gun that fired last, recorded before
    /// [`m_NextWeaponToFire`](crate::equipment::weapon::WeaponBase::m_NextWeaponToFire) advances.
    pub m_LastWeaponToFire: u8,
    _field_162: [u8; 1],
    /// Whether firing this weapon adds no heat (wanted level) to the user.
    pub m_NoHeatOnFiring: bool,
    _field_164: [u8; 4],
    /// The weapon's persistent projectile direction mode.
    pub m_ProjectileFireDirection: crate::equipment::weapon::ProjectileFireDirectionMode,
    /// A temporary override of [`m_ProjectileFireDirection`](crate::equipment::weapon::WeaponBase::m_ProjectileFireDirection);
    /// [`E_PROJECTILE_FIRE_DIRECTION_UNSET`](crate::equipment::weapon::ProjectileFireDirectionMode::E_PROJECTILE_FIRE_DIRECTION_UNSET)
    /// defers to it.
    pub m_ProjectileTempFireDirection: crate::equipment::weapon::ProjectileFireDirectionMode,
    _field_170: [u8; 8],
    /// A one-shot fire request: while set, the next
    /// [`UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) clears it and calls
    /// [`Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) once for each gun, whatever the fire input and the
    /// aim angle. Animation track messages raise it at the release point of a throw.
    pub m_ForceFireProjectile: bool,
    _field_179: [u8; 3],
    /// Seconds of forced fire remaining; the weapon fires regardless of input while positive.
    pub m_ForceFireTimer: f32,
    /// Shots of forced fire remaining; decremented once per
    /// [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase) call.
    pub m_ForceFireCount: i32,
    /// Seconds a blocked trigger pull stays buffered. A pull blocked by wall penetration or by the
    /// aim-angle check sets it to [`INPUT_BUFFER_TIME`](crate::equipment::weapon::WeaponBase::INPUT_BUFFER_TIME), so the shot fires
    /// if the block clears within that window; a shot resets it to zero.
    pub m_InputBufferTimer: f32,
    _field_188: [u8; 16],
    /// Seconds since the weapon last fired.
    pub m_TimeSinceLastFire: f32,
    _field_19c: [u8; 8],
    /// The radius of the spread ring [`GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix) offsets the aim point
    /// by, while [`m_ProjectileOffsetFireCount`](crate::equipment::weapon::WeaponBase::m_ProjectileOffsetFireCount) shots remain.
    pub m_ProjectileOffsetFromAimPos: f32,
    _field_1a8: [u8; 4],
    /// Shots remaining that use the spread ring; decremented once per shot.
    pub m_ProjectileOffsetFireCount: i32,
    _field_1b0: [u8; 103],
    /// Whether picking the weapon up gives the character a copy rather than the weapon itself.
    /// [`PickupStart`](crate::equipment::weapon::WeaponBase::PickupStart) then calls `CInventory::GivePlayerWeapon` with the
    /// weapon's spawn tag, and [`PickupItem`](crate::equipment::weapon::WeaponBase::PickupItem) puts nothing into the inventory:
    /// it only plays the pickup vocal and clears
    /// [`m_StoredNonMagazineAmmo`](crate::equipment::weapon::WeaponBase::m_StoredNonMagazineAmmo). The weapon's pickup handling
    /// leaves its pickup interaction enabled while it is set.
    pub m_InfinitePickup: bool,
    _field_218: [u8; 9],
    /// Whether the magazine is out of the weapon. Only the throw-away-magazine track message
    /// ([`TRACK_MESSAGE_THROW_AWAY_MAGAZINE`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_THROW_AWAY_MAGAZINE)) sets it,
    /// and only [`Reload`](crate::equipment::weapon::WeaponBase::Reload) clears it. While it is set the ammunition check
    /// ([`CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase)) clears
    /// the shot and calls
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)
    /// on every tick, the throw message does nothing, and the animation condition
    /// `CWeaponHasMagazineInsertedCondition` (through `CWeaponBase::WeaponHasMagazineInserted`)
    /// reads false.
    pub m_WeaponIsWithoutMagazine: bool,
    _field_222: [u8; 2],
    /// The weapon's kind of ammunition, set from the weapon's properties; it selects the reserve
    /// in the holder's [`Inventory::m_Ammunition`](crate::character::inventory::Inventory::m_Ammunition).
    pub m_AmmunitionType: crate::equipment::weapon::AmmunitionType,
    /// The rounds in the magazine. A shot consumes from it; a reload refills it from the holder's
    /// inventory stock of the weapon's ammunition type. The guns of a multi-gun weapon share it.
    /// Write it through [`SetMagazineAmmo`](crate::equipment::weapon::WeaponBase::SetMagazineAmmo), which also updates the HUD.
    pub m_MagazineAmmo: i32,
    /// The weapon's stored rounds outside the magazine (`CWeaponBase::GetStoredNonMagazineAmmo`).
    pub m_StoredNonMagazineAmmo: i32,
    /// How the weapon reloads, set from the weapon's `reload_type` property.
    pub m_ReloadType: crate::equipment::weapon::ReloadType,
    /// Whether an [`E_RELOAD_BULLET`](crate::equipment::weapon::ReloadType::E_RELOAD_BULLET) reload is to insert another
    /// round. [`StartReloadSequence`](crate::equipment::weapon::WeaponBase::StartReloadSequence) and each
    /// [`Reload`](crate::equipment::weapon::WeaponBase::Reload) that leaves the magazine short of full set it; `Reload` clears it
    /// once the magazine is full or the reserve is empty, and
    /// [`ReloadAnimationFinished`](crate::equipment::weapon::WeaponBase::ReloadAnimationFinished) clears it. While it is clear,
    /// the ammunition check cancels the holder's reload act for a weapon that does not reload by
    /// magazine.
    pub m_KeepReloading: bool,
    /// Whether an [`E_RELOAD_NO_ANIMATION`](crate::equipment::weapon::ReloadType::E_RELOAD_NO_ANIMATION) reload is timing:
    /// set by the ammunition check when it starts one, and cleared by [`Reload`](crate::equipment::weapon::WeaponBase::Reload) and
    /// [`ReloadAnimationFinished`](crate::equipment::weapon::WeaponBase::ReloadAnimationFinished). The ammunition check treats it
    /// like the holder's reloading state.
    pub m_IsReloading: bool,
    _field_236: [u8; 2],
    /// The magazine count below which the local player's idle weapon reloads on its own (the
    /// predicted reload of the ammunition check), set from the weapon's properties.
    pub m_PredictedReloadAmmunitionLimit: i32,
    /// The seconds without fire input after which the predicted reload may start; a negative
    /// value lets it start even where the holder does not allow predicted reloads.
    pub m_PredictedReloadTime: f32,
    /// The seconds left of an [`E_RELOAD_NO_ANIMATION`](crate::equipment::weapon::ReloadType::E_RELOAD_NO_ANIMATION)
    /// reload, counted down by [`UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) while
    /// [`m_IsReloading`](crate::equipment::weapon::WeaponBase::m_IsReloading) is set.
    pub m_ReloadTimeLeft: f32,
    _field_244: [u8; 16],
    /// The animation modifier id (`ava::idstring<NAnimationSystem::SModifierIdTag>`) for holding
    /// this weapon without aiming.
    /// [`Character::SetCharacterModifiers`](crate::character::character::Character::SetCharacterModifiers)
    /// sets it for the wielded weapon while the holder is not trying to aim.
    pub m_AnimationModifier: u32,
    /// The animation modifier id for aiming this weapon, set by
    /// [`Character::SetCharacterModifiers`](crate::character::character::Character::SetCharacterModifiers)
    /// in place of [`m_AnimationModifier`](crate::equipment::weapon::WeaponBase::m_AnimationModifier) while the holder is trying
    /// to aim.
    pub m_AimAnimationModifier: u32,
    /// The animation modifier id for having this weapon equipped, set alongside the held or aimed
    /// modifier every tick the weapon is wielded.
    pub m_EquippedAnimationModifier: u32,
    _field_260: [u8; 8],
    /// The accumulated scatter, raised by each shot up to the tuning's maximum and scaling the
    /// shot's scatter rotation.
    pub m_CurrentScatterValue: f32,
    /// The recoil pitch still to be recovered, raised by each shot.
    pub m_CurrentRecoilPitchValueToRecover: f32,
    /// The physical recoil impulse strength; a non-zero value defers a physical recoil after each
    /// shot.
    pub m_PhysicalRecoilStrength: f32,
    /// This tick's recoil yaw, radians: the base update
    /// ([`UpdateFromUserPostSim_ParallelBase`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase)) zeroes it
    /// at the start of every tick, and each shot ([`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase))
    /// kicks it left or right at random. The weapon camera modifier
    /// ([`WeaponCameraModifier::ProcessCameraContext`](crate::camera::weapon_camera_modifier::WeaponCameraModifier::ProcessCameraContext))
    /// turns the camera boom by it.
    pub m_RecoilYaw: f32,
    /// This tick's recoil pitch, radians: the base update zeroes it at the start of every tick and
    /// then, while [`m_CurrentRecoilPitchValueToRecover`](crate::equipment::weapon::WeaponBase::m_CurrentRecoilPitchValueToRecover)
    /// is positive, sets it to the negated share recovered this tick; each shot raises it by the
    /// shot's pitch kick. The weapon camera modifier pitches the camera boom by it.
    pub m_RecoilPitch: f32,
    _field_27c: [u8; 272],
    /// Whether the weapon overheats.
    pub m_CanBeOverheated: bool,
    /// Whether the weapon is overheated and cannot fire.
    pub m_IsOverheated: bool,
    _field_38e: [u8; 2],
    /// The heat level in `[0, 1]`; each shot adds heat and reaching 1 overheats the weapon.
    pub m_CurrentHeatValue: f32,
    _field_394: [u8; 100],
    /// Whether the weapon can be aimed at all; `CCharacter::IsAimingWeaponPossible` requires it.
    pub m_AimingWeaponIsPossible: bool,
    _field_3f9: [u8; 3],
    /// The weapon's aim target in world space, written each tick by
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel) from the point the
    /// holder passes in. [`GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix) aims every fire point of the
    /// weapon at it.
    pub m_AimTargetPosition: crate::types::math::Vector3,
    _field_408: [u8; 2],
    /// Whether aim assist stays on in precision aim. `CPlayer::SetAimMode` leaves the aim
    /// control's aim type untouched when entering precision aim with a weapon that sets this.
    pub m_IsAutoAimInPrecisionAimEnabled: bool,
    _field_40b: [u8; 21],
    /// The inventory slot the weapon is equipped in.
    pub m_EquipSlot: crate::character::inventory::WeaponSlot,
    /// The weapon's state.
    pub m_State: crate::equipment::weapon::WeaponState,
    _field_428: [u8; 4],
    /// The weapon's class, [`E_WEAPON_CLASS_NONE`](crate::equipment::weapon::WeaponClass::E_WEAPON_CLASS_NONE) until the
    /// weapon type's data sets it.
    pub m_Class: crate::equipment::weapon::WeaponClass,
    _field_430: [u8; 48],
    /// The hash of the weapon type's name, which identifies the weapon type (the damage messages
    /// carry it, and type-specific code compares it against named hashes).
    pub m_WeaponNameHash: u32,
    _field_464: [u8; 2],
    /// Whether precision aim is disabled for this weapon; the player's aim-input handling zeroes
    /// the aim timer while it is set.
    pub m_IsPrecisionAimDisabled: bool,
    /// Whether the weapon supports precision aim. The player's weapon handling
    /// (`CInventory::UpdateWeaponHandling`) enters precision aim on the precision-aim input only
    /// for a weapon that sets it.
    pub m_SupportsPrecisionAim: bool,
    _field_468: [u8; 20],
    /// Whether the weapon is registered for pre-simulation updates; the fire path sets it and
    /// registers the weapon when a deferred action is queued.
    pub m_RequiresPreSimUpdates: bool,
    _field_47d: [u8; 19],
    /// The weapon type's tuning, or null.
    pub m_Tuning: *const crate::equipment::weapon::WeaponTuning,
    _field_498: [u8; 96],
    /// The character using the weapon (a `boost::weak_ptr<CCharacter>`: object pointer and weak
    /// control block).
    pub m_User: [*mut ::std::ffi::c_void; 2],
    /// The object holding the weapon (`m_Holder`, a weak reference to its `CGameObject`
    /// subobject): a character or a vehicle. [`OnPickup`](crate::equipment::weapon::WeaponBase::OnPickup) sets it and
    /// [`OnDrop`](crate::equipment::weapon::WeaponBase::OnDrop) clears it. The weapon's placement
    /// ([`UpdateAttachmentTransforms`](crate::equipment::weapon::WeaponBase::UpdateAttachmentTransforms)) places its guns on the
    /// holder's bones.
    pub m_Holder: crate::types::shared_ptr::WeakPtrGameObject,
    _field_518: [u8; 1],
    /// Whether the weapon's user is the local player. The trigger's semi-automatic rule, the fire
    /// rumble ([`RumbleFire`](crate::equipment::weapon::WeaponBase::RumbleFire)), and the fire camera shake apply only while it is
    /// set.
    pub m_IsUserLocalPlayer: bool,
    /// Whether the weapon's user is a player, local or remote. The sniper rifle's bolt lock
    /// ([`SniperWeapon::m_HasFiredThisInput`](crate::equipment::weapon::SniperWeapon::m_HasFiredThisInput)) and the barrel recoil's empty-magazine lock-back
    /// apply only while it is set, and the rocket launcher's own reload request only while it is
    /// clear.
    pub m_IsUserPlayer: bool,
    _field_51b: [u8; 37],
    /// The weapon's components ([`WeaponComponentBase`](crate::equipment::weapon::WeaponComponentBase)): its model parts
    /// ([`ModelAttachementWeaponComponent`](crate::equipment::weapon::ModelAttachementWeaponComponent) and the barrel recoil and spin parts derived from it),
    /// wall penetration, turret, and the rest. An entry's
    /// [`m_Type`](crate::equipment::weapon::WeaponComponentBase::m_Type) names its class, and
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType) tests its class or a base of it.
    pub m_WeaponComponents: crate::types::std_vector::Vector<
        crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponComponentBase,
        >,
    >,
    /// Whether the weapon blocks firing while its barrel penetrates a wall in aim.
    pub m_IsWeaponPenetratingWallIfInAim: bool,
    /// Whether the weapon's barrel penetrates a character in aim;
    /// [`GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix) skips the near-muzzle barrel fallback while set.
    pub m_IsWeaponPenetratingCharacterIfInAim: bool,
    /// Whether the holder can lean over an obstruction to fire, which lifts the wall-penetration
    /// block.
    pub m_IsLeanOverFirePossible: bool,
    _field_563: [u8; 5],
    /// The weapon's physical guns, one [`WeaponData`](crate::equipment::weapon::WeaponData) per gun.
    pub m_lWeaponData: crate::equipment::weapon::WeaponDataArray,
}
fn _WeaponBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x588], WeaponBase>([0u8; 0x588]);
    }
    unreachable!()
}
impl WeaponBase {
    pub fn vftable(&self) -> *const crate::equipment::weapon::WeaponBaseVftable {
        self.vftable as *const crate::equipment::weapon::WeaponBaseVftable
    }
    pub const TYPE_ID_ADDRESS: usize = 0x14060F2F0;
    /// `CWeaponBase::TYPE_ID`: the run-time type id of `CWeaponBase`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use. A weapon is a
    /// physics game object of its own (not a rigid object): a weapon lying in the world has its own
    /// body, and a physics query that hits it reports the weapon.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const HasUnlimitedAmmo_ADDRESS: usize = 0x140964F90;
    /// Whether the weapon's rounds come without limit: true when the holder has
    /// [`m_UnlimitedAmmo`](crate::character::character::Character::m_UnlimitedAmmo), the holder is not a
    /// player, or the challenge-island game mode is running; otherwise the tuning's
    /// [`m_unlimited_ammo`](crate::equipment::weapon::WeaponTuning::m_unlimited_ammo). False without a holder.
    /// [`Reload`](crate::equipment::weapon::WeaponBase::Reload) then fills the magazine without drawing on the reserve, and
    /// [`CanReload`](crate::equipment::weapon::WeaponBase::CanReload) allows a reload with an empty reserve.
    pub unsafe fn HasUnlimitedAmmo(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::HasUnlimitedAmmo_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetMagazineAmmoBase_ADDRESS: usize = 0x14096E520;
    /// The base implementation of the [`SetMagazineAmmo`](crate::equipment::weapon::WeaponBase::SetMagazineAmmo) virtual.
    pub unsafe fn SetMagazineAmmoBase(&mut self, ammo: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, ammo: i32) = ::std::mem::transmute(
                Self::SetMagazineAmmoBase_ADDRESS,
            );
            f(self as *mut Self as _, ammo)
        }
    }
    pub const ReloadBase_ADDRESS: usize = 0x140976480;
    /// The base implementation of the [`Reload`](crate::equipment::weapon::WeaponBase::Reload) virtual, which the rocket
    /// launcher's and the mortar's overrides call.
    pub unsafe fn ReloadBase(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::ReloadBase_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HandleTrackMessage_ADDRESS: usize = 0x1409918B0;
    /// Handles a weapon message from an animation track, `data`, for `character` (the weapon's
    /// override of the weapon track-message virtual, which the rocket launcher, the mortar, the
    /// thrown grenade, the planted explosive, and the guitar case extend and then call). The
    /// holder's track-message handling
    /// ([`Character::HandleTrackMessage`](crate::character::character::Character::HandleTrackMessage))
    /// forwards the message, so it runs in the holder's serial post-physics pass on the game
    /// update thread. It dispatches on the message's name hash. The reload messages are
    /// [`TRACK_MESSAGE_THROW_AWAY_MAGAZINE`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_THROW_AWAY_MAGAZINE),
    /// [`TRACK_MESSAGE_RELOAD`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_RELOAD),
    /// [`TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT),
    /// [`TRACK_MESSAGE_START_RELOAD`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_START_RELOAD), and
    /// [`TRACK_MESSAGE_START_RELOAD_IF_EMPTY`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_START_RELOAD_IF_EMPTY). The
    /// others raise [`m_ForceFireProjectile`](crate::equipment::weapon::WeaponBase::m_ForceFireProjectile), complete a pickup
    /// ([`PickupItem`](crate::equipment::weapon::WeaponBase::PickupItem)), play an attached effect on a gun, show or hide the
    /// weapon, equip, unequip, drop, or stop using it, and detach a mounted gun.
    pub unsafe fn HandleTrackMessage(
        &mut self,
        data: *mut crate::animation::message::AnimationMessageData,
        character: *mut crate::character::character::Character,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut crate::animation::message::AnimationMessageData,
                character: *mut crate::character::character::Character,
            ) = ::std::mem::transmute(Self::HandleTrackMessage_ADDRESS);
            f(self as *mut Self as _, data, character)
        }
    }
    pub const PickupStart_ADDRESS: usize = 0x1409912B0;
    /// Starts picking the weapon up for `activator` (a character's `CGameObject` subobject; any
    /// other object is ignored). A hand grenade is taken at once: its stored ammunition goes to the
    /// inventory, the HUD shows the grenade pickup, and the weapon despawns. Another weapon enters
    /// [`E_WS_PICKING_UP`](crate::equipment::weapon::WeaponState::E_WS_PICKING_UP) and leaves the physics world; the weapon's
    /// pickup track message then completes the pickup through [`PickupItem`](crate::equipment::weapon::WeaponBase::PickupItem).
    /// A weapon with infinite pickups gives the character a copy instead. The weapon's pickup
    /// interaction calls it when it executes, and on activation for a weapon that
    /// `ShouldAutoPickup` takes without input.
    pub unsafe fn PickupStart(&mut self, activator: *mut crate::game::GameObject) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                activator: *mut crate::game::GameObject,
            ) = ::std::mem::transmute(Self::PickupStart_ADDRESS);
            f(self as *mut Self as _, activator)
        }
    }
    pub const PickupItem_ADDRESS: usize = 0x1409798C0;
    /// Completes a pickup for `activator` (a character's `CGameObject` subobject): unequips the
    /// wielded weapon, puts this one into the inventory and equips it, selects its slot, adds its
    /// stored ammunition to the inventory, and plays the pickup vocal. It does not refill the
    /// magazine. Putting the weapon into the inventory runs its pickup virtual (the holder is set,
    /// the state becomes holstered, which takes its bodies out of the physics world, and its pickup
    /// interaction is disabled) and its equip virtual (the state becomes wielding, or
    /// dual-wielding for the player's dual-wield slot), with no animation act. A weapon already in
    /// the slot is dropped or despawned by the inventory's add. A hand grenade is left to
    /// [`PickupStart`](crate::equipment::weapon::WeaponBase::PickupStart), and a weapon with
    /// [`m_InfinitePickup`](crate::equipment::weapon::WeaponBase::m_InfinitePickup) set is not put into the inventory at all; both
    /// still lose their stored ammunition. Called by the weapon's pickup track message, and
    /// directly by [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)
    /// for a swimming character.
    pub unsafe fn PickupItem(&mut self, activator: *mut crate::game::GameObject) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                activator: *mut crate::game::GameObject,
            ) = ::std::mem::transmute(Self::PickupItem_ADDRESS);
            f(self as *mut Self as _, activator)
        }
    }
    pub const StartUsing_ADDRESS: usize = 0x140982600;
    /// Starts `character` using the weapon: sets the user ([`m_User`](crate::equipment::weapon::WeaponBase::m_User)) and
    /// [`m_IsUserLocalPlayer`](crate::equipment::weapon::WeaponBase::m_IsUserLocalPlayer) and
    /// [`m_IsUserPlayer`](crate::equipment::weapon::WeaponBase::m_IsUserPlayer), adds the weapon's camera hint for the locally
    /// controlled character, and records the use in the statistics. A mounted gun is also put into
    /// the user's inventory. For the player it leaves precision aim when the weapon does not
    /// support it ([`m_SupportsPrecisionAim`](crate::equipment::weapon::WeaponBase::m_SupportsPrecisionAim)), and hands the
    /// player's aim mode to the weapon ([`SetAimMode`](crate::equipment::weapon::WeaponBase::SetAimMode)). It does not change the
    /// weapon's state. The equip virtual ([`OnEquip`](crate::equipment::weapon::WeaponBase::OnEquip)) calls it, and the inventory's
    /// add calls it for the hand grenade, the planted explosive, and the beacon, which are in use
    /// without being wielded.
    pub unsafe fn StartUsing(
        &mut self,
        character: *mut crate::character::character::Character,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
            ) = ::std::mem::transmute(Self::StartUsing_ADDRESS);
            f(self as *mut Self as _, character)
        }
    }
    pub const GetGripPosition_ADDRESS: usize = 0x140966840;
    /// Writes the world transform of the weapon's grip point for `hand` into `matrix`: the
    /// weapon's grip-table entry for that hand (the entry for the mounted state while
    /// [`m_State`](crate::equipment::weapon::WeaponBase::m_State) is [`E_WS_MOUNTED`](crate::equipment::weapon::WeaponState::E_WS_MOUNTED)), a translation in
    /// the first gun's frame, composed with that gun's current world matrix. Returns false, writing
    /// nothing, when the weapon has no entry for that hand. The secondary hand IK pass and the hand
    /// IK tasks place a hand on a held weapon through it.
    pub unsafe fn GetGripPosition(
        &self,
        hand: crate::equipment::weapon::WeaponGripHand,
        matrix: *mut crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                hand: crate::equipment::weapon::WeaponGripHand,
                matrix: *mut crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::GetGripPosition_ADDRESS);
            f(self as *const Self as _, hand, matrix)
        }
    }
    pub const GetShotMatrix_ADDRESS: usize = 0x140985AA0;
    /// Builds the launch transform for a shot from `weapon_data` into `shot_matrix`. The origin is
    /// the muzzle ([`GetFireFromPosition`](crate::equipment::weapon::WeaponBase::GetFireFromPosition)); the direction points at
    /// [`m_AimTargetPosition`](crate::equipment::weapon::WeaponBase::m_AimTargetPosition) (for a remote player, the network
    /// component's aim point). It falls back to 10 m along the barrel when the aim point is within
    /// about 0.3 m of the muzzle, in barrel fire-direction mode, or with `force_barrel_direction`.
    /// While projectile-offset shots remain it offsets the aim point by
    /// [`m_ProjectileOffsetFromAimPos`](crate::equipment::weapon::WeaponBase::m_ProjectileOffsetFromAimPos) on a random ring, and
    /// it layers the scatter rotation on top unless `ignore_scatter`.
    pub unsafe fn GetShotMatrix(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        shot_matrix: *mut crate::types::math::Matrix4,
        ignore_scatter: bool,
        force_barrel_direction: bool,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                shot_matrix: *mut crate::types::math::Matrix4,
                ignore_scatter: bool,
                force_barrel_direction: bool,
            ) -> bool = ::std::mem::transmute(Self::GetShotMatrix_ADDRESS);
            f(
                self as *mut Self as _,
                weapon_data,
                shot_matrix,
                ignore_scatter,
                force_barrel_direction,
            )
        }
    }
    pub const GetFireFromPositionBase_ADDRESS: usize = 0x140966940;
    /// The base implementation of the [`GetFireFromPosition`](crate::equipment::weapon::WeaponBase::GetFireFromPosition) virtual.
    pub unsafe fn GetFireFromPositionBase(
        &self,
        result: *mut crate::types::math::Vector3,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::math::Vector3,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetFireFromPositionBase_ADDRESS,
            );
            f(self as *const Self as _, result, weapon_data)
        }
    }
    pub const Fire_ProjectileBase_ADDRESS: usize = 0x140991FF0;
    /// The base implementation of the [`Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) virtual: consumes
    /// ammunition, resets the fire timers, applies recoil, scatter, and heat, plays the fire effect
    /// on `weapon_data`, decrements the forced-fire and projectile-offset counts, and then advances
    /// the fire-point cursor of the gun at [`m_NextWeaponToFire`](crate::equipment::weapon::WeaponBase::m_NextWeaponToFire) and the
    /// gun index itself.
    pub unsafe fn Fire_ProjectileBase(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_ProjectileBase_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const RumbleFire_ADDRESS: usize = 0x1409510C0;
    /// Spawns the fire rumble for a shot through [`RumbleManager::SpawnRumble`](crate::rumble_manager::RumbleManager::SpawnRumble),
    /// named `"WeaponFire"`, with the tuning's rumble strength and time, when the user is the local
    /// player ([`m_IsUserLocalPlayer`](crate::equipment::weapon::WeaponBase::m_IsUserLocalPlayer)) and the strength is positive. On
    /// the dual-wield slot the gun about to fire ([`m_NextWeaponToFire`](crate::equipment::weapon::WeaponBase::m_NextWeaponToFire))
    /// picks the motor: gun 1 drives the left motor only, and any other gun the right motor only.
    /// Called only from [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase), after the shot's recoil
    /// is added to [`m_RecoilPitch`](crate::equipment::weapon::WeaponBase::m_RecoilPitch) and [`m_RecoilYaw`](crate::equipment::weapon::WeaponBase::m_RecoilYaw)
    /// and before the gun index advances.
    pub unsafe fn RumbleFire(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RumbleFire_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CheckIfReloadShouldBeTriggeredBase_ADDRESS: usize = 0x140965CF0;
    /// The base implementation of the
    /// [`CheckIfReloadShouldBeTriggered`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggered) virtual; the
    /// rocket launcher's and the sniper rifle's overrides call it. It clears `should_try_to_fire`
    /// while the holder reloads, draws, or holsters. For a player's weapon with limited
    /// ammunition and an empty magazine ([`m_MagazineAmmo`](crate::equipment::weapon::WeaponBase::m_MagazineAmmo); a weapon whose
    /// tuning sets no magazine size counts as empty only while the holder has no stock of its
    /// ammunition), while no reload is running, it starts a reload when
    /// [`CanReload`](crate::equipment::weapon::WeaponBase::CanReload) holds and the animation allows one, whether or not the
    /// trigger is pulled. When the weapon cannot reload and the shot was requested, it:
    ///
    /// - except for the hand grenade and the planted explosive, calls
    ///   [`Inventory::TryToSwitchToValidWeapon`](crate::character::inventory::Inventory::TryToSwitchToValidWeapon),
    ///   the automatic switch to a weapon with ammunition;
    /// - at the pull's press edge (`was_trigger_pulled` clear) only, for the local player's weapon
    ///   and when no switch happened, calls
    ///   [`HUDUI::OnOutOfAmmoFire`](crate::ui::hud_ui::HUDUI::OnOutOfAmmoFire) (or the grenade
    ///   counterpart for the hand grenade), and, whether or not a switch happened, activates the
    ///   gun's `OutOfAmmoFire` attached-effect usage on `weapon_data`;
    ///
    /// and in either case it clears `should_try_to_fire`, so an empty weapon never fires. The
    /// switch is tried on every tick the shot is requested, while the out-of-ammo feedback plays
    /// once per pull. It goes on to cancel a reload that should not continue, clear
    /// `should_try_to_fire` while the holder may not fire, and start a predicted reload of a
    /// partly empty magazine.
    pub unsafe fn CheckIfReloadShouldBeTriggeredBase(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        should_try_to_fire: *mut bool,
        was_trigger_pulled: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                should_try_to_fire: *mut bool,
                was_trigger_pulled: bool,
            ) = ::std::mem::transmute(Self::CheckIfReloadShouldBeTriggeredBase_ADDRESS);
            f(
                self as *mut Self as _,
                weapon_data,
                should_try_to_fire,
                was_trigger_pulled,
            )
        }
    }
    pub const IsWeaponAimingBase_ADDRESS: usize = 0x140950F90;
    /// The base implementation of the [`IsWeaponAiming`](crate::equipment::weapon::WeaponBase::IsWeaponAiming) virtual.
    pub unsafe fn IsWeaponAimingBase(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsWeaponAimingBase_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateFromUserPostSim_ParallelBase_ADDRESS: usize = 0x1409A3C50;
    /// The base implementation of the
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel) virtual. It returns
    /// early, processing no input, when any gun's muzzle is within about 0.1 m of `aim_position`.
    /// After the aim target, the recoil recovery, and the fire timer, it reads
    /// [`m_NextWeaponToFire`](crate::equipment::weapon::WeaponBase::m_NextWeaponToFire) once and walks
    /// [`m_lWeaponData`](crate::equipment::weapon::WeaponBase::m_lWeaponData): only the gun at that index gets
    /// [`UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel), and so only that gun can start a
    /// shot this tick, while every gun gets its attached-effect update. A weapon owned by a remote
    /// peer skips the per-gun update and fires from its network component's fire flags instead.
    pub unsafe fn UpdateFromUserPostSim_ParallelBase(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_ParallelBase_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const UpdateWeapon_Parallel_ADDRESS: usize = 0x140992B00;
    /// The per-gun fire-input update: cools heat, reads the trigger against the tuning's
    /// sensitivities, applies semi-automatic, forced-fire, wall-penetration, and aim-angle gating,
    /// and starts or stops fire on `weapon_data`. The trigger, semi-automatic, cancel, and buffer
    /// state it keeps ([`m_IsTriggerPulled`](crate::equipment::weapon::WeaponBase::m_IsTriggerPulled),
    /// [`m_FiredThisInput`](crate::equipment::weapon::WeaponBase::m_FiredThisInput),
    /// [`m_IsFiringCancelled`](crate::equipment::weapon::WeaponBase::m_IsFiringCancelled), and
    /// [`m_InputBufferTimer`](crate::equipment::weapon::WeaponBase::m_InputBufferTimer)) and the fire interval
    /// ([`m_TimeSinceLastFire`](crate::equipment::weapon::WeaponBase::m_TimeSinceLastFire)) are the weapon's, shared by its guns;
    /// only the burst counter ([`WeaponData::m_BurstShotsLeft`](crate::equipment::weapon::WeaponData::m_BurstShotsLeft)) is the gun's. A shot it allows
    /// calls [`Fire_Start`](crate::equipment::weapon::WeaponBase::Fire_Start) once, on `weapon_data` alone; with no shot while
    /// firing, it calls [`Fire_Stop`](crate::equipment::weapon::WeaponBase::Fire_Stop) on every gun.
    pub unsafe fn UpdateWeapon_Parallel(
        &mut self,
        dt: f32,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) -> bool = ::std::mem::transmute(Self::UpdateWeapon_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, weapon_data, input)
        }
    }
    pub const Fire_StartBase_ADDRESS: usize = 0x140965620;
    /// The base implementation of the [`Fire_Start`](crate::equipment::weapon::WeaponBase::Fire_Start) virtual. At the first shot
    /// of a pull (the weapon not yet [`m_IsFiring`](crate::equipment::weapon::WeaponBase::m_IsFiring)) it sets
    /// [`m_IsFirstShot`](crate::equipment::weapon::WeaponBase::m_IsFirstShot), resets the gun's tracer counters
    /// ([`WeaponData::m_TracerCounter`](crate::equipment::weapon::WeaponData::m_TracerCounter)), and loads its burst
    /// ([`WeaponData::m_BurstShotsLeft`](crate::equipment::weapon::WeaponData::m_BurstShotsLeft)) from the tuning. It then sets `m_IsFiring`, runs the
    /// enabled weapon components' fire start on `weapon_data`, and fires one shot from it through
    /// [`Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile). The mech's magnet and machine-gun weapons skip
    /// all of it under conditions on game-wide state.
    pub unsafe fn Fire_StartBase(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_StartBase_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const Fire_StopBase_ADDRESS: usize = 0x140965820;
    /// The base implementation of the [`Fire_Stop`](crate::equipment::weapon::WeaponBase::Fire_Stop) virtual: clears
    /// [`m_IsFiring`](crate::equipment::weapon::WeaponBase::m_IsFiring) and runs the enabled weapon components' fire stop on
    /// `weapon_data`. The disassembler mislabels it `CEffectInstance::Release`.
    pub unsafe fn Fire_StopBase(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) = ::std::mem::transmute(Self::Fire_StopBase_ADDRESS);
            f(self as *mut Self as _, weapon_data)
        }
    }
    pub const UpdateFromUserPostSim_SerialBase_ADDRESS: usize = 0x14096E3B0;
    /// The base implementation of the
    /// [`UpdateFromUserPostSim_Serial`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Serial) virtual. Like the
    /// parallel base, it reads [`m_NextWeaponToFire`](crate::equipment::weapon::WeaponBase::m_NextWeaponToFire) once and calls
    /// [`UpdateWeapon_Serial`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Serial) only for the gun at that index, and only
    /// for a weapon owned locally.
    pub unsafe fn UpdateFromUserPostSim_SerialBase(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_SerialBase_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const UpdateWeapon_Serial_ADDRESS: usize = 0x140965BF0;
    /// The serial per-gun update: runs the enabled weapon components' serial update on
    /// `weapon_data` and, for the local player's weapon, refreshes a readiness flag. It never
    /// starts or stops fire.
    pub unsafe fn UpdateWeapon_Serial(
        &mut self,
        dt: f32,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                input: *const crate::equipment::weapon::WeaponInputs,
            ) = ::std::mem::transmute(Self::UpdateWeapon_Serial_ADDRESS);
            f(self as *mut Self as _, dt, weapon_data, input)
        }
    }
    pub const GetLastFireFromPosition_ADDRESS: usize = 0x140966B50;
    /// Writes the start point of a projectile fired from `weapon_data` into `result` and returns
    /// it: the gun's fire point at its current fire-point cursor
    /// ([`WeaponData::m_InternalFirePositionCurrentIndex`](crate::equipment::weapon::WeaponData::m_InternalFirePositionCurrentIndex)), composed with the gun's world matrix
    /// of the previous simulation tick ([`WeaponData::m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0)). In the
    /// [`E_WS_WINGSUIT_WEAPON`](crate::equipment::weapon::WeaponState::E_WS_WINGSUIT_WEAPON) state it is the translation of
    /// the gun's current world matrix instead. The projectile weapons' shots read it after
    /// [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase) has advanced the gun index and the
    /// cursor.
    pub unsafe fn GetLastFireFromPosition(
        &self,
        result: *mut crate::types::math::Vector3,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::math::Vector3,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetLastFireFromPosition_ADDRESS,
            );
            f(self as *const Self as _, result, weapon_data)
        }
    }
    pub const GetCurrentScatterValue_ADDRESS: usize = 0x140981EC0;
    /// The scatter a shot is given: [`m_CurrentScatterValue`](crate::equipment::weapon::WeaponBase::m_CurrentScatterValue),
    /// multiplied by a tuning factor while the player holding the weapon is in precision aim and
    /// by another while the holder moves faster than a metre per second; zero without a holder.
    /// [`GetShotMatrix`](crate::equipment::weapon::WeaponBase::GetShotMatrix) multiplies the tuning's yaw and pitch scatter ranges
    /// by it, unless asked to ignore the scatter.
    pub unsafe fn GetCurrentScatterValue(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetCurrentScatterValue_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAimTimerOverride_ADDRESS: usize = 0x140958F70;
    /// Returns the tuning's aim-timer override
    /// ([`WeaponTuning::m_aim_timer_override`](crate::equipment::weapon::WeaponTuning::m_aim_timer_override)), or zero without tuning.
    pub unsafe fn GetAimTimerOverride(&self) -> f32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> f32 = ::std::mem::transmute(
                Self::GetAimTimerOverride_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetMagazineSize_ADDRESS: usize = 0x140958690;
    /// Returns the tuning's magazine size ([`WeaponTuning::m_magazine_size`](crate::equipment::weapon::WeaponTuning::m_magazine_size)), or zero without
    /// tuning.
    pub unsafe fn GetMagazineSize(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetMagazineSize_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const ChangeBoneAttachement_ADDRESS: usize = 0x140958AF0;
    /// Sets every weapon-state attach bone of gun `weapon_index` to `bone` (an
    /// `NBone::ESafeBoneIndex`).
    pub unsafe fn ChangeBoneAttachement(&mut self, bone: u32, weapon_index: i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                bone: u32,
                weapon_index: i32,
            ) = ::std::mem::transmute(Self::ChangeBoneAttachement_ADDRESS);
            f(self as *mut Self as _, bone, weapon_index)
        }
    }
    pub const SkipRenderInterpolationForOneFrame_ADDRESS: usize = 0x140982240;
    /// Sets [`WeaponData::m_SkipRenderInterpolation`](crate::equipment::weapon::WeaponData::m_SkipRenderInterpolation) on gun `weapon_data_index`, and makes the
    /// weapon's model-attachment components for that gun skip theirs, so the gun's next
    /// attachment placement is drawn without interpolation. `-1` does so for every gun and every
    /// model-attachment component. The flag lasts until
    /// [`UpdatePostFinalizePose`](crate::equipment::weapon::WeaponBase::UpdatePostFinalizePose), so it acts on placements made
    /// before the end of the holder's pose-finalization pass. The holster and predefined-bone
    /// state tasks call it when they move a gun between bones.
    pub unsafe fn SkipRenderInterpolationForOneFrame(&mut self, weapon_data_index: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, weapon_data_index: i32) = ::std::mem::transmute(
                Self::SkipRenderInterpolationForOneFrame_ADDRESS,
            );
            f(self as *mut Self as _, weapon_data_index)
        }
    }
    pub const UpdatePostFinalizePose_ADDRESS: usize = 0x1409A5010;
    /// The weapon's post-pose-finalization update, run for every inventory weapon by
    /// [`Inventory::UpdateInventoryPostFinalizePose`](crate::character::inventory::Inventory::UpdateInventoryPostFinalizePose)
    /// at the end of the holder's pose-finalization pass. It clears every gun's
    /// [`WeaponData::m_SkipRenderInterpolation`](crate::equipment::weapon::WeaponData::m_SkipRenderInterpolation), rewrites every gun's
    /// [`WeaponData::m_lBoneAttachementPerWeaponState`](crate::equipment::weapon::WeaponData::m_lBoneAttachementPerWeaponState) entries for the
    /// [`E_WS_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_WIELDING) to
    /// [`E_WS_HOLSTERED_INVISIBLE`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED_INVISIBLE) states from
    /// [`m_EquipSlot`](crate::equipment::weapon::WeaponBase::m_EquipSlot) (the `SetupWeaponAttachementBones` table, inlined), and
    /// runs the enabled weapon components' post-finalization hook. For the
    /// [`E_SLOT_DUAL_WIELD`](crate::character::inventory::WeaponSlot::E_SLOT_DUAL_WIELD) slot the table puts the first gun on
    /// [`ATTACH_HAND_RIGHT`](crate::character::character::SafeBoneIndex::ATTACH_HAND_RIGHT) when wielding
    /// or dual-wielding and on
    /// [`RIGHT_HOLSTER`](crate::character::character::SafeBoneIndex::RIGHT_HOLSTER) when holstered, and
    /// the second gun on
    /// [`LEFT_HOLSTER`](crate::character::character::SafeBoneIndex::LEFT_HOLSTER) except when
    /// dual-wielding, where it rides
    /// [`ATTACH_HAND_LEFT`](crate::character::character::SafeBoneIndex::ATTACH_HAND_LEFT); with a single
    /// gun wielded the second waits on its holster. A bone a state task moves a gun to lasts only
    /// until the end of the pass it was written in. The
    /// disassembler mislabels it `CWeaponBase::CalculateAttachOffsets`.
    pub unsafe fn UpdatePostFinalizePose(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdatePostFinalizePose_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetCurrentBoneAttachement_ADDRESS: usize = 0x140958B30;
    /// Returns the attach bone of gun `weapon_index` for the weapon's current
    /// [`m_State`](crate::equipment::weapon::WeaponBase::m_State), or an invalid-bone value when the index is out of range.
    pub unsafe fn GetCurrentBoneAttachement(&self, weapon_index: i32) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                weapon_index: i32,
            ) -> u32 = ::std::mem::transmute(Self::GetCurrentBoneAttachement_ADDRESS);
            f(self as *const Self as _, weapon_index)
        }
    }
    pub const GetTransform_ADDRESS: usize = 0x140964E50;
    /// The `CGameObject::GetTransform` override: copies the first gun's
    /// [`WeaponData::GetWorldMatrixT1`](crate::equipment::weapon::WeaponData::GetWorldMatrixT1) into `transform`. `game_object` is the weapon's
    /// `CGameObject` subobject, not the start of the weapon.
    pub unsafe fn GetTransform(
        game_object: *const ::std::ffi::c_void,
        transform: *mut crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                game_object: *const ::std::ffi::c_void,
                transform: *mut crate::types::math::Matrix4,
            ) -> bool = ::std::mem::transmute(Self::GetTransform_ADDRESS);
            f(game_object, transform)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x140966380;
    /// The render-phase `CGameObject::UpdateRender` override. For each gun it interpolates
    /// [`WeaponData::m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) toward [`WeaponData::m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1) by the render
    /// context's interpolation factor, applies the holstered scale when holstered, pushes the
    /// result to [`WeaponData::m_BaseModelInstance`](crate::equipment::weapon::WeaponData::m_BaseModelInstance), and updates the gun's animated-model
    /// skinning; it then renders the weapon's components, among them the separate model parts of
    /// its guns ([`ModelAttachementWeaponComponent::UpdateRender`](crate::equipment::weapon::ModelAttachementWeaponComponent::UpdateRender)), which interpolate their own
    /// cached matrices. `game_object` is the weapon's `CGameObject` subobject, not the start of
    /// the weapon.
    pub unsafe fn UpdateRender(
        game_object: *mut ::std::ffi::c_void,
        context: *const crate::game::GameObjectRenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                game_object: *mut ::std::ffi::c_void,
                context: *const crate::game::GameObjectRenderContext,
            ) = ::std::mem::transmute(Self::UpdateRender_ADDRESS);
            f(game_object, context)
        }
    }
    /// Writes the muzzle position for `weapon_data`: the gun's world matrix composed with its
    /// current fire point ([`WeaponData::m_InternalFirePositionCurrentIndex`](crate::equipment::weapon::WeaponData::m_InternalFirePositionCurrentIndex)). The base
    /// implementation is [`GetFireFromPositionBase`](crate::equipment::weapon::WeaponBase::GetFireFromPositionBase).
    pub unsafe fn GetFireFromPosition(
        &self,
        result: *mut crate::types::math::Vector3,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetFireFromPosition).read();
            f(self as *const Self as _, result, weapon_data)
        }
    }
    /// Starts fire on one gun, `weapon_data`. The base implementation is
    /// [`Fire_StartBase`](crate::equipment::weapon::WeaponBase::Fire_StartBase).
    pub unsafe fn Fire_Start(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Fire_Start).read();
            f(self as *mut Self as _, weapon_data)
        }
    }
    /// Stops fire. The base implementation is [`Fire_StopBase`](crate::equipment::weapon::WeaponBase::Fire_StopBase).
    pub unsafe fn Fire_Stop(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Fire_Stop).read();
            f(self as *mut Self as _, weapon_data)
        }
    }
    /// Fires one shot from `weapon_data`. The base implementation is
    /// [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase); subclasses such as
    /// `CBulletWeaponBase` spawn their projectile and then call it.
    pub unsafe fn Fire_Projectile(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Fire_Projectile).read();
            f(self as *mut Self as _, weapon_data)
        }
    }
    /// Takes the weapon into `holder`'s keeping (a character's or vehicle's `CGameObject`
    /// subobject): sets the holder, and for a weapon outside the heavy slot sets the holstered
    /// state (holstered-invisible for the hand grenade, the planted explosive, and the beacon),
    /// which takes its bodies out of the physics world. Unless
    /// [`m_InfinitePickup`](crate::equipment::weapon::WeaponBase::m_InfinitePickup) is set it disables the weapon's pickup
    /// interaction and its pickup hint, and for the locally controlled character it sets the
    /// avatar's mental state to
    /// [`E_READY_FOR_COMBAT`](crate::character::avatar::MentalState::E_READY_FOR_COMBAT) and shows
    /// the pickup on the HUD. The inventory's add
    /// calls it when the weapon goes into a slot.
    pub unsafe fn OnPickup(&mut self, holder: *mut crate::game::GameObject) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnPickup).read();
            f(self as *mut Self as _, holder)
        }
    }
    /// Equips the weapon for `character`, which must hold it: shows it, runs its components'
    /// equip, selects its slot, starts using it (the user becomes `character`), puts the
    /// locally controlled character's avatar into
    /// [`E_COMBAT`](crate::character::avatar::MentalState::E_COMBAT) and shows the weapon on the HUD, and
    /// sets the wielding state, or dual-wielding for the dual-wield slot of a player. It plays
    /// no act: the draw animation comes from the character's draw, not from here. It does
    /// nothing for a dead character. The inventory's add calls it with the equipping mode.
    pub unsafe fn OnEquip(
        &mut self,
        character: *mut crate::character::character::Character,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnEquip).read();
            f(self as *mut Self as _, character)
        }
    }
    /// Unequips the weapon from its user: cancels the user's reload, sets the holstered state
    /// (which takes its bodies out of the physics world) unless it is mounted, drops it through
    /// the user's inventory instead when it is in the heavy slot, and then stops using it
    /// ([`StopUsing`](crate::equipment::weapon::WeaponBase::StopUsing)). It does nothing without a user.
    pub unsafe fn OnUnequip(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnUnequip).read();
            f(self as *mut Self as _)
        }
    }
    /// Drops the weapon into the world, run by the deferred drop on the weapon's next
    /// pre-simulation update and directly by a swimming character's pickup on the occupant of
    /// the slot it picks into. A weapon still wielded or dual-wielded is unequipped first
    /// ([`OnUnequip`](crate::equipment::weapon::WeaponBase::OnUnequip)), leaving the player's precision aim. It then sets the
    /// dropped state, adds the bodies to the physics world and wakes them, and sets the first
    /// gun's body velocity ([`WeaponData::m_PfxInstance`](crate::equipment::weapon::WeaponData::m_PfxInstance)) from the state the weapon was in by
    /// then: from holstered, with a character holder, the holder's velocity plus its forward
    /// direction scaled by the slot (forward for the two-handed slot, backward for the special
    /// slot, none for the others); from wielding or dual-wielding, the first gun's
    /// [`m_MeasuredWorldVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredWorldVelocity) and
    /// [`m_MeasuredAngularVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredAngularVelocity). A non-player
    /// holder's weapon first gets stored ammunition from the holder's dropped-magazine range.
    /// It clears the holder and enables the weapon's pickup interaction.
    pub unsafe fn OnDrop(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).OnDrop).read();
            f(self as *mut Self as _)
        }
    }
    /// Stops the user using the weapon: stops its fire, its effects, and its components, clears
    /// the user, and, when the weapon is wielding or dual-wielding outside the heavy slot, sets
    /// the holstered state. A mounted gun also leaves its user's inventory, which then selects
    /// another weapon.
    pub unsafe fn StopUsing(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).StopUsing).read();
            f(self as *mut Self as _)
        }
    }
    /// The ammunition the weapon can still use: its magazine plus the holder's inventory stock
    /// of its ammunition type. The thrown grenade weapon adds one while it holds a primed
    /// grenade ([`ThrownGrenadeWeapon::m_PrimedGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimedGrenade)).
    pub unsafe fn GetTotalAmmo(&self) -> i32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetTotalAmmo).read();
            f(self as *const Self as _)
        }
    }
    /// Sets [`m_MagazineAmmo`](crate::equipment::weapon::WeaponBase::m_MagazineAmmo) to `ammo`. For the local player's
    /// selected weapon outside the planted-explosive slot it also pushes the count to the HUD
    /// (`CHUDUI::OnSetSelectedMagazineAmmo`), flagging an empty magazine; the HUD keeps no
    /// other view of the count, so a write that bypasses this leaves the HUD stale. The base
    /// implementation is [`SetMagazineAmmoBase`](crate::equipment::weapon::WeaponBase::SetMagazineAmmoBase).
    pub unsafe fn SetMagazineAmmo(&mut self, ammo: i32) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetMagazineAmmo).read();
            f(self as *mut Self as _, ammo)
        }
    }
    /// Starts the weapon side of a reload once the holder's reload act is accepted (the act's
    /// callback calls it): resets the fire-input timers, plays each gun's reload effect,
    /// clears the guns' bursts, sets [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) for an
    /// [`E_RELOAD_BULLET`](crate::equipment::weapon::ReloadType::E_RELOAD_BULLET) weapon, and registers the weapon for
    /// pre-simulation updates. It moves no ammunition.
    pub unsafe fn StartReloadSequence(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).StartReloadSequence).read();
            f(self as *mut Self as _)
        }
    }
    /// Whether the weapon can start a reload: its ammunition type is not
    /// [`AMMO_UNLIMITED`](crate::equipment::weapon::AmmunitionType::AMMO_UNLIMITED), it has a magazine limit, the
    /// magazine is below it, and either [`HasUnlimitedAmmo`](crate::equipment::weapon::WeaponBase::HasUnlimitedAmmo) holds or
    /// its living holder has a reserve of its type (a holder in god mode,
    /// [`GOD_MODE`](crate::character::character::DamageStateFlags::GOD_MODE), always counts as
    /// having one). A weapon whose magazine and reserve are both empty cannot.
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)
    /// queues the reload act only when it holds.
    pub unsafe fn CanReload(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).CanReload).read();
            f(self as *const Self as _)
        }
    }
    /// Refills the magazine from the holder's reserve and returns whether it did. The base
    /// implementation ([`ReloadBase`](crate::equipment::weapon::WeaponBase::ReloadBase)) does nothing and returns false
    /// without a holder, for an [`AMMO_UNLIMITED`](crate::equipment::weapon::AmmunitionType::AMMO_UNLIMITED) weapon, or
    /// for one without a magazine limit. Otherwise it first clears
    /// [`m_WeaponIsWithoutMagazine`](crate::equipment::weapon::WeaponBase::m_WeaponIsWithoutMagazine), even when it then
    /// refills nothing. With the magazine already full, or with no reserve
    /// ([`Inventory::GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount)) and no
    /// [`HasUnlimitedAmmo`](crate::equipment::weapon::WeaponBase::HasUnlimitedAmmo), it clears
    /// [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) and returns false. Otherwise:
    ///
    /// - the new count is the magazine size, or the old count plus the reserve when that is
    ///   less; with unlimited ammunition the reserve counts as a full magazine;
    /// - for an [`E_RELOAD_BULLET`](crate::equipment::weapon::ReloadType::E_RELOAD_BULLET) weapon the new count is the
    ///   old count plus one instead, and [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) is set
    ///   while that is still short of full;
    /// - it writes the count through [`SetMagazineAmmo`](crate::equipment::weapon::WeaponBase::SetMagazineAmmo) (which pushes
    ///   the HUD), records the reload statistic for the local player, resets the scatter
    ///   balancer, and tells the HUD (`CHUDUI::OnReloadSelectedWeapon`) for the local player's
    ///   selected weapon;
    /// - it clears [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) once the magazine is full;
    /// - without unlimited ammunition it writes the reserve back as the old count plus the
    ///   reserve minus the new count
    ///   ([`Inventory::SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount), allowing
    ///   an autosave), so the rounds left in the old magazine return to the reserve and nothing
    ///   is lost;
    /// - it clears [`m_IsReloading`](crate::equipment::weapon::WeaponBase::m_IsReloading) and returns true.
    ///
    /// It never touches the weapon's model parts: a part hidden by the throw-away-magazine
    /// message stays hidden until the
    /// [`TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT)
    /// message shows it. The rocket launcher's override ([`RPGWeapon::Reload`](crate::equipment::weapon::RPGWeapon::Reload)) handles its
    /// rocket model. It runs from the reload track message
    /// ([`TRACK_MESSAGE_RELOAD`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_RELOAD)), in the holder's serial
    /// post-physics pass, and for an
    /// [`E_RELOAD_NO_ANIMATION`](crate::equipment::weapon::ReloadType::E_RELOAD_NO_ANIMATION) weapon from the ammunition
    /// check in the weapon's parallel update; a refill of the hand-grenade reserve from empty
    /// also calls it on the hand-grenade weapon.
    pub unsafe fn Reload(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).Reload).read();
            f(self as *mut Self as _)
        }
    }
    /// The per-gun ammunition check, called by
    /// [`UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) for `weapon_data` with the
    /// tick's fire decision in `should_try_to_fire` and whether the trigger was already pulled
    /// before this tick. The base implementation is
    /// [`CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase).
    pub unsafe fn CheckIfReloadShouldBeTriggered(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        should_try_to_fire: *mut bool,
        was_trigger_pulled: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).CheckIfReloadShouldBeTriggered).read();
            f(
                self as *mut Self as _,
                weapon_data,
                should_try_to_fire,
                was_trigger_pulled,
            )
        }
    }
    /// Ends a reload: clears [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) and
    /// [`m_IsReloading`](crate::equipment::weapon::WeaponBase::m_IsReloading). The reload state machine's
    /// `NWeaponReloadFinishedTask` calls it on the wielded weapon when its state exits.
    pub unsafe fn ReloadAnimationFinished(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).ReloadAnimationFinished).read();
            f(self as *mut Self as _)
        }
    }
    /// Deducts `count` rounds of ammunition for `weapon_data`.
    pub unsafe fn ConsumeAmmo(
        &mut self,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        count: i32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).ConsumeAmmo).read();
            f(self as *mut Self as _, weapon_data, count)
        }
    }
    /// Whether the weapon is being aimed: the base implementation
    /// ([`IsWeaponAimingBase`](crate::equipment::weapon::WeaponBase::IsWeaponAimingBase)) returns
    /// [`m_IsTryingToFire`](crate::equipment::weapon::WeaponBase::m_IsTryingToFire) or
    /// [`m_IsFeatheringTrigger`](crate::equipment::weapon::WeaponBase::m_IsFeatheringTrigger). The character's aim-input
    /// handling refills its aim timer while this holds.
    pub unsafe fn IsWeaponAiming(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsWeaponAiming).read();
            f(self as *const Self as _)
        }
    }
    /// Applies the holder's aim mode: normal or precision aim. `CPlayer::SetAimMode` calls it
    /// after changing the player's aim mode.
    pub unsafe fn SetAimMode(&mut self, aim_mode: crate::aim::aim::AimMode) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetAimMode).read();
            f(self as *mut Self as _, aim_mode)
        }
    }
    /// The per-tick weapon update from its user after simulation: validates `aim_position` and
    /// stores it as [`m_AimTargetPosition`](crate::equipment::weapon::WeaponBase::m_AimTargetPosition), then processes the
    /// fire inputs. The base implementation is
    /// [`UpdateFromUserPostSim_ParallelBase`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase).
    pub unsafe fn UpdateFromUserPostSim_Parallel(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdateFromUserPostSim_Parallel).read();
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    /// The serial counterpart of
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel), called with the
    /// same arguments by
    /// [`Inventory::UpdateWeapons_Serial`](crate::character::inventory::Inventory::UpdateWeapons_Serial).
    /// It does not write
    /// [`m_AimTargetPosition`](crate::equipment::weapon::WeaponBase::m_AimTargetPosition).
    pub unsafe fn UpdateFromUserPostSim_Serial(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdateFromUserPostSim_Serial).read();
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    /// Returns the weapon type's tuning block (`SWeaponTuningBase`), from which the fire code
    /// reads recoil, scatter, and spread ranges.
    pub unsafe fn GetTuningBase(&self) -> *const ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).GetTuningBase).read();
            f(self as *const Self as _)
        }
    }
    /// Shows or hides the weapon's guns and their components. Showing with `fade` fades the
    /// guns' models in. A weapon in the
    /// [`E_WS_HOLSTERED_INVISIBLE`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED_INVISIBLE) state is shown only
    /// with `force`. Hiding takes effect at once. A hand grenade picked up hides itself this
    /// way before it despawns, and a placement rule hides a weapon it places invisible.
    pub unsafe fn SetVisible(&mut self, visible: bool, fade: bool, force: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetVisible).read();
            f(self as *mut Self as _, visible, fade, force)
        }
    }
    /// Places the weapon's guns for its state: the base implementation
    /// (`CWeaponBase::UpdateAttachmentTransforms`) locks the holder
    /// ([`m_Holder`](crate::equipment::weapon::WeaponBase::m_Holder)) and runs
    /// [`WeaponData::UpdateAttachmentTransforms`](crate::equipment::weapon::WeaponData::UpdateAttachmentTransforms) for every gun with it, then places the
    /// weapon's components. `p2` and `p3` are the snow and water factors the gun placement
    /// takes, and `transform_type` its update type. The inventory's placement
    /// ([`Inventory::UpdateAttachmentTransforms`](crate::character::inventory::Inventory::UpdateAttachmentTransforms))
    /// calls it for each slot's weapon.
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        p2: f32,
        p3: f32,
        transform_type: u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdateAttachmentTransforms).read();
            f(self as *mut Self as _, p2, p3, transform_type)
        }
    }
}
impl WeaponBase {
    /// The offset of the embedded `CGameObject` subobject, which the `CGameObject` overrides
    /// ([`GetTransform`](crate::equipment::weapon::WeaponBase::GetTransform), [`UpdateRender`](crate::equipment::weapon::WeaponBase::UpdateRender)) receive as
    /// their `game_object` in place of the weapon.
    pub const GAME_OBJECT_BASE_OFFSET: u64 = 8;
    /// The seconds a blocked trigger pull stays buffered in
    /// [`m_InputBufferTimer`](crate::equipment::weapon::WeaponBase::m_InputBufferTimer).
    pub const INPUT_BUFFER_TIME: f32 = 0.5;
    /// The track message that shows or hides a model part: for every one of
    /// [`m_WeaponComponents`](crate::equipment::weapon::WeaponBase::m_WeaponComponents) that passes
    /// [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType) for [`ModelAttachementWeaponComponent::TYPE_ID`](crate::equipment::weapon::ModelAttachementWeaponComponent::TYPE_ID)
    /// and whose [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) is the message's
    /// first argument, it calls [`Enable`](crate::equipment::weapon::WeaponComponentBase::Enable) with whether the second
    /// argument is non-zero. Reload clips use it to show the magazine part again.
    pub const TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT: u32 = 2840635779;
    /// The track message that refills the magazine: calls the [`Reload`](crate::equipment::weapon::WeaponBase::Reload) virtual.
    pub const TRACK_MESSAGE_RELOAD: u32 = 3817934976;
    /// The track message that asks the holder to start a reload
    /// ([`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)).
    pub const TRACK_MESSAGE_START_RELOAD: u32 = 3327133307;
    /// The track message that asks the holder to start a reload only while the magazine
    /// ([`m_MagazineAmmo`](crate::equipment::weapon::WeaponBase::m_MagazineAmmo)) is empty.
    pub const TRACK_MESSAGE_START_RELOAD_IF_EMPTY: u32 = 3370731902;
    /// The track message ([`HandleTrackMessage`](crate::equipment::weapon::WeaponBase::HandleTrackMessage)) that drops the
    /// magazine. Its arguments are the part name hash for
    /// [`WeaponData::ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject), the index of the gun in
    /// [`m_lWeaponData`](crate::equipment::weapon::WeaponBase::m_lWeaponData), and a flag that, when set, leaves
    /// [`m_WeaponIsWithoutMagazine`](crate::equipment::weapon::WeaponBase::m_WeaponIsWithoutMagazine) clear. It does nothing for a
    /// gun index out of range or while the magazine is already out. When the throw succeeds it
    /// requests the gun's next spare ([`WeaponData::SpawnMagazineObject`](crate::equipment::weapon::WeaponData::SpawnMagazineObject)) and, unless the flag is
    /// set, sets [`m_WeaponIsWithoutMagazine`](crate::equipment::weapon::WeaponBase::m_WeaponIsWithoutMagazine).
    pub const TRACK_MESSAGE_THROW_AWAY_MAGAZINE: u32 = 175000704;
}
impl std::convert::AsRef<WeaponBase> for WeaponBase {
    fn as_ref(&self) -> &WeaponBase {
        self
    }
}
impl std::convert::AsMut<WeaponBase> for WeaponBase {
    fn as_mut(&mut self) -> &mut WeaponBase {
        self
    }
}
#[repr(C, align(8))]
pub struct WeaponBaseVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::equipment::weapon::WeaponBase),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Writes the muzzle position for `weapon_data`: the gun's world matrix composed with its
    /// current fire point ([`WeaponData::m_InternalFirePositionCurrentIndex`](crate::equipment::weapon::WeaponData::m_InternalFirePositionCurrentIndex)). The base
    /// implementation is [`GetFireFromPositionBase`](crate::equipment::weapon::WeaponBase::GetFireFromPositionBase).
    pub GetFireFromPosition: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
        result: *mut crate::types::math::Vector3,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ) -> *mut crate::types::math::Vector3,
    /// Starts fire on one gun, `weapon_data`. The base implementation is
    /// [`Fire_StartBase`](crate::equipment::weapon::WeaponBase::Fire_StartBase).
    pub Fire_Start: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ),
    /// Stops fire. The base implementation is [`Fire_StopBase`](crate::equipment::weapon::WeaponBase::Fire_StopBase).
    pub Fire_Stop: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ),
    /// Fires one shot from `weapon_data`. The base implementation is
    /// [`Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase); subclasses such as
    /// `CBulletWeaponBase` spawn their projectile and then call it.
    pub Fire_Projectile: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
    ),
    _vfunc_29: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_30: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_31: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Takes the weapon into `holder`'s keeping (a character's or vehicle's `CGameObject`
    /// subobject): sets the holder, and for a weapon outside the heavy slot sets the holstered
    /// state (holstered-invisible for the hand grenade, the planted explosive, and the beacon),
    /// which takes its bodies out of the physics world. Unless
    /// [`m_InfinitePickup`](crate::equipment::weapon::WeaponBase::m_InfinitePickup) is set it disables the weapon's pickup
    /// interaction and its pickup hint, and for the locally controlled character it sets the
    /// avatar's mental state to
    /// [`E_READY_FOR_COMBAT`](crate::character::avatar::MentalState::E_READY_FOR_COMBAT) and shows
    /// the pickup on the HUD. The inventory's add
    /// calls it when the weapon goes into a slot.
    pub OnPickup: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        holder: *mut crate::game::GameObject,
    ),
    /// Equips the weapon for `character`, which must hold it: shows it, runs its components'
    /// equip, selects its slot, starts using it (the user becomes `character`), puts the
    /// locally controlled character's avatar into
    /// [`E_COMBAT`](crate::character::avatar::MentalState::E_COMBAT) and shows the weapon on the HUD, and
    /// sets the wielding state, or dual-wielding for the dual-wield slot of a player. It plays
    /// no act: the draw animation comes from the character's draw, not from here. It does
    /// nothing for a dead character. The inventory's add calls it with the equipping mode.
    pub OnEquip: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        character: *mut crate::character::character::Character,
    ),
    /// Unequips the weapon from its user: cancels the user's reload, sets the holstered state
    /// (which takes its bodies out of the physics world) unless it is mounted, drops it through
    /// the user's inventory instead when it is in the heavy slot, and then stops using it
    /// ([`StopUsing`](crate::equipment::weapon::WeaponBase::StopUsing)). It does nothing without a user.
    pub OnUnequip: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Drops the weapon into the world, run by the deferred drop on the weapon's next
    /// pre-simulation update and directly by a swimming character's pickup on the occupant of
    /// the slot it picks into. A weapon still wielded or dual-wielded is unequipped first
    /// ([`OnUnequip`](crate::equipment::weapon::WeaponBase::OnUnequip)), leaving the player's precision aim. It then sets the
    /// dropped state, adds the bodies to the physics world and wakes them, and sets the first
    /// gun's body velocity ([`WeaponData::m_PfxInstance`](crate::equipment::weapon::WeaponData::m_PfxInstance)) from the state the weapon was in by
    /// then: from holstered, with a character holder, the holder's velocity plus its forward
    /// direction scaled by the slot (forward for the two-handed slot, backward for the special
    /// slot, none for the others); from wielding or dual-wielding, the first gun's
    /// [`m_MeasuredWorldVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredWorldVelocity) and
    /// [`m_MeasuredAngularVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredAngularVelocity). A non-player
    /// holder's weapon first gets stored ammunition from the holder's dropped-magazine range.
    /// It clears the holder and enables the weapon's pickup interaction.
    pub OnDrop: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Stops the user using the weapon: stops its fire, its effects, and its components, clears
    /// the user, and, when the weapon is wielding or dual-wielding outside the heavy slot, sets
    /// the holstered state. A mounted gun also leaves its user's inventory, which then selects
    /// another weapon.
    pub StopUsing: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// The ammunition the weapon can still use: its magazine plus the holder's inventory stock
    /// of its ammunition type. The thrown grenade weapon adds one while it holds a primed
    /// grenade ([`ThrownGrenadeWeapon::m_PrimedGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimedGrenade)).
    pub GetTotalAmmo: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
    ) -> i32,
    /// Sets [`m_MagazineAmmo`](crate::equipment::weapon::WeaponBase::m_MagazineAmmo) to `ammo`. For the local player's
    /// selected weapon outside the planted-explosive slot it also pushes the count to the HUD
    /// (`CHUDUI::OnSetSelectedMagazineAmmo`), flagging an empty magazine; the HUD keeps no
    /// other view of the count, so a write that bypasses this leaves the HUD stale. The base
    /// implementation is [`SetMagazineAmmoBase`](crate::equipment::weapon::WeaponBase::SetMagazineAmmoBase).
    pub SetMagazineAmmo: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        ammo: i32,
    ),
    /// Starts the weapon side of a reload once the holder's reload act is accepted (the act's
    /// callback calls it): resets the fire-input timers, plays each gun's reload effect,
    /// clears the guns' bursts, sets [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) for an
    /// [`E_RELOAD_BULLET`](crate::equipment::weapon::ReloadType::E_RELOAD_BULLET) weapon, and registers the weapon for
    /// pre-simulation updates. It moves no ammunition.
    pub StartReloadSequence: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Whether the weapon can start a reload: its ammunition type is not
    /// [`AMMO_UNLIMITED`](crate::equipment::weapon::AmmunitionType::AMMO_UNLIMITED), it has a magazine limit, the
    /// magazine is below it, and either [`HasUnlimitedAmmo`](crate::equipment::weapon::WeaponBase::HasUnlimitedAmmo) holds or
    /// its living holder has a reserve of its type (a holder in god mode,
    /// [`GOD_MODE`](crate::character::character::DamageStateFlags::GOD_MODE), always counts as
    /// having one). A weapon whose magazine and reserve are both empty cannot.
    /// [`Character::StartReloadingWieldingWeapon`](crate::character::character::Character::StartReloadingWieldingWeapon)
    /// queues the reload act only when it holds.
    pub CanReload: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
    ) -> bool,
    /// Refills the magazine from the holder's reserve and returns whether it did. The base
    /// implementation ([`ReloadBase`](crate::equipment::weapon::WeaponBase::ReloadBase)) does nothing and returns false
    /// without a holder, for an [`AMMO_UNLIMITED`](crate::equipment::weapon::AmmunitionType::AMMO_UNLIMITED) weapon, or
    /// for one without a magazine limit. Otherwise it first clears
    /// [`m_WeaponIsWithoutMagazine`](crate::equipment::weapon::WeaponBase::m_WeaponIsWithoutMagazine), even when it then
    /// refills nothing. With the magazine already full, or with no reserve
    /// ([`Inventory::GetAmmoCount`](crate::character::inventory::Inventory::GetAmmoCount)) and no
    /// [`HasUnlimitedAmmo`](crate::equipment::weapon::WeaponBase::HasUnlimitedAmmo), it clears
    /// [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) and returns false. Otherwise:
    ///
    /// - the new count is the magazine size, or the old count plus the reserve when that is
    ///   less; with unlimited ammunition the reserve counts as a full magazine;
    /// - for an [`E_RELOAD_BULLET`](crate::equipment::weapon::ReloadType::E_RELOAD_BULLET) weapon the new count is the
    ///   old count plus one instead, and [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) is set
    ///   while that is still short of full;
    /// - it writes the count through [`SetMagazineAmmo`](crate::equipment::weapon::WeaponBase::SetMagazineAmmo) (which pushes
    ///   the HUD), records the reload statistic for the local player, resets the scatter
    ///   balancer, and tells the HUD (`CHUDUI::OnReloadSelectedWeapon`) for the local player's
    ///   selected weapon;
    /// - it clears [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) once the magazine is full;
    /// - without unlimited ammunition it writes the reserve back as the old count plus the
    ///   reserve minus the new count
    ///   ([`Inventory::SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount), allowing
    ///   an autosave), so the rounds left in the old magazine return to the reserve and nothing
    ///   is lost;
    /// - it clears [`m_IsReloading`](crate::equipment::weapon::WeaponBase::m_IsReloading) and returns true.
    ///
    /// It never touches the weapon's model parts: a part hidden by the throw-away-magazine
    /// message stays hidden until the
    /// [`TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_ENABLE_MODEL_ATTACHEMENT)
    /// message shows it. The rocket launcher's override ([`RPGWeapon::Reload`](crate::equipment::weapon::RPGWeapon::Reload)) handles its
    /// rocket model. It runs from the reload track message
    /// ([`TRACK_MESSAGE_RELOAD`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_RELOAD)), in the holder's serial
    /// post-physics pass, and for an
    /// [`E_RELOAD_NO_ANIMATION`](crate::equipment::weapon::ReloadType::E_RELOAD_NO_ANIMATION) weapon from the ammunition
    /// check in the weapon's parallel update; a refill of the hand-grenade reserve from empty
    /// also calls it on the hand-grenade weapon.
    pub Reload: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ) -> bool,
    /// The per-gun ammunition check, called by
    /// [`UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) for `weapon_data` with the
    /// tick's fire decision in `should_try_to_fire` and whether the trigger was already pulled
    /// before this tick. The base implementation is
    /// [`CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase).
    pub CheckIfReloadShouldBeTriggered: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        should_try_to_fire: *mut bool,
        was_trigger_pulled: bool,
    ),
    /// Ends a reload: clears [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) and
    /// [`m_IsReloading`](crate::equipment::weapon::WeaponBase::m_IsReloading). The reload state machine's
    /// `NWeaponReloadFinishedTask` calls it on the wielded weapon when its state exits.
    pub ReloadAnimationFinished: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Deducts `count` rounds of ammunition for `weapon_data`.
    pub ConsumeAmmo: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        count: i32,
    ),
    _vfunc_46: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Whether the weapon is being aimed: the base implementation
    /// ([`IsWeaponAimingBase`](crate::equipment::weapon::WeaponBase::IsWeaponAimingBase)) returns
    /// [`m_IsTryingToFire`](crate::equipment::weapon::WeaponBase::m_IsTryingToFire) or
    /// [`m_IsFeatheringTrigger`](crate::equipment::weapon::WeaponBase::m_IsFeatheringTrigger). The character's aim-input
    /// handling refills its aim timer while this holds.
    pub IsWeaponAiming: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
    ) -> bool,
    /// Applies the holder's aim mode: normal or precision aim. `CPlayer::SetAimMode` calls it
    /// after changing the player's aim mode.
    pub SetAimMode: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        aim_mode: crate::aim::aim::AimMode,
    ),
    _vfunc_49: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_50: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_51: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_52: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_53: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// The per-tick weapon update from its user after simulation: validates `aim_position` and
    /// stores it as [`m_AimTargetPosition`](crate::equipment::weapon::WeaponBase::m_AimTargetPosition), then processes the
    /// fire inputs. The base implementation is
    /// [`UpdateFromUserPostSim_ParallelBase`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase).
    pub UpdateFromUserPostSim_Parallel: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ),
    /// The serial counterpart of
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel), called with the
    /// same arguments by
    /// [`Inventory::UpdateWeapons_Serial`](crate::character::inventory::Inventory::UpdateWeapons_Serial).
    /// It does not write
    /// [`m_AimTargetPosition`](crate::equipment::weapon::WeaponBase::m_AimTargetPosition).
    pub UpdateFromUserPostSim_Serial: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const crate::equipment::weapon::WeaponInputs,
    ),
    _vfunc_56: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Returns the weapon type's tuning block (`SWeaponTuningBase`), from which the fire code
    /// reads recoil, scatter, and spread ranges.
    pub GetTuningBase: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
    ) -> *const ::std::ffi::c_void,
    _vfunc_58: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Shows or hides the weapon's guns and their components. Showing with `fade` fades the
    /// guns' models in. A weapon in the
    /// [`E_WS_HOLSTERED_INVISIBLE`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED_INVISIBLE) state is shown only
    /// with `force`. Hiding takes effect at once. A hand grenade picked up hides itself this
    /// way before it despawns, and a placement rule hides a weapon it places invisible.
    pub SetVisible: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        visible: bool,
        fade: bool,
        force: bool,
    ),
    _vfunc_60: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Places the weapon's guns for its state: the base implementation
    /// (`CWeaponBase::UpdateAttachmentTransforms`) locks the holder
    /// ([`m_Holder`](crate::equipment::weapon::WeaponBase::m_Holder)) and runs
    /// [`WeaponData::UpdateAttachmentTransforms`](crate::equipment::weapon::WeaponData::UpdateAttachmentTransforms) for every gun with it, then places the
    /// weapon's components. `p2` and `p3` are the snow and water factors the gun placement
    /// takes, and `transform_type` its update type. The inventory's placement
    /// ([`Inventory::UpdateAttachmentTransforms`](crate::character::inventory::Inventory::UpdateAttachmentTransforms))
    /// calls it for each slot's weapon.
    pub UpdateAttachmentTransforms: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
        p2: f32,
        p3: f32,
        transform_type: u32,
    ),
}
fn _WeaponBaseVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F0], WeaponBaseVftable>([0u8; 0x1F0]);
    }
    unreachable!()
}
impl WeaponBaseVftable {}
impl std::convert::AsRef<WeaponBaseVftable> for WeaponBaseVftable {
    fn as_ref(&self) -> &WeaponBaseVftable {
        self
    }
}
impl std::convert::AsMut<WeaponBaseVftable> for WeaponBaseVftable {
    fn as_mut(&mut self) -> &mut WeaponBaseVftable {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A weapon's class (`CWeaponBase::EWeaponClass`), stored on the weapon as [`WeaponBase::m_Class`](crate::equipment::weapon::WeaponBase::m_Class).
/// Type-specific code compares it, for example the character's level-of-detail update, which keeps
/// full detail for a character wielding a rocket launcher or a sniper rifle.
pub enum WeaponClass {
    E_WEAPON_CLASS_NONE = 1000isize as _,
    E_WEAPON_CLASS_PISTOL = 1001isize as _,
    E_WEAPON_CLASS_SMG = 1002isize as _,
    E_WEAPON_CLASS_ASSAULT_RIFLE = 1003isize as _,
    E_WEAPON_CLASS_SHOTGUN = 1004isize as _,
    E_WEAPON_CLASS_SNIPER = 1005isize as _,
    E_WEAPON_CLASS_GRENADE_LAUNCHER = 1006isize as _,
    E_WEAPON_CLASS_RPG = 1007isize as _,
    E_WEAPON_CLASS_MINIGUN = 1008isize as _,
    E_WEAPON_CLASS_AA_GUN = 1009isize as _,
    E_WEAPON_CLASS_MORTAR = 1010isize as _,
    E_WEAPON_CLASS_50CAL = 1011isize as _,
    E_WEAPON_CLASS_SPECIAL = 1012isize as _,
    E_WEAPON_CLASS_REVOLVER = 1013isize as _,
}
fn _WeaponClass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponClass>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `CWeaponComponentBase`: the base of a weapon's components, the entries of
/// [`WeaponBase::m_WeaponComponents`](crate::equipment::weapon::WeaponBase::m_WeaponComponents). A component is a `CGameObject` of its own, so its run-time
/// type test is the game object's. Only some members are mapped, at their offsets from the start
/// of the component; the derived component types repeat the ones they use.
pub struct WeaponComponentBase {
    vftable: *const crate::equipment::weapon::WeaponComponentBaseVftable,
    _field_8: [u8; 200],
    /// The weapon the component belongs to.
    pub m_Owner: *mut crate::equipment::weapon::WeaponBase,
    /// The component's kind.
    pub m_Type: crate::equipment::weapon::WeaponComponentType,
    _field_dc: [u8; 20],
    /// Whether the component is enabled, written by [`Enable`](crate::equipment::weapon::WeaponComponentBase::Enable). The weapon runs the
    /// per-shot, per-update, and post-finalization hooks of enabled components only.
    pub m_Enabled: bool,
    _field_f1: [u8; 7],
}
fn _WeaponComponentBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xF8], WeaponComponentBase>([0u8; 0xF8]);
    }
    unreachable!()
}
impl WeaponComponentBase {
    pub fn vftable(
        &self,
    ) -> *const crate::equipment::weapon::WeaponComponentBaseVftable {
        self.vftable as *const crate::equipment::weapon::WeaponComponentBaseVftable
    }
    /// `CRtti::IsType`: whether the component is of the class `type_id` names or derives from
    /// it, as for [`GameObject::IsType`](crate::game::GameObject::IsType). The weapon code tests a
    /// component against [`ModelAttachementWeaponComponent::TYPE_ID`](crate::equipment::weapon::ModelAttachementWeaponComponent::TYPE_ID) before treating it as a
    /// model part, and that test also accepts the barrel recoil and barrel spin parts.
    pub unsafe fn IsType(&self, type_id: *const u32) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsType).read();
            f(self as *const Self as _, type_id)
        }
    }
    /// Enables or disables the component, storing [`m_Enabled`](crate::equipment::weapon::WeaponComponentBase::m_Enabled). The
    /// model-attachment component's override also shows or hides its model part through
    /// [`SetVisible`](crate::equipment::weapon::WeaponComponentBase::SetVisible) when the value changes, so on a model part it is the
    /// show and hide.
    pub unsafe fn Enable(&mut self, enable: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).Enable).read();
            f(self as *mut Self as _, enable)
        }
    }
    /// Shows or hides the component's models.
    pub unsafe fn SetVisible(&mut self, visible: bool, p2: bool) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetVisible).read();
            f(self as *mut Self as _, visible, p2)
        }
    }
}
impl std::convert::AsRef<WeaponComponentBase> for WeaponComponentBase {
    fn as_ref(&self) -> &WeaponComponentBase {
        self
    }
}
impl std::convert::AsMut<WeaponComponentBase> for WeaponComponentBase {
    fn as_mut(&mut self) -> &mut WeaponComponentBase {
        self
    }
}
#[repr(C, align(8))]
pub struct WeaponComponentBaseVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    /// `CRtti::IsType`: whether the component is of the class `type_id` names or derives from
    /// it, as for [`GameObject::IsType`](crate::game::GameObject::IsType). The weapon code tests a
    /// component against [`ModelAttachementWeaponComponent::TYPE_ID`](crate::equipment::weapon::ModelAttachementWeaponComponent::TYPE_ID) before treating it as a
    /// model part, and that test also accepts the barrel recoil and barrel spin parts.
    pub IsType: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponComponentBase,
        type_id: *const u32,
    ) -> bool,
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_25: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_26: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_27: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_28: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_29: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_30: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_31: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_33: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_34: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_35: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_36: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    /// Enables or disables the component, storing [`m_Enabled`](crate::equipment::weapon::WeaponComponentBase::m_Enabled). The
    /// model-attachment component's override also shows or hides its model part through
    /// [`SetVisible`](crate::equipment::weapon::WeaponComponentBase::SetVisible) when the value changes, so on a model part it is the
    /// show and hide.
    pub Enable: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
        enable: bool,
    ),
    _vfunc_39: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_40: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_41: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_42: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_43: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_44: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_45: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_46: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_47: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_48: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_49: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_50: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_51: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_52: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_53: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    _vfunc_54: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
    ),
    /// Shows or hides the component's models.
    pub SetVisible: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponComponentBase,
        visible: bool,
        p2: bool,
    ),
}
fn _WeaponComponentBaseVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C0], WeaponComponentBaseVftable>([0u8; 0x1C0]);
    }
    unreachable!()
}
impl WeaponComponentBaseVftable {}
impl std::convert::AsRef<WeaponComponentBaseVftable> for WeaponComponentBaseVftable {
    fn as_ref(&self) -> &WeaponComponentBaseVftable {
        self
    }
}
impl std::convert::AsMut<WeaponComponentBaseVftable> for WeaponComponentBaseVftable {
    fn as_mut(&mut self) -> &mut WeaponComponentBaseVftable {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The kind of a weapon component (`CWeaponBase::EWeaponComponentType`), stored on the component
/// as [`WeaponComponentBase::m_Type`](crate::equipment::weapon::WeaponComponentBase::m_Type). A component of a derived class carries the derived kind:
/// the barrel recoil and barrel spin components derive from the model-attachment component but
/// carry their own kinds.
pub enum WeaponComponentType {
    E_WEAPON_COMPONENT_LASER_BEAM = 0isize as _,
    E_WEAPON_COMPONENT_TURRET = 1isize as _,
    E_WEAPON_COMPONENT_BARREL_SPIN = 2isize as _,
    E_WEAPON_COMPONENT_BARREL_RECOIL = 3isize as _,
    E_WEAPON_COMPONENT_DO_ACT = 4isize as _,
    E_WEAPON_COMPONENT_MODEL_ATTACHEMENT = 5isize as _,
    E_WEAPON_COMPONENT_FOW_CONTROLLER = 6isize as _,
    E_WEAPON_COMPONENT_MAGNET = 7isize as _,
    E_WEAPON_COMPONENT_WALL_PENETRATION = 8isize as _,
    E_WEAPON_COMPONENT_SPOTLIGHT = 9isize as _,
    E_WEAPON_COMPONENT_LIGHT = 10isize as _,
    E_WEAPON_COMPONENT_VEHICLE_PART_REMOVER = 11isize as _,
    E_WEAPON_COMPONENT_AIM_POI = 12isize as _,
    E_WEAPON_COMPONENT_AIRPLANE_FLYBY = 13isize as _,
    E_WEAPON_COMPONENT_INVALID = 14isize as _,
}
fn _WeaponComponentType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponComponentType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// One physical gun of a weapon (`CWeaponData`, a `CCombinedAttachedEffectContainer`): its model,
/// attach bones, muzzle fire points, cached world matrices, and fire effects. A weapon holds one per
/// gun in [`WeaponBase::m_lWeaponData`](crate::equipment::weapon::WeaponBase::m_lWeaponData); a dual-wield weapon holds two.
pub struct WeaponData {
    _field_0: [u8; 192],
    /// The weapon this gun belongs to.
    pub m_Owner: *mut crate::equipment::weapon::WeaponBase,
    _field_c8: [u8; 8],
    /// The attach bone (an `NBone::ESafeBoneIndex`) this gun rides for each [`WeaponState`](crate::equipment::weapon::WeaponState),
    /// indexed by [`WeaponBase::m_State`](crate::equipment::weapon::WeaponBase::m_State).
    pub m_lBoneAttachementPerWeaponState: [u32; 11],
    _field_fc: [u8; 4],
    /// The gun's muzzle fire points, one per barrel.
    pub m_InternalFirePositions: crate::types::std_vector::Vector<
        crate::equipment::weapon::InternalFireInformation,
    >,
    /// The fire-point cursor: the index into
    /// [`m_InternalFirePositions`](crate::equipment::weapon::WeaponData::m_InternalFirePositions) that
    /// [`WeaponBase::GetFireFromPosition`](crate::equipment::weapon::WeaponBase::GetFireFromPosition) uses as the muzzle. The tail of
    /// [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) advances it modulo the fire-point count after each shot.
    pub m_InternalFirePositionCurrentIndex: i32,
    _field_124: [u8; 4],
    /// The gun's animated model, when it has one; its pose is computed in
    /// [`UpdatePreSim`](crate::equipment::weapon::WeaponData::UpdatePreSim) and its skinning is updated in the render phase.
    pub m_AnimatedModel: *mut crate::character::character::AnimatedModel,
    /// The gun's base model instance (an `NModelSystem::CModelInstance`). The render update pushes
    /// the interpolated world matrix to it.
    pub m_BaseModelInstance: *mut ::std::ffi::c_void,
    /// The gun's own rigid body. It is in the physics world only while the weapon's state keeps
    /// the weapon's bodies there (dropped, mounted, or turret; see [`WeaponState`](crate::equipment::weapon::WeaponState)), and its id
    /// ([`PfxRigidBody::m_BodyId`](crate::physics::PfxRigidBody::m_BodyId)) stays the same while it is out.
    /// The weapon's drop virtual ([`OnDrop`](crate::equipment::weapon::WeaponBase::OnDrop)) sets the first gun's velocity on
    /// it, and gives every gun's a fresh collision system group when the holder was a vehicle.
    pub m_PfxInstance: *mut crate::physics::PfxRigidBody,
    _field_140: [u8; 8],
    /// The gun's spare magazine: a disabled [`RigidObject`](crate::physics::RigidObject) that
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) turns into the dropped magazine,
    /// or null. [`SpawnMagazineObject`](crate::equipment::weapon::WeaponData::SpawnMagazineObject) requests it from the spawn
    /// system, which fills it in asynchronously and disables it on arrival, so it stays null for a
    /// while after each request and for good on a gun without a magazine prop.
    pub m_MagazineObject: crate::types::shared_ptr::SharedPtr<
        crate::physics::RigidObject,
    >,
    /// The gun's world matrix as of the previous simulation tick.
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    /// The gun's world matrix as of the current simulation tick, refreshed from the attach bone by
    /// the model-instance attachment update. Sim-side readers such as the fire origin read it
    /// through [`GetWorldMatrixT1`](crate::equipment::weapon::WeaponData::GetWorldMatrixT1); the render update interpolates from
    /// [`m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) toward it.
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    _field_1d8: [u8; 152],
    /// The gun's measured world velocity, metres per second.
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) gives it to the dropped magazine.
    pub m_MeasuredWorldVelocity: crate::types::math::Vector3,
    /// The gun's measured angular velocity, which
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) gives to the dropped magazine.
    pub m_MeasuredAngularVelocity: crate::types::math::Vector3,
    _field_288: [u8; 28],
    /// The spawn tag of the gun's magazine prop, from the weapon's properties, or
    /// [`NO_MAGAZINE_UNIQUE_TAG`](crate::equipment::weapon::WeaponData::NO_MAGAZINE_UNIQUE_TAG) for a gun without one, for which
    /// [`SpawnMagazineObject`](crate::equipment::weapon::WeaponData::SpawnMagazineObject) does nothing.
    pub m_MagazineUniqueTag: i32,
    /// Seconds since this gun last fired; reset by [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile).
    pub m_TimeSinceLastFire: f32,
    /// The gun's shot counter for tracers: the bullet weapons' shot
    /// ([`BulletWeaponBase::Fire_Projectile`](crate::equipment::weapon::BulletWeaponBase::Fire_Projectile)) increments it per bullet and draws a tracer every
    /// tuning-set number of bullets. [`WeaponBase::Fire_StartBase`](crate::equipment::weapon::WeaponBase::Fire_StartBase) resets it at the first shot of
    /// a pull, and [`WeaponBase::UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) resets it after a long enough pause in
    /// fire.
    pub m_TracerCounter: u16,
    /// The gun's counter of tracers, which the bullet weapons' shot increments per tracer to give
    /// every tuning-set number of tracers a smoke trail. Reset with
    /// [`m_TracerCounter`](crate::equipment::weapon::WeaponData::m_TracerCounter).
    pub m_SmokeTrailCounter: u16,
    /// Shots of the gun's current burst still to fire. [`WeaponBase::Fire_StartBase`](crate::equipment::weapon::WeaponBase::Fire_StartBase) loads it
    /// from the tuning at the first shot of a pull, and while it is non-zero
    /// [`WeaponBase::UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) tries to fire this gun whatever the trigger input.
    pub m_BurstShotsLeft: u32,
    _field_2b4: [u8; 40],
    /// The uniform scale applied to the gun's world matrix while holstered.
    pub m_HolsteredScaleValue: f32,
    _field_2e0: [u8; 5],
    /// Whether the gun's next placement skips the render interpolation: when set, the gun's
    /// attachment placement copies the new world matrix into
    /// [`m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) as well, so the render update draws the gun
    /// at its new place without blending from the old. Set by
    /// [`WeaponBase::SkipRenderInterpolationForOneFrame`](crate::equipment::weapon::WeaponBase::SkipRenderInterpolationForOneFrame) and cleared by
    /// [`WeaponBase::UpdatePostFinalizePose`](crate::equipment::weapon::WeaponBase::UpdatePostFinalizePose).
    pub m_SkipRenderInterpolation: bool,
    /// Whether this gun leads a multi-gun first shot: [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile) clears it on
    /// every gun and sets it on the firing one.
    pub m_IsLeadingWeaponFire: bool,
    _field_2e7: [u8; 1],
}
fn _WeaponData_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2E8], WeaponData>([0u8; 0x2E8]);
    }
    unreachable!()
}
impl WeaponData {
    pub const AddToWorld_ADDRESS: usize = 0x140959570;
    /// Puts the gun's own body ([`m_PfxInstance`](crate::equipment::weapon::WeaponData::m_PfxInstance)) into the physics world
    /// (`CWeaponData::AddToWorld`, which the disassembler names `CPfxVehicle::OnStoppedTrailing`).
    /// It keeps the body's collision system group, or takes a fresh one when the group is zero,
    /// sets its collision layer for the owning weapon's state, makes it dynamic, and places it at
    /// the gun's world matrix ([`m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1)), so the body's frame is
    /// the gun's model frame and its shape is in that frame. The weapon's own world addition
    /// calls it for each gun but a dual-wielded weapon's second (`CWeaponBase::AddToWorld`, which
    /// the disassembler names `CWeaponBase::RemoveFromWorld`; the disassembler's
    /// `CWeaponBase::AddToWorld` is the removal).
    pub unsafe fn AddToWorld(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::AddToWorld_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetWorldMatrixT1_ADDRESS: usize = 0x1409601E0;
    /// Returns [`m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1), scaled by
    /// [`m_HolsteredScaleValue`](crate::equipment::weapon::WeaponData::m_HolsteredScaleValue) when the owning weapon is
    /// [`E_WS_HOLSTERED`](crate::equipment::weapon::WeaponState::E_WS_HOLSTERED). The result is written through `result`
    /// and returned.
    pub unsafe fn GetWorldMatrixT1(
        &self,
        result: *mut crate::types::math::Matrix4,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                result: *mut crate::types::math::Matrix4,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::GetWorldMatrixT1_ADDRESS,
            );
            f(self as *const Self as _, result)
        }
    }
    pub const UpdatePreSim_ADDRESS: usize = 0x140959160;
    /// The gun's pre-simulation update (`CWeaponData::UpdatePreSim`), run for each gun by the
    /// weapon's own pre-simulation update (`CWeaponBase::UpdatePreSim`). When the gun has an
    /// animated model ([`m_AnimatedModel`](crate::equipment::weapon::WeaponData::m_AnimatedModel)) and the weapon's LOD does not
    /// switch its animation off, it advances the model's clips, track messages, rule systems, and
    /// state tasks by the context's step, then computes the model-space pose
    /// (`CPoseProducer::CalculateModelSpacePose`), which the joint accessors of the model's
    /// animation controller
    /// ([`AnimationController::GetJoint`](crate::character::character::AnimationController::GetJoint) and
    /// [`SetJoint`](crate::character::character::AnimationController::SetJoint)) read and write and which
    /// the render phase skins. It then measures the gun's velocities from its two cached world
    /// matrices ([`m_MeasuredWorldVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredWorldVelocity) and
    /// [`m_MeasuredAngularVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredAngularVelocity)). Returns whether the
    /// animated model was updated. `context` is the game object update context, whose first field
    /// is the step in seconds.
    pub unsafe fn UpdatePreSim(&mut self, context: *mut ::std::ffi::c_void) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::UpdatePreSim_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const UpdateAttachmentTransforms_ADDRESS: usize = 0x140986F80;
    /// Places the gun for `state` (`CWeaponData::UpdateAttachmentTransforms`), writing
    /// [`m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1). With `update_type` 2 (advance) it first copies
    /// the current matrix into [`m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0). Held by a vehicle, the
    /// gun follows its anchor or part; while wingsuiting it is built from the holder's reference
    /// and head bones and its flight direction; dropped, mounted, or on a turret it follows its own
    /// rigid body when that is in the world; on a weapon rack it follows its placement rule.
    /// Otherwise, held by a character, it takes the world matrix of the bone its attach-bone table
    /// ([`m_lBoneAttachementPerWeaponState`](crate::equipment::weapon::WeaponData::m_lBoneAttachementPerWeaponState)) names for
    /// `state`
    /// ([`Character::GetWorldMatrixForBone`](crate::character::character::Character::GetWorldMatrixForBone)):
    /// a hand attach bone's matrix as it is, and a holster or back bone's composed with the gun's
    /// attach offset when the weapon's attach tuning asks for it. So a hand-held gun's pose in the
    /// hand is the attach bone's pose relative to the wrist, which the animation sets. While
    /// [`m_SkipRenderInterpolation`](crate::equipment::weapon::WeaponData::m_SkipRenderInterpolation) or
    /// `skip_render_interpolation` is set, and for the initial placement (`update_type` 0), it
    /// copies the new matrix into [`m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) too. Each weapon's
    /// placement calls it for every gun; the character's pose-finalization pass places the weapons
    /// through
    /// [`Inventory::UpdateAttachmentTransforms`](crate::character::inventory::Inventory::UpdateAttachmentTransforms).
    pub unsafe fn UpdateAttachmentTransforms(
        &mut self,
        holder: *mut crate::game::GameObject,
        snow_factor: f32,
        water_factor: f32,
        state: crate::equipment::weapon::WeaponState,
        skip_render_interpolation: bool,
        update_type: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                holder: *mut crate::game::GameObject,
                snow_factor: f32,
                water_factor: f32,
                state: crate::equipment::weapon::WeaponState,
                skip_render_interpolation: bool,
                update_type: u32,
            ) = ::std::mem::transmute(Self::UpdateAttachmentTransforms_ADDRESS);
            f(
                self as *mut Self as _,
                holder,
                snow_factor,
                water_factor,
                state,
                skip_render_interpolation,
                update_type,
            )
        }
    }
    pub const ThrowAwayMagazineObject_ADDRESS: usize = 0x140986B60;
    /// Drops the gun's spare magazine ([`m_MagazineObject`](crate::equipment::weapon::WeaponData::m_MagazineObject)) as a physics
    /// prop, for `character` holding the gun. `model_name_hash` picks where the magazine leaves
    /// from:
    ///
    /// - zero, or [`NO_MAGAZINE_MODEL_NAME_HASH`](crate::equipment::weapon::WeaponData::NO_MAGAZINE_MODEL_NAME_HASH): from the gun's
    ///   own [`m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1), touching no model part;
    /// - any other hash: from the world matrix
    ///   ([`ModelAttachementWeaponComponent::m_WorldMatrix`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_WorldMatrix)) of the owner's model-attachment
    ///   component whose [`m_NameHash`](crate::equipment::weapon::ModelAttachementWeaponComponent::m_NameHash) matches. It
    ///   walks a copy of the owner's [`m_WeaponComponents`](crate::equipment::weapon::WeaponBase::m_WeaponComponents),
    ///   taking only a component whose [`m_Type`](crate::equipment::weapon::WeaponComponentBase::m_Type) is
    ///   [`E_WEAPON_COMPONENT_MODEL_ATTACHEMENT`](crate::equipment::weapon::WeaponComponentType::E_WEAPON_COMPONENT_MODEL_ATTACHEMENT)
    ///   and that passes [`IsType`](crate::equipment::weapon::WeaponComponentBase::IsType) for
    ///   [`ModelAttachementWeaponComponent::TYPE_ID`](crate::equipment::weapon::ModelAttachementWeaponComponent::TYPE_ID), and hides that part with
    ///   [`Enable`](crate::equipment::weapon::WeaponComponentBase::Enable)`(false)`. With no matching part nothing is
    ///   dropped or hidden, and it returns false.
    ///
    /// When there is a spare magazine and a place to drop it from, it makes the prop dynamic,
    /// places it there, gives its body the character's collision system group (so it does not
    /// collide with the character), the gun's
    /// [`m_MeasuredWorldVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredWorldVelocity) with any upward component
    /// removed, and the gun's [`m_MeasuredAngularVelocity`](crate::equipment::weapon::WeaponData::m_MeasuredAngularVelocity), and
    /// a body material, then enables it, adds it to the update, and releases the gun's reference,
    /// so [`m_MagazineObject`](crate::equipment::weapon::WeaponData::m_MagazineObject) is null afterwards. It returns true when it
    /// found a place to drop from, whether or not a spare magazine was there to drop.
    ///
    /// It writes physics-body state (velocities and the body material through the world), which
    /// the physics system allows only on the thread holding the physics world's write mark. Its
    /// only caller is the throw-away-magazine track message
    /// ([`TRACK_MESSAGE_THROW_AWAY_MAGAZINE`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_THROW_AWAY_MAGAZINE)), which
    /// runs in the holder's serial post-physics pass on the game update thread.
    pub unsafe fn ThrowAwayMagazineObject(
        &mut self,
        character: *mut crate::character::character::Character,
        model_name_hash: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                character: *mut crate::character::character::Character,
                model_name_hash: u32,
            ) -> bool = ::std::mem::transmute(Self::ThrowAwayMagazineObject_ADDRESS);
            f(self as *mut Self as _, character, model_name_hash)
        }
    }
    pub const SpawnMagazineObject_ADDRESS: usize = 0x14097B1D0;
    /// Requests the gun's next spare magazine: releases [`m_MagazineObject`](crate::equipment::weapon::WeaponData::m_MagazineObject)
    /// and asks the spawn system to spawn the prop tagged
    /// [`m_MagazineUniqueTag`](crate::equipment::weapon::WeaponData::m_MagazineUniqueTag) at the gun's
    /// [`m_WorldMatrixT1`](crate::equipment::weapon::WeaponData::m_WorldMatrixT1). The spawn completes later; its callback stores
    /// the prop in `m_MagazineObject` and disables it until it is thrown. Does nothing for a gun
    /// without a magazine prop. The weapon's post-initialization calls it for the first spare, and
    /// the throw-away-magazine track message after each throw.
    pub unsafe fn SpawnMagazineObject(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SpawnMagazineObject_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DoAct_ADDRESS: usize = 0x1409512F0;
    /// Runs `act` (an event id, as [`Character::QueueAct`](crate::character::character::Character::QueueAct)
    /// takes) on the gun's own animated model ([`m_AnimatedModel`](crate::equipment::weapon::WeaponData::m_AnimatedModel)), on
    /// its first layer, and returns whether the model took it; false for a gun without an
    /// animated model. The do-act component ([`DoActWeaponComponent`](crate::equipment::weapon::DoActWeaponComponent)) plays a gun's own clips,
    /// such as a pump, through it.
    pub unsafe fn DoAct(&mut self, act: *const u32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, act: *const u32) -> bool = ::std::mem::transmute(
                Self::DoAct_ADDRESS,
            );
            f(self as *mut Self as _, act)
        }
    }
}
impl WeaponData {
    /// The `model_name_hash` (the hash of `no_magazine_model`) that makes
    /// [`ThrowAwayMagazineObject`](crate::equipment::weapon::WeaponData::ThrowAwayMagazineObject) drop the spare magazine from the
    /// gun's own transform without looking for a model part.
    pub const NO_MAGAZINE_MODEL_NAME_HASH: u32 = 423049129;
    /// The [`m_MagazineUniqueTag`](crate::equipment::weapon::WeaponData::m_MagazineUniqueTag) of a gun without a magazine prop.
    pub const NO_MAGAZINE_UNIQUE_TAG: i32 = -1;
    /// The name hash (`hashlittle` of `Pump`) of the pump bone in a pump-action gun's own skeleton,
    /// the U-Rys 96's: the skeleton's one bone besides its `Reference` root, which carries the pump
    /// and which the gun's pump clip moves. Resolved to an index through the gun's animation
    /// controller ([`AnimationController::GetBoneIndex`](crate::character::character::AnimationController::GetBoneIndex)).
    pub const PUMP_BONE_HASH: u32 = 2521734759;
    /// The name hash (`hashlittle` of `Slider`) of the bolt bone in a bolt-action rifle's own
    /// skeleton, the CS111 Archangel's and the Final Argument's: the skeleton's one bone besides its
    /// `Reference` root, which carries the bolt and which the rifle's bolt-action clip moves.
    pub const SLIDER_BONE_HASH: u32 = 2064990683;
}
impl std::convert::AsRef<WeaponData> for WeaponData {
    fn as_ref(&self) -> &WeaponData {
        self
    }
}
impl std::convert::AsMut<WeaponData> for WeaponData {
    fn as_mut(&mut self) -> &mut WeaponData {
        self
    }
}
#[repr(C, align(8))]
/// The weapon's list of [`WeaponData`](crate::equipment::weapon::WeaponData) entries (an engine `TArray<CWeaponData*>`): begin, end, and
/// capacity-end pointers followed by a user-allocated flag.
pub struct WeaponDataArray {
    /// The first entry.
    pub m_Begin: *mut *mut crate::equipment::weapon::WeaponData,
    /// One past the last live entry.
    pub m_End: *mut *mut crate::equipment::weapon::WeaponData,
    /// One past the allocated capacity.
    pub m_Last: *mut *mut crate::equipment::weapon::WeaponData,
    _field_18: [u8; 8],
}
fn _WeaponDataArray_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], WeaponDataArray>([0u8; 0x20]);
    }
    unreachable!()
}
impl WeaponDataArray {}
impl std::convert::AsRef<WeaponDataArray> for WeaponDataArray {
    fn as_ref(&self) -> &WeaponDataArray {
        self
    }
}
impl std::convert::AsMut<WeaponDataArray> for WeaponDataArray {
    fn as_mut(&mut self) -> &mut WeaponDataArray {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The hand a weapon grip-table entry is for (`EWeaponGripHand`), as
/// [`WeaponBase::GetGripPosition`](crate::equipment::weapon::WeaponBase::GetGripPosition) takes it.
pub enum WeaponGripHand {
    E_WEAPON_GRIP_HAND_LEFT = 0isize as _,
    E_WEAPON_GRIP_HAND_RIGHT = 1isize as _,
}
fn _WeaponGripHand_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponGripHand>([0u8; 0x4]);
    }
    unreachable!()
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// The fire inputs a weapon's user hands its per-tick update (`SWeaponInputs`), each `0..=1`. The
/// base update ([`WeaponBase::UpdateFromUserPostSim_ParallelBase`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_ParallelBase)) copies them before passing them
/// on to [`WeaponBase::UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel).
pub struct WeaponInputs {
    /// The primary trigger, compared against the tuning's
    /// [`m_trigger_sensitivity`](crate::equipment::weapon::WeaponTuning::m_trigger_sensitivity).
    pub m_PrimaryFireInput: f32,
    /// The secondary trigger, for weapons with a secondary action.
    pub m_SecondaryFireInput: f32,
}
fn _WeaponInputs_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], WeaponInputs>([0u8; 0x8]);
    }
    unreachable!()
}
impl WeaponInputs {}
impl std::convert::AsRef<WeaponInputs> for WeaponInputs {
    fn as_ref(&self) -> &WeaponInputs {
        self
    }
}
impl std::convert::AsMut<WeaponInputs> for WeaponInputs {
    fn as_mut(&mut self) -> &mut WeaponInputs {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The situation a weapon is in (`CWeaponData::EWeaponState`), stored on the weapon as
/// [`WeaponBase::m_State`](crate::equipment::weapon::WeaponBase::m_State). It selects the weapon's attach bone
/// ([`WeaponData::m_lBoneAttachementPerWeaponState`](crate::equipment::weapon::WeaponData::m_lBoneAttachementPerWeaponState) is indexed by it) and, when holstered, the
/// holstered scale applied to the weapon's world matrix.
pub enum WeaponState {
    E_WS_DROPPED = 0isize as _,
    E_WS_WIELDING = 1isize as _,
    E_WS_DUAL_WIELDING = 2isize as _,
    E_WS_HOLSTERED = 3isize as _,
    E_WS_HOLSTERED_INVISIBLE = 4isize as _,
    E_WS_MOUNTED = 5isize as _,
    E_WS_TURRET = 6isize as _,
    E_WS_VEHICLE = 7isize as _,
    E_WS_PICKING_UP = 8isize as _,
    E_WS_WEAPON_RACK = 9isize as _,
    E_WS_WINGSUIT_WEAPON = 10isize as _,
    E_WS_NUM = 11isize as _,
}
fn _WeaponState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A weapon's tuning record (`SWeaponTuning`), an ADF resource shared by every instance of a weapon
/// type and reached through [`WeaponBase::m_Tuning`](crate::equipment::weapon::WeaponBase::m_Tuning). Only the fields read by the mapped functions
/// are bound.
pub struct WeaponTuning {
    _field_0: [u8; 64],
    /// The magazine capacity. Negative means the weapon has no magazine limit: it never reloads,
    /// and [`WeaponBase::Reload`](crate::equipment::weapon::WeaponBase::Reload) and [`WeaponBase::CanReload`](crate::equipment::weapon::WeaponBase::CanReload) refuse it.
    pub m_magazine_size: i32,
    /// The weapon's maximum aim range in metres. The player's aim control takes the largest of
    /// these over the wielded, wingsuit, and vehicle weapons as the weapon slot's maximum aim
    /// distance.
    pub m_max_range: f32,
    /// The reload timing. For an
    /// [`E_RELOAD_NO_ANIMATION`](crate::equipment::weapon::ReloadType::E_RELOAD_NO_ANIMATION) weapon the ammunition check
    /// loads it into [`WeaponBase::m_ReloadTimeLeft`](crate::equipment::weapon::WeaponBase::m_ReloadTimeLeft) as the reload's duration in seconds.
    pub m_reload_time_scale: f32,
    _field_4c: [u8; 4],
    /// The seconds between shots: [`WeaponBase::UpdateWeapon_Parallel`](crate::equipment::weapon::WeaponBase::UpdateWeapon_Parallel) starts a shot only once
    /// [`WeaponBase::m_TimeSinceLastFire`](crate::equipment::weapon::WeaponBase::m_TimeSinceLastFire) has reached it. The interval is weapon-wide, so the guns
    /// of a multi-gun weapon share it.
    pub m_fire_rate: f32,
    /// The replacement for [`m_fire_rate`](crate::equipment::weapon::WeaponTuning::m_fire_rate) once the player owns the weapon's
    /// fire-rate upgrade.
    pub m_fire_rate_level_2: f32,
    _field_58: [u8; 8],
    /// Whether a held trigger keeps firing. Otherwise a local player's weapon fires once per pull
    /// ([`WeaponBase::m_FiredThisInput`](crate::equipment::weapon::WeaponBase::m_FiredThisInput)); other users always fire while the trigger is pulled.
    pub m_fully_automatic: bool,
    _field_61: [u8; 3],
    /// The trigger thresholds: index 0 is the value a released trigger must exceed to count as
    /// pulled, index 1 the lower value a pulled trigger must stay above.
    pub m_trigger_sensitivity: [f32; 2],
    /// A per-weapon replacement for the character's aim-timer refill value; non-negative values
    /// override the character's aim settings (see [`WeaponBase::GetAimTimerOverride`](crate::equipment::weapon::WeaponBase::GetAimTimerOverride)).
    pub m_aim_timer_override: f32,
    _field_70: [u8; 20],
    /// Whether the weapon type has unlimited ammunition: one of the conditions of
    /// [`WeaponBase::HasUnlimitedAmmo`](crate::equipment::weapon::WeaponBase::HasUnlimitedAmmo).
    pub m_unlimited_ammo: bool,
    _field_85: [u8; 3],
}
fn _WeaponTuning_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x88], WeaponTuning>([0u8; 0x88]);
    }
    unreachable!()
}
impl WeaponTuning {}
impl std::convert::AsRef<WeaponTuning> for WeaponTuning {
    fn as_ref(&self) -> &WeaponTuning {
        self
    }
}
impl std::convert::AsMut<WeaponTuning> for WeaponTuning {
    fn as_mut(&mut self) -> &mut WeaponTuning {
        self
    }
}
