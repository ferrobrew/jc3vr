#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(4))]
/// One heat level's tuning (`CHeatManager::SLevelDef`), loaded from the heat settings. Partial:
/// only the point threshold is mapped; the rest of the record is pursuer and vehicle
/// budgets, timers, and the level's send/receive events.
pub struct HeatLevelDef {
    /// The heat-point total at which the target's heat reaches this level.
    pub m_PointThreshold: i32,
    _field_4: [u8; 120],
}
fn _HeatLevelDef_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x7C], HeatLevelDef>([0u8; 0x7C]);
    }
    unreachable!()
}
impl HeatLevelDef {}
impl std::convert::AsRef<HeatLevelDef> for HeatLevelDef {
    fn as_ref(&self) -> &HeatLevelDef {
        self
    }
}
impl std::convert::AsMut<HeatLevelDef> for HeatLevelDef {
    fn as_mut(&mut self) -> &mut HeatLevelDef {
        self
    }
}
#[repr(C, align(8))]
/// The heat (wanted level) system: tracks the heat target (the player), its heat points and
/// level, the pursuer and ambient enemy lists, reinforcement calls, and roadblocks. Updated once
/// per frame from the game systems update; every setter asserts the main thread.
pub struct HeatManager {
    _field_0: [u8; 32],
    /// Whether the heat system runs at all. While clear, the per-frame update skips heat
    /// accumulation, spawning, and vocals entirely. Toggled by the heat area triggers and by
    /// `SetHeatActive`.
    pub m_SystemActive: bool,
    /// Whether heat changes are locked. While set, the update neither raises nor decays the level:
    /// heat points still accumulate on the target but no level transition, reinforcement call, or
    /// clear timer fires. Read together with the initial-heat timer by `GetHeatLocked`. Set by the
    /// `heat_lock` / `heat_unlock` events and `SetHeatLock`.
    pub m_SystemLocked: bool,
    _field_22: [u8; 30],
    /// Per-level tuning, indexed by level (`0..=MAX_LEVEL`).
    pub m_Levels: [crate::heat_manager::HeatLevelDef; 6],
    _field_328: [u8; 56],
    /// The current heat level, `0..=MAX_LEVEL`. Written by [`SetHeat`](crate::heat_manager::HeatManager::SetHeat) and
    /// [`ClearHeat`](crate::heat_manager::HeatManager::ClearHeat); read by every consumer (`GetHeat`).
    pub m_Level: i32,
    /// The maximum level heat may reach, or [`LEVEL_CAP_NONE`](crate::heat_manager::HeatManager::LEVEL_CAP_NONE) for no cap.
    /// [`SetHeat`](crate::heat_manager::HeatManager::SetHeat) clamps the requested level to it unless told to ignore it.
    pub m_LevelCap: i32,
    _field_368: [u8; 1512],
}
fn _HeatManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x950], HeatManager>([0u8; 0x950]);
    }
    unreachable!()
}
impl HeatManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418092224usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl HeatManager {
    pub const SetHeat_ADDRESS: usize = 0x140A4FC70;
    /// Sets the heat level on `target`: clears any previous target's heat, tops the target's heat
    /// points up to the level's threshold, records the target, and fires the level's heat-change
    /// (and vocal) events. `level` is clamped to `0..=m_LevelCap` unless `ignore_cap`.
    /// `force_initial_heat_lock` restarts the initial-heat lock timer.
    pub unsafe fn SetHeat(
        &mut self,
        level: i32,
        target: *mut crate::character::character::Character,
        ignore_cap: bool,
        force_initial_heat_lock: bool,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                level: i32,
                target: *mut crate::character::character::Character,
                ignore_cap: bool,
                force_initial_heat_lock: bool,
            ) = ::std::mem::transmute(Self::SetHeat_ADDRESS);
            f(self as *mut Self as _, level, target, ignore_cap, force_initial_heat_lock)
        }
    }
    pub const ClearHeat_ADDRESS: usize = 0x140A4FA30;
    /// Drops the heat to zero: zeroes the target's heat points and timers, clears the pursuer,
    /// ambient, and vehicle lists, fires the current level's heat-cleared event (and the vocal
    /// unless `silent`), and restarts the initial-heat lock.
    pub unsafe fn ClearHeat(&mut self, silent: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, silent: bool) = ::std::mem::transmute(
                Self::ClearHeat_ADDRESS,
            );
            f(self as *mut Self as _, silent)
        }
    }
    pub const SetLevelCap_ADDRESS: usize = 0x140A50B90;
    /// Sets [`m_LevelCap`](crate::heat_manager::HeatManager::m_LevelCap), clamped to `0..=MAX_LEVEL`, and if the current level
    /// exceeds it, lowers the level to the cap and trims the target's heat points to just below the
    /// next threshold. It clamps negative caps to zero, so it cannot set
    /// [`LEVEL_CAP_NONE`](crate::heat_manager::HeatManager::LEVEL_CAP_NONE); `CHeatManager::Reset` does.
    pub unsafe fn SetLevelCap(&mut self, cap: i32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, cap: i32) = ::std::mem::transmute(
                Self::SetLevelCap_ADDRESS,
            );
            f(self as *mut Self as _, cap)
        }
    }
    pub const GetHeatTarget_ADDRESS: usize = 0x140A44B80;
    /// The current heat target (the character carrying the heat), or null when none.
    pub unsafe fn GetHeatTarget(&self) -> *mut crate::character::character::Character {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::character::character::Character = ::std::mem::transmute(
                Self::GetHeatTarget_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetHeatPointData_ADDRESS: usize = 0x140A406A0;
    /// The heat-point tuning (`SHeatPointTuning`): the heat points each kind of offence adds, and
    /// the ranges they apply within. A weapon's shot
    /// ([`WeaponBase::Fire_ProjectileBase`](crate::equipment::weapon::WeaponBase::Fire_ProjectileBase))
    /// reads its firing entry and passes it to
    /// [`Character::AddHeatPoints`](crate::character::character::Character::AddHeatPoints).
    pub unsafe fn GetHeatPointData(&self) -> *const ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const ::std::ffi::c_void = ::std::mem::transmute(
                Self::GetHeatPointData_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl HeatManager {
    /// The [`m_LevelCap`](crate::heat_manager::HeatManager::m_LevelCap) value meaning no cap. Set by `CHeatManager::Reset`.
    pub const LEVEL_CAP_NONE: i32 = -1;
    /// The number of heat levels above zero: valid levels are `0..=MAX_LEVEL`.
    pub const MAX_LEVEL: i32 = 5;
}
impl std::convert::AsRef<HeatManager> for HeatManager {
    fn as_ref(&self) -> &HeatManager {
        self
    }
}
impl std::convert::AsMut<HeatManager> for HeatManager {
    fn as_mut(&mut self) -> &mut HeatManager {
        self
    }
}
