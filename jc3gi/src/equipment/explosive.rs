#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A thrown or placed explosive object (`CExplosive`, a `CGameObjectAmmunition<CDamageable>`): its
/// physics body, trail effect, detonation timers, and the deferred launch that
/// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) sets up for the physics body to apply later.
pub struct Explosive {
    _field_0: [u8; 1208],
    /// The explosive's physics body (`CPfxRigidBody`, the pointer of a shared pointer), or null
    /// until the first `UpdatePostSimFromManager` after the explosive spawns creates it.
    /// [`PropelToward`](crate::equipment::explosive::Explosive::PropelToward) and [`SetPhysicsTransform`](crate::equipment::explosive::Explosive::SetPhysicsTransform)
    /// do nothing without it.
    pub m_PfxInstance: *mut ::std::ffi::c_void,
    _field_4c0: [u8; 128],
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
    _field_5f8: [u8; 12],
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
    _field_72c: [u8; 4],
}
fn _Grenade_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x730], Grenade>([0u8; 0x730]);
    }
    unreachable!()
}
impl Grenade {
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
