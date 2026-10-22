#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A thrown or placed explosive object (`CExplosive`, a `CGameObjectAmmunition<CDamageable>`): its
/// physics body, trail effect, detonation timers, and the deferred launch that
/// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) sets up for the physics body to apply later.
pub struct Explosive {
    _field_0: [u8; 1200],
    /// The explosive's model instance, or null. [`UpdateRender`](crate::equipment::explosive::Explosive::UpdateRender) pushes the
    /// interpolated world matrix to it. The first `UpdatePostSimFromManager` after the explosive
    /// spawns creates it from [`m_ModelHash`](crate::equipment::explosive::Explosive::m_ModelHash)
    /// ([`ModelInstanceManager::CreateInstanceFromHash`](crate::graphics_engine::model_instance::ModelInstanceManager::CreateInstanceFromHash),
    /// at [`m_WorldMatrixT1`](crate::equipment::explosive::Explosive::m_WorldMatrixT1), casting no shadow, added to the world),
    /// clears its [`FLAG_CLIPONDISTANCE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_CLIPONDISTANCE),
    /// and shows it; the destructor releases it.
    pub m_ModelInstance: *mut crate::graphics_engine::model_instance::ModelInstance,
    /// The explosive's physics body (the pointer of a shared pointer), or null until the first
    /// `UpdatePostSimFromManager` after the explosive spawns creates it.
    /// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) and [`SetPhysicsTransform`](crate::equipment::explosive::Explosive::SetPhysicsTransform)
    /// do nothing without it.
    pub m_PfxInstance: *mut crate::physics::PfxRigidBody,
    _field_4c0: [u8; 64],
    /// The explosive's world matrix as of the previous simulation tick, which
    /// [`UpdateRender`](crate::equipment::explosive::Explosive::UpdateRender) interpolates from toward
    /// [`m_WorldMatrixT1`](crate::equipment::explosive::Explosive::m_WorldMatrixT1).
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    /// The explosive's world matrix as of the current simulation tick. Its translation row is the
    /// launch start position [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) solves from. The `CGameObject`
    /// transform setter writes it, and the post-simulation update moves a physics body that is not
    /// simulating to it.
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    /// Whether [`m_DeferredPhysicsTransform`](crate::equipment::explosive::Explosive::m_DeferredPhysicsTransform) is pending: set by
    /// [`SetPhysicsTransform`](crate::equipment::explosive::Explosive::SetPhysicsTransform) and applied to the physics body, then
    /// cleared, by the next `UpdatePostSimFromManager`, before a pending launch.
    pub m_DeferredSetPhysicsTransform: bool,
    _field_581: [u8; 3],
    /// The transform awaiting application to the physics body.
    pub m_DeferredPhysicsTransform: crate::types::math::Matrix4,
    _field_5c4: [u8; 24],
    /// The velocity of the thrower when the explosive was launched, stored by
    /// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward).
    pub m_RelativeVelocity: crate::types::math::Vector3,
    _field_5e8: [u8; 1],
    /// Whether a launch is pending: set by [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) and applied to the
    /// physics body with [`m_DeferredInitialVelocity`](crate::equipment::explosive::Explosive::m_DeferredInitialVelocity) on a later
    /// update.
    pub m_DeferredPropelTowards: bool,
    _field_5ea: [u8; 2],
    /// The solved launch velocity awaiting application to the physics body, written by
    /// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) from the virtual `VelocityToHitTarget`.
    pub m_DeferredInitialVelocity: crate::types::math::Vector3,
    _field_5f8: [u8; 4],
    /// The compiled-name hash of the explosive's model, from which
    /// [`m_ModelInstance`](crate::equipment::explosive::Explosive::m_ModelInstance) is created; no instance is created unless the
    /// model is in the model manager's cache. Zero for an explosive without a model.
    pub m_ModelHash: u32,
    _field_600: [u8; 4],
    /// Seconds since the explosive was thrown; [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) resets it.
    pub m_TimeSinceThrow: f32,
    /// Seconds until the explosive detonates. The constructor sets it to the grenade weapon's
    /// detonation time; the post-simulation update counts it down by the tick delta while the
    /// virtual `ShouldTickDetonationTimer` holds (a hand grenade's does from the moment it spawns,
    /// primed or thrown) and calls the virtual `Detonate` once it reaches zero.
    pub m_DetonationTimer: f32,
    /// The detonation time [`m_DetonationTimer`](crate::equipment::explosive::Explosive::m_DetonationTimer) started from.
    pub m_StartDetonationTimer: f32,
}
fn _Explosive_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x610], Explosive>([0u8; 0x610]);
    }
    unreachable!()
}
impl Explosive {
    pub const SetPhysicsTransform_ADDRESS: usize = 0x1408C8180;
    /// Queues `transform` for the physics body: when the explosive has a
    /// [`m_PfxInstance`](crate::equipment::explosive::Explosive::m_PfxInstance), copies it to
    /// [`m_DeferredPhysicsTransform`](crate::equipment::explosive::Explosive::m_DeferredPhysicsTransform) and raises
    /// [`m_DeferredSetPhysicsTransform`](crate::equipment::explosive::Explosive::m_DeferredSetPhysicsTransform). A later call before
    /// the body applies it replaces it.
    pub unsafe fn SetPhysicsTransform(
        &mut self,
        transform: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                transform: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::SetPhysicsTransform_ADDRESS);
            f(self as *mut Self as _, transform)
        }
    }
    pub const PropelToward_ADDRESS: usize = 0x1408E4D50;
    /// Launches the explosive toward `target_position`: when it has a physics body, stores
    /// `relative_velocity` in [`m_RelativeVelocity`](crate::equipment::explosive::Explosive::m_RelativeVelocity), raises
    /// [`m_DeferredPropelTowards`](crate::equipment::explosive::Explosive::m_DeferredPropelTowards), solves the launch velocity from
    /// its current position with the virtual `VelocityToHitTarget` into
    /// [`m_DeferredInitialVelocity`](crate::equipment::explosive::Explosive::m_DeferredInitialVelocity), starts the trail effect,
    /// and resets [`m_TimeSinceThrow`](crate::equipment::explosive::Explosive::m_TimeSinceThrow). The target point is the only aim
    /// input; the solver chooses the flight time.
    pub unsafe fn PropelToward(
        &mut self,
        target_position: *const crate::types::math::Vector3,
        relative_velocity: *const crate::types::math::Vector3,
        target_is_on_roof: bool,
        exact_target_aiming: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target_position: *const crate::types::math::Vector3,
                relative_velocity: *const crate::types::math::Vector3,
                target_is_on_roof: bool,
                exact_target_aiming: bool,
            ) = ::std::mem::transmute(Self::PropelToward_ADDRESS);
            f(
                self as *mut Self as _,
                target_position,
                relative_velocity,
                target_is_on_roof,
                exact_target_aiming,
            )
        }
    }
    pub const VelocityToHitTarget_ADDRESS: usize = 0x14091EC50;
    /// Solves the launch velocity from `start_pos` to `aim_pos`: clamps the target to
    /// `max_range`, chooses a flight time from the grenade weapon's arc tuning (flat, up, and down
    /// times blended by distance and elevation; roof targets use a height-based time), and solves
    /// with [`NWeaponUtil::Trajectory_GetVelocityToHitPosition`](crate::equipment::weapon::NWeaponUtil::Trajectory_GetVelocityToHitPosition)
    /// under world gravity.
    pub unsafe fn VelocityToHitTarget(
        &mut self,
        result: *mut crate::types::math::Vector3,
        aim_pos: *const crate::types::math::Vector3,
        start_pos: *const crate::types::math::Vector3,
        max_range: f32,
        roof_target: bool,
        exact_target_aiming: bool,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::types::math::Vector3,
                aim_pos: *const crate::types::math::Vector3,
                start_pos: *const crate::types::math::Vector3,
                max_range: f32,
                roof_target: bool,
                exact_target_aiming: bool,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::VelocityToHitTarget_ADDRESS,
            );
            f(
                self as *mut Self as _,
                result,
                aim_pos,
                start_pos,
                max_range,
                roof_target,
                exact_target_aiming,
            )
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1408C81B0;
    /// The render-phase `CGameObject::UpdateRender` override: interpolates
    /// [`m_WorldMatrixT0`](crate::equipment::explosive::Explosive::m_WorldMatrixT0) toward [`m_WorldMatrixT1`](crate::equipment::explosive::Explosive::m_WorldMatrixT1)
    /// by the render context's interpolation factor (`CTransform::Lerp`) and pushes the result to
    /// [`m_ModelInstance`](crate::equipment::explosive::Explosive::m_ModelInstance) when there is one. `game_object` is the
    /// explosive's `CGameObject` subobject
    /// ([`GAME_OBJECT_BASE_OFFSET`](crate::equipment::explosive::Explosive::GAME_OBJECT_BASE_OFFSET)), not the start of the
    /// explosive. [`PlantedExplosive::UpdateRender`](crate::equipment::explosive::PlantedExplosive::UpdateRender) calls it before drawing its in-hand model.
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
    pub const PlayTrailEffect_ADDRESS: usize = 0x1408D6E20;
    /// Plays the explosive's trail effect at [`m_WorldMatrixT1`](crate::equipment::explosive::Explosive::m_WorldMatrixT1), when it
    /// has one.
    pub unsafe fn PlayTrailEffect(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PlayTrailEffect_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl Explosive {
    /// The offset of the embedded `CGameObject` subobject. Weak references to the explosive (such
    /// as [`ThrownGrenadeWeapon::m_PrimedGrenade`](crate::equipment::weapon::ThrownGrenadeWeapon::m_PrimedGrenade))
    /// point there rather than at the start of the explosive.
    pub const GAME_OBJECT_BASE_OFFSET: u64 = 112;
}
impl std::convert::AsRef<Explosive> for Explosive {
    fn as_ref(&self) -> &Explosive {
        self
    }
}
impl std::convert::AsMut<Explosive> for Explosive {
    fn as_mut(&mut self) -> &mut Explosive {
        self
    }
}
#[repr(C, align(8))]
/// A hand grenade (`CGrenade`, a [`Explosive`](crate::equipment::explosive::Explosive)): adds bounce counting, homing and smart-fuse
/// targets, extra trail effects, and the landing position.
pub struct Grenade {
    _field_0: [u8; 1568],
    /// Whether the grenade applies the thrower's grenade upgrades (homing and similar).
    pub m_CanUseUpgrades: bool,
    _field_621: [u8; 255],
    /// The target position the grenade was thrown at, recorded by
    /// [`PropelToward`](crate::equipment::explosive::Grenade::PropelToward).
    pub m_InitialLandingPos: crate::types::math::Vector3,
    _field_72c: [u8; 2],
    /// What the grenade's override of the virtual `ShouldTickDetonationTimer` returns, so whether
    /// its post-simulation update counts [`Explosive::m_DetonationTimer`](crate::equipment::explosive::Explosive::m_DetonationTimer) down. The constructor
    /// sets it, so a hand grenade's fuse runs from the moment it spawns, primed in the hand or
    /// thrown; the beacon weapon clears it on the beacon it primes, whose timer then never runs.
    pub m_ShouldTickDetonationTimer: bool,
    _field_72f: [u8; 1],
}
fn _Grenade_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x730], Grenade>([0u8; 0x730]);
    }
    unreachable!()
}
impl Grenade {
    pub const TYPE_ID_ADDRESS: usize = 0x14060FD60;
    /// `CGrenade::TYPE_ID`: the run-time type id of `CGrenade`, for
    /// [`GameObject::IsType`](crate::game::GameObject::IsType). Registers it on first use. The thrown
    /// grenade weapon checks its primed object against it before treating it as a grenade.
    pub unsafe fn TYPE_ID() -> *const u32 {
        unsafe {
            let f: unsafe extern "system" fn() -> *const u32 = ::std::mem::transmute(
                Self::TYPE_ID_ADDRESS,
            );
            f()
        }
    }
    pub const PropelToward_ADDRESS: usize = 0x14091F5C0;
    /// The [`Explosive::PropelToward`](crate::equipment::explosive::Explosive::PropelToward) override: calls it, records `target_position` in
    /// [`m_InitialLandingPos`](crate::equipment::explosive::Grenade::m_InitialLandingPos), plays the extra trail effects, and, with
    /// upgrades enabled and the homing upgrade owned, starts the homing-target search.
    pub unsafe fn PropelToward(
        &mut self,
        target_position: *const crate::types::math::Vector3,
        relative_velocity: *const crate::types::math::Vector3,
        target_is_on_roof: bool,
        exact_target_aiming: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                target_position: *const crate::types::math::Vector3,
                relative_velocity: *const crate::types::math::Vector3,
                target_is_on_roof: bool,
                exact_target_aiming: bool,
            ) = ::std::mem::transmute(Self::PropelToward_ADDRESS);
            f(
                self as *mut Self as _,
                target_position,
                relative_velocity,
                target_is_on_roof,
                exact_target_aiming,
            )
        }
    }
    pub const VelocityToHitTarget_ADDRESS: usize = 0x14091F780;
    /// The [`Explosive::VelocityToHitTarget`](crate::equipment::explosive::Explosive::VelocityToHitTarget) override: wraps it and adds the thrower's relative
    /// velocity unless the grenade has a homing target or `exact_target_aiming` is set.
    pub unsafe fn VelocityToHitTarget(
        &mut self,
        result: *mut crate::types::math::Vector3,
        aim_pos: *const crate::types::math::Vector3,
        start_pos: *const crate::types::math::Vector3,
        max_range: f32,
        roof_target: bool,
        exact_target_aiming: bool,
    ) -> *mut crate::types::math::Vector3 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                result: *mut crate::types::math::Vector3,
                aim_pos: *const crate::types::math::Vector3,
                start_pos: *const crate::types::math::Vector3,
                max_range: f32,
                roof_target: bool,
                exact_target_aiming: bool,
            ) -> *mut crate::types::math::Vector3 = ::std::mem::transmute(
                Self::VelocityToHitTarget_ADDRESS,
            );
            f(
                self as *mut Self as _,
                result,
                aim_pos,
                start_pos,
                max_range,
                roof_target,
                exact_target_aiming,
            )
        }
    }
}
impl std::convert::AsRef<Grenade> for Grenade {
    fn as_ref(&self) -> &Grenade {
        self
    }
}
impl std::convert::AsMut<Grenade> for Grenade {
    fn as_mut(&mut self) -> &mut Grenade {
        self
    }
}
#[repr(C, align(8))]
/// A C4 charge (`CPlantedExplosive`, a [`Explosive`](crate::equipment::explosive::Explosive)), spawned and owned by the
/// [`PlantedExplosiveWeapon`](crate::equipment::weapon::PlantedExplosiveWeapon). It has two models: the
/// in-hand model, shown while the charge is primed and placed at
/// [`m_InactiveModelWorldMatrixT1`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelWorldMatrixT1), and the live model at
/// the explosive's own transform, which [`Activate`](crate::equipment::explosive::PlantedExplosive::Activate) shows in its place.
///
/// The charge keeps the base explosive's collision handler (the grenade overrides it): on a
/// collision it moves the charge to the contact point, oriented by the
/// contact normal, and attaches itself to the object it hit (or plays its stick effect against the
/// world), so a charge that is dropped
/// ([`DropExplosive`](crate::equipment::explosive::PlantedExplosive::DropExplosive)) sticks where it lands.
pub struct PlantedExplosive {
    _field_0: [u8; 1572],
    /// The compiled-name hash of the in-hand model, from which
    /// [`m_InactiveModelInstance`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelInstance) is created.
    pub m_InactiveModelHash: u32,
    /// The in-hand model's world matrix as of the previous simulation tick, which
    /// [`UpdateRender`](crate::equipment::explosive::PlantedExplosive::UpdateRender) interpolates from toward
    /// [`m_InactiveModelWorldMatrixT1`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelWorldMatrixT1).
    pub m_InactiveModelWorldMatrixT0: crate::types::math::Matrix4,
    /// The in-hand model's world matrix, written by
    /// [`SetInactiveModelMatrix`](crate::equipment::explosive::PlantedExplosive::SetInactiveModelMatrix). The pre-simulation update copies
    /// it to the previous frame's matrix, and the render update draws the in-hand model between
    /// the two.
    pub m_InactiveModelWorldMatrixT1: crate::types::math::Matrix4,
    /// The in-hand model's instance, or null. The charge's `UpdatePostSimFromManager` creates it
    /// from [`m_InactiveModelHash`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelHash) in the same update that creates
    /// the live model's [`Explosive::m_ModelInstance`](crate::equipment::explosive::Explosive::m_ModelInstance), hiding the live one and showing this one
    /// with [`FLAG_CLIPONDISTANCE`](crate::graphics_engine::model_instance::ModelInstanceFlags::FLAG_CLIPONDISTANCE)
    /// cleared.
    pub m_InactiveModelInstance: *mut crate::graphics_engine::model_instance::ModelInstance,
    _field_6b0: [u8; 4],
    /// Whether the charge is live, set by [`Activate`](crate::equipment::explosive::PlantedExplosive::Activate).
    pub m_ExplosiveIsPlanted: bool,
    _field_6b5: [u8; 3],
}
fn _PlantedExplosive_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x6B8], PlantedExplosive>([0u8; 0x6B8]);
    }
    unreachable!()
}
impl PlantedExplosive {
    pub const Activate_ADDRESS: usize = 0x140922BE0;
    /// Makes the charge live: sets [`m_ExplosiveIsPlanted`](crate::equipment::explosive::PlantedExplosive::m_ExplosiveIsPlanted), plays the
    /// stick effect, shows the live model, and hides the in-hand model.
    pub unsafe fn Activate(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Activate_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const DropExplosive_ADDRESS: usize = 0x140922CD0;
    /// Drops the charge into the simulation: makes its physics body dynamic in the world, disables
    /// collision between it and the thrower's character body, and, under a parachute, moves the body
    /// into the thrower's collision group. The body must exist
    /// ([`Explosive::m_PfxInstance`](crate::equipment::explosive::Explosive::m_PfxInstance)).
    pub unsafe fn DropExplosive(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DropExplosive_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetVelocity_ADDRESS: usize = 0x1408C8870;
    /// Sets the physics body's linear velocity, world axes, metres per second. The body must exist
    /// ([`Explosive::m_PfxInstance`](crate::equipment::explosive::Explosive::m_PfxInstance)); the call dereferences it unchecked. The velocity is a
    /// by-value `CVector3f`, which the calling convention passes by address.
    pub unsafe fn SetVelocity(&mut self, velocity: *const crate::types::math::Vector3) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                velocity: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetVelocity_ADDRESS);
            f(self as *mut Self as _, velocity)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1408C8920;
    /// The render-phase `CGameObject::UpdateRender` override: runs [`Explosive::UpdateRender`](crate::equipment::explosive::Explosive::UpdateRender)
    /// for the live model, then interpolates
    /// [`m_InactiveModelWorldMatrixT0`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelWorldMatrixT0) toward
    /// [`m_InactiveModelWorldMatrixT1`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelWorldMatrixT1) by the render
    /// context's interpolation factor and pushes the result to
    /// [`m_InactiveModelInstance`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelInstance) when there is one.
    /// `game_object` is the charge's `CGameObject` subobject
    /// ([`Explosive::GAME_OBJECT_BASE_OFFSET`](crate::equipment::explosive::Explosive::GAME_OBJECT_BASE_OFFSET)), not the start of the charge.
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
    pub const SetInactiveModelMatrix_ADDRESS: usize = 0x1408BCC40;
    /// Copies `world_matrix` to [`m_InactiveModelWorldMatrixT1`](crate::equipment::explosive::PlantedExplosive::m_InactiveModelWorldMatrixT1).
    pub unsafe fn SetInactiveModelMatrix(
        &mut self,
        world_matrix: *const crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                world_matrix: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::SetInactiveModelMatrix_ADDRESS);
            f(self as *mut Self as _, world_matrix)
        }
    }
}
impl std::convert::AsRef<PlantedExplosive> for PlantedExplosive {
    fn as_ref(&self) -> &PlantedExplosive {
        self
    }
}
impl std::convert::AsMut<PlantedExplosive> for PlantedExplosive {
    fn as_mut(&mut self) -> &mut PlantedExplosive {
        self
    }
}
