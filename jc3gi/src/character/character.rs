#![cfg_attr(any(), rustfmt::skip)]
crate::__bitflags! {
    #[doc =
    " A byte of `CCharacter` bitfields that enable traversal abilities, each written by its own"]
    #[doc = " setter. `CGameWorld::TeleportPlayerInstant` sets"] #[doc =
    " [`m_IsStuntAutoExitEnabled`](crate::character::character::AbilityFlags::m_IsStuntAutoExitEnabled) on the player character as part of"]
    #[doc = " the teleport."] pub struct AbilityFlags : u8 { const m_IsParachuteEnabled =
    1usize as _; const m_IsWingsuitEnabled = 2usize as _; const m_IsStuntAutoExitEnabled
    = 4usize as _; }
}
fn _AbilityFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], AbilityFlags>([0u8; 0x1]);
    }
    unreachable!()
}
crate::__bitflags! {
    #[doc =
    " The character's packed aiming and presentation bit-flags (one byte of `CCharacter` bitfields)."]
    #[doc =
    " [`HandleAimingInputPlayer`](crate::character::character::Character::HandleAimingInputPlayer) writes the weapon-aim bit each"]
    #[doc =
    " tick for the player and [`PreSimUpdateGrapple`](crate::character::character::Character::PreSimUpdateGrapple) the grapple-aim"]
    #[doc =
    " bit; the locomotion state machine reads them to choose aim-relative (strafe) movement over"]
    #[doc =
    " run/steer: `m_AimingWeapon` (or `m_AimingGrapple`) is the switch that makes the body face the aim"]
    #[doc =
    " direction and turns the directional keys into strafes. `m_WasAiming` reflects the state as of the"]
    #[doc =
    " start of this frame. The other bits are unrelated flags that share the byte."] pub
    struct AimState : u8 { const m_WasVisible = 1usize as _; const m_UseFinalTransparency
    = 2usize as _; const m_AimingEnabled = 4usize as _; const m_AimingWeapon = 8usize as
    _; const m_AimingGrapple = 16usize as _; const m_WasAiming = 32usize as _; const
    m_GodModeSequence = 64usize as _; const m_InCargoBay = 128usize as _; }
}
fn _AimState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], AimState>([0u8; 0x1]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct AnimatedModel {
    _field_0: [u8; 152],
    pub m_RuleSystems: crate::types::std_vector::Vector<
        crate::types::shared_ptr::SharedPtr<
            crate::animation::rule_system::AnimationRuleSystem,
        >,
    >,
    _field_b8: [u8; 192],
    pub m_AnimationController: *mut crate::character::character::AnimationController,
    _field_180: [u8; 88],
    /// The model's animation instance data (`SAnimatedModelInstanceData*`), null until the animation
    /// instance is created (the first frames after spawn or stream-in). Some character-state
    /// predicates, [`Character::IsUsingWingsuit`](crate::character::character::Character::IsUsingWingsuit) among them, report false while it is null.
    pub m_InstanceData: *mut u8,
    _field_1e0: [u8; 32],
    /// The model-instance slots (`NModelSystem::CModelInstance*`, null when unused): the base
    /// body model plus attachments and variants. `CAnimatedModel::GetModel`/`SetModel` index this
    /// array directly. Each instance embeds its render-block-instance info (`CRBIInfo`) at
    /// [`MODEL_INSTANCE_RBI_INFO_OFFSET`](crate::character::character::AnimatedModel::MODEL_INSTANCE_RBI_INFO_OFFSET), which is the
    /// `info` pointer its render blocks are drawn with.
    pub m_ModelInstances: [u64; 8],
}
fn _AnimatedModel_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x240], AnimatedModel>([0u8; 0x240]);
    }
    unreachable!()
}
impl AnimatedModel {
    pub const TryAct_ADDRESS: usize = 0x1404B2E00;
    /// Tests whether the animation state machine would accept the act from its current state,
    /// without queuing it. A `CCharacter` virtual forwards to this on
    /// [`Character::m_AnimatedModel`](crate::character::character::Character::m_AnimatedModel); the game's act dispatchers call it before
    /// [`Character::QueueAct`](crate::character::character::Character::QueueAct) to pick a supported act.
    pub unsafe fn TryAct(&mut self, act: *const u32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, act: *const u32) -> bool = ::std::mem::transmute(
                Self::TryAct_ADDRESS,
            );
            f(self as *mut Self as _, act)
        }
    }
    pub const QueueAct_ADDRESS: usize = 0x1404C8DA0;
    /// Queues an act (`act` points at the runtime `idstring` value) on the model's state machines,
    /// appending it to the model's act queue, which holds at most 32 acts; an act queued with the
    /// queue full is dropped. [`Character::QueueAct`](crate::character::character::Character::QueueAct) forwards here for
    /// [`Character::m_AnimatedModel`](crate::character::character::Character::m_AnimatedModel), and engine code that holds the model, such as
    /// [`Character::HitReaction`](crate::character::character::Character::HitReaction), calls it directly.
    pub unsafe fn QueueAct(&mut self, act: *const u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, act: *const u32) = ::std::mem::transmute(
                Self::QueueAct_ADDRESS,
            );
            f(self as *mut Self as _, act)
        }
    }
}
impl AnimatedModel {
    /// The [`m_ModelInstances`](crate::character::character::AnimatedModel::m_ModelInstances) slot holding the character's
    /// *body* model -- the one whose skeleton the character's bone indices belong to. The other
    /// slots are attachments with their own skeletons (the parachute among them), where the same
    /// numeric indices land on arbitrary bones.
    pub const BODY_MODEL_SLOT: u64 = 0;
    /// The offset of a model instance's embedded render-block-instance info (`CRBIInfo`) within
    /// `NModelSystem::CModelInstance` (`CModelInstance::AddToPass` passes this offset to
    /// `ForEachRb`); that pointer is the `info` every one of its block draws receives. A constant
    /// rather than a field because the instance type itself is unmapped.
    pub const MODEL_INSTANCE_RBI_INFO_OFFSET: u64 = 80;
    /// The number of [`m_ModelInstances`](crate::character::character::AnimatedModel::m_ModelInstances) slots. Keep in sync
    /// with that array's length.
    pub const MODEL_INSTANCE_SLOTS: u64 = 8;
}
impl std::convert::AsRef<AnimatedModel> for AnimatedModel {
    fn as_ref(&self) -> &AnimatedModel {
        self
    }
}
impl std::convert::AsMut<AnimatedModel> for AnimatedModel {
    fn as_mut(&mut self) -> &mut AnimatedModel {
        self
    }
}
#[repr(C, align(8))]
/// A character's animation controller (`NCharacterSystem::CAnimationControl`): the skeleton, the
/// pose producer, and one body part per animation layer.
pub struct AnimationController {
    _field_0: [u8; 160],
    /// The skeleton resource, through which [`GetBoneIndex`](crate::character::character::AnimationController::GetBoneIndex) resolves a bone by
    /// the hash of its name.
    pub m_Skeleton: *mut crate::animation::skeleton::Skeleton,
    _field_a8: [u8; 16],
    /// The pose producer, whose [`m_PoseT1`](crate::animation::pose_producer::PoseProducer::m_PoseT1) the
    /// joint accessors ([`GetJoint`](crate::character::character::AnimationController::GetJoint) and [`SetJoint`](crate::character::character::AnimationController::SetJoint)) read and
    /// write.
    pub m_PoseProducer: *mut crate::animation::pose_producer::PoseProducer,
    _field_c0: [u8; 8],
    /// The animation layers' body parts, in layer order: index *i* belongs to the layer named by
    /// [`AnimationLayer`](crate::character::character::AnimationLayer) *i*, alongside the rule system at the same index of
    /// [`AnimatedModel::m_RuleSystems`](crate::character::character::AnimatedModel::m_RuleSystems).
    pub m_BodyParts: crate::types::std_vector::Vector<
        crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimatedBodyPart,
        >,
    >,
}
fn _AnimationController_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE8], AnimationController>([0u8; 0xE8]);
    }
    unreachable!()
}
impl AnimationController {
    pub const GetBoneIndex_ADDRESS: usize = 0x140434E30;
    pub unsafe fn GetBoneIndex(&self, hash: u32) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self, hash: u32) -> u32 = ::std::mem::transmute(
                Self::GetBoneIndex_ADDRESS,
            );
            f(self as *const Self as _, hash)
        }
    }
    pub const GetRawRootVelocity_ADDRESS: usize = 0x140434F20;
    /// The current animation's unfiltered root velocity, in the character's local frame. The
    /// movement actuators take its horizontal magnitude as the character's speed for the frame, so
    /// the shipped clips are what set how fast a given locomotion state travels -- both swim
    /// movement cores scale their wanted velocity by it.
    pub unsafe fn GetRawRootVelocity(&self) -> *const crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetRawRootVelocity_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetBoneMatrix_ADDRESS: usize = 0x14043FE70;
    pub unsafe fn GetBoneMatrix(
        &self,
        index: u32,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u32,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetBoneMatrix_ADDRESS);
            f(self as *const Self as _, index, matrix)
        }
    }
    pub const GetJoint_ADDRESS: usize = 0x14043FF90;
    pub unsafe fn GetJoint(
        &self,
        index: u32,
        joint: *mut crate::character::character::Joint,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u32,
                joint: *mut crate::character::character::Joint,
            ) = ::std::mem::transmute(Self::GetJoint_ADDRESS);
            f(self as *const Self as _, index, joint)
        }
    }
    pub const SetJoint_ADDRESS: usize = 0x14043FFF0;
    pub unsafe fn SetJoint(
        &mut self,
        index: u32,
        joint: *mut crate::character::character::Joint,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u32,
                joint: *mut crate::character::character::Joint,
            ) = ::std::mem::transmute(Self::SetJoint_ADDRESS);
            f(self as *mut Self as _, index, joint)
        }
    }
    pub const GetBindPoseBoneMatrix_ADDRESS: usize = 0x140440000;
    /// Writes the model-space matrix of bone `index` in the skeleton's bind pose through `matrix`
    /// (`CAnimationControl::GetBindPoseBoneMatrix`, through `CSkeleton::GetJoint`). The bind pose
    /// belongs to the skeleton resource and does not change with the animation, so two bones' bind
    /// matrices give their rest-pose relation.
    pub unsafe fn GetBindPoseBoneMatrix(
        &self,
        index: u32,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u32,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetBindPoseBoneMatrix_ADDRESS);
            f(self as *const Self as _, index, matrix)
        }
    }
    pub const UpdateAnimationsTime_ADDRESS: usize = 0x1404794D0;
    /// Advances the playback clocks of the controller's time-updatable blenders by `dt` (via
    /// `CPoseProducer::UpdateBlendersTimeInRange`) without recomputing the pose. Runs each SIM frame
    /// in the character's animation pass, before [`UpdateAnimations`](crate::character::character::AnimationController::UpdateAnimations).
    pub unsafe fn UpdateAnimationsTime(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateAnimationsTime_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdateAnimations_ADDRESS: usize = 0x14077E200;
    /// Recomputes the controller's pose from its pose producers: advances each blender's clock by
    /// `dt` (`CPoseProducer::Update` -> `CBlender::UpdateTime`) and samples and blends the active
    /// clips into the model-space pose over `num_of_bones` bones. Runs each SIM frame in the
    /// character's animation pass. A `dt` of `0` holds the animation clocks, producing a static pose.
    pub unsafe fn UpdateAnimations(&mut self, dt: f32, num_of_bones: i32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                num_of_bones: i32,
            ) = ::std::mem::transmute(Self::UpdateAnimations_ADDRESS);
            f(self as *mut Self as _, dt, num_of_bones)
        }
    }
    pub const UpdateSkinning_ADDRESS: usize = 0x14043FCA0;
    /// The render-phase skinning update (`CAnimationControl::UpdateSkinning`): hands the pose
    /// producer's skinning job to
    /// [`PoseProducer::MakeSkinningPalette`](crate::animation::pose_producer::PoseProducer::MakeSkinningPalette)
    /// with `dtf` and the current frame's palette buffer (the control double-buffers its palettes by
    /// frame parity). `world_matrix` is unused in this build. `CCharacter::UpdateRender` calls it
    /// (the character-level `UpdateSkinning` is inlined there).
    pub unsafe fn UpdateSkinning(
        &mut self,
        dtf: f32,
        world_matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dtf: f32,
                world_matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::UpdateSkinning_ADDRESS);
            f(self as *mut Self as _, dtf, world_matrix)
        }
    }
    pub const PlayCustomAnimation_ADDRESS: usize = 0x1404772F0;
    /// Plays the clip at `path` on the body part `layer`
    /// (`CAnimationControl::PlayCustomAnimation`): acquires the clip resource by its path, builds
    /// an [`AnimationInstance`](crate::animation::pose_producer::AnimationInstance) of it over the
    /// controller's skeleton, hands it to the layer's driver
    /// ([`AnimationDriverTransition::PlayCustomAnimation`](crate::animation::pose_producer::AnimationDriverTransition::PlayCustomAnimation))
    /// with the `blend` time, and writes the instance into `result`. `looping` selects the
    /// instance's loop mode.
    pub unsafe fn PlayCustomAnimation(
        &mut self,
        result: *mut crate::types::shared_ptr::SharedPtr<
            crate::animation::pose_producer::AnimationInstance,
        >,
        layer: u64,
        path: *const crate::types::std_string::String,
        blend: f32,
        looping: bool,
    ) -> *mut crate::types::shared_ptr::SharedPtr<
        crate::animation::pose_producer::AnimationInstance,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::types::shared_ptr::SharedPtr<
                    crate::animation::pose_producer::AnimationInstance,
                >,
                layer: u64,
                path: *const crate::types::std_string::String,
                blend: f32,
                looping: bool,
            ) -> *mut crate::types::shared_ptr::SharedPtr<
                crate::animation::pose_producer::AnimationInstance,
            > = ::std::mem::transmute(Self::PlayCustomAnimation_ADDRESS);
            f(self as *mut Self as _, result, layer, path, blend, looping)
        }
    }
}
impl std::convert::AsRef<AnimationController> for AnimationController {
    fn as_ref(&self) -> &AnimationController {
        self
    }
}
impl std::convert::AsMut<AnimationController> for AnimationController {
    fn as_mut(&mut self) -> &mut AnimationController {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A character's animation layers (`CCharacter::s_LayerNames`). `CCharacter::InitAnimationModel`
/// creates layer *i* from the *i*-th state-machine file of the character's construction info, named
/// by this enum, and stops at the first missing file; the layer's rule system, body part, and
/// blender all sit at index *i*. The engine's level of detail (`CCharacter::UpdateLOD`) switches
/// the partial and additive layers' body parts off by these indices for distant characters.
pub enum AnimationLayer {
    MAINBODY = 0isize as _,
    SYNCED_PARTIAL = 1isize as _,
    GLOBAL_PARTIAL = 2isize as _,
    SYNCED_ADDITIVE = 3isize as _,
    GLOBAL_ADDITIVE = 4isize as _,
    VOCALS = 5isize as _,
}
fn _AnimationLayer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AnimationLayer>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct Character {
    vftable: *const crate::character::character::CharacterVftable,
    _field_8: [u8; 96],
    /// A weak reference to the character itself (`m_WeakThis`), from which the engine takes the
    /// references it hands out to the character, such as a damage message's
    /// [`m_Damager`](crate::damage::DamageMsg::m_Damager).
    pub m_WeakThis: crate::types::shared_ptr::WeakPtrGameObject,
    _field_78: [u8; 312],
    /// The character's maximum health (`CDamageable::m_MaxHealth`), the scale of
    /// [`GetHealth`](crate::character::character::Character::GetHealth). [`HUDUI::InitHealth`](crate::ui::hud_ui::HUDUI::InitHealth) passes
    /// the local player character's to the movie.
    pub m_MaxHealth: i16,
    _field_1b2: [u8; 174],
    /// The character's attached effects (the `CAttachedEffectContainer` base of its
    /// `CCombinedAttachedEffectContainer` base): its bone-attached effects and its dynamic usages,
    /// such as the melee impact
    /// ([`MELEE_IMPACT_USAGE`](crate::equipment::attached_effects::AttachedEffectContainer::MELEE_IMPACT_USAGE)).
    pub m_AttachedEffects: crate::equipment::attached_effects::AttachedEffectContainer,
    _field_2f0: [u8; 364],
    /// The hit reaction's per-type impulse scales: [`HitReaction`](crate::character::character::Character::HitReaction) scales a
    /// hit's impulse by the entry of the lowest [`DamageFlags`](crate::damage::DamageFlags) type bit set
    /// (the first when none is). Per-character tuning read from data, as are the thresholds below.
    pub m_HitReaction_DamageImpulseScale: [f32; 20],
    /// The further impulse scale of a headshot.
    pub m_HitReaction_HeadshotImpulseScale: f32,
    /// The most a scaled impulse moves the character's ragdoll: the impulse
    /// [`HitReaction`](crate::character::character::Character::HitReaction) queues on the ragdoll is the hit's direction times the
    /// smaller of this and the scaled impulse.
    pub m_HitReaction_DamageImpulseMax: f32,
    /// The scaled impulse at or above which a hit knocks a non-player character into its ragdoll.
    pub m_HitReaction_ImpulseRagdollThreshold: f32,
    pub m_HitReaction_StuntFallOffImpactWeight: f32,
    /// The scaled impulse above which a hit staggers the character: it becomes the character's
    /// current reaction force, which the hit-reaction animation reads.
    pub m_HitReaction_ImpulseStumbleThreshold: f32,
    /// The impulse, accumulated over hits in quick succession, at which a hit too weak to stagger
    /// the character alone counts as staggering.
    pub m_HitReaction_AccumalatedImpulseStumbleThreshold: f32,
    _field_4c4: [u8; 1140],
    /// The character's aiming-settings resource (`TResourceCachePtr<SCharacterAimingSettings>`, a
    /// resource handle resolved through the resource cache). Both aim-input updates read the
    /// aim-timer refill value from the resolved settings.
    pub m_AimingSettings: *mut u8,
    _field_940: [u8; 64],
    /// The character's embedded equipment inventory.
    pub m_Inventory: crate::character::inventory::Inventory,
    _field_df8: [u8; 744],
    /// The character's embedded HumanIK solver. Driven each frame in
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel), after the
    /// animation graph finalizes the local pose and before the model-space pose is computed. See
    /// [`HumanIK`](crate::animation::ik::HumanIK) for the per-pass lifecycle.
    pub m_HIK: crate::animation::ik::HumanIK,
    pub m_AnimatedModel: crate::character::character::AnimatedModel,
    _field_19c0: [u8; 72],
    /// The character's user side of the interaction system: the interaction graphs near it and
    /// the entries it could take. Its getter (`CCharacter::GetInteractionUserProxy`) is mislabelled
    /// `CContextAction::GetSecondaryOccupant` by the disassembler.
    pub m_InteractionUserProxy: crate::interaction::InteractionUserProxy,
    _field_1c30: [u8; 1056],
    /// The character's physics-side body instance. The movement actuators drive the character
    /// through it: they write the wanted velocity into its input and its orientation through
    /// [`SetOrientation`](crate::physics::PfxCharacterInstance::SetOrientation).
    pub m_PfxCharacter: crate::types::shared_ptr::SharedPtr<
        crate::physics::PfxCharacterInstance,
    >,
    /// The character's embedded [`ObjectBlackboard`](crate::blackboard::ObjectBlackboard). The loco tasks' blackboard call sites address
    /// it in place on the character.
    pub m_Blackboard: crate::blackboard::ObjectBlackboard,
    _field_2090: [u8; 89],
    /// Packed damage-state bit-flags; see [`DamageStateFlags`](crate::character::character::DamageStateFlags).
    pub m_DamageFlags: crate::character::character::DamageStateFlags,
    _field_20ea: [u8; 6],
    /// Whether the character's weapons draw ammunition without consuming it. Read by
    /// `CWeaponBase::HasUnlimitedAmmo` together with the non-player and challenge-island rules.
    /// Set through [`SetUnlimitedAmmo`](crate::character::character::Character::SetUnlimitedAmmo), which also refreshes the HUD's
    /// weapon readout for the player.
    pub m_UnlimitedAmmo: bool,
    /// Packed aiming-state bit-flags; see [`AimState`](crate::character::character::AimState).
    pub m_AimFlags: crate::character::character::AimState,
    _field_20f2: [u8; 66],
    /// The proxy state requested for the character, a [`ProxyState`](crate::physics::ProxyState) value.
    /// [`SetProxyState`](crate::character::character::Character::SetProxyState) writes it, and
    /// [`UpdateProxyState`](crate::character::character::Character::UpdateProxyState) applies it to the physics proxy on the
    /// simulation thread. The animation graph's proxy-state task requests states on its state's
    /// entry and exit.
    pub m_PendingProxyState: i32,
    /// The proxy state the character returns to, a [`ProxyState`](crate::physics::ProxyState) value. The graph's proxy-state
    /// restores and the paratrooper logic request it. The player's is
    /// [`PLAYER_STANDING`](crate::physics::ProxyState::PLAYER_STANDING), set at player initialization through
    /// [`SetDefaultProxyState`](crate::character::character::Character::SetDefaultProxyState).
    pub m_DefaultProxyState: i32,
    _field_213c: [u8; 856],
    /// How the character's motion is integrated this frame (`CCharacter::EMotionState`), read by
    /// the dispatch in `CCharacter::UpdateIntegratingPhysics`. Each movement actuator sets it as
    /// part of its per-frame update. Left as a raw integer because only a few of its ~25 values have
    /// been confirmed: `MOTION_STATE_SWIMMING`, `MOTION_STATE_RAGDOLL`, and
    /// `MOTION_STATE_TASK_DEFINED` on this type.
    pub m_CurrentMotionState: u32,
    _field_2498: [u8; 402],
    pub m_IsLocalCharacter: bool,
    _field_262b: [u8; 145],
    /// The world-space point the character's weapons aim at, written each frame by
    /// [`UpdateWeaponAiming`](crate::character::character::Character::UpdateWeaponAiming) (for the player, from the aim control's
    /// weapon aim point). The weapons read it as their fire target.
    pub m_AimTargetPositionWeapons: crate::types::math::Vector3,
    /// The world-space point thrown grenades aim at, written by
    /// [`UpdateGrenadeAiming`](crate::character::character::Character::UpdateGrenadeAiming).
    pub m_AimTargetPositionGrenade: crate::types::math::Vector3,
    /// The per-frame weapon input values (fire, precision aim, plant, and the rest), indexed by
    /// [`WeaponInput`](crate::character::character::WeaponInput), cleared by [`UpdateWeapons_Parallel`](crate::character::character::Character::UpdateWeapons_Parallel) after
    /// the inventory consumes them.
    pub m_WeaponInputs: [f32; 16],
    /// Set by [`UpdateWeaponAiming`](crate::character::character::Character::UpdateWeaponAiming) and cleared by
    /// [`UpdateWeapons_Parallel`](crate::character::character::Character::UpdateWeapons_Parallel): whether the weapon aim target was
    /// supplied this frame.
    pub m_AimTargetPositionWeaponWasSetThisFrame: bool,
    _field_2715: [u8; 3],
    /// The character's animation control parameters, which own the character state-bitflag words
    /// ([`ControlParameters::m_UserVM`](crate::character::control_parameters::ControlParameters::m_UserVM)). Almost every one-bit "what is the character doing"
    /// predicate on this type reads through this pointer.
    pub m_ControlParameters: *mut crate::character::control_parameters::ControlParameters,
    _field_2720: [u8; 108],
    /// Per-character LOD/update-gating flags; see [`CharacterLodFlags`](crate::character::character::CharacterLodFlags).
    pub m_LodFlags: crate::character::character::CharacterLodFlags,
    _field_278d: [u8; 27],
    /// The inventory slot of the weapon the character has selected. Its one writer is
    /// [`SetSelectedWeaponSlot`](crate::character::character::Character::SetSelectedWeaponSlot); the animation graph's weapon
    /// conditions read the selected slot's weapon
    /// ([`WeaponSelectedCondition`](crate::equipment::weapon_animation::WeaponSelectedCondition) and its
    /// siblings).
    pub m_SelectedWeaponSlot: crate::character::inventory::WeaponSlot,
    _field_27ac: [u8; 68],
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    _field_2870: [u8; 196],
    /// The orientation the character is turning toward (`CQuaternion`, `x, y, z, w`).
    /// [`SetOrientation`](crate::character::character::Character::SetOrientation) writes the rotation into
    /// [`m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1) and stores the same quaternion here.
    pub m_RotationGoal: [f32; 4],
    _field_2944: [u8; 36],
    /// The character's measured world velocity, metres per second.
    /// [`PlantedExplosiveWeapon::Fire_Projectile`](crate::equipment::weapon::PlantedExplosiveWeapon::Fire_Projectile)
    /// drops a charge with it under a parachute.
    pub m_MeasuredWorldVelocity: crate::types::math::Vector3,
    _field_2974: [u8; 60],
    /// The character's avatar (`CAvatar`), the controller that drives it. For the player it is the
    /// [`Avatar`](crate::character::avatar::Avatar) base of the `CPlayer`. Holds the mental state written by
    /// [`SetMentalState`](crate::character::character::Character::SetMentalState).
    pub m_Avatar: *mut crate::character::avatar::Avatar,
    _field_29b8: [u8; 16],
    /// Whether a scripted sequence (`CSequenceObject2`) drives the character. Set by
    /// [`SetupForSequence`](crate::character::character::Character::SetupForSequence) when a sequence takes the character over and
    /// cleared by [`ResetAfterSequence3`](crate::character::character::Character::ResetAfterSequence3) when it hands it back; while
    /// it is set, the sequence's transform tracks place the character and its animation layers run
    /// the sequence's own state machines.
    pub m_InSequence: bool,
    _field_29c9: [u8; 7],
    /// Whether this character is a player character (local or remote). Gates the player-only
    /// side effects of setters such as [`SetUnlimitedAmmo`](crate::character::character::Character::SetUnlimitedAmmo).
    pub m_IsPlayer: bool,
    _field_29d1: [u8; 47],
    /// The weapon-aim countdown timer, in seconds. It is refreshed while an aim input is held and
    /// decremented by the frame delta otherwise; a positive value keeps
    /// [`m_AimingWeapon`](crate::character::character::AimState::m_AimingWeapon) set in [`m_AimFlags`](crate::character::character::Character::m_AimFlags),
    /// so the character stays in aim/strafe locomotion for a short tail after the aim button is
    /// released.
    pub m_AimTimer: f32,
    _field_2a04: [u8; 260],
    /// The traversal-ability enable bits; see [`AbilityFlags`](crate::character::character::AbilityFlags).
    pub m_AbilityFlags: crate::character::character::AbilityFlags,
    _field_2b09: [u8; 167],
    /// The idle-variation countdown, in seconds, driven by
    /// [`NStateTask_InputLocoIdleTask::Update`](crate::input::locomotion::NStateTask_InputLocoIdleTask::Update):
    /// while the character stands idle it is decremented by the frame delta, and once it goes
    /// negative the task queues
    /// [`ACT_TO_IDLE_ONE_OFF`](crate::animation::symbol_table::EventIdSymbolTable::ACT_TO_IDLE_ONE_OFF) to
    /// play a one-off idle break, then resets it to a random ~8.5-12 s the next time the character's
    /// aim reference moves.
    pub m_IdleFidgetTimer: f32,
    _field_2bb4: [u8; 1104],
    /// The swim locomotion family's per-character state; see [`SwimActionParams`](crate::character::swim_action_params::SwimActionParams).
    pub m_SwimActionParams: crate::character::swim_action_params::SwimActionParams,
    _field_3280: [u8; 704],
}
fn _Character_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3540], Character>([0u8; 0x3540]);
    }
    unreachable!()
}
impl Character {
    pub fn vftable(&self) -> *const crate::character::character::CharacterVftable {
        self.vftable as *const crate::character::character::CharacterVftable
    }
    pub const GetLocalPlayerCharacter_ADDRESS: usize = 0x1407D5B00;
    pub unsafe fn GetLocalPlayerCharacter() -> *mut crate::character::character::Character {
        unsafe {
            let f: unsafe extern "system" fn() -> *mut crate::character::character::Character = ::std::mem::transmute(
                Self::GetLocalPlayerCharacter_ADDRESS,
            );
            f()
        }
    }
    pub const TYPE_ID_ADDRESS: usize = 0x14060F370;
    /// `CCharacter::TYPE_ID`: the run-time type id of `CCharacter`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const GetFaction_ADDRESS: usize = 0x14075FBF0;
    /// The character's faction: its avatar's when it has one ([`m_Avatar`](crate::character::character::Character::m_Avatar)),
    /// otherwise the character's own.
    pub unsafe fn GetFaction(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetFaction_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetCurrentDamageSourceStateInfo_ADDRESS: usize = 0x1407F57D0;
    /// What the character is doing, as a damage message's
    /// [`m_DamagerSourceStateInfo`](crate::damage::DamageMsg::m_DamagerSourceStateInfo) records it: in a
    /// stunt, under the parachute, flying, aiming down the sights, near death, in a vehicle of each
    /// class, reeled in, swimming, or hanging from the grapple.
    pub unsafe fn GetCurrentDamageSourceStateInfo(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetCurrentDamageSourceStateInfo_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const HandleTrackMessage_ADDRESS: usize = 0x1407FADD0;
    /// Handles a message from the character's animation tracks (`data`, an
    /// `NAnimationSystem::SAnimationMessageData`), dispatching on its id. The melee hit message
    /// deals the player's melee: with a character as the link target, it sends that character a
    /// [`DMG_TYPE_MELEE`](crate::damage::DamageFlags::DMG_TYPE_MELEE) [`DamageMsg`](crate::damage::DamageMsg) of
    /// [`Player::m_MeleeDamage`](crate::character::player::Player::m_MeleeDamage) and no impulse, and
    /// clears the link target; with none, it finds the objects in a box in front of the chest
    /// ([`PhysicsSystem::CollideBox`](crate::physics::PhysicsSystem::CollideBox)), damages each (a
    /// vehicle once), and pushes each body forward by
    /// [`Player::m_MeleeImpulse`](crate::character::player::Player::m_MeleeImpulse)
    /// ([`hknpWorld::applyBodyImpulseAt`](crate::physics::hknpWorld::applyBodyImpulseAt)). Both paths
    /// spawn a rumble and the hit effect.
    pub unsafe fn HandleTrackMessage(&mut self, data: *mut ::std::ffi::c_void) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::HandleTrackMessage_ADDRESS);
            f(self as *mut Self as _, data)
        }
    }
    pub const IsInVehicleAttachState_ADDRESS: usize = 0x14077F080;
    /// Whether the character is attached to a vehicle (riding, or in the mount/dismount
    /// transitions), read from the animation rule system's current state. The state machine
    /// pointers it walks are game-thread data, so the call is only safe on the game update thread.
    pub unsafe fn IsInVehicleAttachState(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInVehicleAttachState_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetHeadPosition_ADDRESS: usize = 0x1407AF550;
    pub unsafe fn GetHeadPosition(
        &self,
        position: *mut crate::types::math::Vector3,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                position: *mut crate::types::math::Vector3,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetHeadPosition_ADDRESS,
            );
            f(self as *const Self as _, position)
        }
    }
    pub const SetUnlimitedAmmo_ADDRESS: usize = 0x1407B14B0;
    /// Sets [`m_UnlimitedAmmo`](crate::character::character::Character::m_UnlimitedAmmo). For the locally controlled player, a
    /// change also re-selects the current weapon on the HUD so its ammo readout updates.
    pub unsafe fn SetUnlimitedAmmo(&mut self, unlimited: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, unlimited: bool) = ::std::mem::transmute(
                Self::SetUnlimitedAmmo_ADDRESS,
            );
            f(self as *mut Self as _, unlimited)
        }
    }
    pub const SetGodMode_ADDRESS: usize = 0x1407FD7C0;
    /// Sets the [`GOD_MODE`](crate::character::character::DamageStateFlags::GOD_MODE) damage flag and the damageable's
    /// invulnerability. Enabling it on a living character also restores full health, forces the
    /// neutral animation state, and notifies the network component.
    pub unsafe fn SetGodMode(&mut self, enabled: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enabled: bool) = ::std::mem::transmute(
                Self::SetGodMode_ADDRESS,
            );
            f(self as *mut Self as _, enabled)
        }
    }
    pub const GetGrapplingHook_ADDRESS: usize = 0x140760830;
    /// Returns a reference to the character's grappling-hook slot
    /// ([`Inventory::m_GrapplingHook`](crate::character::inventory::Inventory::m_GrapplingHook)).
    pub unsafe fn GetGrapplingHook(
        &self,
    ) -> *const crate::types::shared_ptr::SharedPtr<
        crate::equipment::grappling_hook::GrapplingHook,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const crate::types::shared_ptr::SharedPtr<
                crate::equipment::grappling_hook::GrapplingHook,
            > = ::std::mem::transmute(Self::GetGrapplingHook_ADDRESS);
            f(self as *const Self as _)
        }
    }
    pub const GetSafeIndex_ADDRESS: usize = 0x14079AB30;
    pub unsafe fn GetSafeIndex(
        &self,
        safe_index: crate::character::character::SafeBoneIndex,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                safe_index: crate::character::character::SafeBoneIndex,
            ) -> u32 = ::std::mem::transmute(Self::GetSafeIndex_ADDRESS);
            f(self as *const Self as _, safe_index)
        }
    }
    pub const GetSafeBoneMatrix_ADDRESS: usize = 0x14079AC30;
    pub unsafe fn GetSafeBoneMatrix(
        &self,
        safe_index: crate::character::character::SafeBoneIndex,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                safe_index: crate::character::character::SafeBoneIndex,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetSafeBoneMatrix_ADDRESS);
            f(self as *const Self as _, safe_index, matrix)
        }
    }
    pub const GetWorldMatrixForBone_ADDRESS: usize = 0x14079AEB0;
    /// Writes the world matrix of the safe bone `safe_index` through `matrix`: the bone's
    /// model-space matrix from the animation controller's current pose
    /// ([`AnimationController::GetBoneMatrix`](crate::character::character::AnimationController::GetBoneMatrix)) composed with the character's
    /// [`m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1). A gun's attachment placement
    /// ([`WeaponData::UpdateAttachmentTransforms`](crate::equipment::weapon::WeaponData::UpdateAttachmentTransforms))
    /// takes a hand-held gun's world matrix from it.
    pub unsafe fn GetWorldMatrixForBone(
        &self,
        safe_index: crate::character::character::SafeBoneIndex,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                safe_index: crate::character::character::SafeBoneIndex,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetWorldMatrixForBone_ADDRESS);
            f(self as *const Self as _, safe_index, matrix)
        }
    }
    pub const UpdatePassFinalizePose_Parallel_ADDRESS: usize = 0x1407F9B10;
    /// The character's SIM-phase pose-finalization pass. After the animation graph has finalized the
    /// local pose, this first runs the current states' tasks registered in the
    /// [`FINALIZE_POSE_STATE_TASK_RANGE`](crate::character::character::Character::FINALIZE_POSE_STATE_TASK_RANGE) through
    /// [`UpdateStateTasksInRange`](crate::character::character::Character::UpdateStateTasksInRange) (the aim, look-at, grapple-fire
    /// arm, and weapon off-hand IK tasks among them), then drives the [`HumanIK`](crate::character::character::Character::m_HIK) solve
    /// (the `MAIN` body pass, gated on [`HumanIK::HasTargets`](crate::animation::ik::HumanIK::HasTargets) and run through
    /// [`UpdateIKForTargets`](crate::character::character::Character::UpdateIKForTargets), followed by
    /// [`UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass)), then computes the
    /// model-space pose (`CalculateModelSpacePose`), and finally calls
    /// [`UpdatePropEffects`](crate::character::character::Character::UpdatePropEffects). Effector targets queued before this runs
    /// are solved into the pose the same frame; blackboard keys those tasks write are written after
    /// its entry.
    pub unsafe fn UpdatePassFinalizePose_Parallel(
        &mut self,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::UpdatePassFinalizePose_Parallel_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const UpdateIKForTargets_ADDRESS: usize = 0x14077E040;
    /// The `MAIN`-pass HumanIK solve, run from
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) once
    /// [`HumanIK::HasTargets`](crate::animation::ik::HumanIK::HasTargets) reports a queued target. Copies the current pose into the solver
    /// ([`HumanIK::CharacterToIKState`](crate::animation::ik::HumanIK::CharacterToIKState)), pushes the queued targets and control values
    /// ([`HumanIK::UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets)), runs the solve ([`HumanIK::Solve`](crate::animation::ik::HumanIK::Solve)), and writes the
    /// solved state back into the pose ([`HumanIK::IKToCharacterState`](crate::animation::ik::HumanIK::IKToCharacterState)). The IDB mislabels this
    /// function `CAnimationRuleSystem::GetState`; its body and call site (the sole caller is
    /// `UpdatePassFinalizePose_Parallel`) identify it as the character's own IK-for-targets solve.
    pub unsafe fn UpdateIKForTargets(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateIKForTargets_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdateSecondaryHandIKForTargets_ADDRESS: usize = 0x14077E0C0;
    /// The `SECONDARY`-pass HumanIK solve, run from
    /// [`UpdateSecondaryHandIKPass`](crate::character::character::Character::UpdateSecondaryHandIKPass) once that pass has queued a
    /// target. Runs the same four-step sequence as [`UpdateIKForTargets`](crate::character::character::Character::UpdateIKForTargets),
    /// except that it skips [`HumanIK::CharacterToIKState`](crate::animation::ik::HumanIK::CharacterToIKState) when `ik_has_run` is already set:
    /// `UpdateSecondaryHandIKPass` passes through the same flag the `MAIN` pass's gate set this
    /// frame, so the pose copied into the solver by that earlier solve is reused instead of being
    /// refetched.
    pub unsafe fn UpdateSecondaryHandIKForTargets(
        &mut self,
        dt: f32,
        ik_has_run: *mut bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                ik_has_run: *mut bool,
            ) = ::std::mem::transmute(Self::UpdateSecondaryHandIKForTargets_ADDRESS);
            f(self as *mut Self as _, dt, ik_has_run)
        }
    }
    pub const UpdateStateTasksInRange_ADDRESS: usize = 0x14075E710;
    /// Runs the update of every task of each rule system's current state that is registered in the
    /// state-task range `range_id` (`CAnimatedModel::StateTaskUpdate`), with `dt` as the step. Each
    /// update pass of the character runs its own range.
    pub unsafe fn UpdateStateTasksInRange(&mut self, dt: f32, range_id: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, range_id: u32) = ::std::mem::transmute(
                Self::UpdateStateTasksInRange_ADDRESS,
            );
            f(self as *mut Self as _, dt, range_id)
        }
    }
    pub const UpdatePropTransforms_ADDRESS: usize = 0x14075EAA0;
    /// Places the character's weapons and props on its current pose: calls
    /// [`Inventory::UpdateAttachmentTransforms`](crate::character::inventory::Inventory::UpdateAttachmentTransforms)
    /// with the character's snow and water factors and `transform_type`, unless a character flag
    /// beside [`m_LodFlags`](crate::character::character::Character::m_LodFlags) (unmapped) switches the placement off.
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) makes the same
    /// call inline twice: with [`PROP_TRANSFORM_ADVANCE`](crate::character::character::Character::PROP_TRANSFORM_ADVANCE) after the
    /// finalize state tasks, and with
    /// [`PROP_TRANSFORM_AFTER_SOLVE`](crate::character::character::Character::PROP_TRANSFORM_AFTER_SOLVE) after the `MAIN` HumanIK
    /// solve, which runs only when [`HumanIK::HasTargets`](crate::animation::ik::HumanIK::HasTargets) reports a queued target.
    pub unsafe fn UpdatePropTransforms(&mut self, transform_type: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, transform_type: u32) = ::std::mem::transmute(
                Self::UpdatePropTransforms_ADDRESS,
            );
            f(self as *mut Self as _, transform_type)
        }
    }
    pub const ShouldPreventFall_ADDRESS: usize = 0x14075F9D0;
    /// Whether the on-foot input should stop the character short of a drop ahead
    /// (`CCharacter::ShouldPreventFall`): false when `predicted_fall_height` is below the
    /// movement settings' [`m_PredictedFallHeightTolerance`](crate::input::locomotion::CharacterOnFootSettings::m_PredictedFallHeightTolerance),
    /// and otherwise true unless the player holds a player action (unmapped), read as an analog
    /// value or a digital state by a game setting.
    /// [`NStateTask_InputLocoSetTargetSpeedsTask::SetupTargetSpeeds`](crate::input::locomotion::NStateTask_InputLocoSetTargetSpeedsTask::SetupTargetSpeeds)
    /// calls it with the predicted fall height from the character's blackboard.
    pub unsafe fn ShouldPreventFall(
        &mut self,
        predicted_fall_height: f32,
        settings: *const crate::input::locomotion::CharacterMovementSettings,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                predicted_fall_height: f32,
                settings: *const crate::input::locomotion::CharacterMovementSettings,
            ) -> bool = ::std::mem::transmute(Self::ShouldPreventFall_ADDRESS);
            f(self as *mut Self as _, predicted_fall_height, settings)
        }
    }
    pub const UpdatePropEffects_ADDRESS: usize = 0x1407C2380;
    /// The per-frame update of the character's attached prop visual effects. Runs last in
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel), after the IK
    /// solve and the model-space pose computation.
    pub unsafe fn UpdatePropEffects(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdatePropEffects_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const UpdateGroundInfo_ADDRESS: usize = 0x1407F4D40;
    /// Refreshes the character's ground information. Runs in
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) after the
    /// finalize state-task range and before the weapons' first attachment refresh and the
    /// HumanIK solve: calls [`DoGroundInfoRaycasts`](crate::character::character::Character::DoGroundInfoRaycasts) (always for the
    /// player, under a state-bit gate for others), resolves the ground material from a grapple,
    /// helicopter, or stunt attachment, and measures the water depth over the root.
    pub unsafe fn UpdateGroundInfo(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateGroundInfo_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DoGroundInfoRaycasts_ADDRESS: usize = 0x1407F49B0;
    /// Fills the character's ground information. While the proxy is supported (or sliding), not
    /// owned by a ragdoll, and not hanging from the grapple, the ground material and normal come
    /// from the proxy's support and the height over ground is zero, with no ray cast. Otherwise
    /// it measures the height over ground with
    /// [`CalculateHeightOverGround`](crate::character::character::Character::CalculateHeightOverGround) and, when above the ground,
    /// the fall distance.
    pub unsafe fn DoGroundInfoRaycasts(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DoGroundInfoRaycasts_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CalculateHeightOverGround_ADDRESS: usize = 0x1407D55E0;
    /// Casts a ray straight down from just above the character's root (the ragdoll's transform
    /// when a ragdoll owns the displacement) through
    /// [`PhysicsSystem::CastRay`](crate::physics::PhysicsSystem::CastRay), with a hit record and a
    /// character raycast filter (`CCharacterRaycastFilter`, which ignores the character) both
    /// built on the stack, on [`QUERY_LAYER`](crate::physics::PhysicsSystem::QUERY_LAYER), to 200 metres.
    /// Records the hit's material, normal, and supporting vehicle, and the height over the ground
    /// or the water surface ([`PhysicsSystem::CastRayWaterSurface`](crate::physics::PhysicsSystem::CastRayWaterSurface)),
    /// whichever is nearer. Its call chain from
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) makes it the
    /// engine's scene ray query inside the pose-finalization pass, which the post-physics pass
    /// queues for the parallel update of every character.
    pub unsafe fn CalculateHeightOverGround(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CalculateHeightOverGround_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateSecondaryHandIKPass_ADDRESS: usize = 0x1407EF690;
    /// The `SECONDARY` HumanIK pass, called from
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) after the `MAIN`
    /// solve. It is driven entirely by keys on [`m_Blackboard`](crate::character::character::Character::m_Blackboard); most suppliers
    /// write theirs fresh every frame, but a key's own persistence class is up to its writer — the
    /// parachute handle IK task's position and weight keys, for instance, are written once with
    /// session persistence and left on the blackboard across ticks while the task is active (see
    /// [`NParachuteHandleIKTask::Update`](crate::input::parachute::NParachuteHandleIKTask::Update)).
    /// For each wrist whose weight key
    /// ([`SECONDARY_HAND_WEIGHT_RIGHT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_RIGHT_ID),
    /// [`SECONDARY_HAND_WEIGHT_LEFT_ID`](crate::blackboard::ObjectBlackboard::SECONDARY_HAND_WEIGHT_LEFT_ID)) is
    /// positive, it queues a position target on that wrist's effector, taken from the hand's
    /// world-space key transformed into model space or, when that key is absent, from the hand's
    /// model-space key, with the weight as the reach. It then places the off-hand on the selected
    /// weapon's grip: while
    /// [`OFFHAND_IK_WEIGHT_ID`](crate::blackboard::ObjectBlackboard::OFFHAND_IK_WEIGHT_ID) is positive and
    /// `CWeaponBase::GetGripPosition` succeeds for the hand named by
    /// [`OFFHAND_IK_HAND_ID`](crate::blackboard::ObjectBlackboard::OFFHAND_IK_HAND_ID), it targets that wrist so that
    /// the hand's second attach bone
    /// ([`ATTACH_HAND_LEFT2`](crate::character::character::SafeBoneIndex::ATTACH_HAND_LEFT2) or
    /// [`ATTACH_HAND_RIGHT2`](crate::character::character::SafeBoneIndex::ATTACH_HAND_RIGHT2)) lands on the grip point. The
    /// weapon is the explosive slot while planting, otherwise the selected slot, and only while its
    /// state is wielding, dual-wielding, or mounted. When anything was queued it solves from the
    /// `MAIN`-solved pose and refreshes the inventory's attachment transforms; it always clears the
    /// pass's targets afterwards.
    pub unsafe fn UpdateSecondaryHandIKPass(&mut self, dt: f32, ik_has_run: *mut bool) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                ik_has_run: *mut bool,
            ) = ::std::mem::transmute(Self::UpdateSecondaryHandIKPass_ADDRESS);
            f(self as *mut Self as _, dt, ik_has_run)
        }
    }
    pub const HandleAimingInputPlayer_ADDRESS: usize = 0x1407F0160;
    /// The per-tick aiming-input update of a character driven by player input. Called from
    /// `UpdatePassPreStateMachines_Parallel` on the branch that also runs the player's global input
    /// and the inventory's pre-state update; other characters run
    /// [`HandleAimingInputNPC`](crate::character::character::Character::HandleAimingInputNPC) instead. It is the only code that
    /// reads [`SB_NO_WEAPON_AIM`](crate::state::StateBitFlag::SB_NO_WEAPON_AIM) besides its NPC
    /// counterpart.
    ///
    /// With the selected weapon (the planted explosive while planting), it refills
    /// [`m_AimTimer`](crate::character::character::Character::m_AimTimer) from [`m_AimingSettings`](crate::character::character::Character::m_AimingSettings), or
    /// from a non-negative [`WeaponBase::GetAimTimerOverride`](crate::equipment::weapon::WeaponBase::GetAimTimerOverride),
    /// while the weapon reports that it is aiming and
    /// [`WEAPONS_ENABLED`](crate::character::character::DamageStateFlags::WEAPONS_ENABLED) is set, or while the player's
    /// [`m_AimMode`](crate::character::player::Player::m_AimMode) is
    /// [`AIM_PRECISION`](crate::character::player::AimMode::AIM_PRECISION). It zeroes the timer instead
    /// when the weapon disables precision aim
    /// ([`m_IsPrecisionAimDisabled`](crate::equipment::weapon::WeaponBase::m_IsPrecisionAimDisabled)), when
    /// it cannot be aimed
    /// ([`m_AimingWeaponIsPossible`](crate::equipment::weapon::WeaponBase::m_AimingWeaponIsPossible)
    /// clear), or while the mental state is
    /// [`E_RELAXED`](crate::character::avatar::MentalState::E_RELAXED). With no weapon it zeroes the timer
    /// unless [`SB_IS_DRAWING_WEAPON`](crate::state::StateBitFlag::SB_IS_DRAWING_WEAPON) or
    /// [`SB_IN_FULLBODY_PICKUP`](crate::state::StateBitFlag::SB_IN_FULLBODY_PICKUP) is set.
    ///
    /// The timer then counts down, held at a 0.5 s floor while
    /// [`SB_PAUSE_AIM_TIMER`](crate::state::StateBitFlag::SB_PAUSE_AIM_TIMER) is set, and a positive timer
    /// means aiming. Planting an explosive outside
    /// [`SB_IS_HOLSTERING_WEAPON`](crate::state::StateBitFlag::SB_IS_HOLSTERING_WEAPON), and
    /// [`SB_IS_SWITCHING_WEAPONS`](crate::state::StateBitFlag::SB_IS_SWITCHING_WEAPONS) in one switch
    /// phase, force aiming. [`SB_NO_WEAPON_AIM_NOAIM_TRANSITIONS`](crate::state::StateBitFlag::SB_NO_WEAPON_AIM_NOAIM_TRANSITIONS)
    /// keeps the previous result, and [`SB_NO_WEAPON_AIM`](crate::state::StateBitFlag::SB_NO_WEAPON_AIM)
    /// forces the result to "not aiming" over everything else. The result is written to
    /// [`m_AimingWeapon`](crate::character::character::AimState::m_AimingWeapon); [`m_AimingGrapple`](crate::character::character::AimState::m_AimingGrapple)
    /// is left as [`PreSimUpdateGrapple`](crate::character::character::Character::PreSimUpdateGrapple) wrote it. The function then
    /// records the previous state in `m_WasAiming`, mirrors `m_AimingWeapon | m_AimingGrapple` into
    /// [`SB_IS_AIMING_WPN_OR_GRPL`](crate::state::StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL), and queues
    /// `ACT_ON_STARTING_AIM`, `ACT_AIM`, or `ACT_ON_LEAVING_AIM` on the animated model directly
    /// (not through [`QueueAct`](crate::character::character::Character::QueueAct)). While
    /// [`m_AimingEnabled`](crate::character::character::AimState::m_AimingEnabled) is clear it only clears `m_AimingWeapon`.
    ///
    /// The locomotion tasks read the aim flags to select aim-relative (strafe) movement.
    pub unsafe fn HandleAimingInputPlayer(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::HandleAimingInputPlayer_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const IsAimingWeaponPossible_ADDRESS: usize = 0x1407F2B90;
    /// Whether the character is currently permitted to aim a weapon (an aimable weapon is equipped
    /// and the character state allows it). Gates the aim path in the locomotion tasks.
    pub unsafe fn IsAimingWeaponPossible(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAimingWeaponPossible_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsPlayer_ADDRESS: usize = 0x14075FA60;
    /// Whether the character is the local player. Used by the locomotion queue helpers to restrict
    /// the aim-relative movement path to the player.
    pub unsafe fn IsPlayer(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPlayer_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const QueueAct_ADDRESS: usize = 0x14075DF10;
    /// Queues an animation act by id (`act` points at the runtime `idstring` value, e.g. the
    /// `ACT_*` id globals initialized at startup). Acts drive the animation state machine's
    /// transitions; unmatched acts are dropped at the end of the frame.
    pub unsafe fn QueueAct(&mut self, act: *const u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, act: *const u32) = ::std::mem::transmute(
                Self::QueueAct_ADDRESS,
            );
            f(self as *mut Self as _, act)
        }
    }
    pub const IsUsingWingsuit_ADDRESS: usize = 0x14075F630;
    /// Whether the character is flying the wingsuit:
    /// [`SB_IS_WINGSUIT`](crate::state::StateBitFlag::SB_IS_WINGSUIT), and only
    /// once [`AnimatedModel::m_InstanceData`](crate::character::character::AnimatedModel::m_InstanceData) exists (false before the animation instance is created).
    pub unsafe fn IsUsingWingsuit(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUsingWingsuit_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const HasBoostWingsuit_ADDRESS: usize = 0x14077ECC0;
    /// Whether the character has the boost wingsuit, the wingsuit with guns: the flag on its aim
    /// control, false without one. While it has it and flies the wingsuit
    /// ([`IsUsingWingsuit`](crate::character::character::Character::IsUsingWingsuit)), the inventory's weapon updates
    /// ([`Inventory::UpdateWeapons_Parallel`](crate::character::inventory::Inventory::UpdateWeapons_Parallel))
    /// update the wingsuit's guns in place of the wielded weapon.
    pub unsafe fn HasBoostWingsuit(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::HasBoostWingsuit_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsUsingParachute_ADDRESS: usize = 0x14075F550;
    /// Whether the parachute is deployed:
    /// [`SB_IS_PARACHUTING`](crate::state::StateBitFlag::SB_IS_PARACHUTING).
    pub unsafe fn IsUsingParachute(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUsingParachute_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsDetonatingExplosives_ADDRESS: usize = 0x14075F4B0;
    /// Whether the character is playing the detonate act:
    /// [`SB_IS_DETONATING_EXPLOSIVES`](crate::state::StateBitFlag::SB_IS_DETONATING_EXPLOSIVES).
    pub unsafe fn IsDetonatingExplosives(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsDetonatingExplosives_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsFreefalling_ADDRESS: usize = 0x14075F570;
    /// Whether the character is in free fall:
    /// [`SB_IS_FREE_FALLING`](crate::state::StateBitFlag::SB_IS_FREE_FALLING).
    pub unsafe fn IsFreefalling(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsFreefalling_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsFastfalling_ADDRESS: usize = 0x14075F910;
    /// Whether the character is in a fast (dive) fall:
    /// [`SB_IS_FASTFALLING`](crate::state::StateBitFlag::SB_IS_FASTFALLING).
    pub unsafe fn IsFastfalling(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsFastfalling_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsReelingIn_ADDRESS: usize = 0x14075F530;
    /// Whether a grapple reel-in traversal is in progress:
    /// [`SB_IN_REELING_IN`](crate::state::StateBitFlag::SB_IN_REELING_IN).
    pub unsafe fn IsReelingIn(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsReelingIn_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsRidingMC_ADDRESS: usize = 0x14075F380;
    /// Whether the character is riding a motorcycle:
    /// [`SB_IS_RIDING_MOTORBIKE`](crate::state::StateBitFlag::SB_IS_RIDING_MOTORBIKE).
    pub unsafe fn IsRidingMC(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsRidingMC_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsSwimming_ADDRESS: usize = 0x14075F450;
    /// Whether the character is swimming (surface or underwater):
    /// [`SB_IS_SWIMMING`](crate::state::StateBitFlag::SB_IS_SWIMMING).
    pub unsafe fn IsSwimming(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsSwimming_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsUnderwaterSwimming_ADDRESS: usize = 0x14075F470;
    /// Whether the character is swimming underwater:
    /// [`SB_IS_UW_SWIMMING`](crate::state::StateBitFlag::SB_IS_UW_SWIMMING).
    pub unsafe fn IsUnderwaterSwimming(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsUnderwaterSwimming_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInAttachedToParachute_ADDRESS: usize = 0x14075F7C0;
    /// Whether the character hangs from a vehicle-attached parachute:
    /// [`SB_IN_ATTACHED_TO_PARACHUTE`](crate::state::StateBitFlag::SB_IN_ATTACHED_TO_PARACHUTE).
    pub unsafe fn IsInAttachedToParachute(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInAttachedToParachute_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsPreReelingIn_ADDRESS: usize = 0x14075F770;
    /// Whether the character is in the reel-in wind-up:
    /// [`SB_IN_PRE_REEL`](crate::state::StateBitFlag::SB_IN_PRE_REEL).
    pub unsafe fn IsPreReelingIn(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPreReelingIn_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsPreHang_ADDRESS: usize = 0x14075F790;
    /// Whether the character is in the wind-up to a grapple hang:
    /// [`SB_IN_PRE_HANG`](crate::state::StateBitFlag::SB_IN_PRE_HANG).
    pub unsafe fn IsPreHang(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPreHang_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInRidingInVehicleState_ADDRESS: usize = 0x14077EA60;
    /// Whether the character is seated and riding a vehicle:
    /// [`SB_IS_RIDING_MOTORBIKE`](crate::state::StateBitFlag::SB_IS_RIDING_MOTORBIKE),
    /// or the body rule system's current state is one of eight riding states (idle, driver,
    /// passenger, and reverse variants). Reads game-thread animation state.
    pub unsafe fn IsInRidingInVehicleState(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInRidingInVehicleState_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInDrivingVehicleState_ADDRESS: usize = 0x14077EAF0;
    /// Whether the character is in a driver-seat driving state:
    /// [`SB_IS_RIDING_MOTORBIKE`](crate::state::StateBitFlag::SB_IS_RIDING_MOTORBIKE),
    /// or the body rule system's current state is one of four driving states. Reads game-thread
    /// animation state.
    pub unsafe fn IsInDrivingVehicleState(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInDrivingVehicleState_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsReeledIn_ADDRESS: usize = 0x14077ED10;
    /// Whether the character is reeled onto an attach point: the grappling hook's state is one of
    /// the reeled states
    /// ([`GHS_REELED_ATTACHED`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_ATTACHED),
    /// [`GHS_REELED_HANG`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_HANG),
    /// [`GHS_REELED_UPSIDEDOWN`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_UPSIDEDOWN),
    /// or [`GHS_REELED_STUNT`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_STUNT)),
    /// and either
    /// [`SB_IN_GRAPPLE_ATTACHED`](crate::state::StateBitFlag::SB_IN_GRAPPLE_ATTACHED)
    /// or [`SB_IS_GRAPPLE_UPSIDEDOWN`](crate::state::StateBitFlag::SB_IS_GRAPPLE_UPSIDEDOWN)
    /// is set or the body rule system is in one of eight reeled-in stunt and hang states.
    pub unsafe fn IsReeledIn(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsReeledIn_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsGrappleHanging_ADDRESS: usize = 0x14079E290;
    /// Whether the character hangs from a reeled grapple: with a hook present, the body rule
    /// system is in `S_GRAPPLE_HANG` or `S_IDLE_HANG_STUNT`, the hook's state is
    /// [`GHS_REELED_HANG`](crate::equipment::grappling_hook::GrapplingHookState::GHS_REELED_HANG), or
    /// [`SB_IS_GRAPPLE_UPSIDEDOWN`](crate::state::StateBitFlag::SB_IS_GRAPPLE_UPSIDEDOWN)
    /// is set.
    pub unsafe fn IsGrappleHanging(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsGrappleHanging_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInAir_ADDRESS: usize = 0x14079E3C0;
    /// Whether the character is in the air: its proxy has no valid ground body
    /// ([`PfxCharacterInstance::IsGroundBodyValid`](crate::physics::PfxCharacterInstance::IsGroundBodyValid)), or is neither supported
    /// ([`PfxCharacterInstance::IsSupported`](crate::physics::PfxCharacterInstance::IsSupported)) nor sliding, or it has a ragdoll that does not
    /// follow the character; and a flag of the control parameters (not yet named) is clear.
    pub unsafe fn IsInAir(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInAir_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdatePassPrePhysics_Serial_ADDRESS: usize = 0x1407F4710;
    /// The character's serial pre-physics update pass: the queued impulses, the pre-physics state
    /// tasks (the on-foot movement task among them), the ragdoll's pre-physics update, and the
    /// integrating-physics update, which runs the proxy's update
    /// ([`PfxCharacterInstance::UpdateContext`](crate::physics::PfxCharacterInstance::UpdateContext)). The character registers it in the game world's
    /// pre-simulation update queue after its state-machine passes, which the queue runs in order
    /// of registration priority, so each tick the state machines evaluate their conditions on the
    /// proxy's support as the previous tick's proxy update left it, and the physics system's
    /// pre-simulation callbacks run after this pass.
    pub unsafe fn UpdatePassPrePhysics_Serial(
        &mut self,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::UpdatePassPrePhysics_Serial_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const UpdatePassPostPhysics_Serial_ADDRESS: usize = 0x1407FCEB0;
    /// The character's registered serial post-physics update. Performs post-physics state work,
    /// then polls the character's message port to dispatch animation track messages.
    pub unsafe fn UpdatePassPostPhysics_Serial(
        &mut self,
        context: *mut crate::game::GameObjectUpdateContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *mut crate::game::GameObjectUpdateContext,
            ) = ::std::mem::transmute(Self::UpdatePassPostPhysics_Serial_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const IsStuntTraversing_ADDRESS: usize = 0x14077EF60;
    /// Whether the body rule system is in one of the four stunt-position traversal states
    /// (`S_STUNT_FWD`, `S_STUNT_RIGHT`, `S_STUNT_LEFT`, `S_STUNT_BWD`).
    pub unsafe fn IsStuntTraversing(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsStuntTraversing_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInStuntPosition_ADDRESS: usize = 0x1407F5080;
    /// Whether the character rides a vehicle in a stunt position: attached to a vehicle, and the
    /// body rule system is in `S_IDLE_HANG_STUNT`, the interaction-graph node the character is on
    /// is of node type 4 (not yet named), or
    /// [`SB_IS_GRAPPLE_UPSIDEDOWN`](crate::state::StateBitFlag::SB_IS_GRAPPLE_UPSIDEDOWN) is set.
    pub unsafe fn IsInStuntPosition(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInStuntPosition_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsInStuntArea_ADDRESS: usize = 0x1407F1FA0;
    /// Whether the character stands in a stunt area of a vehicle: attached to a vehicle, and the
    /// interaction-graph node it is on is a stunt-area node (node type 4, the type
    /// [`IsInStuntPosition`](crate::character::character::Character::IsInStuntPosition) also accepts).
    pub unsafe fn IsInStuntArea(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInStuntArea_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const ShouldApplyStuntModification_ADDRESS: usize = 0x1407F1A00;
    /// Whether the on-foot input tasks apply their stunt modification, the stunt turns
    /// ([`QueueStuntTurns`](crate::input::locomotion::NStateTask_LocoUtil::QueueStuntTurns)): the
    /// character is attached to a vehicle, or a global override flag is set.
    pub unsafe fn ShouldApplyStuntModification(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::ShouldApplyStuntModification_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsStuntHanging_ADDRESS: usize = 0x1407F12D0;
    /// Whether the character hangs from a helicopter in its stunt position: the vehicle it is
    /// attached to ([`GetVehicle`](crate::character::character::Character::GetVehicle)) is a
    /// [`Helicopter`](crate::vehicle::vehicle::Helicopter), and the body rule system is in one of the
    /// stunt idles (`S_IDLE_STUNT`, `S_IDLE_HANG_STUNT`, or one of the five `S_IDLE_STUNT_FIRE_*`
    /// states). A helicopter has no stunt areas to stand on; its stunt position is this hang.
    pub unsafe fn IsStuntHanging(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsStuntHanging_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const AutoAttachToClosestStuntAreaOnVehicle_ADDRESS: usize = 0x1407C8CD0;
    /// Attaches the character to `vehicle` in a stunt position near `from_position`, returning
    /// whether it did. On a [`Helicopter`](crate::vehicle::vehicle::Helicopter) it runs the components of
    /// the vehicle interaction context's hang interaction, which puts the character in the hang
    /// ([`IsStuntHanging`](crate::character::character::Character::IsStuntHanging)); it fails when the helicopter has none. On any
    /// other vehicle it finds the stunt area nearest `from_position`
    /// ([`GetNearestStuntArea_World`](crate::vehicle::stunt_areas::GetNearestStuntArea_World) over
    /// [`Vehicle::m_StuntAreas`](crate::vehicle::vehicle::Vehicle::m_StuntAreas)) and that area's node in
    /// the vehicle's interaction graph, runs the context's stunt interaction, enters the node,
    /// hands the character's velocity to the game (the physics input state's game-controlled
    /// velocity flag), turns the character and its proxy to the vehicle's orientation
    /// ([`SetOrientation`](crate::character::character::Character::SetOrientation)), and places it on the area at the nearest point,
    /// or at the node's centre when `from_position` is outside the area, as the interaction user's
    /// local matrix and the move action's capsule position. It fails for a vehicle without areas,
    /// an area without a node, or a context without the stunt interaction. The graph-script node
    /// `NGSONodes::TryAttachToClosestStuntArea` calls it; the on-foot movement tasks attach a
    /// character that lands on a vehicle through
    /// [`AutoAttachToStuntAreaOnVehicleBelow`](crate::character::character::Character::AutoAttachToStuntAreaOnVehicleBelow) instead.
    pub unsafe fn AutoAttachToClosestStuntAreaOnVehicle(
        &mut self,
        vehicle: *mut crate::vehicle::vehicle::Vehicle,
        from_position: *const crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                vehicle: *mut crate::vehicle::vehicle::Vehicle,
                from_position: *const crate::types::math::Vector3,
            ) -> bool = ::std::mem::transmute(
                Self::AutoAttachToClosestStuntAreaOnVehicle_ADDRESS,
            );
            f(self as *mut Self as _, vehicle, from_position)
        }
    }
    pub const AutoAttachToStuntAreaOnVehicleBelow_ADDRESS: usize = 0x1407F1A30;
    /// Attaches a character that stands on a vehicle to its nearest stunt area, returning whether
    /// it did: with the character's vehicle-below flag set and the vehicle it names not upside
    /// down, and the character not yet attached to a vehicle, the area the character's position
    /// projects onto, if the projection is close enough; a point near the area's edge is pulled in
    /// toward the node's centre. It joins the area's interaction-graph node as an active user
    /// (`CInteractionGraph::AddUser`, which the IDB labels
    /// `CGrapplingHook::DoReReelToStaticPosition`), hands the character's
    /// velocity to the game, and places it there as
    /// [`AutoAttachToClosestStuntAreaOnVehicle`](crate::character::character::Character::AutoAttachToClosestStuntAreaOnVehicle)
    /// does, without turning it. The on-foot locomotion and stunting movement tasks and the NPC
    /// path-follow and on-spot tasks call it each update.
    pub unsafe fn AutoAttachToStuntAreaOnVehicleBelow(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::AutoAttachToStuntAreaOnVehicleBelow_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IsInMountedGunState_ADDRESS: usize = 0x1407B0F60;
    /// Whether the character mans a mounted gun: the weapon in the
    /// [`E_SLOT_HEAVY`](crate::character::inventory::WeaponSlot::E_SLOT_HEAVY) slot is in its mounted weapon state.
    pub unsafe fn IsInMountedGunState(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInMountedGunState_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetVehicle_ADDRESS: usize = 0x1407D5D90;
    /// Fills `out` with the vehicle the character is attached to, found by walking the interaction
    /// graph's top root object and casting it to `CVehicle`; empty when there is none. Takes a
    /// reference on the vehicle.
    pub unsafe fn GetVehicle(
        &self,
        out: *mut crate::types::shared_ptr::SharedPtr<crate::vehicle::vehicle::Vehicle>,
    ) -> *mut crate::types::shared_ptr::SharedPtr<crate::vehicle::vehicle::Vehicle> {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::shared_ptr::SharedPtr<
                    crate::vehicle::vehicle::Vehicle,
                >,
            ) -> *mut crate::types::shared_ptr::SharedPtr<
                crate::vehicle::vehicle::Vehicle,
            > = ::std::mem::transmute(Self::GetVehicle_ADDRESS);
            f(self as *const Self as _, out)
        }
    }
    pub const GetVehiclePtr_ADDRESS: usize = 0x1407D5E30;
    /// The raw-pointer form of [`GetVehicle`](crate::character::character::Character::GetVehicle); still takes and releases a
    /// reference internally.
    pub unsafe fn GetVehiclePtr(&self) -> *mut crate::vehicle::vehicle::Vehicle {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::vehicle::vehicle::Vehicle = ::std::mem::transmute(
                Self::GetVehiclePtr_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAttachedToVehicle_ADDRESS: usize = 0x1407D5EA0;
    /// Whether the character is attached to any vehicle through the interaction graph.
    pub unsafe fn IsAttachedToVehicle(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToVehicle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetProxyState_ADDRESS: usize = 0x14075EB20;
    /// Requests proxy state `state` (a [`ProxyState`](crate::physics::ProxyState) value) by writing
    /// [`m_PendingProxyState`](crate::character::character::Character::m_PendingProxyState); the physics proxy changes shape when
    /// [`UpdateProxyState`](crate::character::character::Character::UpdateProxyState) next runs.
    pub unsafe fn SetProxyState(&mut self, state: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, state: i32) = ::std::mem::transmute(
                Self::SetProxyState_ADDRESS,
            );
            f(self as *mut Self as _, state)
        }
    }
    pub const SetDefaultProxyState_ADDRESS: usize = 0x14077D6C0;
    /// Sets [`m_DefaultProxyState`](crate::character::character::Character::m_DefaultProxyState) to `state`, and also requests it as
    /// the pending state when the proxy was in the previous default.
    pub unsafe fn SetDefaultProxyState(&mut self, state: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, state: i32) = ::std::mem::transmute(
                Self::SetDefaultProxyState_ADDRESS,
            );
            f(self as *mut Self as _, state)
        }
    }
    pub const UpdateProxyState_ADDRESS: usize = 0x14077E4D0;
    /// Applies [`m_PendingProxyState`](crate::character::character::Character::m_PendingProxyState) to the physics proxy when it
    /// differs from the proxy's current state
    /// ([`PfxCharacterInstance::SetProxyState`](crate::physics::PfxCharacterInstance::SetProxyState)),
    /// then refreshes the character's obstacle in the AI pathfinder. The same body is inlined at the
    /// end of the character's serial state-machine pass, after the state tasks run.
    pub unsafe fn UpdateProxyState(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::UpdateProxyState_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetWantedWorldVelocity_ADDRESS: usize = 0x14077E9A0;
    /// Writes the proxy's wanted velocity ([`CharacterInput::wantedVelocity`](crate::physics::CharacterInput::wantedVelocity),
    /// character-local) rotated into world space by [`m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1) to
    /// `out`, and returns `out`.
    pub unsafe fn GetWantedWorldVelocity(
        &self,
        out: *mut crate::types::math::Vector3,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::math::Vector3,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetWantedWorldVelocity_ADDRESS,
            );
            f(self as *const Self as _, out)
        }
    }
    pub const GetHipsPosition_ADDRESS: usize = 0x1407AF370;
    /// Writes the world position of the hips bone to `out` and returns `out`: the bone's model-space
    /// matrix, looked up by the bone's name hash, composed with the
    /// character's world matrix.
    pub unsafe fn GetHipsPosition(
        &self,
        out: *mut crate::types::math::Vector3,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::math::Vector3,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::GetHipsPosition_ADDRESS,
            );
            f(self as *const Self as _, out)
        }
    }
    pub const SetWantedWorldVelocity_ADDRESS: usize = 0x14077E9F0;
    /// Sets the proxy's wanted velocity from the world-space `velocity`, converting it into the
    /// character-local frame with the inverse of the proxy's transform.
    pub unsafe fn SetWantedWorldVelocity(
        &mut self,
        velocity: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                velocity: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetWantedWorldVelocity_ADDRESS);
            f(self as *mut Self as _, velocity)
        }
    }
    pub const IsAttachedToLandVehicle_ADDRESS: usize = 0x1407D5FA0;
    /// Whether the attached vehicle is a `CLandVehicle` (cars and motorcycles), by RTTI `IsType`.
    pub unsafe fn IsAttachedToLandVehicle(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToLandVehicle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAttachedToAirVehicle_ADDRESS: usize = 0x1407D6060;
    /// Whether the attached vehicle is a `CAirVehicle` (planes and helicopters), by RTTI `IsType`.
    pub unsafe fn IsAttachedToAirVehicle(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToAirVehicle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAttachedToSeaVehicle_ADDRESS: usize = 0x1407D6120;
    /// Whether the attached vehicle is a `CSeaVehicle` (boats and jetskis), by RTTI `IsType`.
    pub unsafe fn IsAttachedToSeaVehicle(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToSeaVehicle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAttachedToHelicopter_ADDRESS: usize = 0x1407D61E0;
    /// Whether the attached vehicle is a `CHelicopter`, by RTTI `IsType`.
    pub unsafe fn IsAttachedToHelicopter(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToHelicopter_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAttachedToMech_ADDRESS: usize = 0x1407D62A0;
    /// Whether the attached vehicle is a mech ([`Vehicle::IsMech`](crate::vehicle::vehicle::Vehicle::IsMech)).
    pub unsafe fn IsAttachedToMech(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAttachedToMech_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateWeaponAiming_ADDRESS: usize = 0x1407602C0;
    /// Stores `target` in [`m_AimTargetPositionWeapons`](crate::character::character::Character::m_AimTargetPositionWeapons) and
    /// sets [`m_AimTargetPositionWeaponWasSetThisFrame`](crate::character::character::Character::m_AimTargetPositionWeaponWasSetThisFrame).
    pub unsafe fn UpdateWeaponAiming(
        &mut self,
        target: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::UpdateWeaponAiming_ADDRESS);
            f(self as *mut Self as _, target)
        }
    }
    pub const UpdateGrenadeAiming_ADDRESS: usize = 0x1407602F0;
    /// Stores `target` in [`m_AimTargetPositionGrenade`](crate::character::character::Character::m_AimTargetPositionGrenade).
    pub unsafe fn UpdateGrenadeAiming(
        &mut self,
        target: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::UpdateGrenadeAiming_ADDRESS);
            f(self as *mut Self as _, target)
        }
    }
    pub const UpdateWeapons_Parallel_ADDRESS: usize = 0x140760660;
    /// Runs the inventory's parallel weapon update
    /// ([`Inventory::Update_Parallel`](crate::character::inventory::Inventory::Update_Parallel)) with the
    /// weapon and grenade aim targets, then clears [`m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs) and
    /// the aim-target-set flag.
    pub unsafe fn UpdateWeapons_Parallel(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateWeapons_Parallel_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const CanPrecisionAim_ADDRESS: usize = 0x1407D9200;
    /// Whether the equipped weapon and the character state allow precision (down-sights) aiming.
    pub unsafe fn CanPrecisionAim(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::CanPrecisionAim_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const HandleAimingInputNPC_ADDRESS: usize = 0x1407F0530;
    /// The per-tick aiming-input update of a character not driven by player input (the other
    /// branch of `UpdatePassPreStateMachines_Parallel` from
    /// [`HandleAimingInputPlayer`](crate::character::character::Character::HandleAimingInputPlayer)). It refills
    /// [`m_AimTimer`](crate::character::character::Character::m_AimTimer) from [`m_AimingSettings`](crate::character::character::Character::m_AimingSettings) (or the
    /// wielded weapon's non-negative aim-timer override) while the wielded weapon reports that it
    /// is aiming with [`WEAPONS_ENABLED`](crate::character::character::DamageStateFlags::WEAPONS_ENABLED) set, or while the
    /// behaviour-tree avatar's aim request is set, and zeroes it with no wielded weapon or when the
    /// weapon disables precision aim. The countdown and the tail are the player variant's: the
    /// pause-aim-timer floor, the no-aim-transitions latch,
    /// [`SB_NO_WEAPON_AIM`](crate::state::StateBitFlag::SB_NO_WEAPON_AIM), the write of
    /// [`m_AimingWeapon`](crate::character::character::AimState::m_AimingWeapon), the mirror into
    /// [`SB_IS_AIMING_WPN_OR_GRPL`](crate::state::StateBitFlag::SB_IS_AIMING_WPN_OR_GRPL), and the
    /// aim-transition act.
    pub unsafe fn HandleAimingInputNPC(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::HandleAimingInputNPC_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const PreSimUpdateGrapple_ADDRESS: usize = 0x14079D420;
    /// The pre-simulation grapple update: runs the hook's own pre-sim update and recomputes the
    /// grapple-aim bit of [`m_AimFlags`](crate::character::character::Character::m_AimFlags).
    pub unsafe fn PreSimUpdateGrapple(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::PreSimUpdateGrapple_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const IsTryingToAim_ADDRESS: usize = 0x1407F5140;
    /// Whether the character is trying to aim (an aim input is active and aiming is permitted).
    pub unsafe fn IsTryingToAim(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsTryingToAim_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetCharacterModifiers_ADDRESS: usize = 0x1407F52E0;
    /// Sets the character's frame-bound animation modifiers for this tick, which select the variant
    /// clips of the animation set's nodes. Called once per tick from the pre-state-machine pass,
    /// after the aim input and before the state tasks. In order: the grapple modifier on every rule
    /// system while the hook is in its aiming state
    /// ([`GrapplingHook::IsHookAimingState`](crate::equipment::grappling_hook::GrapplingHook::IsHookAimingState));
    /// then, for the wielded weapon (the planted explosive while planting, otherwise the selected
    /// slot's weapon while it is wielded, dual-wielded, or mounted, or the weapon being switched to
    /// or drawn), through [`SetModifier`](crate::character::character::Character::SetModifier), the weapon's
    /// [`m_AimAnimationModifier`](crate::equipment::weapon::WeaponBase::m_AimAnimationModifier) when
    /// [`IsTryingToAim`](crate::character::character::Character::IsTryingToAim) holds and the hook is not aiming, its
    /// [`m_AnimationModifier`](crate::equipment::weapon::WeaponBase::m_AnimationModifier) otherwise, and
    /// always its
    /// [`m_EquippedAnimationModifier`](crate::equipment::weapon::WeaponBase::m_EquippedAnimationModifier);
    /// for a character other than the player, a faction or ghost modifier; the relaxed modifier
    /// while the avatar's mental state is relaxed; and a stunt modifier (combat with a weapon,
    /// non-combat without) while attached to a vehicle or while stunt mode is forced. Rico's on-foot
    /// idle node picks its unarmed relaxed clip with no weapon modifier, the weapon's held stance
    /// with its plain modifier, and the aimed stance with its aim modifier, the grapple modifier,
    /// or the explosive's modifier.
    pub unsafe fn SetCharacterModifiers(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SetCharacterModifiers_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetModifier_ADDRESS: usize = 0x14079A7B0;
    /// Marks an animation modifier active with `modifier_type` on every rule system of the
    /// character's animated model, through [`AnimationRuleSystem::SetModifier`](crate::animation::rule_system::AnimationRuleSystem::SetModifier).
    pub unsafe fn SetModifier(
        &mut self,
        modifier: *const u32,
        modifier_type: crate::animation::rule_system::AnimationModifierType,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                modifier: *const u32,
                modifier_type: crate::animation::rule_system::AnimationModifierType,
            ) = ::std::mem::transmute(Self::SetModifier_ADDRESS);
            f(self as *mut Self as _, modifier, modifier_type)
        }
    }
    pub const IsTryingToFireAnyWeapons_ADDRESS: usize = 0x1407F2AF0;
    /// Whether any weapon fire input is active: fire, precision-aim fire, or explosive planting.
    /// `ignore_animation` skips the inventory's animation-permission checks.
    pub unsafe fn IsTryingToFireAnyWeapons(&self, ignore_animation: bool) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ignore_animation: bool,
            ) -> bool = ::std::mem::transmute(Self::IsTryingToFireAnyWeapons_ADDRESS);
            f(self as *const Self as _, ignore_animation)
        }
    }
    pub const GetWieldingWeapon_ADDRESS: usize = 0x1407D8520;
    /// The weapon the character wields: the selected slot's weapon while it is
    /// [`E_WS_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_WIELDING),
    /// [`E_WS_DUAL_WIELDING`](crate::equipment::weapon::WeaponState::E_WS_DUAL_WIELDING), or
    /// [`E_WS_MOUNTED`](crate::equipment::weapon::WeaponState::E_WS_MOUNTED), and otherwise a shared
    /// empty pointer. While the character plants an explosive it is the planted explosive slot's
    /// weapon, or the empty pointer with `ignore_planting`. The result points into the inventory or
    /// at a static, so it outlives the call.
    /// [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)
    /// reads it to decide whether a weapon input draws a weapon.
    pub unsafe fn GetWieldingWeapon(
        &self,
        ignore_planting: bool,
    ) -> *const crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ignore_planting: bool,
            ) -> *const crate::types::shared_ptr::SharedPtr<
                crate::equipment::weapon::WeaponBase,
            > = ::std::mem::transmute(Self::GetWieldingWeapon_ADDRESS);
            f(self as *const Self as _, ignore_planting)
        }
    }
    pub const HasAnyWeaponInput_ADDRESS: usize = 0x1407F2D10;
    /// Whether any weapon input is active this frame.
    pub unsafe fn HasAnyWeaponInput(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::HasAnyWeaponInput_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetSelectedWeapon_ADDRESS: usize = 0x1407B1120;
    /// The selected slot's weapon reference
    /// ([`m_SelectedWeaponSlot`](crate::character::character::Character::m_SelectedWeaponSlot)) whatever its state, so a holstered
    /// weapon is returned too.
    pub unsafe fn GetSelectedWeapon(
        &self,
    ) -> *const crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const crate::types::shared_ptr::SharedPtr<
                crate::equipment::weapon::WeaponBase,
            > = ::std::mem::transmute(Self::GetSelectedWeapon_ADDRESS);
            f(self as *const Self as _)
        }
    }
    pub const GetWeaponInInventory_ADDRESS: usize = 0x1407A0130;
    /// The weapon reference in `slot` of the inventory
    /// ([`Inventory::m_Weapons`](crate::character::inventory::Inventory::m_Weapons)), whatever its state,
    /// or a shared empty pointer for an out-of-range slot.
    pub unsafe fn GetWeaponInInventory(
        &self,
        slot: crate::character::inventory::WeaponSlot,
    ) -> *const crate::types::shared_ptr::SharedPtr<
        crate::equipment::weapon::WeaponBase,
    > {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                slot: crate::character::inventory::WeaponSlot,
            ) -> *const crate::types::shared_ptr::SharedPtr<
                crate::equipment::weapon::WeaponBase,
            > = ::std::mem::transmute(Self::GetWeaponInInventory_ADDRESS);
            f(self as *const Self as _, slot)
        }
    }
    pub const IsPlantingExplosive_ADDRESS: usize = 0x1407D5BC0;
    /// Whether the planted explosive is in its planting mode.
    pub unsafe fn IsPlantingExplosive(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPlantingExplosive_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetSelectedWeaponSlot_ADDRESS: usize = 0x1407C4ED0;
    /// Writes the selected slot ([`m_SelectedWeaponSlot`](crate::character::character::Character::m_SelectedWeaponSlot)) and, for
    /// the dual-wield, two-handed, and special slots, the last gun slot, and queues a
    /// weapon-switched statistic for the local player when the slot changes to an occupied one. It
    /// does nothing else: no act, no equip, no state change, so a weapon in another slot that was
    /// wielded stays wielded. It is the selected slot's one writer.
    pub unsafe fn SetSelectedWeaponSlot(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::SetSelectedWeaponSlot_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const SelectWeapon_ADDRESS: usize = 0x1407F2E70;
    /// The game's request to switch to `slot`: selects it and draws it
    /// ([`DrawWeapon`](crate::character::character::Character::DrawWeapon)), holstering for
    /// [`E_NONE`](crate::character::inventory::WeaponSlot::E_NONE). Its callers are the committed
    /// selection ([`SwitchWeapon`](crate::character::character::Character::SwitchWeapon)), [`PickUpWeapon`](crate::character::character::Character::PickUpWeapon),
    /// the switch away from an empty weapon
    /// ([`Inventory::TryToSwitchToValidWeapon`](crate::character::inventory::Inventory::TryToSwitchToValidWeapon)),
    /// the graph script's `SpawnWeapons` node
    /// ([`NGSONodes_SpawnWeapons`](crate::graph_script::NGSONodes_SpawnWeapons)), the player's script
    /// event handler, the AI, and the network.
    pub unsafe fn SelectWeapon(
        &mut self,
        slot: crate::character::inventory::WeaponSlot,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                slot: crate::character::inventory::WeaponSlot,
            ) = ::std::mem::transmute(Self::SelectWeapon_ADDRESS);
            f(self as *mut Self as _, slot)
        }
    }
    pub const SwitchWeapon_ADDRESS: usize = 0x1407F8220;
    /// Commits the weapon selection's pending slot through [`SelectWeapon`](crate::character::character::Character::SelectWeapon).
    pub unsafe fn SwitchWeapon(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SwitchWeapon_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const PickUpWeapon_ADDRESS: usize = 0x1407F82A0;
    /// Takes `weapon` into the inventory with `mode`: fills its magazine to the tuned size
    /// ([`WeaponBase::GetMagazineSize`](crate::equipment::weapon::WeaponBase::GetMagazineSize)), puts it
    /// into its slot ([`Inventory::AddWeapon`](crate::character::inventory::Inventory::AddWeapon)), and
    /// with [`E_EQUIP`](crate::character::inventory::EquipMode::E_EQUIP) selects the slot for the
    /// dual-wield, two-handed, special, heavy, and FOW slots ([`SelectWeapon`](crate::character::character::Character::SelectWeapon)).
    /// The respawn hand-over
    /// ([`Inventory::WeaponSpawned`](crate::character::inventory::Inventory::WeaponSpawned)) and the
    /// spawn-and-equip callback call it.
    pub unsafe fn PickUpWeapon(
        &mut self,
        weapon: *const crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponBase,
        >,
        mode: crate::character::inventory::EquipMode,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon: *const crate::types::shared_ptr::SharedPtr<
                    crate::equipment::weapon::WeaponBase,
                >,
                mode: crate::character::inventory::EquipMode,
            ) = ::std::mem::transmute(Self::PickUpWeapon_ADDRESS);
            f(self as *mut Self as _, weapon, mode)
        }
    }
    pub const PutWeaponIntoInventory_ADDRESS: usize = 0x140760A20;
    /// Puts `weapon` into its slot with `mode`, a direct call of
    /// [`Inventory::AddWeapon`](crate::character::inventory::Inventory::AddWeapon). The pickup
    /// ([`WeaponBase::PickupItem`](crate::equipment::weapon::WeaponBase::PickupItem)), a mounted gun's
    /// [`StartUsing`](crate::equipment::weapon::WeaponBase::StartUsing), the weapon's track-message
    /// handling, and the supply drop's cargo call it.
    pub unsafe fn PutWeaponIntoInventory(
        &mut self,
        weapon: *const crate::types::shared_ptr::SharedPtr<
            crate::equipment::weapon::WeaponBase,
        >,
        mode: crate::character::inventory::EquipMode,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                weapon: *const crate::types::shared_ptr::SharedPtr<
                    crate::equipment::weapon::WeaponBase,
                >,
                mode: crate::character::inventory::EquipMode,
            ) = ::std::mem::transmute(Self::PutWeaponIntoInventory_ADDRESS);
            f(self as *mut Self as _, weapon, mode)
        }
    }
    pub const DropAllWeapons_ADDRESS: usize = 0x140760930;
    /// Drops every slot's weapon
    /// ([`Inventory::DropWeapon`](crate::character::inventory::Inventory::DropWeapon)). The script "drop
    /// all weapons" event and the player's reset to the default loadout call it.
    pub unsafe fn DropAllWeapons(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DropAllWeapons_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IsLocalPlayerControlled_ADDRESS: usize = 0x1407813D0;
    /// Whether the character is controlled locally by a player.
    pub unsafe fn IsLocalPlayerControlled(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsLocalPlayerControlled_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetAiEntityHandle_ADDRESS: usize = 0x14075E060;
    /// The character's AI entity handle, which identifies it as the source of an AI awareness
    /// event ([`BroadcastAwarenessEvent`](crate::ai::ai::BroadcastAwarenessEvent)).
    pub unsafe fn GetAiEntityHandle(&self) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> u32 = ::std::mem::transmute(
                Self::GetAiEntityHandle_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const AddHeatPoints_ADDRESS: usize = 0x1407C3340;
    /// Adds `points` of heat (wanted level) to the character within `range`. The meanings of the
    /// four flags are unverified. A weapon's shot
    /// ([`WeaponBase::Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase))
    /// calls it on the weapon's user.
    pub unsafe fn AddHeatPoints(
        &mut self,
        points: i32,
        p2: bool,
        p3: bool,
        p4: bool,
        range: f32,
        p6: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                points: i32,
                p2: bool,
                p3: bool,
                p4: bool,
                range: f32,
                p6: bool,
            ) = ::std::mem::transmute(Self::AddHeatPoints_ADDRESS);
            f(self as *mut Self as _, points, p2, p3, p4, range, p6)
        }
    }
    pub const IsDrawingOrHolsteringWeapon_ADDRESS: usize = 0x14075F420;
    /// Whether the character is drawing or holstering a weapon: the
    /// [`SB_IS_DRAWING_WEAPON`](crate::state::StateBitFlag::SB_IS_DRAWING_WEAPON) or
    /// [`SB_IS_HOLSTERING_WEAPON`](crate::state::StateBitFlag::SB_IS_HOLSTERING_WEAPON) state bit.
    pub unsafe fn IsDrawingOrHolsteringWeapon(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsDrawingOrHolsteringWeapon_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsNoFiringSet_ADDRESS: usize = 0x14075F600;
    /// Whether firing is blocked: the [`SB_IS_NO_FIRING`](crate::state::StateBitFlag::SB_IS_NO_FIRING)
    /// state bit.
    pub unsafe fn IsNoFiringSet(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsNoFiringSet_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsPredictedReloadingAllowed_ADDRESS: usize = 0x1407D6AD0;
    /// Whether the ammunition check's predicted reload may start: the character is a player not
    /// in precision aim, plus a condition on the player's aim control whose meaning is
    /// unverified.
    pub unsafe fn IsPredictedReloadingAllowed(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsPredictedReloadingAllowed_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetMentalState_ADDRESS: usize = 0x140760BE0;
    /// Sets the character's mental state on its [`m_Avatar`](crate::character::character::Character::m_Avatar) with a hold `timer`.
    /// Unless `force` is set, the write happens only once the avatar's current timer has run out.
    pub unsafe fn SetMentalState(
        &mut self,
        state: crate::character::avatar::MentalState,
        force: bool,
        timer: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                state: crate::character::avatar::MentalState,
                force: bool,
                timer: f32,
            ) = ::std::mem::transmute(Self::SetMentalState_ADDRESS);
            f(self as *mut Self as _, state, force, timer)
        }
    }
    pub const DrawWeapon_ADDRESS: usize = 0x1407608A0;
    /// Draws the selected weapon when the inventory's animation state allows it: selects a valid
    /// weapon ([`Inventory::SelectValidWeapon`](crate::character::inventory::Inventory::SelectValidWeapon),
    /// with the two flags passed through), queues the draw act (`UB_ACT_DRAW`), and blocks drawing
    /// and grenades until the animation re-enables them. The equip itself happens when the draw
    /// clip's equip track message reaches
    /// [`WeaponBase::OnEquip`](crate::equipment::weapon::WeaponBase::OnEquip). Its callers are
    /// [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)
    /// (a fire input with nothing wielded, the precision-aim draw, the release of a pending
    /// selection, and the combat mental state's draw), [`SelectWeapon`](crate::character::character::Character::SelectWeapon),
    /// [`Player::SetAimMode`](crate::character::player::Player::SetAimMode), and the AI.
    pub unsafe fn DrawWeapon(
        &mut self,
        planted_explosive_allowed: bool,
        only_if_ammo: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                planted_explosive_allowed: bool,
                only_if_ammo: bool,
            ) = ::std::mem::transmute(Self::DrawWeapon_ADDRESS);
            f(self as *mut Self as _, planted_explosive_allowed, only_if_ammo)
        }
    }
    pub const HolsterWeapon_ADDRESS: usize = 0x1407F2D80;
    /// Holsters the selected weapon (the planted explosive while planting). For a character with
    /// [`m_IsPlayer`](crate::character::character::Character::m_IsPlayer) set it queues nothing itself: it forces the avatar's
    /// [`m_MentalState`](crate::character::avatar::Avatar::m_MentalState) to
    /// [`E_RELAXED`](crate::character::avatar::MentalState::E_RELAXED) with a zero hold timer, and
    /// [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling)'s
    /// relaxed branch then queues `UB_ACT_HOLSTER`. Other characters queue `UB_ACT_HOLSTER` on the
    /// animated model directly when the inventory's animation state allows holstering.
    pub unsafe fn HolsterWeapon(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HolsterWeapon_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const StartReloadingWieldingWeapon_ADDRESS: usize = 0x1407D8F50;
    /// Starts a reload of the wielded weapon: queues the upper-body reload act (`UB_ACT_RELOAD`)
    /// and blocks grenades, when the inventory allows reloading
    /// ([`Inventory::GetAnimationAllowsReloading`](crate::character::inventory::Inventory::GetAnimationAllowsReloading)),
    /// the detonate input
    /// ([`E_INPUT_DETONATE_EXPLOSIVES`](crate::character::character::WeaponInput::E_INPUT_DETONATE_EXPLOSIVES)) is zero, a
    /// weapon is wielded ([`GetWieldingWeapon`](crate::character::character::Character::GetWieldingWeapon)), and that weapon
    /// [`CanReload`](crate::equipment::weapon::WeaponBase::CanReload). Otherwise it does nothing. The
    /// act's callback runs the weapon's
    /// [`StartReloadSequence`](crate::equipment::weapon::WeaponBase::StartReloadSequence) once the act
    /// is accepted; the ammunition itself moves later, when the reload clip's track message calls
    /// [`Reload`](crate::equipment::weapon::WeaponBase::Reload). The reload state raises
    /// [`SB_IS_RELOADING`](crate::state::StateBitFlag::SB_IS_RELOADING).
    ///
    /// Every animated reload starts here. Its callers are:
    ///
    /// - [`Inventory::UpdateWeaponHandling`](crate::character::inventory::Inventory::UpdateWeaponHandling),
    ///   on a positive reload input ([`E_INPUT_RELOAD`](crate::character::character::WeaponInput::E_INPUT_RELOAD)) while the
    ///   inventory allows reloading;
    /// - [`WeaponBase::CheckIfReloadShouldBeTriggeredBase`](crate::equipment::weapon::WeaponBase::CheckIfReloadShouldBeTriggeredBase),
    ///   for the automatic reload of a player's empty magazine, on every tick while the magazine
    ///   is out ([`m_WeaponIsWithoutMagazine`](crate::equipment::weapon::WeaponBase::m_WeaponIsWithoutMagazine)),
    ///   and for the predicted reload of the local player's idle weapon;
    /// - [`Inventory::SetAmmoCount`](crate::character::inventory::Inventory::SetAmmoCount),
    ///   [`Inventory::GiveMaxAmmo`](crate::character::inventory::Inventory::GiveMaxAmmo),
    ///   [`Inventory::GiveMaxAmmoToAllWeaponInInventory`](crate::character::inventory::Inventory::GiveMaxAmmoToAllWeaponInInventory),
    ///   `CInventory::SetMaxBeaconsByLevel`, and
    ///   `CInventory::SetMaxFlaresByLevel`, when a reserve is refilled from empty while the
    ///   wielded weapon of that kind has an empty magazine;
    /// - [`WeaponBase::HandleTrackMessage`](crate::equipment::weapon::WeaponBase::HandleTrackMessage),
    ///   for the [`TRACK_MESSAGE_START_RELOAD`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_START_RELOAD)
    ///   and [`TRACK_MESSAGE_START_RELOAD_IF_EMPTY`](crate::equipment::weapon::WeaponBase::TRACK_MESSAGE_START_RELOAD_IF_EMPTY)
    ///   messages;
    /// - the rocket launcher's [`Fire_Start`](crate::equipment::weapon::RPGWeapon::Fire_Start), firing an
    ///   unloaded launcher, and its
    ///   [`CheckIfReloadShouldBeTriggered`](crate::equipment::weapon::RPGWeapon::CheckIfReloadShouldBeTriggered),
    ///   for a holder that is not a player;
    /// - the sniper rifle's per-tick update, after a shot that emptied the magazine with a
    ///   reserve left.
    pub unsafe fn StartReloadingWieldingWeapon(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::StartReloadingWieldingWeapon_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CancelReload_ADDRESS: usize = 0x140760850;
    /// Cancels a running reload: while
    /// [`SB_IS_RELOADING`](crate::state::StateBitFlag::SB_IS_RELOADING) is set, queues the cancel act
    /// (`UB_ACT_CANCEL_RELOAD`) and clears the inventory's reload permission
    /// ([`Inventory::m_DoesAnimationAllowReloading`](crate::character::inventory::Inventory::m_DoesAnimationAllowReloading)).
    /// The ammunition check calls it for a weapon that does not reload by magazine once
    /// [`m_KeepReloading`](crate::equipment::weapon::WeaponBase::m_KeepReloading) is clear, and the
    /// planted explosive's detonation calls it when the detonate act cannot be taken.
    pub unsafe fn CancelReload(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CancelReload_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IsReloading_ADDRESS: usize = 0x14075F3A0;
    /// Whether the character is reloading: the
    /// [`SB_IS_RELOADING`](crate::state::StateBitFlag::SB_IS_RELOADING) state bit, which the reload
    /// state of the animation graph raises.
    pub unsafe fn IsReloading(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsReloading_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const CanUseMelee_ADDRESS: usize = 0x1407FAD40;
    /// Whether the character may start a melee attack: the melee block bit that dropping a heavy
    /// weapon sets is clear, the inventory's animation permission allows melee, the character is not
    /// squeezing valid dual tethers while the gamepad is the active input device, and it is on the
    /// ground or in a stunt position. `NStateTask_InputMeleeTask` tests it before the melee input
    /// and the hammer ability ([`hash_ability_hammer`](crate::character::player::get_hash_ability_hammer)).
    pub unsafe fn CanUseMelee(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::CanUseMelee_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const ForceNeutralState_ADDRESS: usize = 0x1407FD120;
    /// Forces the character's animation and control state back to neutral (on foot, idle). It
    /// drops the heavy weapon, stops using the planted explosive, unequips the wielded weapon
    /// ([`WeaponBase::OnUnequip`](crate::equipment::weapon::WeaponBase::OnUnequip)), and cancels a primed
    /// grenade. It runs on every revive, at the start of every teleport ([`UpdateTeleport`](crate::game_world::UpdateTeleport)
    /// and [`GameWorld::TeleportPlayerInstant`](crate::game_world::GameWorld::TeleportPlayerInstant)), on
    /// the vehicle exit's upward teleport, from [`SetGodMode`](crate::character::character::Character::SetGodMode), from
    /// [`SetupForSequence`](crate::character::character::Character::SetupForSequence), when the character is enabled, and from the
    /// network and the AI's mission-critical teleport.
    pub unsafe fn ForceNeutralState(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ForceNeutralState_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetupForSequence_ADDRESS: usize = 0x1407FD730;
    /// Hands the character to a scripted sequence: forces the neutral state
    /// ([`ForceNeutralState`](crate::character::character::Character::ForceNeutralState)), pauses a behaviour-tree avatar, sets
    /// [`m_InSequence`](crate::character::character::Character::m_InSequence), and makes a player invulnerable for the sequence unless
    /// the god mode already holds. Called from `CSequenceObject2::UpdateSequencePreSim` for each
    /// character the sequence drives.
    pub unsafe fn SetupForSequence(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::SetupForSequence_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ResetAfterSequence3_ADDRESS: usize = 0x1407D6590;
    /// Hands the character back from a scripted sequence: lifts the sequence's invulnerability,
    /// restores each default animation layer's state machine and animation set, unpauses a
    /// behaviour-tree avatar, and clears [`m_InSequence`](crate::character::character::Character::m_InSequence). Called by
    /// `CSequenceObject2::ResetGameObject`, by [`ForceNeutralState`](crate::character::character::Character::ForceNeutralState), and
    /// on entering the cinematic mode.
    pub unsafe fn ResetAfterSequence3(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ResetAfterSequence3_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const HitReaction_ADDRESS: usize = 0x1407D7980;
    /// Reacts to a hit's impulse along `direction`, scaled per damage type and capped. Accumulates
    /// the impulse, and from the character's thresholds picks the reaction: a ragdoll, a knock-back
    /// ([`ACT_HITREACT_FLY`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_FLY), from
    /// an explosion), a stumble
    /// ([`ACT_HITREACT_STUMBLE`](crate::animation::symbol_table::EventIdSymbolTable::ACT_HITREACT_STUMBLE)
    /// and its bullet and melee variants), or a flinch, queued on
    /// [`m_AnimatedModel`](crate::character::character::Character::m_AnimatedModel) with [`AnimatedModel::QueueAct`](crate::character::character::AnimatedModel::QueueAct). A player and
    /// other characters take different branches. `zone` is the hit body-part group's `idstring`,
    /// passed by address and released by the callee, so the caller passes a copy that holds its own
    /// use of the group symbol table
    /// ([`RagdollRBGroupId::init_ptr`](crate::animation::symbol_table::RagdollRBGroupId::init_ptr)). The
    /// impulse is scaled by the type's entry of
    /// [`m_HitReaction_DamageImpulseScale`](crate::character::character::Character::m_HitReaction_DamageImpulseScale) and compared
    /// against [`m_HitReaction_ImpulseRagdollThreshold`](crate::character::character::Character::m_HitReaction_ImpulseRagdollThreshold)
    /// and [`m_HitReaction_ImpulseStumbleThreshold`](crate::character::character::Character::m_HitReaction_ImpulseStumbleThreshold);
    /// a ragdoll takes the impulse, along `direction` and capped by
    /// [`m_HitReaction_DamageImpulseMax`](crate::character::character::Character::m_HitReaction_DamageImpulseMax). The character's
    /// damage virtual runs it at the end of every damage message; it takes no health itself.
    pub unsafe fn HitReaction(
        &mut self,
        hit_position: *const crate::types::math::Vector3,
        direction: *const crate::types::math::Vector3,
        impulse: f32,
        killing_blow: bool,
        zone: *mut u32,
        damage_flags: u32,
        damaged_by_vehicle: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                hit_position: *const crate::types::math::Vector3,
                direction: *const crate::types::math::Vector3,
                impulse: f32,
                killing_blow: bool,
                zone: *mut u32,
                damage_flags: u32,
                damaged_by_vehicle: bool,
            ) = ::std::mem::transmute(Self::HitReaction_ADDRESS);
            f(
                self as *mut Self as _,
                hit_position,
                direction,
                impulse,
                killing_blow,
                zone,
                damage_flags,
                damaged_by_vehicle,
            )
        }
    }
    pub const SetOrientation_ADDRESS: usize = 0x14075FD90;
    /// Sets the character's orientation from a quaternion (`CQuaternion`, `x, y, z, w`): writes
    /// the rotation into [`m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1), keeping its translation, and
    /// stores the quaternion in [`m_RotationGoal`](crate::character::character::Character::m_RotationGoal). A direct world-matrix
    /// write; it does not go through the physics proxy.
    pub unsafe fn SetOrientation(&mut self, orientation: *const [f32; 4]) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                orientation: *const [f32; 4],
            ) = ::std::mem::transmute(Self::SetOrientation_ADDRESS);
            f(self as *mut Self as _, orientation)
        }
    }
    pub const SetYAlignedAttachTo_ADDRESS: usize = 0x14079D540;
    /// Attaches the character to `target` with a yaw-only alignment: computes the yaw between the
    /// character's and the target's forward directions and calls the
    /// [`AttachTo`](crate::character::character::Character::AttachTo) virtual with that rotation as the offset.
    pub unsafe fn SetYAlignedAttachTo(
        &mut self,
        target: *mut crate::types::math::Matrix4,
        object: *mut ::std::ffi::c_void,
        attach_type: u32,
        flags: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target: *mut crate::types::math::Matrix4,
                object: *mut ::std::ffi::c_void,
                attach_type: u32,
                flags: u32,
            ) = ::std::mem::transmute(Self::SetYAlignedAttachTo_ADDRESS);
            f(self as *mut Self as _, target, object, attach_type, flags)
        }
    }
    pub const IsInCargoBay_ADDRESS: usize = 0x14075AEB0;
    /// Whether the character rides in a vehicle's cargo bay
    /// ([`m_InCargoBay`](crate::character::character::AimState::m_InCargoBay)).
    pub unsafe fn IsInCargoBay(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInCargoBay_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const SetWeaponsEnabled_ADDRESS: usize = 0x140818810;
    /// Sets [`WEAPONS_ENABLED`](crate::character::character::DamageStateFlags::WEAPONS_ENABLED).
    pub unsafe fn SetWeaponsEnabled(&mut self, enable: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enable: bool) = ::std::mem::transmute(
                Self::SetWeaponsEnabled_ADDRESS,
            );
            f(self as *mut Self as _, enable)
        }
    }
    pub const SetParachuteEnabled_ADDRESS: usize = 0x140818830;
    /// Sets [`m_IsParachuteEnabled`](crate::character::character::AbilityFlags::m_IsParachuteEnabled).
    pub unsafe fn SetParachuteEnabled(&mut self, enabled: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enabled: bool) = ::std::mem::transmute(
                Self::SetParachuteEnabled_ADDRESS,
            );
            f(self as *mut Self as _, enabled)
        }
    }
    pub const SetWingsuitEnabled_ADDRESS: usize = 0x1408B8440;
    /// Sets [`m_IsWingsuitEnabled`](crate::character::character::AbilityFlags::m_IsWingsuitEnabled).
    pub unsafe fn SetWingsuitEnabled(&mut self, enabled: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enabled: bool) = ::std::mem::transmute(
                Self::SetWingsuitEnabled_ADDRESS,
            );
            f(self as *mut Self as _, enabled)
        }
    }
    pub const SetStuntAutoExitEnabled_ADDRESS: usize = 0x1409AA6E0;
    /// Sets [`m_IsStuntAutoExitEnabled`](crate::character::character::AbilityFlags::m_IsStuntAutoExitEnabled).
    pub unsafe fn SetStuntAutoExitEnabled(&mut self, enabled: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enabled: bool) = ::std::mem::transmute(
                Self::SetStuntAutoExitEnabled_ADDRESS,
            );
            f(self as *mut Self as _, enabled)
        }
    }
    /// The character's current health (`CDamageable::GetHealth`, returning `m_CurHealth`).
    /// [`HUDUI::UpdateHealth`](crate::ui::hud_ui::HUDUI::UpdateHealth) reads it every frame.
    pub unsafe fn GetHealth(&self) -> i16 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetHealth).read();
            f(self as *const Self as _)
        }
    }
    /// Attaches the character to a parent (`CCharacter::AttachTo`): `target` is the parent's
    /// attach frame, `object` the parent object, `attach_type` a `CCharacter::AttachType`, and
    /// `offset` the character's transform relative to the parent frame. While attached, the
    /// character's world transform follows the parent instead of the physics proxy.
    /// [`SetYAlignedAttachTo`](crate::character::character::Character::SetYAlignedAttachTo) is the yaw-aligned wrapper.
    pub unsafe fn AttachTo(
        &mut self,
        target: *const crate::types::math::Matrix4,
        object: *mut ::std::ffi::c_void,
        attach_type: u32,
        offset: *const crate::types::math::Matrix4,
        flags: u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).AttachTo).read();
            f(self as *mut Self as _, target, object, attach_type, offset, flags)
        }
    }
}
impl Character {
    /// The state-task range that
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) runs at its start,
    /// before the IK solve: the range the aim IK, look-at IK, grapple-fire arm IK, and weapon
    /// off-hand IK tasks are registered in.
    pub const FINALIZE_POSE_STATE_TASK_RANGE: u32 = 2608925927;
    /// The offset of the character's `CGameObject` base subobject. RTTI tests (`IsType`) and the
    /// `CGameObject` virtuals go through the vtable at this offset rather than the primary one,
    /// and state tasks recover the character from that subobject by subtracting this offset.
    pub const GAME_OBJECT_BASE_OFFSET: u64 = 8;
    /// The slot, in the vtable of the [`GAME_OBJECT_BASE_OFFSET`](crate::character::character::Character::GAME_OBJECT_BASE_OFFSET)
    /// subobject, of `CGameObject::InitTransform(const CMatrix4f*)`: it re-bases the object's world
    /// transform directly. `CGameWorld::TeleportPlayerInstant` moves the player character through it.
    pub const GAME_OBJECT_INIT_TRANSFORM_SLOT: u64 = 18;
    /// `m_CurrentMotionState` while ragdolling.
    pub const MOTION_STATE_RAGDOLL: u32 = 20;
    /// `m_CurrentMotionState` while swimming, written by both swim movement cores every frame.
    pub const MOTION_STATE_SWIMMING: u32 = 24;
    /// `m_CurrentMotionState` while a movement task drives the proxy velocity itself. The on-foot
    /// locomotion task (`NStateTask_MovementLocomotionTask::Update`) sets it on every update in
    /// which it writes the proxy's wanted velocity, and leaves the value alone when it returns
    /// early because the character is attached to a vehicle.
    pub const MOTION_STATE_TASK_DEFINED: u32 = 25;
    /// The `transform_type` of the advancing weapon placement, which copies each weapon's current
    /// world matrix into its previous-tick matrix before placing it: the first placement in
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel).
    pub const PROP_TRANSFORM_ADVANCE: u32 = 2;
    /// The `transform_type` of the weapons' placement after the `MAIN` HumanIK solve in
    /// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel): it places the
    /// weapons on the solved pose without advancing their previous-tick matrices.
    pub const PROP_TRANSFORM_AFTER_SOLVE: u32 = 1;
}
impl std::convert::AsRef<Character> for Character {
    fn as_ref(&self) -> &Character {
        self
    }
}
impl std::convert::AsMut<Character> for Character {
    fn as_mut(&mut self) -> &mut Character {
        self
    }
}
crate::__bitflags! {
    #[doc =
    " Per-character LOD/update-gating flags. Checked before the SIM-phase animation and IK work:"]
    #[doc =
    " [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) skips the"]
    #[doc =
    " HumanIK solve when [`REDUCED_LOD`](crate::character::character::CharacterLodFlags::REDUCED_LOD) is set. Only that bit is mapped."]
    pub struct CharacterLodFlags : u8 { const REDUCED_LOD = 2usize as _; }
}
fn _CharacterLodFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], CharacterLodFlags>([0u8; 0x1]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct CharacterVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_25: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_26: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    /// The character's current health (`CDamageable::GetHealth`, returning `m_CurHealth`).
    /// [`HUDUI::UpdateHealth`](crate::ui::hud_ui::HUDUI::UpdateHealth) reads it every frame.
    pub GetHealth: unsafe extern "system" fn(
        this: *const crate::character::character::Character,
    ) -> i16,
    _vfunc_28: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_29: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_30: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_31: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_33: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_34: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_35: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_36: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_38: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    _vfunc_39: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
    ),
    /// Attaches the character to a parent (`CCharacter::AttachTo`): `target` is the parent's
    /// attach frame, `object` the parent object, `attach_type` a `CCharacter::AttachType`, and
    /// `offset` the character's transform relative to the parent frame. While attached, the
    /// character's world transform follows the parent instead of the physics proxy.
    /// [`SetYAlignedAttachTo`](crate::character::character::Character::SetYAlignedAttachTo) is the yaw-aligned wrapper.
    pub AttachTo: unsafe extern "system" fn(
        this: *mut crate::character::character::Character,
        target: *const crate::types::math::Matrix4,
        object: *mut ::std::ffi::c_void,
        attach_type: u32,
        offset: *const crate::types::math::Matrix4,
        flags: u32,
    ),
}
fn _CharacterVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x148], CharacterVftable>([0u8; 0x148]);
    }
    unreachable!()
}
impl CharacterVftable {}
impl std::convert::AsRef<CharacterVftable> for CharacterVftable {
    fn as_ref(&self) -> &CharacterVftable {
        self
    }
}
impl std::convert::AsMut<CharacterVftable> for CharacterVftable {
    fn as_mut(&mut self) -> &mut CharacterVftable {
        self
    }
}
/// The global HumanIK enable (`CCharacter::m_EnableHIK`): the engine gates the whole IK pass in
/// [`UpdatePassFinalizePose_Parallel`](crate::character::character::Character::UpdatePassFinalizePose_Parallel) on it.
pub unsafe fn get_Character_EnableHIK() -> &'static mut bool {
    unsafe { &mut *(0x142D621C8 as *mut bool) }
}
pub unsafe fn get_Character_EnableLocoStrafing() -> &'static mut bool {
    unsafe { &mut *(0x142F2F300 as *mut bool) }
}
/// `CCharacter::m_ForceCombat`: while set, `CPlayer::UpdateMentalState` holds the player in the
/// combat mental state.
pub unsafe fn get_Character_ForceCombat() -> &'static mut bool {
    unsafe { &mut *(0x142F2F2FE as *mut bool) }
}
/// `CCharacter::m_ForceNonCombat`: while set, `CPlayer::UpdateMentalState` holds the player in the
/// relaxed mental state. Checked before [`Character_ForceCombat`](get_Character_ForceCombat).
pub unsafe fn get_Character_ForceNonCombat() -> &'static mut bool {
    unsafe { &mut *(0x142F2F2FF as *mut bool) }
}
pub unsafe fn get_Character_GoreEnabled() -> &'static mut bool {
    unsafe { &mut *(0x142F2F301 as *mut bool) }
}
crate::__bitflags! {
    #[doc =
    " A byte of `CCharacter` bitfields holding the visibility, invulnerability, and weapons-enabled"]
    #[doc =
    " flags. Not every bit is mapped: three of the remaining bits are, in ascending order, `Enabled`,"]
    #[doc =
    " `m_GhostMode`, and `m_PermanentGhostMode`, and these names are unconfirmed."] pub
    struct DamageStateFlags : u8 { const VISIBLE = 1usize as _; const GOD_MODE = 2usize
    as _; const JESUS_MODE = 4usize as _; const WEAPONS_ENABLED = 64usize as _; }
}
fn _DamageStateFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], DamageStateFlags>([0u8; 0x1]);
    }
    unreachable!()
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(16))]
pub struct Joint {
    pub m_Translation: crate::types::vector_math::AlignedVector3,
    pub m_Orientation: crate::types::vector_math::AlignedQuat,
    pub m_Scale: crate::types::vector_math::AlignedVector3,
}
fn _Joint_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Joint>([0u8; 0x30]);
    }
    unreachable!()
}
impl Joint {}
impl std::convert::AsRef<Joint> for Joint {
    fn as_ref(&self) -> &Joint {
        self
    }
}
impl std::convert::AsMut<Joint> for Joint {
    fn as_mut(&mut self) -> &mut Joint {
        self
    }
}
#[repr(u32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub enum SafeBoneIndex {
    REFERENCE = 2394094972isize as _,
    OFFSET = 1252689883isize as _,
    HIPS = 1757849759isize as _,
    LEFT_FOOT = 1712403628isize as _,
    RIGHT_FOOT = 4282253387isize as _,
    LEFT_TOE = 3005147562isize as _,
    RIGHT_TOE = 3710742389isize as _,
    LEFT_HAND = 1472741269isize as _,
    RIGHT_HAND = 1776779174isize as _,
    HEAD = 2826426828isize as _,
    NECK = 2714329432isize as _,
    SPINE = 237553739isize as _,
    SPINE1 = 3839615855isize as _,
    SPINE2 = 1877494024isize as _,
    STERNUM = 2647308479isize as _,
    LEFT_SHOULDER = 2268405885isize as _,
    RIGHT_SHOULDER = 808382080isize as _,
    BACK_ATTACH = 227257561isize as _,
    BACK_ATTACH_2 = 3083906404isize as _,
    EQUIPPED_EXPLOSIVE = 2808756785isize as _,
    LEFT_HOLSTER = 1672209727isize as _,
    RIGHT_HOLSTER = 2077750035isize as _,
    ATTACH_HAND_RIGHT = 1707463403isize as _,
    ATTACH_HAND_LEFT = 1100005367isize as _,
    ATTACH_HAND_RIGHT2 = 2079854409isize as _,
    ATTACH_HAND_LEFT2 = 1576246544isize as _,
    RIGHT_HAND_IK_TARGET = 4159613865isize as _,
    LEFT_HAND_IK_TARGET = 2805860545isize as _,
    RIGHT_FOOT_IK_TARGET = 1012388403isize as _,
    LEFT_FOOT_IK_TARGET = 2290357383isize as _,
    AIM_TARGET = 2541339648isize as _,
    TARGET_REF_1 = 3212017811isize as _,
    TARGET_REF_2 = 994268807isize as _,
    RIGHT_LEG = 2828697949isize as _,
    LEFT_LEG = 2016147705isize as _,
    RIGHT_UP_LEG = 2401446677isize as _,
    LEFT_UP_LEG = 641280962isize as _,
    RIGHT_ARM = 433370831isize as _,
    LEFT_ARM = 1307615921isize as _,
    GRAPPLE_ATTACH = 4033878745isize as _,
    GRAPPLE_SHOULDER_ATTACH = 1040341121isize as _,
    GRAPPLE_DEVICE_ATTACH_BONE = 2618472340isize as _,
    CAMERA = 3048058670isize as _,
    NORMAL_MAP0 = 4017403211isize as _,
    NORMAL_MAP1 = 2614705093isize as _,
    NORMAL_MAP2 = 4032476450isize as _,
    NORMAL_MAP3 = 333548isize as _,
    NORMAL_MAP4 = 2094004130isize as _,
    NORMAL_MAP5 = 205856033isize as _,
    BONE_AMOUNT = 4294967295isize as _,
}
fn _SafeBoneIndex_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SafeBoneIndex>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// An index into [`Character::m_WeaponInputs`](crate::character::character::Character::m_WeaponInputs) (`CCharacter::EWeaponInput`). The player's weapon
/// input state task writes the fire, grenade, detonate, plant, reload, precision-aim, and use-item
/// entries from the action map each frame, and the global input update the weapon-select entries.
pub enum WeaponInput {
    E_INPUT_MAIN_WEAPON = 0isize as _,
    E_INPUT_GRENADE = 1isize as _,
    E_INPUT_DETONATE_EXPLOSIVES = 2isize as _,
    E_INPUT_PLANT_EXPLOSIVES = 3isize as _,
    E_INPUT_RELOAD = 4isize as _,
    E_INPUT_PRECISION_AIM = 5isize as _,
    E_INPUT_USE_ITEM = 6isize as _,
    E_INPUT_PICKUP_ITEM = 7isize as _,
    E_INPUT_SELECT_DUEL_WIELD = 8isize as _,
    E_INPUT_SELECT_TWO_HANDED = 9isize as _,
    E_INPUT_SELECT_TWO_HANDED_SPECIAL = 10isize as _,
    E_INPUT_SELECT_PLANT_EXPLOSIVES = 11isize as _,
    E_INPUT_SELECT_PREVIOUS_WEAPON = 12isize as _,
    E_INPUT_SELECT_NEXT_WEAPON = 13isize as _,
    E_INPUT_BAVARIUM_WINGSUIT_MAIN = 14isize as _,
    E_INPUT_BAVARIUM_WINGSUIT_SECONDARY = 15isize as _,
    E_INPUT_COUNT = 16isize as _,
}
fn _WeaponInput_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], WeaponInput>([0u8; 0x4]);
    }
    unreachable!()
}
pub const Character_GetRenderTransform_ADDRESS: usize = 0x14075DE50;
/// `CCharacter::GetRenderTransform`, a `CGameObject` virtual reached through the
/// [`GAME_OBJECT_BASE_OFFSET`](crate::character::character::Character::GAME_OBJECT_BASE_OFFSET) subobject (`game_object` is the
/// character plus that offset): writes the render transform, the lerp of
/// [`m_WorldMatrixT0`](crate::character::character::Character::m_WorldMatrixT0) to [`m_WorldMatrixT1`](crate::character::character::Character::m_WorldMatrixT1)
/// by `dtf`, into `out`, and returns true.
pub unsafe fn Character_GetRenderTransform(
    game_object: *mut u8,
    out: *mut crate::types::math::Matrix4,
    dtf: f32,
) -> bool {
    unsafe {
        let f: unsafe extern "system" fn(
            game_object: *mut u8,
            out: *mut crate::types::math::Matrix4,
            dtf: f32,
        ) -> bool = ::std::mem::transmute(Character_GetRenderTransform_ADDRESS);
        f(game_object, out, dtf)
    }
}
