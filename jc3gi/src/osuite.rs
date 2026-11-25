#![cfg_attr(any(), rustfmt::skip)]
//! The online suite (`OSuite` and `OSuitePrivate`), the publisher's middleware for its HTTP-based
//! online services: the player and service accounts, their authentication against the suite's
//! servlet, the Steam friends list, the Square Enix Membership account, and the consumables. Its
//! timers run on the C runtime's `clock()` in whole seconds, so they follow wall-clock time and
//! ignore the game's time scale. Every update here runs on the thread that calls
//! [`OnlineSuite::Update`](crate::osuite::OnlineSuite::Update).
#[repr(C, align(8))]
/// `OSuitePrivate::AuthenticationBase`: a service account's authentication with the suite's
/// servlet, a state machine guarded by a mutex: no ticket, waiting for the platform's ticket, ticket
/// received, waiting for the servlet, and confirmed. Its layout is unmapped.
pub struct AuthenticationBase {}
impl AuthenticationBase {
    pub const Update_ADDRESS: usize = 0x14115E0CC;
    /// `OSuitePrivate::AuthenticationBase::Update` (virtual, inherited unchanged by the Steam
    /// authentication): holding the mutex, steps the state machine. With no ticket it requests one
    /// when forced or when its retry delay has passed; the delay is one second for the first ten
    /// attempts since the last ticket arrived and sixty seconds after that. For Steam the request is
    /// `ISteamUser::RequestEncryptedAppTicket`, whose result arrives through the Steam callbacks and
    /// which Steam answers with a reconnect-within-a-minute error when asked too often. With a ticket
    /// it sends the servlet's `os_Authenticate` operation; a failed reply drops the ticket, which
    /// starts the retries again.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const AuthenticateOnServlet_ADDRESS: usize = 0x14115DFD0;
    /// `OSuitePrivate::AuthenticationBase::AuthenticateOnServlet`: when the suite's default web
    /// service client is ready, sends the servlet's `os_Authenticate` operation with the account's
    /// user id, its reply going to the authentication's callback through [`Worker::CallCallbacks`](crate::osuite::Worker::CallCallbacks),
    /// and moves to waiting for the servlet. [`Update`](crate::osuite::AuthenticationBase::Update) calls it once a ticket has
    /// arrived.
    pub unsafe fn AuthenticateOnServlet(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::AuthenticateOnServlet_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<AuthenticationBase> for AuthenticationBase {
    fn as_ref(&self) -> &AuthenticationBase {
        self
    }
}
impl std::convert::AsMut<AuthenticationBase> for AuthenticationBase {
    fn as_mut(&mut self) -> &mut AuthenticationBase {
        self
    }
}
#[repr(C, align(8))]
/// `OSuitePrivate::InternalSEMAccount`: the Square Enix Membership service account. Its layout is
/// unmapped.
pub struct InternalSemAccount {}
impl InternalSemAccount {
    pub const Update_ADDRESS: usize = 0x14115B9AC;
    /// `OSuitePrivate::InternalSEMAccount::Update` (virtual): validates the pending requests, then
    /// logs in with the platform user id `uid` when the account was never initialized, or, while
    /// not signed in with an unconfirmed email, retries the login at most every three hundred
    /// seconds.
    pub unsafe fn Update(&mut self, uid: *const u8) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, uid: *const u8) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, uid)
        }
    }
}
impl std::convert::AsRef<InternalSemAccount> for InternalSemAccount {
    fn as_ref(&self) -> &InternalSemAccount {
        self
    }
}
impl std::convert::AsMut<InternalSemAccount> for InternalSemAccount {
    fn as_mut(&mut self) -> &mut InternalSemAccount {
        self
    }
}
#[repr(C, align(8))]
/// `OSuite::ZOnlineSuite`: the online suite's singleton. Its layout is unmapped.
pub struct OnlineSuite {}
impl OnlineSuite {
    pub const Update_ADDRESS: usize = 0x14112ED38;
    /// `OSuite::ZOnlineSuite::Update`: updates the player accounts
    /// ([`PlayerAccountManager::Update`](crate::osuite::PlayerAccountManager::Update)), refreshes the suite's language string
    /// ([`OsInfo::Update`](crate::osuite::OsInfo::Update)), and runs the completion callbacks the suite's worker thread queued
    /// ([`Worker::CallCallbacks`](crate::osuite::Worker::CallCallbacks)). Returns the suite's error code, which is always no error.
    /// [`OnlineSuiteManager::Update`](crate::online::OnlineSuiteManager::Update) calls it first, once per
    /// tick.
    pub unsafe fn Update(&mut self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> i32 = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<OnlineSuite> for OnlineSuite {
    fn as_ref(&self) -> &OnlineSuite {
        self
    }
}
impl std::convert::AsMut<OnlineSuite> for OnlineSuite {
    fn as_mut(&mut self) -> &mut OnlineSuite {
        self
    }
}
#[repr(C, align(8))]
/// `OSuitePrivate::OnlineSuiteConsumables`: the consumable-item transactions with the suite,
/// guarded by a mutex. Its layout is unmapped.
pub struct OnlineSuiteConsumables {}
impl OnlineSuiteConsumables {
    pub const Update_ADDRESS: usize = 0x141163720;
    /// `OSuitePrivate::OnlineSuiteConsumables::Update`: holding the mutex, processes the finished
    /// transactions and, with none queued and no request in flight, fetches the incomplete
    /// transactions or transfers the console consumables.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<OnlineSuiteConsumables> for OnlineSuiteConsumables {
    fn as_ref(&self) -> &OnlineSuiteConsumables {
        self
    }
}
impl std::convert::AsMut<OnlineSuiteConsumables> for OnlineSuiteConsumables {
    fn as_mut(&mut self) -> &mut OnlineSuiteConsumables {
        self
    }
}
#[repr(C, align(8))]
/// `OSuitePrivate::OSInfo`: the suite's platform and session information, guarded by a mutex. Its
/// layout is unmapped.
pub struct OsInfo {}
impl OsInfo {
    pub const Update_ADDRESS: usize = 0x14113175C;
    /// `OSuitePrivate::OSInfo::Update`: holding the mutex, copies the game configuration's language
    /// string.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<OsInfo> for OsInfo {
    fn as_ref(&self) -> &OsInfo {
        self
    }
}
impl std::convert::AsMut<OsInfo> for OsInfo {
    fn as_mut(&mut self) -> &mut OsInfo {
        self
    }
}
#[repr(C, align(8))]
/// `OSuite::PlayerAccountManager`: the list of player accounts, guarded by a mutex. Its layout is
/// unmapped.
pub struct PlayerAccountManager {}
impl PlayerAccountManager {
    pub const Update_ADDRESS: usize = 0x1411585E0;
    /// `OSuite::PlayerAccountManager::Update` (virtual): holding the manager's mutex, updates every
    /// player account, which updates each of its service accounts. For the Steam service account
    /// that is `InternalSteamAccount::Update`: it checks the Steam connection, and while signed in
    /// updates the account's authentication ([`AuthenticationBase::Update`](crate::osuite::AuthenticationBase::Update)); once authenticated it
    /// also updates the friends list ([`SteamFriends::Update`](crate::osuite::SteamFriends::Update)), the Square Enix Membership account
    /// ([`InternalSemAccount::Update`](crate::osuite::InternalSemAccount::Update)), and the consumables ([`OnlineSuiteConsumables::Update`](crate::osuite::OnlineSuiteConsumables::Update)).
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<PlayerAccountManager> for PlayerAccountManager {
    fn as_ref(&self) -> &PlayerAccountManager {
        self
    }
}
impl std::convert::AsMut<PlayerAccountManager> for PlayerAccountManager {
    fn as_mut(&mut self) -> &mut PlayerAccountManager {
        self
    }
}
#[repr(C, align(8))]
/// `OSuitePrivate::SteamAuthentication`: the Steam service account's authentication, an
/// [`AuthenticationBase`](crate::osuite::AuthenticationBase) whose ticket is a Steam encrypted app ticket. Its layout is unmapped.
pub struct SteamAuthentication {}
impl SteamAuthentication {
    pub const RequestTicket_ADDRESS: usize = 0x14115DC38;
    /// `OSuitePrivate::SteamAuthentication::RequestTicket` (virtual): asks Steam for an encrypted
    /// app ticket carrying the account's id (`ISteamUser::RequestEncryptedAppTicket`), registers
    /// the call result, and moves to waiting for the ticket; without a Steam user it logs the
    /// failure and stays without a ticket. The result handler, run by the Steam callbacks, fetches
    /// the ticket and base64-encodes it, or logs Steam's refusal and drops back to no ticket.
    /// [`AuthenticationBase::Update`](crate::osuite::AuthenticationBase::Update) calls it on the retry timer.
    pub unsafe fn RequestTicket(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RequestTicket_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<SteamAuthentication> for SteamAuthentication {
    fn as_ref(&self) -> &SteamAuthentication {
        self
    }
}
impl std::convert::AsMut<SteamAuthentication> for SteamAuthentication {
    fn as_mut(&mut self) -> &mut SteamAuthentication {
        self
    }
}
#[repr(C, align(8))]
/// `OSuite::SteamFriends`: the online suite's copy of the Steam friends list. Its layout is
/// unmapped.
pub struct SteamFriends {}
impl SteamFriends {
    pub const Update_ADDRESS: usize = 0x14115D4F8;
    /// `OSuite::SteamFriends::Update`: a three-state refresh cycle. From the starting state it moves
    /// to refreshing; refreshing reads the friend count (`ISteamFriends::GetFriendCount`, immediate
    /// friends and clan members) and walks every second index with
    /// `ISteamFriends::GetFriendByIndex`, one Steam client call and one list allocation per visited
    /// index, then waits; waiting returns to the starting state once sixty seconds have passed since
    /// the last return. So it walks the friends list through the Steam client once a minute, on the
    /// calling thread. Each visited index assigns a fresh one-entry list to the stored list, so the
    /// stored list ends up holding only the last visited friend.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<SteamFriends> for SteamFriends {
    fn as_ref(&self) -> &SteamFriends {
        self
    }
}
impl std::convert::AsMut<SteamFriends> for SteamFriends {
    fn as_mut(&mut self) -> &mut SteamFriends {
        self
    }
}
#[repr(C, align(8))]
/// `OSuitePrivate::ZWorker`: the suite's worker thread, which runs the HTTP requests. Its layout is
/// unmapped.
pub struct Worker {}
impl Worker {
    pub const CallCallbacks_ADDRESS: usize = 0x14116DC20;
    /// `OSuitePrivate::ZWorker::CallCallbacks`: when the worker is running, takes the completion
    /// callbacks it queued (under the callback manager's mutex) and runs them on the calling thread.
    /// The web service replies, such as the authentication's servlet reply, are handled here.
    pub unsafe fn CallCallbacks(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::CallCallbacks_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<Worker> for Worker {
    fn as_ref(&self) -> &Worker {
        self
    }
}
impl std::convert::AsMut<Worker> for Worker {
    fn as_mut(&mut self) -> &mut Worker {
        self
    }
}
