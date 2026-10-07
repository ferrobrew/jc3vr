# Two 80-second stalls and a camera at desk height

## Symptom

Late in the 2026-10-05 session (`sessions/2026-10-05_23-57-57_683`, Bigscreen Beyond 2e on Monado), the game froze twice for about 80 seconds, around 22:30 UTC. Between and after the freezes the view sat 0.85 m below the character's eyes, which read as the camera-height remap having fallen back to the floor.

## What the log shows

Each stall is one game-thread frame. The ten-second `perf: frame breakdown` lines that cover them report `untracked` of 275.55 and 401.98 ms per frame over 299 and 200 frames, which multiply out to 82.4 and 80.4 s, matching each window's `frame_max_ms` (82406 and 80374). Untracked time is the frame interval minus every scope on the game thread, so for about 80 s the game thread was outside `Game::Update` and outside every mod and OpenXR scope: in the engine's main loop or message pump, or below it in the OS, the driver, or Wine.

Nothing in the mod grew. Mod time per frame in both windows (game 1.6, tail 0.4, other 0.4 ms) is at or below its usual level, and every slot's longest call is in its normal range: `vr_lock` peaked at 6 ms, `vr_wait_frame` and `vr_begin_frame` at their usual 13 ms. There are no drain timeouts, no frame-begin failures, no warnings, no OpenXR session state changes after startup, no reference-space change events after 22:17:55, and every `VR frame-loop health` line has `rendered == frames`.

What does line up is the keyboard and mouse. The controller scheme flipped to the gamepad scheme because the OS reported keyboard or mouse input (`keyboard_mouse: true`) 0.2 s before the first stall began, and the two shorter untracked hitches earlier, at 22:26:45 (1.0 s) and 22:26:56 (1.4 s), followed the same kind of flip. Other keyboard input in the session caused no hitch, so the link is suggestive, not proven. The session ended a second after the second stall with the process exiting (a first-chance fault in the water simulation's CUDA module during unload, in `jc3vr-crash.log`).

The log could not say what the window was doing: the window procedure logged no focus or minimize events, and an 80-second frame appeared only averaged into a 10-second window. Both now log (below).

## The camera height

The low camera was not a fallback. The one camera-height readout between the stalls (22:30:47) reads:

```
camera height above Rico's root place="on foot" rico_eyes_m=1.665 camera_eyes_m=0.815 camera_minus_rico_m=-0.850
remap_source="the floor" floor_head_height_m=Some(0.8529031) player_eye_height_m=1.68 posture="standing" crouched=true
```

`the floor` is the normal standing source and every readout in the session uses it; the fallback (`HeadHeightSource::Baseline`, labelled `the recenter baseline`) never appears. The remap did what it is meant to: the runtime reported the headset 0.853 m above the STAGE floor, and `remap_head_height` mapped that to 0.85 m below a standing character's eyes, with the crouch following.

The head height is not latched. `vr::stage::locate_head` recomputes it every frame from the mid-eye pose and the LOCAL-in-STAGE location, so the camera follows the next height the runtime reports. The posture classifier's sit dwell confirms the low readings were continuous and real to the runtime: it had accumulated 17 s below the sit threshold, though at most 0.25 s of that can come from one stalled frame, so the head was already low from about 22:29:08, some 14 s before the first stall and the moment the on-foot readouts stopped.

A headset 0.85 m above the floor for that long, while the keyboard and mouse are in use, is consistent with the headset taken off and set on a desk. That is an inference; the log cannot show it.

## Did the mod cause or worsen it

No evidence that it did. All of the stalled time is outside every mod scope. The one piece of mod code on the game thread outside `Game::Update` without a scope is the window procedure hook, which only checks hotkeys and logged nothing in the window. The camera height followed the runtime's tracking as designed.

## What changed

- `frame_timing::frame_tick` logs any game frame longer than a second on its own line (`perf: a game frame stalled`), so a stall is visible as one frame with the time it ended rather than as a raised ten-second average.
- The window procedure hook logs the application's activation changes and minimization (target `window`), so a stall can be placed against the window losing the foreground.
- The frame loop now reads the view state flags. While the views' position or orientation is not valid, their positions are undefined, so the head pose is not published: the recenter keeps the last tracked head pose and the STAGE head height keeps the last tracked height, instead of taking whatever the runtime returns during a tracking loss. Both follow the head again on the first valid frame, and the loss and the recovery are logged once each. This session never lost tracking, so it would not have changed the camera here; it closes the case where a tracking loss could put the view somewhere the head is not.

## Open questions

- Whether the stalls are the engine pausing its loop while the window is in the background (an alt-tab), or something else blocking the main thread. The window lines above answer it the next time.
- Whether the headset really was off the head. Monado's user-presence reporting, where the runtime offers `XR_EXT_user_presence`, would let the mod say so in the log, and would be the basis for holding the camera height while the headset is not worn.
