//! Timing detours around the online managers and the online suite's parts, for the always-on frame
//! breakdown (`crate::frame_timing`).
//!
//! The online managers run on the game thread inside the sim tick and keep their own wall-clock
//! timers, so a periodic stall in the tick's head or tail can be one of them. Each hook opens a
//! [`frame_timing::scope`] around the original call and changes nothing the game does. The managers
//! are the Steam platform system's callbacks, the statistics, brags, and profile deadlines, the
//! online-suite manager, the operations, and the login. Inside the online-suite manager, the online
//! suite's update ([`OnlineSuite::Update`]) is split into its player accounts, the Steam account's
//! authentication (with its ticket and servlet requests on their own), friends refresh, membership
//! account, and consumables, the language refresh, and the worker's callbacks, so a stall in the
//! suite names the part.
//!
//! One hook changes what the game does. While a session is logged in offline, the online-suite
//! manager asks the account manager on every tick whether Steam is logged on
//! ([`AccountManager::IsConnectedToFirstPartyService`]), to decide whether to retry the online suite.
//! The Steam account answers through `ISteamUser::BLoggedOn`, a call into the Steam client, which
//! under Proton crosses into the native client; it is the only untimed per-tick work in the
//! manager, where the periodic stalls of hundreds of milliseconds sit
//! (`docs/issues/periodic-sim-tick-hitches.md`). With
//! [`OnlineConfig::skip_logged_on_check`] on, that one call site answers false without calling
//! Steam, so the retry never starts; the account manager's other callers (the title screen's login
//! choice, the achievements, the pause menu, the reconnect from the menus) still reach Steam. The
//! call site is told apart by the thread being inside [`OnlineSuiteManager::Update`] and outside the
//! online suite's own update, whose parts are all hooked here.
//!
//! Threading: every hooked function runs on the game thread, inside the sim tick, except the
//! account manager's check, which only acts inside the online-suite manager's update.
//!
//! [`OnlineSuite::Update`]: jc3gi::osuite::OnlineSuite::Update
//! [`AccountManager::IsConnectedToFirstPartyService`]: jc3gi::online::AccountManager
//! [`OnlineSuiteManager::Update`]: jc3gi::online::OnlineSuiteManager::Update

use std::cell::Cell;

use detours_macro::detour;
use jc3gi::{
    online::{
        AccountManagerSteam, AsynchronousBragManager, LoginManager, OnlinePlatformSystemSteam,
        OnlineSuiteManager, OperationManager, StatisticManager,
    },
    osuite::{
        AuthenticationBase, InternalSemAccount, OnlineSuiteConsumables, OsInfo,
        PlayerAccountManager, SteamAuthentication, SteamFriends, Worker,
    },
    profile_manager::ProfileManager,
};
use re_utilities::hook_library::HookLibrary;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    frame_timing::{self, Slot},
};

/// The online hooks' settings.
#[derive(Clone, Serialize, Deserialize)]
pub struct OnlineConfig {
    /// Whether the online-suite manager's per-tick first-party check answers false without calling
    /// Steam. Off restores the call. On, an offline session never retries the online suite, so it
    /// stays offline until the game is restarted.
    pub skip_logged_on_check: bool,
}

impl OnlineConfig {
    pub const fn new() -> Self {
        Self {
            skip_logged_on_check: true,
        }
    }
}

impl Default for OnlineConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&STATISTIC_MANAGER_UPDATE_BINDER)
        .with_static_binder(&BRAG_MANAGER_UPDATE_BINDER)
        .with_static_binder(&PROFILE_MANAGER_UPDATE_BINDER)
        .with_static_binder(&ONLINE_SUITE_UPDATE_BINDER)
        .with_static_binder(&STEAM_PLATFORM_UPDATE_BINDER)
        .with_static_binder(&OPERATION_MANAGER_UPDATE_BINDER)
        .with_static_binder(&LOGIN_MANAGER_UPDATE_BINDER)
        .with_static_binder(&PLAYER_ACCOUNTS_UPDATE_BINDER)
        .with_static_binder(&AUTHENTICATION_UPDATE_BINDER)
        .with_static_binder(&TICKET_REQUEST_BINDER)
        .with_static_binder(&SERVLET_AUTHENTICATION_BINDER)
        .with_static_binder(&STEAM_FRIENDS_UPDATE_BINDER)
        .with_static_binder(&SEM_ACCOUNT_UPDATE_BINDER)
        .with_static_binder(&CONSUMABLES_UPDATE_BINDER)
        .with_static_binder(&OS_INFO_UPDATE_BINDER)
        .with_static_binder(&WORKER_CALL_CALLBACKS_BINDER)
        .with_static_binder(&FIRST_PARTY_CHECK_BINDER)
}

thread_local! {
    /// Whether this thread is inside the online-suite manager's update and outside the online
    /// suite's own update: the stretch that holds the manager's queued login and its suite retry,
    /// whose first-party check is the only call to [`first_party_check`] there. The suite's parts
    /// clear it, because its reply handlers can reach the account manager for other reasons.
    static IN_SUITE_RETRY: Cell<bool> = const { Cell::new(false) };
}

/// Sets [`IN_SUITE_RETRY`] for its lifetime and restores the previous value on drop, so a nested
/// suspension inside the manager's update and a panic unwinding out of it both leave it right.
struct SuiteRetryScope {
    previous: bool,
}

impl SuiteRetryScope {
    fn set(value: bool) -> Self {
        Self {
            previous: IN_SUITE_RETRY.with(|cell| cell.replace(value)),
        }
    }
}

impl Drop for SuiteRetryScope {
    fn drop(&mut self) {
        IN_SUITE_RETRY.with(|cell| cell.set(self.previous));
    }
}

#[detour(address = jc3gi::online::StatisticManager::Update_ADDRESS)]
fn statistic_manager_update(this: *mut StatisticManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimStats);
    STATISTIC_MANAGER_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::online::AsynchronousBragManager::Update_ADDRESS)]
fn brag_manager_update(this: *mut AsynchronousBragManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimStats);
    BRAG_MANAGER_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::profile_manager::ProfileManager::Update_ADDRESS)]
fn profile_manager_update(this: *mut ProfileManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimStats);
    PROFILE_MANAGER_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::online::OnlineSuiteManager::Update_ADDRESS)]
fn online_suite_update(this: *mut OnlineSuiteManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimOnline);
    let _retry = SuiteRetryScope::set(true);
    ONLINE_SUITE_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::online::OnlinePlatformSystemSteam::Update_ADDRESS)]
fn steam_platform_update(this: *mut OnlinePlatformSystemSteam, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimSteam);
    STEAM_PLATFORM_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::online::OperationManager::Update_ADDRESS)]
fn operation_manager_update(this: *mut OperationManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimOperations);
    OPERATION_MANAGER_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::online::LoginManager::Update_ADDRESS)]
fn login_manager_update(this: *mut LoginManager, dt: f32) {
    let _timing = frame_timing::scope(Slot::SimLogin);
    LOGIN_MANAGER_UPDATE.get().unwrap().call(this, dt);
}

#[detour(address = jc3gi::osuite::PlayerAccountManager::Update_ADDRESS)]
fn player_accounts_update(this: *mut PlayerAccountManager) {
    let _timing = frame_timing::scope(Slot::OsuiteAccounts);
    let _retry = SuiteRetryScope::set(false);
    PLAYER_ACCOUNTS_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::AuthenticationBase::Update_ADDRESS)]
fn authentication_update(this: *mut AuthenticationBase) {
    let _timing = frame_timing::scope(Slot::OsuiteAuth);
    AUTHENTICATION_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::SteamAuthentication::RequestTicket_ADDRESS)]
fn ticket_request(this: *mut SteamAuthentication) {
    let _timing = frame_timing::scope(Slot::OsuiteTicket);
    TICKET_REQUEST.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::AuthenticationBase::AuthenticateOnServlet_ADDRESS)]
fn servlet_authentication(this: *mut AuthenticationBase) {
    let _timing = frame_timing::scope(Slot::OsuiteServlet);
    SERVLET_AUTHENTICATION.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::SteamFriends::Update_ADDRESS)]
fn steam_friends_update(this: *mut SteamFriends) {
    let _timing = frame_timing::scope(Slot::OsuiteFriends);
    STEAM_FRIENDS_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::InternalSemAccount::Update_ADDRESS)]
fn sem_account_update(this: *mut InternalSemAccount, uid: *const u8) {
    let _timing = frame_timing::scope(Slot::OsuiteSem);
    SEM_ACCOUNT_UPDATE.get().unwrap().call(this, uid);
}

#[detour(address = jc3gi::osuite::OnlineSuiteConsumables::Update_ADDRESS)]
fn consumables_update(this: *mut OnlineSuiteConsumables) {
    let _timing = frame_timing::scope(Slot::OsuiteConsumables);
    CONSUMABLES_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::OsInfo::Update_ADDRESS)]
fn os_info_update(this: *mut OsInfo) {
    let _timing = frame_timing::scope(Slot::OsuiteInfo);
    let _retry = SuiteRetryScope::set(false);
    OS_INFO_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::Worker::CallCallbacks_ADDRESS)]
fn worker_call_callbacks(this: *mut Worker) {
    let _timing = frame_timing::scope(Slot::OsuiteCallbacks);
    let _retry = SuiteRetryScope::set(false);
    WORKER_CALL_CALLBACKS.get().unwrap().call(this);
}

#[detour(address = jc3gi::online::AccountManagerSteam::IsConnectedToFirstPartyService_ADDRESS)]
fn first_party_check(this: *mut AccountManagerSteam) -> bool {
    if !IN_SUITE_RETRY.with(Cell::get) {
        return FIRST_PARTY_CHECK.get().unwrap().call(this);
    }
    let _timing = frame_timing::scope(Slot::SimLoggedOn);
    if Config::lock_query(|c| c.online.skip_logged_on_check) {
        return false;
    }
    FIRST_PARTY_CHECK.get().unwrap().call(this)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_suite_retry() -> bool {
        IN_SUITE_RETRY.with(Cell::get)
    }

    #[test]
    fn the_suite_parts_suspend_the_retry_and_restore_it() {
        assert!(!in_suite_retry());
        {
            let _manager = SuiteRetryScope::set(true);
            assert!(in_suite_retry());
            {
                let _suite_part = SuiteRetryScope::set(false);
                assert!(!in_suite_retry());
            }
            assert!(in_suite_retry());
        }
        assert!(!in_suite_retry());
    }

    #[test]
    fn an_unwind_out_of_the_manager_clears_the_retry() {
        let unwound = std::panic::catch_unwind(|| {
            let _manager = SuiteRetryScope::set(true);
            panic!("the manager's update unwinds");
        });
        assert!(unwound.is_err());
        assert!(!in_suite_retry());
    }
}
