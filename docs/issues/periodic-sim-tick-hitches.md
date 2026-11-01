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

[`GameStateRun::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameStateRun##func-Update) calls several dozen managers outside the phases the breakdown timed. The stretch before the world's pre-simulation queue includes the spin on the input job's completion and the operations and login updates; the stretch after the streaming block includes the AI waits and every online manager: the statistics, friends, online suite, Steam platform system (which runs `SteamAPI_RunCallbacks`), matchmaking, asynchronous brags, notifications, profile, player reporting, and achievements. Several of these keep their own clocks on the clock's real time or on the unscaled step, which fits a beat that ignores slow motion:

- [`OnlineSuiteManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::OnlineSuiteManager##func-Update) retries the online suite's platform initialization on a real-time timer, though one several minutes long.
- [`AsynchronousBragManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::AsynchronousBragManager##func-Update) runs per-bucket statistic timers with data-driven thresholds, and refreshes the friends' statistics on a timer several minutes long.
- [`ProfileManager::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/profile_manager::ProfileManager##func-Update) fires real-time item and research deadlines.
- The Steam platform system's update, [`OnlinePlatformSystemSteam::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/online::OnlinePlatformSystemSteam##func-Update), runs whatever Steam callbacks are due. Under Proton those cross into the Linux Steam client.

None of them is confirmed. The cause is not settled from the code alone.

## Instrumentation

The breakdown now splits the rest of the tick so the next session names the region and, if it is an online manager, the manager (see [frame-timing.md](../mod/performance/frame-timing.md)):

- `sim_head` and `sim_tail`: the stretches before the world's pre-simulation queue and after the streaming block, timed as intervals of `sim_tick`. `sim_tick` keeps only the short gaps between the phases.
- `sim_steam`, `sim_stats`, and `sim_online`: the online managers.
- `input_job`: the input job's body, which the head spins on.

A stall window's slot maxima then read as one of:

- `sim_head` near the stall with `input_job` near it too: the input job (the device poll, the UI's input update, the message boxes, or the account manager).
- `sim_head` without `input_job`: one of the managers before the world update.
- `sim_tail` with `sim_steam`, `sim_stats`, or `sim_online` near the stall: that online manager.
- `sim_tail` alone: the AI or light waits or another post-simulation manager, which a further split narrows.
