#![cfg_attr(any(), rustfmt::skip)]
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
/// `CSniperWeapon`: a precision-aim rifle. Its layout is unmapped.
pub struct SniperWeapon {}
impl SniperWeapon {
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
    _field_8: [u8; 336],
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
    _field_170: [u8; 12],
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
    _field_1b0: [u8; 184],
    /// The accumulated scatter, raised by each shot up to the tuning's maximum and scaling the
    /// shot's scatter rotation.
    pub m_CurrentScatterValue: f32,
    /// The recoil pitch still to be recovered, raised by each shot.
    pub m_CurrentRecoilPitchValueToRecover: f32,
    /// The physical recoil impulse strength; a non-zero value defers a physical recoil after each
    /// shot.
    pub m_PhysicalRecoilStrength: f32,
    /// The accumulated recoil yaw, kicked left or right at random by each shot.
    pub m_RecoilYaw: f32,
    /// The accumulated recoil pitch, raised by each shot.
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
    _field_428: [u8; 62],
    /// Whether precision aim is disabled for this weapon; the player's aim-input handling zeroes
    /// the aim timer while it is set.
    pub m_IsPrecisionAimDisabled: bool,
    _field_467: [u8; 21],
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
    _field_508: [u8; 88],
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
    pub const GetGripPosition_ADDRESS: usize = 0x140966840;
    /// Writes the grip transform for the given `hand` (a `E_WEAPON_GRIP_HAND_*` value).
    pub unsafe fn GetGripPosition(
        &self,
        hand: i32,
        matrix: *mut crate::types::math::Matrix4,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                hand: i32,
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
    /// [`UpdateFromUserPostSim_Parallel`](crate::equipment::weapon::WeaponBase::UpdateFromUserPostSim_Parallel) virtual.
    pub unsafe fn UpdateFromUserPostSim_ParallelBase(
        &mut self,
        dt: f32,
        aim_position: *const crate::types::math::Vector3,
        input: *const ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                aim_position: *const crate::types::math::Vector3,
                input: *const ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::UpdateFromUserPostSim_ParallelBase_ADDRESS);
            f(self as *mut Self as _, dt, aim_position, input)
        }
    }
    pub const UpdateWeapon_Parallel_ADDRESS: usize = 0x140992B00;
    /// The per-gun fire-input update: cools heat, reads the trigger against the tuning's
    /// sensitivities, applies semi-automatic, forced-fire, wall-penetration, and aim-angle gating,
    /// and starts or stops fire on `weapon_data`.
    pub unsafe fn UpdateWeapon_Parallel(
        &mut self,
        dt: f32,
        weapon_data: *mut crate::equipment::weapon::WeaponData,
        input: *const ::std::ffi::c_void,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                weapon_data: *mut crate::equipment::weapon::WeaponData,
                input: *const ::std::ffi::c_void,
            ) -> bool = ::std::mem::transmute(Self::UpdateWeapon_Parallel_ADDRESS);
            f(self as *mut Self as _, dt, weapon_data, input)
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
    /// skinning; it then renders the weapon's components. `game_object` is the weapon's
    /// `CGameObject` subobject, not the start of the weapon.
    pub unsafe fn UpdateRender(
        game_object: *mut ::std::ffi::c_void,
        context: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                game_object: *mut ::std::ffi::c_void,
                context: *mut ::std::ffi::c_void,
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
        input: *const ::std::ffi::c_void,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).UpdateFromUserPostSim_Parallel).read();
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
}
impl WeaponBase {
    /// The seconds a blocked trigger pull stays buffered in
    /// [`m_InputBufferTimer`](crate::equipment::weapon::WeaponBase::m_InputBufferTimer).
    pub const INPUT_BUFFER_TIME: f32 = 0.5;
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
    _vfunc_26: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_27: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
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
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_33: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_34: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_35: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_36: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_37: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_38: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_39: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_40: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_41: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_42: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_43: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_44: unsafe extern "system" fn(
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
        input: *const ::std::ffi::c_void,
    ),
    _vfunc_55: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    _vfunc_56: unsafe extern "system" fn(
        this: *mut crate::equipment::weapon::WeaponBase,
    ),
    /// Returns the weapon type's tuning block (`SWeaponTuningBase`), from which the fire code
    /// reads recoil, scatter, and spread ranges.
    pub GetTuningBase: unsafe extern "system" fn(
        this: *const crate::equipment::weapon::WeaponBase,
    ) -> *const ::std::ffi::c_void,
}
fn _WeaponBaseVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1D0], WeaponBaseVftable>([0u8; 0x1D0]);
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
    /// The gun's animated model, when it has one; its skinning is updated in the render phase.
    pub m_AnimatedModel: *mut crate::character::character::AnimatedModel,
    /// The gun's base model instance (an `NModelSystem::CModelInstance`). The render update pushes
    /// the interpolated world matrix to it.
    pub m_BaseModelInstance: *mut ::std::ffi::c_void,
    _field_138: [u8; 32],
    /// The gun's world matrix as of the previous simulation tick.
    pub m_WorldMatrixT0: crate::types::math::Matrix4,
    /// The gun's world matrix as of the current simulation tick, refreshed from the attach bone by
    /// the model-instance attachment update. Sim-side readers such as the fire origin read it
    /// through [`GetWorldMatrixT1`](crate::equipment::weapon::WeaponData::GetWorldMatrixT1); the render update interpolates from
    /// [`m_WorldMatrixT0`](crate::equipment::weapon::WeaponData::m_WorldMatrixT0) toward it.
    pub m_WorldMatrixT1: crate::types::math::Matrix4,
    _field_1d8: [u8; 208],
    /// Seconds since this gun last fired; reset by [`WeaponBase::Fire_Projectile`](crate::equipment::weapon::WeaponBase::Fire_Projectile).
    pub m_TimeSinceLastFire: f32,
    _field_2ac: [u8; 48],
    /// The uniform scale applied to the gun's world matrix while holstered.
    pub m_HolsteredScaleValue: f32,
    _field_2e0: [u8; 6],
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
    /// The magazine capacity. Negative means the weapon has no magazine limit.
    pub m_magazine_size: i32,
    /// The weapon's maximum aim range in metres. The player's aim control takes the largest of
    /// these over the wielded, wingsuit, and vehicle weapons as the weapon slot's maximum aim
    /// distance.
    pub m_max_range: f32,
    _field_48: [u8; 36],
    /// A per-weapon replacement for the character's aim-timer refill value; non-negative values
    /// override the character's aim settings (see [`WeaponBase::GetAimTimerOverride`](crate::equipment::weapon::WeaponBase::GetAimTimerOverride)).
    pub m_aim_timer_override: f32,
}
fn _WeaponTuning_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], WeaponTuning>([0u8; 0x70]);
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
