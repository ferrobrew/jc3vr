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
//! Threading: every hooked function runs on the game thread, inside the sim tick.
//!
//! [`OnlineSuite::Update`]: jc3gi::osuite::OnlineSuite::Update

use detours_macro::detour;
use jc3gi::{
    online::{
        AsynchronousBragManager, LoginManager, OnlinePlatformSystemSteam, OnlineSuiteManager,
        OperationManager, StatisticManager,
    },
    osuite::{
        AuthenticationBase, InternalSemAccount, OnlineSuiteConsumables, OsInfo,
        PlayerAccountManager, SteamAuthentication, SteamFriends, Worker,
    },
    profile_manager::ProfileManager,
};
use re_utilities::hook_library::HookLibrary;

use crate::frame_timing::{self, Slot};

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
    OS_INFO_UPDATE.get().unwrap().call(this);
}

#[detour(address = jc3gi::osuite::Worker::CallCallbacks_ADDRESS)]
fn worker_call_callbacks(this: *mut Worker) {
    let _timing = frame_timing::scope(Slot::OsuiteCallbacks);
    WORKER_CALL_CALLBACKS.get().unwrap().call(this);
}
