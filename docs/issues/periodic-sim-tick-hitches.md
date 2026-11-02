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

The manager's own work is a queued login and a platform-initialization retry at most every five minutes, neither of which fits. Everything else is the online suite's update, [`OnlineSuite::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::OnlineSuite##func-Update), the publisher's middleware. It runs, on the game thread:

- the player accounts, [`PlayerAccountManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::PlayerAccountManager##func-Update), under the manager's mutex. For the Steam account that is the connection check, then the authentication, and once authenticated the friends refresh, the Square Enix Membership account, and the consumables;
- the language refresh, [`OsInfo::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::OsInfo##func-Update), under a mutex;
- the replies of the suite's HTTP requests, which its worker thread queued, [`Worker::CallCallbacks`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::Worker##func-CallCallbacks).

The suite's timers run on the C runtime's `clock()` in whole seconds, so they keep wall-clock time through slow motion. Two of them run on a period of a minute, which matches two cycles of the same period at a fixed offset:

- [`SteamFriends::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::SteamFriends##func-Update) walks the Steam friends list once a minute: the friend count, then one `ISteamFriends::GetFriendByIndex` call into the Steam client and one allocation for every second index. Under Proton every one of those calls crosses into the Linux Steam client, so a long friends list makes a slow tick. Its result is useless besides: each step replaces the stored list with a one-entry list.
- [`AuthenticationBase::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::AuthenticationBase##func-Update) authenticates the Steam account with the suite's servlet. Without a ticket it requests a Steam encrypted app ticket ([`SteamAuthentication::RequestTicket`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::SteamAuthentication##func-RequestTicket)), once a second for ten attempts and then once a minute; when a ticket arrives (through the Steam callbacks, which fits the `sim_steam` blip) it sends the servlet's `os_Authenticate` request ([`AuthenticationBase::AuthenticateOnServlet`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::AuthenticationBase##func-AuthenticateOnServlet)), and a failed reply drops the ticket and starts over. If the servlet no longer answers, this loops for the whole session.
- [`InternalSemAccount::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::InternalSemAccount##func-Update) retries the membership login every five minutes, too slow for either cycle.

A period of 61.0 s rather than 60 is unexplained; both cycles show it, which fits two timers on the same clock. Which of the two minute timers costs 300 ms, and on what (a Steam client call, a mutex the worker thread holds, a blocking socket call in the HTTP request), is not settled from the code alone.

## Instrumentation

The breakdown splits the tick so a session names the region and the manager (see [frame-timing.md](../mod/performance/frame-timing.md)):

- `sim_head` and `sim_tail`: the stretches before the world's pre-simulation queue and after the streaming block, timed as intervals of `sim_tick`. `sim_tick` keeps only the short gaps between the phases.
- `sim_steam`, `sim_stats`, `sim_online`, `sim_ops`, and `sim_login`: the online managers, with the operations and login (`sim_ops`, `sim_login`) apart from the online-suite manager.
- `osuite_accounts`, `osuite_auth`, `osuite_ticket`, `osuite_servlet`, `osuite_friends`, `osuite_sem`, `osuite_consumables`, `osuite_info`, and `osuite_callbacks`: the parts of the online suite's update, nested in `sim_online`.
- `input_job`: the input job's body, which the head spins on.

In the next session's stall windows the `osuite_*` slot whose maximum matches the stall names the part. `osuite_friends` is the friends walk; `osuite_ticket` or `osuite_servlet` is the authentication's request, and `osuite_auth` alone its mutex; `osuite_accounts` alone is the accounts mutex or the connection check; `osuite_callbacks` is a reply handler; and `sim_online` keeping the stall with every `osuite_*` slot small is the manager's own login or retry.

## Remedies

Nothing is shipped: each option changes what the game does, and the evidence does not yet say which part to change.

- If it is `osuite_friends`: skip [`SteamFriends::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/osuite::SteamFriends##func-Update), or let it run once and then hold it in its waiting state. The risk is small, since the list it maintains holds only one friend anyway; the suite features that read it (friend-based challenges and brags) see no friends.
- If it is the authentication: hold its retry delay at a long value once the servlet has failed, or stop calling it after a number of failures. The risk is that the suite never authenticates within a session after a transient failure, which takes the membership rewards and other suite services offline until a restart. Whether the servlet still answers at all should be checked first, from the suite's log lines or the network.
- Moving the suite's update off the game thread is not safe: its reply handlers and the manager's observers touch game state that the game thread owns.
- Throttling the whole suite update (every few ticks) does not help: the cost is one call on a timer, not a per-tick cost.
