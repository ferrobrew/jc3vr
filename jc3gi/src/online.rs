#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// `CAccountManager`: the player's account on the platform, which reacts to the login manager's
/// state changes with the login, connection-lost, and offline message boxes, and decides when an
/// offline session logs back in online (`CAccountManager::TryReconnectOnline`). One instance
/// exists, of the platform's subclass ([`AccountManagerSteam`](crate::online::AccountManagerSteam)). Only some virtual functions are
/// bound.
pub struct AccountManager {
    vftable: *const crate::online::AccountManagerVftable,
}
fn _AccountManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], AccountManager>([0u8; 0x8]);
    }
    unreachable!()
}
impl AccountManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418215784usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl AccountManager {
    pub fn vftable(&self) -> *const crate::online::AccountManagerVftable {
        self.vftable as *const crate::online::AccountManagerVftable
    }
    /// `CAccountManager::IsConnectedToFirstPartyService`: whether the platform's own online
    /// service is reachable (for Steam, [`AccountManagerSteam::IsConnectedToFirstPartyService`](crate::online::AccountManagerSteam::IsConnectedToFirstPartyService)).
    /// Its callers are [`OnlineSuiteManager::Update`](crate::online::OnlineSuiteManager::Update), every tick while logged in offline
    /// without the suite; the frontend's title screen, which logs in online when this and
    /// the internet check pass and offline otherwise; the login-state change out of
    /// [`LoggedOut`](crate::online::LoginState::LoggedOut), which records it in the login attempt;
    /// `TryReconnectOnline`, before logging an offline session back in online; the Steam
    /// account's login completion, which explains the offline mode on the title screen when it
    /// fails; the Steam achievements' bucket activation, which requests the Steam statistics
    /// only when it passes; and the pause menu's update, which enables the DLC store items
    /// with it.
    pub unsafe fn IsConnectedToFirstPartyService(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsConnectedToFirstPartyService).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<AccountManager> for AccountManager {
    fn as_ref(&self) -> &AccountManager {
        self
    }
}
impl std::convert::AsMut<AccountManager> for AccountManager {
    fn as_mut(&mut self) -> &mut AccountManager {
        self
    }
}
#[repr(C, align(8))]
/// `CAccountManager_Steam`: the Steam implementation of [`AccountManager`](crate::online::AccountManager). Its layout is
/// unmapped.
pub struct AccountManagerSteam {}
impl AccountManagerSteam {
    pub const IsConnectedToFirstPartyService_ADDRESS: usize = 0x140F28780;
    /// `CAccountManager_Steam::IsConnectedToFirstPartyService`, the Steam override of
    /// [`AccountManager::IsConnectedToFirstPartyService`](crate::online::AccountManager::IsConnectedToFirstPartyService): returns `ISteamUser::BLoggedOn`,
    /// whether the Steam client is logged on to the Steam servers, or false when `SteamUser()`
    /// returns no interface. Each call is two calls into the Steam client library and one into the
    /// Steam client.
    pub unsafe fn IsConnectedToFirstPartyService(&mut self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> bool = ::std::mem::transmute(
                Self::IsConnectedToFirstPartyService_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<AccountManagerSteam> for AccountManagerSteam {
    fn as_ref(&self) -> &AccountManagerSteam {
        self
    }
}
impl std::convert::AsMut<AccountManagerSteam> for AccountManagerSteam {
    fn as_mut(&mut self) -> &mut AccountManagerSteam {
        self
    }
}
#[repr(C, align(8))]
pub struct AccountManagerVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::online::AccountManager),
    /// `CAccountManager::IsConnectedToFirstPartyService`: whether the platform's own online
    /// service is reachable (for Steam, [`AccountManagerSteam::IsConnectedToFirstPartyService`](crate::online::AccountManagerSteam::IsConnectedToFirstPartyService)).
    /// Its callers are [`OnlineSuiteManager::Update`](crate::online::OnlineSuiteManager::Update), every tick while logged in offline
    /// without the suite; the frontend's title screen, which logs in online when this and
    /// the internet check pass and offline otherwise; the login-state change out of
    /// [`LoggedOut`](crate::online::LoginState::LoggedOut), which records it in the login attempt;
    /// `TryReconnectOnline`, before logging an offline session back in online; the Steam
    /// account's login completion, which explains the offline mode on the title screen when it
    /// fails; the Steam achievements' bucket activation, which requests the Steam statistics
    /// only when it passes; and the pause menu's update, which enables the DLC store items
    /// with it.
    pub IsConnectedToFirstPartyService: unsafe extern "system" fn(
        this: *mut crate::online::AccountManager,
    ) -> bool,
}
fn _AccountManagerVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], AccountManagerVftable>([0u8; 0x60]);
    }
    unreachable!()
}
impl AccountManagerVftable {}
impl std::convert::AsRef<AccountManagerVftable> for AccountManagerVftable {
    fn as_ref(&self) -> &AccountManagerVftable {
        self
    }
}
impl std::convert::AsMut<AccountManagerVftable> for AccountManagerVftable {
    fn as_mut(&mut self) -> &mut AccountManagerVftable {
        self
    }
}
#[repr(C, align(8))]
/// `CAsynchronousBragManager`: the asynchronous brags, challenge scores compared against friends'
/// statistics. Its layout is unmapped.
pub struct AsynchronousBragManager {}
impl AsynchronousBragManager {
    pub const Update_ADDRESS: usize = 0x140756A90;
    /// `CAsynchronousBragManager::Update`: advances the statistic timers, activates the brag buckets
    /// whose accumulated time passes their threshold, refreshes the collections of newly seen
    /// players, updates the brag tracker, and refreshes the friends' collections when its own
    /// accumulated time passes ten minutes. [`GameStateRun::Update`](crate::game::GameStateRun::Update)
    /// calls it once per tick among the online managers.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<AsynchronousBragManager> for AsynchronousBragManager {
    fn as_ref(&self) -> &AsynchronousBragManager {
        self
    }
}
impl std::convert::AsMut<AsynchronousBragManager> for AsynchronousBragManager {
    fn as_mut(&mut self) -> &mut AsynchronousBragManager {
        self
    }
}
#[repr(C, align(8))]
/// `CLoginManager`: the login state machine for the platform and the online services. Only some
/// members are bound.
pub struct LoginManager {
    /// The state machine's current step.
    pub m_State: crate::online::LoginState,
    _field_4: [u8; 164],
    /// The mode of the completed login, which `CLoginManager::GetLoginMode` returns while
    /// [`m_State`](crate::online::LoginManager::m_State) is [`LoggedIn`](crate::online::LoginState::LoggedIn).
    pub m_LoginMode: crate::online::LoginMode,
    /// The mode of the login in progress, which `CLoginManager::GetLoginMode` returns in every
    /// other state.
    pub m_LoggingInMode: crate::online::LoginMode,
}
fn _LoginManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xB0], LoginManager>([0u8; 0xB0]);
    }
    unreachable!()
}
impl LoginManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418216368usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl LoginManager {
    pub const Update_ADDRESS: usize = 0x140B7B150;
    /// `CLoginManager::Update`: steps the login state machine (the system, online-suite, and
    /// platform logins and logouts and their waits).
    /// [`GameStateRun::Update`](crate::game::GameStateRun::Update) calls it once per tick after the
    /// operations, before the game world's pre-simulation queue.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<LoginManager> for LoginManager {
    fn as_ref(&self) -> &LoginManager {
        self
    }
}
impl std::convert::AsMut<LoginManager> for LoginManager {
    fn as_mut(&mut self) -> &mut LoginManager {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A login mode (`NOnline::ELoginMode`): whether a login uses the online services.
pub enum LoginMode {
    None = -1isize as _,
    Online = 0isize as _,
    Offline = 1isize as _,
}
fn _LoginMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], LoginMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A step of the login state machine (`NOnline::ELoginState`), [`LoginManager::m_State`](crate::online::LoginManager::m_State). A login
/// runs from [`LoggedOut`](crate::online::LoginState::LoggedOut) through the system, online-suite, and platform steps to
/// [`LoggedIn`](crate::online::LoginState::LoggedIn), and a logout runs the other way.
pub enum LoginState {
    LoggedOut = 0isize as _,
    LoginInitialize = 1isize as _,
    LoginSystem = 2isize as _,
    LoginSystemWait = 3isize as _,
    LoginOnlineSuite = 4isize as _,
    LoginOnlineSuiteWait = 5isize as _,
    LoginReadyWait = 6isize as _,
    Login = 7isize as _,
    LoginWait = 8isize as _,
    LoggedIn = 9isize as _,
    LogoutReadyWait = 10isize as _,
    Logout = 11isize as _,
    LogoutWait = 12isize as _,
    LogoutOnlineSuite = 13isize as _,
    LogoutOnlineSuiteWait = 14isize as _,
    LogoutSystem = 15isize as _,
    LogoutSystemWait = 16isize as _,
}
fn _LoginState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], LoginState>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// `COnlinePlatformSystem_Steam`: the Steam implementation of the online platform system. Its
/// layout is unmapped.
pub struct OnlinePlatformSystemSteam {}
impl OnlinePlatformSystemSteam {
    pub const Update_ADDRESS: usize = 0x1410A3270;
    /// `COnlinePlatformSystem_Steam::Update` (virtual): runs the Steam API's callbacks
    /// (`SteamAPI_RunCallbacks`), then, when the current matchmaking session's host is not the
    /// local player but the Steam lobby names the local player its owner, takes the host role.
    /// [`GameStateRun::Update`](crate::game::GameStateRun::Update) calls it through the platform system's
    /// virtual table once per tick among the online managers, so every Steam callback the game
    /// registered runs here, on the game thread.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<OnlinePlatformSystemSteam> for OnlinePlatformSystemSteam {
    fn as_ref(&self) -> &OnlinePlatformSystemSteam {
        self
    }
}
impl std::convert::AsMut<OnlinePlatformSystemSteam> for OnlinePlatformSystemSteam {
    fn as_mut(&mut self) -> &mut OnlinePlatformSystemSteam {
        self
    }
}
#[repr(C, align(8))]
/// `COnlineSuiteManager`: the owner of the online suite (`OSuite::ZOnlineSuite`), the publisher's
/// HTTP-based online services. Only some members are bound.
pub struct OnlineSuiteManager {
    _field_0: [u8; 184],
    /// Set while a platform initialization of the suite (`COnlineSuitePlatformInitOperation`) that
    /// the manager started is running, which holds back both the queued login and the retry in
    /// [`Update`](crate::online::OnlineSuiteManager::Update).
    pub m_ProbingConnectivity: bool,
    /// Set when an online login is queued; [`Update`](crate::online::OnlineSuiteManager::Update) starts it and clears the flag
    /// once no platform initialization is running.
    pub m_ShouldStartLogin: bool,
    /// Whether the suite is known to be reachable outside an online login. The logout and the
    /// offline login clear it, and `COnlineSuiteManager::HasConnectivity` reports connectivity when
    /// it is set or the suite's web-service client exists.
    pub m_HasConnectivity: bool,
    _field_bb: [u8; 5],
}
fn _OnlineSuiteManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC0], OnlineSuiteManager>([0u8; 0xC0]);
    }
    unreachable!()
}
impl OnlineSuiteManager {
    pub const Update_ADDRESS: usize = 0x140B3FBC0;
    /// `COnlineSuiteManager::Update`: updates the online suite
    /// ([`OnlineSuite::Update`](crate::osuite::OnlineSuite::Update)), starts a queued online login
    /// ([`m_ShouldStartLogin`](crate::online::OnlineSuiteManager::m_ShouldStartLogin)), and then checks whether to retry the
    /// suite. While the login manager is [`LoggedIn`](crate::online::LoginState::LoggedIn) in the
    /// [`Offline`](crate::online::LoginMode::Offline) mode, with neither a platform initialization running
    /// ([`m_ProbingConnectivity`](crate::online::OnlineSuiteManager::m_ProbingConnectivity)) nor the suite reached
    /// ([`m_HasConnectivity`](crate::online::OnlineSuiteManager::m_HasConnectivity)), it asks the account manager whether the
    /// first-party service is reachable
    /// ([`AccountManager::IsConnectedToFirstPartyService`](crate::online::AccountManager::IsConnectedToFirstPartyService)) on every call; when it is, and more
    /// than five minutes of the clock's real time have passed since the last attempt, it starts the
    /// suite's platform initialization as an operation, whose success registers the account with
    /// the suite. [`GameStateRun::Update`](crate::game::GameStateRun::Update) calls it once per tick among
    /// the online managers.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<OnlineSuiteManager> for OnlineSuiteManager {
    fn as_ref(&self) -> &OnlineSuiteManager {
        self
    }
}
impl std::convert::AsMut<OnlineSuiteManager> for OnlineSuiteManager {
    fn as_mut(&mut self) -> &mut OnlineSuiteManager {
        self
    }
}
#[repr(C, align(8))]
/// `COperationManager`: the queue of asynchronous online operations. Its layout is unmapped.
pub struct OperationManager {}
impl OperationManager {
    pub const Update_ADDRESS: usize = 0x140BDA8C0;
    /// `COperationManager::Update`: advances the queued online operations.
    /// [`GameStateRun::Update`](crate::game::GameStateRun::Update) calls it once per tick after the input
    /// job's completion spin, before the game world's pre-simulation queue.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<OperationManager> for OperationManager {
    fn as_ref(&self) -> &OperationManager {
        self
    }
}
impl std::convert::AsMut<OperationManager> for OperationManager {
    fn as_mut(&mut self) -> &mut OperationManager {
        self
    }
}
#[repr(C, align(8))]
/// `CStatisticManager`: the player statistics, kept in buckets that persist locally and refresh
/// from the online collections. Its layout is unmapped.
pub struct StatisticManager {}
impl StatisticManager {
    pub const Update_ADDRESS: usize = 0x140C07A00;
    /// `CStatisticManager::Update`: while the login manager is logged in, persists or refreshes the
    /// buckets waiting for the online state (an offline login persists them through the local data
    /// store's asynchronous persist, an online login drops the other players from them), then
    /// triggers the queued statistic increments. [`GameStateRun::Update`](crate::game::GameStateRun::Update)
    /// calls it once per tick after the AI threads are kicked, first of the online managers. The
    /// disassembler mislabels it `CAccountManager::OnLoginStateChange`.
    pub unsafe fn Update(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
}
impl std::convert::AsRef<StatisticManager> for StatisticManager {
    fn as_ref(&self) -> &StatisticManager {
        self
    }
}
impl std::convert::AsMut<StatisticManager> for StatisticManager {
    fn as_mut(&mut self) -> &mut StatisticManager {
        self
    }
}
