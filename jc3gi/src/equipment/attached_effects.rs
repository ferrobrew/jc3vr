#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A game object's set of attached effects and sounds (`CAttachedEffectContainer`, a base of weapons,
/// the grappling hook, and other game objects), with the effect parameter sources that feed them.
pub struct AttachedEffectContainer {
    _field_0: [u8; 40],
    /// The head of the parameter-source list.
    pub m_ParameterSources: *mut ::std::ffi::c_void,
    /// The head of the playing-instance list.
    pub m_PlayingInstances: *mut crate::equipment::attached_effects::AttachedInstance,
    /// The usage table's buckets: one entry index per bucket into
    /// [`m_UsageEntries`](crate::equipment::attached_effects::AttachedEffectContainer::m_UsageEntries), `0xFFFF` for an empty bucket. A usage hash's
    /// bucket is the hash modulo [`m_UsageBucketCount`](crate::equipment::attached_effects::AttachedEffectContainer::m_UsageBucketCount).
    pub m_UsageBuckets: *mut u16,
    /// The usage table's entries, chained per bucket; see [`AttachedEffectUsageEntry`](crate::equipment::attached_effects::AttachedEffectUsageEntry).
    pub m_UsageEntries: *mut crate::equipment::attached_effects::AttachedEffectUsageEntry,
    /// The number of buckets in [`m_UsageBuckets`](crate::equipment::attached_effects::AttachedEffectContainer::m_UsageBuckets).
    pub m_UsageBucketCount: u16,
    _field_4a: [u8; 6],
    /// The number of usages in the table; zero for a container that loaded none.
    pub m_UsageCount: u16,
    _field_52: [u8; 54],
    /// The container's accumulated time, advanced by [`UpdateAttachedEffects`](crate::equipment::attached_effects::AttachedEffectContainer::UpdateAttachedEffects).
    pub m_CurrentTime: f32,
    _field_8c: [u8; 4],
}
fn _AttachedEffectContainer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], AttachedEffectContainer>([0u8; 0x90]);
    }
    unreachable!()
}
impl AttachedEffectContainer {
    pub const UpdateAttachedEffects_ADDRESS: usize = 0x140466F30;
    /// Advances the container by `dt`: for each playing instance, runs
    /// [`AttachedInstance::UpdateTransform`](crate::equipment::attached_effects::AttachedInstance::UpdateTransform) and the instance update, and frees instances whose
    /// effects and sounds have all finished; then re-evaluates the parameter sources and pushes
    /// changed values to their sound and effect outputs.
    pub unsafe fn UpdateAttachedEffects(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateAttachedEffects_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const GetDynamicAttachedEffects_ADDRESS: usize = 0x140453900;
    /// The one-off effect and sound the container's dynamic usage `usage_hash` plays against
    /// `target_material` (an `EGameMaterialId`), for the usage's variant whose source material is
    /// `source_material`, falling back to its
    /// [`EGameMaterialId_Default`](crate::physics::GameMaterialId::EGameMaterialId_Default) variant. Writes the
    /// variant's retrigger time to `retrigger_time` when it is not null. A usage the container has
    /// not loaded returns a shared empty pair, both of whose pointers are null.
    pub unsafe fn GetDynamicAttachedEffects(
        &mut self,
        usage_hash: u32,
        source_material: crate::physics::GameMaterialId,
        target_material: u32,
        retrigger_time: *mut f32,
    ) -> *const crate::equipment::attached_effects::EffectAndSoundPair {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                usage_hash: u32,
                source_material: crate::physics::GameMaterialId,
                target_material: u32,
                retrigger_time: *mut f32,
            ) -> *const crate::equipment::attached_effects::EffectAndSoundPair = ::std::mem::transmute(
                Self::GetDynamicAttachedEffects_ADDRESS,
            );
            f(
                self as *mut Self as _,
                usage_hash,
                source_material,
                target_material,
                retrigger_time,
            )
        }
    }
    pub const FindAttachedEffectUsage_ADDRESS: usize = 0x14044BB30;
    /// The container's attached effect usage whose hash is `usage_hash`
    /// (`hashlittle` of the usage's name), or null when the container did not load one. Owners
    /// resolve their usages once, when their properties are set, and play them through
    /// [`ActivateAttachedEffect`](crate::equipment::attached_effects::AttachedEffectContainer::ActivateAttachedEffect).
    pub unsafe fn FindAttachedEffectUsage(
        &mut self,
        usage_hash: u32,
    ) -> *mut crate::equipment::attached_effects::AttachedEffectUsage {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                usage_hash: u32,
            ) -> *mut crate::equipment::attached_effects::AttachedEffectUsage = ::std::mem::transmute(
                Self::FindAttachedEffectUsage_ADDRESS,
            );
            f(self as *mut Self as _, usage_hash)
        }
    }
    pub const ActivateAttachedEffect_ADDRESS: usize = 0x140469340;
    /// Starts `usage`'s effect and sound for `material` (an `EGameMaterialId`, usually
    /// [`EGameMaterialId_Default`](crate::physics::GameMaterialId::EGameMaterialId_Default)), attached to the
    /// container's owner. With `restart_if_playing`, an instance of the usage that is still
    /// playing is stopped and started again; without it, a playing instance for the same material
    /// is left to play. A null `usage` does nothing. Returns whether an instance plays.
    pub unsafe fn ActivateAttachedEffect(
        &mut self,
        usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
        restart_if_playing: bool,
        material: crate::physics::GameMaterialId,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
                restart_if_playing: bool,
                material: crate::physics::GameMaterialId,
            ) -> bool = ::std::mem::transmute(Self::ActivateAttachedEffect_ADDRESS);
            f(self as *mut Self as _, usage, restart_if_playing, material)
        }
    }
    pub const DeactivateAttachedEffect_ADDRESS: usize = 0x140465D60;
    /// Stops `usage`'s playing instances: its effects with `effect_stop_mode` and its sounds with
    /// `sound_stop_mode`.
    pub unsafe fn DeactivateAttachedEffect(
        &mut self,
        usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
        effect_stop_mode: crate::equipment::attached_effects::EffectStopMode,
        sound_stop_mode: crate::equipment::attached_effects::SoundStopMode,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
                effect_stop_mode: crate::equipment::attached_effects::EffectStopMode,
                sound_stop_mode: crate::equipment::attached_effects::SoundStopMode,
            ) = ::std::mem::transmute(Self::DeactivateAttachedEffect_ADDRESS);
            f(self as *mut Self as _, usage, effect_stop_mode, sound_stop_mode)
        }
    }
    pub const IsAttachedEffectLooping_ADDRESS: usize = 0x140441CE0;
    /// Whether `usage` loops for `material`: whether any of its variants resolves to an effect or a
    /// sound that loops. A looping usage plays until it is deactivated.
    pub unsafe fn IsAttachedEffectLooping(
        &self,
        usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
        material: crate::physics::GameMaterialId,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
                material: crate::physics::GameMaterialId,
            ) -> bool = ::std::mem::transmute(Self::IsAttachedEffectLooping_ADDRESS);
            f(self as *const Self as _, usage, material)
        }
    }
}
impl AttachedEffectContainer {
    /// The usage hash of a character's melee impact effects (`hashlittle("MeleeImpact")`): the
    /// effect and sound a character's melee hit plays where it lands, picked by the struck
    /// surface's material. The melee hit track message in `CCharacter::HandleTrackMessage` looks
    /// it up on the attacking character's own container with
    /// [`GetDynamicAttachedEffects`](crate::equipment::attached_effects::AttachedEffectContainer::GetDynamicAttachedEffects), against
    /// [`EGameMaterialId_Flesh`](crate::physics::GameMaterialId::EGameMaterialId_Flesh) for a character and the
    /// hit shape's material for anything else, and plays the result with
    /// [`EffectAndSoundPair::PlayOneOffs`](crate::equipment::attached_effects::EffectAndSoundPair::PlayOneOffs) at a [`CreateCollisionMatrix`](crate::equipment::attached_effects::CreateCollisionMatrix) facing back along the
    /// hit.
    pub const MELEE_IMPACT_USAGE: u32 = 3539425100;
}
impl std::convert::AsRef<AttachedEffectContainer> for AttachedEffectContainer {
    fn as_ref(&self) -> &AttachedEffectContainer {
        self
    }
}
impl std::convert::AsMut<AttachedEffectContainer> for AttachedEffectContainer {
    fn as_mut(&mut self) -> &mut AttachedEffectContainer {
        self
    }
}
#[repr(C, align(8))]
/// One attached effect usage of a container (`NAttachedEffectContainer::SAttachedEffectBase`): a
/// named slot that resolves to an effect and a sound per material.
pub struct AttachedEffectUsage {}
impl AttachedEffectUsage {}
impl std::convert::AsRef<AttachedEffectUsage> for AttachedEffectUsage {
    fn as_ref(&self) -> &AttachedEffectUsage {
        self
    }
}
impl std::convert::AsMut<AttachedEffectUsage> for AttachedEffectUsage {
    fn as_mut(&mut self) -> &mut AttachedEffectUsage {
        self
    }
}
#[repr(C, align(8))]
/// One entry of an [`AttachedEffectContainer`](crate::equipment::attached_effects::AttachedEffectContainer)'s usage table.
pub struct AttachedEffectUsageEntry {
    /// The usage's hash (`hashlittle` of its name).
    pub m_Hash: u32,
    /// The index of the next entry in the bucket's chain, `0xFFFF` at its end.
    pub m_Next: u16,
    _field_6: [u8; 2],
    /// The usage.
    pub m_Usage: *mut crate::equipment::attached_effects::AttachedEffectUsage,
}
fn _AttachedEffectUsageEntry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], AttachedEffectUsageEntry>([0u8; 0x10]);
    }
    unreachable!()
}
impl AttachedEffectUsageEntry {}
impl std::convert::AsRef<AttachedEffectUsageEntry> for AttachedEffectUsageEntry {
    fn as_ref(&self) -> &AttachedEffectUsageEntry {
        self
    }
}
impl std::convert::AsMut<AttachedEffectUsageEntry> for AttachedEffectUsageEntry {
    fn as_mut(&mut self) -> &mut AttachedEffectUsageEntry {
        self
    }
}
#[repr(C, align(16))]
/// One playing attached effect or sound (`NAttachedEffectContainer::SAttachedInstance`): an offset
/// and a transform function that resolves where it is attached (typically a bone or socket on the
/// owner's model, such as a weapon muzzle), the effect and sound instances, and the resolved
/// placement. The container links its playing instances through
/// [`m_NextPlaying`](crate::equipment::attached_effects::AttachedInstance::m_NextPlaying).
pub struct AttachedInstance {
    /// The instance's offset from its attachment point.
    pub m_Offset: crate::types::math::Matrix4,
    /// Resolves the attachment's world placement: called with the result buffer, the owning
    /// container, this instance, and [`m_TransformExtraData`](crate::equipment::attached_effects::AttachedInstance::m_TransformExtraData).
    pub m_TransformFunc: *mut ::std::ffi::c_void,
    _field_48: [u8; 4],
    /// How the instance's rotation follows the attachment.
    pub m_RotationMode: crate::equipment::attached_effects::RotationMode,
    /// How the instance's translation follows the attachment.
    pub m_TranslationMode: crate::equipment::attached_effects::TranslationMode,
    _field_54: [u8; 68],
    /// The next playing instance in the owning container's list, or null.
    pub m_NextPlaying: *mut crate::equipment::attached_effects::AttachedInstance,
    _field_a0: [u8; 80],
    /// The resolved placement, refreshed by [`UpdateTransform`](crate::equipment::attached_effects::AttachedInstance::UpdateTransform).
    pub m_TransformInfo: crate::equipment::attached_effects::EffectTransformInfo,
    /// The per-update velocity of the placement's translation: the position delta multiplied by
    /// 1/30, or zero for a transform that is not
    /// [`ETransformType_Transform3D`](crate::equipment::attached_effects::TransformType::ETransformType_Transform3D).
    pub m_MeasuredVelocity: crate::types::math::Vector3,
    _field_15c: [u8; 4],
    /// The opaque argument passed to [`m_TransformFunc`](crate::equipment::attached_effects::AttachedInstance::m_TransformFunc).
    pub m_TransformExtraData: u64,
    _field_168: [u8; 8],
}
fn _AttachedInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x170], AttachedInstance>([0u8; 0x170]);
    }
    unreachable!()
}
impl AttachedInstance {
    pub const UpdateTransform_ADDRESS: usize = 0x140453D00;
    /// Re-resolves the instance's placement for `container`. When the rotation mode is
    /// [`ERotationMode_ATTACHED`](crate::equipment::attached_effects::RotationMode::ERotationMode_ATTACHED) or
    /// [`ERotationMode_ATTACHED_RANDOM_ROLL`](crate::equipment::attached_effects::RotationMode::ERotationMode_ATTACHED_RANDOM_ROLL), or
    /// the translation mode is
    /// [`ETranslationMode_ATTACHED`](crate::equipment::attached_effects::TranslationMode::ETranslationMode_ATTACHED), it calls [`m_TransformFunc`](crate::equipment::attached_effects::AttachedInstance::m_TransformFunc) and copies
    /// the [`EffectTransformInfo`](crate::equipment::attached_effects::EffectTransformInfo) it returns into
    /// [`m_TransformInfo`](crate::equipment::attached_effects::AttachedInstance::m_TransformInfo); it then applies the transform modes and derives
    /// [`m_MeasuredVelocity`](crate::equipment::attached_effects::AttachedInstance::m_MeasuredVelocity) from the translation delta.
    pub unsafe fn UpdateTransform(
        &mut self,
        container: *mut crate::equipment::attached_effects::AttachedEffectContainer,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                container: *mut crate::equipment::attached_effects::AttachedEffectContainer,
            ) = ::std::mem::transmute(Self::UpdateTransform_ADDRESS);
            f(self as *mut Self as _, container)
        }
    }
}
impl std::convert::AsRef<AttachedInstance> for AttachedInstance {
    fn as_ref(&self) -> &AttachedInstance {
        self
    }
}
impl std::convert::AsMut<AttachedInstance> for AttachedInstance {
    fn as_mut(&mut self) -> &mut AttachedInstance {
        self
    }
}
#[repr(C, align(8))]
/// A one-off effect and sound (`SEffectAndSoundPair`): a particle effect resource and an FMOD
/// sound resource, either of which may be null. Material effect tables and dynamic attached
/// effect usages resolve to these.
pub struct EffectAndSoundPair {
    /// The particle effect (`boost::shared_ptr<SEffectResourceDataWrapper>`).
    pub m_Effect: crate::types::shared_ptr::SharedPtr<::std::ffi::c_void>,
    /// The sound (`boost::shared_ptr<CSoundResource>`).
    pub m_Sound: crate::types::shared_ptr::SharedPtr<::std::ffi::c_void>,
}
fn _EffectAndSoundPair_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], EffectAndSoundPair>([0u8; 0x20]);
    }
    unreachable!()
}
impl EffectAndSoundPair {
    pub const PlayOneOffs_ADDRESS: usize = 0x1404B0740;
    /// Fires the pair once at `transform`, without keeping a handle to either instance: with
    /// [`EffectCull::m_PlayEffect`](crate::equipment::attached_effects::EffectCull::m_PlayEffect), plays the effect there (projecting decals onto
    /// `decal_models` along `decal_normal`, both of which may be null) and sets its velocity
    /// parameter from `velocity`; with [`EffectCull::m_PlaySound`](crate::equipment::attached_effects::EffectCull::m_PlaySound), starts the sound as a 3D
    /// event at `transform` moving at `velocity` and releases it to play out. Bullet impacts,
    /// melee hits, ricochets, explosions, and the grapple's impacts play their material effects
    /// through it.
    pub unsafe fn PlayOneOffs(
        &self,
        transform: *const crate::types::math::Matrix4,
        velocity: *const crate::types::math::Vector3,
        cull: *const crate::equipment::attached_effects::EffectCull,
        decal_models: *const ::std::ffi::c_void,
        decal_normal: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                transform: *const crate::types::math::Matrix4,
                velocity: *const crate::types::math::Vector3,
                cull: *const crate::equipment::attached_effects::EffectCull,
                decal_models: *const ::std::ffi::c_void,
                decal_normal: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::PlayOneOffs_ADDRESS);
            f(
                self as *const Self as _,
                transform,
                velocity,
                cull,
                decal_models,
                decal_normal,
            )
        }
    }
}
impl std::convert::AsRef<EffectAndSoundPair> for EffectAndSoundPair {
    fn as_ref(&self) -> &EffectAndSoundPair {
        self
    }
}
impl std::convert::AsMut<EffectAndSoundPair> for EffectAndSoundPair {
    fn as_mut(&mut self) -> &mut EffectAndSoundPair {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(1))]
/// Which halves of an [`EffectAndSoundPair`](crate::equipment::attached_effects::EffectAndSoundPair) [`PlayOneOffs`](crate::equipment::attached_effects::EffectAndSoundPair::PlayOneOffs)
/// plays (`SEffectCull`). `SEffectCull::ALWAYS_PLAY` sets both.
pub struct EffectCull {
    pub m_PlayEffect: bool,
    pub m_PlaySound: bool,
}
fn _EffectCull_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2], EffectCull>([0u8; 0x2]);
    }
    unreachable!()
}
impl EffectCull {}
impl std::convert::AsRef<EffectCull> for EffectCull {
    fn as_ref(&self) -> &EffectCull {
        self
    }
}
impl std::convert::AsMut<EffectCull> for EffectCull {
    fn as_mut(&mut self) -> &mut EffectCull {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a particle effect instance stops (`CEffectInstance::EStopMode`).
pub enum EffectStopMode {
    E_STOP_SOFTLY = 0isize as _,
    E_STOP_IMMEDIATE = 1isize as _,
}
fn _EffectStopMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], EffectStopMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(16))]
/// The world placement handed to a playing attached effect (`SEffectTransformInfo`): a transform,
/// the velocity of the attachment, and what the transform is used for. Resolved per instance each
/// update by the instance's transform function and copied whole into
/// [`AttachedInstance::m_TransformInfo`](crate::equipment::attached_effects::AttachedInstance::m_TransformInfo).
pub struct EffectTransformInfo {
    /// The world transform (in a union with a custom speaker mix for audio spatialization).
    pub m_Transform: crate::types::math::Matrix4,
    /// The attachment's velocity.
    pub m_Velocity: crate::types::math::Vector3,
    _field_4c: [u8; 4],
    /// What the transform drives.
    pub m_Type: crate::equipment::attached_effects::TransformType,
    _field_54: [u8; 12],
}
fn _EffectTransformInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], EffectTransformInfo>([0u8; 0x60]);
    }
    unreachable!()
}
impl EffectTransformInfo {}
impl std::convert::AsRef<EffectTransformInfo> for EffectTransformInfo {
    fn as_ref(&self) -> &EffectTransformInfo {
        self
    }
}
impl std::convert::AsMut<EffectTransformInfo> for EffectTransformInfo {
    fn as_mut(&mut self) -> &mut EffectTransformInfo {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How an attached instance's rotation follows its attachment
/// (`CAttachedEffectContainer::ERotationMode`).
pub enum RotationMode {
    ERotationMode_ATTACHED = 0isize as _,
    ERotationMode_FIRST_FRAME = 1isize as _,
    ERotationMode_ATTACHED_RANDOM_ROLL = 2isize as _,
    ERotationMode_FIRST_FRAME_RANDOM_ROLL = 3isize as _,
    ERotationMode_WORLD_UP_RANDOM = 4isize as _,
    ERotationMode_RANDOM = 5isize as _,
    ERotationMode_WORLD_UP = 6isize as _,
    ERotationMode_COUNT = 7isize as _,
}
fn _RotationMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RotationMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a sound instance stops (`CSoundInstance::EStopMode`).
pub enum SoundStopMode {
    EStopMode_Immediately = 0isize as _,
    EStopMode_Softly = 1isize as _,
}
fn _SoundStopMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SoundStopMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// What an [`EffectTransformInfo`](crate::equipment::attached_effects::EffectTransformInfo) transform drives (`SEffectTransformInfo::ETransformType`).
pub enum TransformType {
    ETransformType_Transform3D = 0isize as _,
    ETransformType_AudioSpatialization = 1isize as _,
    ETransformType_ReverbSpatialization = 2isize as _,
    ETransformType_EchoSpatialization = 3isize as _,
    ETransformType_Invalid = 4isize as _,
}
fn _TransformType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], TransformType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How an attached instance's translation follows its attachment
/// (`CAttachedEffectContainer::ETranslationMode`).
pub enum TranslationMode {
    ETranslationMode_ATTACHED = 0isize as _,
    ETranslationMode_FIRST_FRAME = 1isize as _,
    ETranslationMode_COUNT = 2isize as _,
}
fn _TranslationMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], TranslationMode>([0u8; 0x4]);
    }
    unreachable!()
}
pub const CreateCollisionMatrix_ADDRESS: usize = 0x14048B390;
/// A transform for an effect at a collision (`CreateCollisionMatrix`): translated to `position`,
/// its up axis along `normal`, rolled about it by a random angle from the effect system's own
/// random sequence. Writes it to `result` and returns `result`.
pub unsafe fn CreateCollisionMatrix(
    result: *mut crate::types::math::Matrix4,
    position: *const crate::types::math::Vector3,
    normal: *const crate::types::math::Vector3,
) -> *mut crate::types::math::Matrix4 {
    unsafe {
        let f: unsafe extern "system" fn(
            result: *mut crate::types::math::Matrix4,
            position: *const crate::types::math::Vector3,
            normal: *const crate::types::math::Vector3,
        ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
            CreateCollisionMatrix_ADDRESS,
        );
        f(result, position, normal)
    }
}
