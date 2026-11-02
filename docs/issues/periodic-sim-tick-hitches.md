# Periodic sim-tick hitches

## Symptom

Every headset session since at least 2026-10-30 hitches on a fixed beat: a stall of about 300 ms twice a minute, and roughly every other minute one of about 600 ms. In the headset each one is a visible freeze of the world.

## What the logs show

The `recordings/*/frames.jsonl` sidecar of `sessions/2026-11-01_03-36-31_488` stamps every captured frame with the wall clock, so the stalls can be placed to the millisecond rather than to a ten-second breakdown window. Taking every gap between captured frames over 150 ms:

- The stalls come in pairs on a cycle of 61.0 s of wall-clock time: one stall, the next 25.9 s later, the next 35.1 s after that, and so on from 02:37:15 to the end of the recording. A few cycles drop the second stall (02:52:56, 02:57:01, 02:58:01).
- The first stall of each cycle alternates between about 600 ms and about 300 ms early in the session (02:37:15 621 ms, 02:38:16 305 ms, 02:39:17 611 ms, 02:40:18 397 ms, 02:41:19 602 ms, ...). The second is 250 to 300 ms.
- The beat is wall-clock time, not game time. From 02:53 to 02:58 the game ran in slow motion (the sidecar's game run time advanced 75 s over five minutes of wall time) and the 61 s cycle held.
- The breakdown lines of `session.log` put every one of them inside `sim_tick` and outside every phase timed in it: `sim_world_pre`, `sim_physics`, `sim_world_post`, `sim_streaming`, and the AI threads all stay at their usual few milliseconds in the windows with a 300 or 600 ms `sim_tick` maximum. No mod slot moves.
- `sessions/2026-11-01_03-18-42_950`, before any recording started, has the same pairs (02:19:03, 02:19:23, 02:20:04, ...), and so does `sessions/2026-10-30_05-43-15_841` (04:44:10, 04:46:16, 04:47:17, 04:48:20 at 594 ms). The recorder is not the cause.
- The very large stalls in `sessions/2026-10-30_05-53-21_827` and `sessions/2026-10-30_05-43-15_841` (8 s to 342 s) are a different thing: each is bracketed by `window: the application activation changed active=false` and `active=true`, and the stalled time is outside `sim_tick`. The game was in the background.

## The mod side

Nothing in the mod runs on a 61 s or 26 s wall-clock beat. Its periodic work is all logging on 0.1 to 10 s intervals, and the breakdown line itself is every 10 s. The mod's hooks inside the sim tick all run inside the timed phases (the character, weapon, and aim hooks run in the world's queues, and the input tick in the pre-simulation queue), and none of their slots shows the stall.

## The game side

[`GameStateRun::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateRun##func-Update) calls several dozen managers outside the phases the breakdown timed. The stretch before the world's pre-simulation queue includes the spin on the input job's completion and the operations and login updates; the stretch after the streaming block includes the AI waits and every online manager: the statistics, friends, online suite, Steam platform system (which runs `SteamAPI_RunCallbacks`), matchmaking, asynchronous brags, notifications, profile, player reporting, and achievements.

## Located: the online-suite manager

`sessions/2026-11-02_02-54-23_753` ran with the split of the tick described under [Instrumentation](#instrumentation). In each of the 32 breakdown windows with a stall, `sim_tail` and `sim_online` carry it and nothing else does: 602 of a 607 ms `sim_tick` maximum at 01:55:14, 262 of 266 at 01:54:43, 248 of 253 at 01:55:45, and the same shape in the rest. `sim_head` stays at a few milliseconds, so of the three managers `sim_online` timed, the operations and the login (which run in the head) are out, and the stall is inside [`OnlineSuiteManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::OnlineSuiteManager##func-Update). The recording's sidecar shows the same beat as before: two cycles of 61.0 s, offset by 26 s, so the gaps alternate 26.0 and 35.1 s, with the first cycle's stall at about 600 ms in some minutes and the other's at about 300 ms.

`sim_steam`, the Steam callbacks, reads 5 to 6 ms in almost every stall window against about 0.1 ms in the others. Something in the stall's tick or soon after it makes a Steam call result arrive.

## What the online-suite manager runs

The manager's own work is a queued login and a check for a platform-initialization retry (see [Narrowed: the Steam logged-on check](#narrowed-the-steam-logged-on-check), which corrects an earlier reading of it). Everything else is the online suite's update, [`OnlineSuite::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::OnlineSuite##func-Update), the publisher's middleware. It runs, on the game thread:

- the player accounts, [`PlayerAccountManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::PlayerAccountManager##func-Update), under the manager's mutex. For the Steam account that is the connection check, then the authentication, and once authenticated the friends refresh, the Square Enix Membership account, and the consumables;
- the language refresh, [`OsInfo::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::OsInfo##func-Update), under a mutex;
- the replies of the suite's HTTP requests, which its worker thread queued, [`Worker::CallCallbacks`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::Worker##func-CallCallbacks).

The suite's timers run on the C runtime's `clock()` in whole seconds, so they keep wall-clock time through slow motion. Two of them run on a period of a minute, which matches two cycles of the same period at a fixed offset:

- [`SteamFriends::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::SteamFriends##func-Update) walks the Steam friends list once a minute: the friend count, then one `ISteamFriends::GetFriendByIndex` call into the Steam client and one allocation for every second index. Under Proton every one of those calls crosses into the Linux Steam client, so a long friends list makes a slow tick. Its result is useless besides: each step replaces the stored list with a one-entry list.
- [`AuthenticationBase::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::AuthenticationBase##func-Update) authenticates the Steam account with the suite's servlet. Without a ticket it requests a Steam encrypted app ticket ([`SteamAuthentication::RequestTicket`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::SteamAuthentication##func-RequestTicket)), once a second for ten attempts and then once a minute; when a ticket arrives (through the Steam callbacks, which fits the `sim_steam` blip) it sends the servlet's `os_Authenticate` request ([`AuthenticationBase::AuthenticateOnServlet`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::AuthenticationBase##func-AuthenticateOnServlet)), and a failed reply drops the ticket and starts over. If the servlet no longer answers, this loops for the whole session.
- [`InternalSemAccount::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::InternalSemAccount##func-Update) retries the membership login every five minutes, too slow for either cycle.

A period of 61.0 s rather than 60 is unexplained; both cycles show it, which fits two timers on the same clock. Which of the two minute timers costs 300 ms, and on what (a Steam client call, a mutex the worker thread holds, a blocking socket call in the HTTP request), is not settled from the code alone.

## Narrowed: the Steam logged-on check

`sessions/2026-11-02_05-51-18_290`, an hour long, has 117 stalls of 200 to 640 ms in its breakdown lines, on a beat of 30.5 s of wall-clock time. Every one is in `sim_online`'s self time: `osuite_accounts`, `osuite_info`, and `osuite_callbacks` stay near zero in the same windows, and the authentication, ticket, servlet, friends, membership, and consumables slots never ran at all in the session, so the Steam service account never got past its connection check. The online suite's update is ruled out, and so are the friends walk and the authentication that the earlier sections suspected.

What is left in `sim_online` is the manager's own code. [`OnlineSuiteManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::OnlineSuiteManager##func-Update) runs the suite's update (only the three timed parts), a one-shot queued login, and then the retry check. The five-minute limit applies only to starting the retry, not to the check: while the [`LoginManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::LoginManager) is [`LoggedIn`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::LoginState##variant-LoggedIn) in the [`Offline`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::LoginMode##variant-Offline) mode and the suite has not been reached, every tick calls [`AccountManager::IsConnectedToFirstPartyService`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::AccountManager##vfunc-IsConnectedToFirstPartyService) before looking at the clock. For Steam that is [`AccountManagerSteam::IsConnectedToFirstPartyService`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::AccountManagerSteam##func-IsConnectedToFirstPartyService), which returns `ISteamUser::BLoggedOn`: a call into the Steam client, which under Proton crosses into the native Linux client. It is the only untimed work in the function that runs on every tick.

The same session supports it. In the windows without a stall, `sim_online`'s self time is about 0.16 ms per tick, with its timed parts at a few microseconds: a round trip into the Steam client on every tick. A call that is usually that cheap and blocks for a few hundred milliseconds when the client is busy fits a stall on a beat of its own that follows wall-clock time. That the session is logged in offline fits too: the offline state is the only one in which the check runs, and the service account never authenticating says the online suite was never reached.

This is the prime suspect but not measured: no slot has yet timed the call on its own.

## Instrumentation

The breakdown splits the tick so a session names the region and the manager (see [frame-timing.md](../mod/performance/frame-timing.md)):

- `sim_head` and `sim_tail`: the stretches before the world's pre-simulation queue and after the streaming block, timed as intervals of `sim_tick`. `sim_tick` keeps only the short gaps between the phases.
- `sim_steam`, `sim_stats`, `sim_online`, `sim_ops`, and `sim_login`: the online managers, with the operations and login (`sim_ops`, `sim_login`) apart from the online-suite manager.
- `sim_logged_on`: the retry check's call to the Steam account, nested in `sim_online`. Its calls count the ticks that reach the check, whether or not the remedy below skips it.
- `osuite_accounts`, `osuite_auth`, `osuite_ticket`, `osuite_servlet`, `osuite_friends`, `osuite_sem`, `osuite_consumables`, `osuite_info`, and `osuite_callbacks`: the parts of the online suite's update, nested in `sim_online`.
- `input_job`: the input job's body, which the head spins on.

In the next session's stall windows the `osuite_*` slot whose maximum matches the stall names the part. `osuite_friends` is the friends walk; `osuite_ticket` or `osuite_servlet` is the authentication's request, and `osuite_auth` alone its mutex; `osuite_accounts` alone is the accounts mutex or the connection check; `osuite_callbacks` is a reply handler; and `sim_online` keeping the stall with every `osuite_*` slot small is the manager's own login or retry. With the remedy switched off, `sim_logged_on` carrying the stall confirms the Steam check.

## Remedy

The payload skips the check (`payload/src/hooks/online.rs`). A detour on the Steam account's check answers false without calling Steam when the call comes from the online-suite manager's retry; it tells that call site apart by the game thread being inside the manager's update and outside the online suite's own update. The retry then never starts, and `BLoggedOn` is never called from there. The switch is in the debug UI under Previews and performance, next to the frame breakdown, and is on by default (the setting is `online.skip_logged_on_check`); off restores the call.

The account manager's other callers of the check keep calling Steam:

- the title screen, which chooses an online or an offline login with it;
- the login-state change out of logged out, which records it in the login attempt;
- the reconnect from the pause menu and the comm link (`CAccountManager::TryReconnectOnline`), which logs an offline session back in online;
- the Steam account's login completion, which explains the offline mode on the title screen;
- the Steam achievements, which request the Steam statistics only when it passes;
- the pause menu's update, which enables the DLC store items with it.

What the player loses is the automatic retry: a session that logged in offline, because the online suite or the network was unreachable at the title screen or because the player chose offline, no longer probes the suite every five minutes and so no longer switches to online by itself mid-session. A retry that succeeds registers the account with the suite, after which the reconnect can log the session in online, which brings back the suite's services (the Square Enix Membership rewards, the online statistics and brags, and the rest); without it the session stays offline until the game is restarted. Steam itself, the achievements, and the store are unaffected. A session that logged in online never runs the check, so nothing changes for it.

The next session should show no stalls in `sim_online`, with `sim_logged_on` present in the breakdown lines (it is listed only in windows where it was called, which is every window while logged in offline) at a maximum of a few microseconds, and `sim_online`'s self time on ordinary ticks dropping from about 0.16 ms to a few microseconds. Stalls that remain in `sim_online` with the switch on mean the Steam check is not the cause, and the next suspect is the queued login or something in the manager's update the hooks do not see. To measure the call itself, switch the remedy off: `sim_logged_on` should then carry the stalls.

The options considered earlier for the online suite's parts (skipping the friends walk, holding back the authentication's retry) are moot: those parts never ran in the hour-long session.
