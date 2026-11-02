#![cfg_attr(any(), rustfmt::skip)]
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
/// `CLoginManager`: the login state machine for the platform and the online services. Its layout
/// is unmapped.
pub struct LoginManager {}
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
/// HTTP-based online services. Its layout is unmapped.
pub struct OnlineSuiteManager {}
impl OnlineSuiteManager {
    pub const Update_ADDRESS: usize = 0x140B3FBC0;
    /// `COnlineSuiteManager::Update`: updates the online suite
    /// ([`OnlineSuite::Update`](crate::osuite::OnlineSuite::Update)), starts a queued online login, and,
    /// while logged in without the suite, retries the suite's platform initialization as an
    /// operation at most once every five minutes of the clock's real time.
    /// [`GameStateRun::Update`](crate::game::GameStateRun::Update) calls it once per tick among the online
    /// managers.
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
