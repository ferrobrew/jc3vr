# Grapple, swing, wingsuit, and parachute: 14 November 02:05 feedback analysis

Written on 15 November from the take and the code as they were before the fixes listed in the [README](../README.md#the-feedback-item-by-item); file and line references are to that code. Scripts, dumps, and frame sheets it names were working files of the analysis and are not kept.

Times are the take's `t`. Velocities are the controller state's `character.velocity_mps` (the game's measured world velocity, `Character::m_MeasuredWorldVelocity`); `L`/`d`/`st`/`T` are the lock record's held length, the hook-to-rider distance, the stretch, and the tension. Per-tick tables are in `analysis/grapple/t*.txt` (scratch), built by `analysis/grapple/scripts/table.py` from `tracking-synth dump --json`. The helicopter and firm-ragdoll research and the parachute and wingsuit items were run as two side investigations; their evidence (frames, dumps, the converted `rico_base.xml` and `ragdoll_params.xml`) is under `analysis/grapple/heli/` and `analysis/grapple/para/`.

## Summary

| Item | Verdict |
|---|---|
| Swing engagement jumps (800.6, 807.6, 833.4, 885.9, 905.2, 918.5, 927.5) | Confirmed in every case. Cause: the take in `lock/model.rs` (`Held::take`, `swing_direction`) turns the whole rush toward the hook into speed across the line within 0.25 s, along whatever small across component there was. Fix: no redirection at all. Rico coasts ballistically while the held length follows him in, and the line goes taut when his radial velocity reverses |
| 906.45 release kills velocity (all releases) | Confirmed. Cause: the retract goes through the reel's cancel (`NStateTask_ReelIn::CancelReelIn` → `ACT_GRAPPLE_CANCEL`), whose next state scales velocity by about 0.1. The mod hands the velocity back one tick too early. A canopy opened out of a reel keeps all its velocity. Fix: leave the reel by a path that keeps velocity, or re-apply the velocity once the fall has taken over |
| 802 ground breaks the grapple | Confirmed: that is the lock's deliberate `Landed` ending (`lock/mod.rs:654`). Remove it for held swings |
| 844.7–866.7 upside down, no jump release | Confirmed: a working pendulum on a 1.5 m line, with Rico turned along the vertical line by the reel's motion. Jump (right A at 863.4 and 865.0) is not an ending of the lock; only the grapple button (866.5) let go |
| 950 and 2539 frozen at the ceiling | Confirmed: the reel's arrival test fires every tick inside about 1 m of the hook, so `FinishReel` runs instead of `UpdateMotion`, and the lock's velocity never reaches the proxy. It is not the spider state. The hang also never pays out (any tiny hand pull cancels the pay-out) |
| 465–468 regrapple mashing | Confirmed: each tap became a lock and an unlock (`tapped`), so nothing was ever fired. The doc's checklist item 6 contradicts the code |
| 92 can't shoot while grappled | Confirmed (right trigger at 94.3 and 94.7 during the snatch line). The game clears every permission while the hook has a live wire, and the mod's waiver excludes live wires |
| 83 ragdoll on the grapple | The ragdoll snatch swings the body on a held line at the hooked distance and never yanks it in. Change it to yank in, then hold at a locked distance (binding) |
| Hint laser while grappled | The beam is drawn whenever an aim exists (`overlay/producers.rs:86`). Hide it while a line is out, but keep the ring, which conflicts with re-fire targeting |
| 1223–1244 wingsuit grapple dead | Confirmed: 33 left-B taps and no grapple action injected. The wingsuit gesture deploy spends the grapple press, and that state clears only when the snatch is available (on foot) |
| 2926 cursor bounce | The aim point is sound from tick to tick; the vehicle moves 1.16 m per sim tick, and the beam's world point is drawn unmoved between sim ticks. Interpolate it like the camera |
| 3518 C4 pickup | The 8 m snatches failed with "the target snagged" (stall: under 0.1 m in 0.5 s); the one at 2.2 m worked. Hypothesis: the dropped charge does not move in the first half second |
| 3442 | Unclear: no snatch was armed, and the right hand took a soldier's ragdoll. Needs the reviewer to say what "it" was |
| Helicopter (743–754, 1794, 2634) | 743–754 was a fast-travel scripted sequence on the skid, not a grapple. At 1800, Y injected `OPEN_WINGSUIT` in the reel. Rail design below |
| 1384 | A scripted sequence-to-freefall with zero horizontal velocity, presented as standing |
| 1459.4 | Not a collision: the canopy opening's velocity alignment yawed toward a near-zero, reversed velocity |
| Parachute handle reach (1153.9, 1400) | The target is the riser link, about 25–30 cm above the grip bar. At 1400 the left controller was untracked |
| Parachute pitch | The brake (both down) works and writes `MOVE_BACKWARD`. There is no dive. Add `MOVE_FORWARD` for both hands up or forward |
| 370.2 wingsuit arms | It is the mod's solver, not the suit: the shoulders follow the head's position, so a glance at one arm bends it |
| Firm ragdoll (322–344, 1166) | The game ships a `GRAPPLE_HANG` ragdoll preset (upper body keyframed, legs motor-driven) and a `PARACHUTING` one. Proposed design below |

## Swing engagement: the tangential jumps

### What happened

Every lock in the reviewer's list swaps the rush toward the hook for a fast swing across the line within 0.05 to 0.27 s:

- 800.55 (onto the parked jet, 68 m line, Rico dragged along the ground by the reel): (11.7, −0.1, −29.1) becomes (1.0, −0.5, −30.9), then (−17.9, −3.0, −17.8) by 800.77. The speed along the reel's heading is turned 45 degrees sideways: "shunted to the left". The lock then ended `landed` at 801.23 (item 802).
- 807.55: (24.8, 0.9, −31.5) becomes (22.3, 12.1, 3.7) in 0.27 s, a 60 degree turn with 11 m/s added upward.
- 833.44: 47 m/s becomes 36 m/s (the take's cap on an 18.8 m line), with vertical speed rising from +2 to +11.5.
- 885.93: vertical +26.2 becomes −13.9 in 0.27 s (15 g), and horizontal speed goes from 17 to 29 m/s.
- 905.16: (9.6, 30.0, 4.0) becomes (25.0, −1.1, 10.5) in 0.19 s: a near-vertical rush up toward an overhead hook turned into 26 m/s level.
- 918.50: rising at +23.3 becomes +2.5 within one tick and −15.2 by 918.71; Rico is driven back into the ground he had just jumped from and lands at 918.77.
- 927.5: the same pattern as 918.5 (not dumped in full; the dump for this window came back empty).

The world review's "entry fixed" measured only the speed kept (61 to 126 percent), not the direction, so it read this as a success.

### Root cause

`payload/src/grapple/physical/lock/model.rs`, `Held::take` (line 127) with `swing_direction` (line 200) and `take_speed` (line 184). On the take's first tick it records the speed relative to the hook. Over `brake_s` (0.25 s), the radial part (toward the hook) falls linearly to zero, and the across part is set to `sqrt(kept² − radial²)` along `swing_direction` (lines 158–171). `swing_direction` takes the direction of the velocity's own across component whenever that exceeds 0.5 m/s. The reel's velocity is almost all along the line, so the across component is a small residue (4 m/s at 905.16; it is computed in the worked example below), yet it receives the whole kept speed. Its direction is arbitrary relative to what the player sees:

- sideways on a level line (800.6);
- downward on a rising diagonal (918.5);
- level on a vertical rush (905.2).

The speed cap in `take_speed` adds a speed loss (833.4).

Worked example at 905.16: the rider sits at (−4.4, −22.9, −1.8) from the hook, so the direction is (−0.19, −0.98, −0.08). The velocity is −31.5 m/s along it and (3.6, −0.9, 1.5) across it. The cap is sqrt((4 − 1 − 0.04) × 9.81 × 23.4) = 26.0 m/s. So the take produces 26 m/s along (3.6, −0.9, 1.5), matching the recorded (25.0, −1.1, 10.5).

The design doc's account (`docs/mod/input/grapple-lock.md`, "The take") describes this behaviour as intended ("a zip up toward a high hook carries on forward and down under it"). The reviewer's model rejects it.

### Fix

Replace the take with a physical engagement, which is the reviewer's model, with no velocity discontinuity:

1. At the lock, stop the reel's pull and keep Rico's velocity exactly as it is. Optionally fade the game's reel acceleration out over about 0.15 s instead of cutting it, which is the "perhaps gently". That needs the reel's own velocity step: run the original `UpdateReelVelocity` on a copy with the tuning's accelerations scaled by the fade, and add the resulting change.
2. While Rico's velocity relative to the hook still points toward the hook, he coasts under gravity alone, and the held length follows him in (`length = min(length, distance)`, the ratchet the take already does at lines 172–175). The line is therefore exactly taut at the moment his radial velocity reverses: at that instant the radial speed is zero, so the rope takes over with no jerk, and from then on it is the pendulum the rope already models. Velocity across the line is never touched, so the swing goes where the player was going, including straight up into an overhead hook.
3. The one case physics cannot handle is a coast that would reach the hook: the apex comes within `min_length_m` with radial speed left. There, brake only the radial component over the remaining distance (a = v²/2(d − min)), or hand over to the reel's arrival (the game's perch). Choose by whether the button is still held.
4. Delete `take_speed`, `swing_direction`, `keeps_speed`, and `brake_s`. The cap is no longer needed, since the line engages at zero radial speed, and the break stretch still protects against later overloads. Keep the hang's "start at rest" branch.

Files: `payload/src/grapple/physical/lock/model.rs` (`Held::take` and its helpers), `lock/tests.rs` (replace the take's tests with properties: there is no velocity change on the take tick beyond gravity; the line becomes taut when radial velocity reaches zero; the energy across the line is conserved), `config.rs` (drop `brake_s`), and `docs/mod/input/grapple-lock.md` ("The take", the settings table, checklist item 2). No game RE is needed for steps 2–4. Step 1's fade needs nothing new either (`UpdateReelVelocity` is bound).

A related observation: with the current take, the line also runs at its cap with a growing stretch for over half a second after engagement (905.4–906.1: `T 3920`, stretch up to 1.22 m; 2691.0–2691.16: stretch to 1.96 m, near the 3 m break). The kept speed exceeds what the line holds once gravity is added, so the cap does not prevent overload. The ballistic engagement removes this.

## 906.45: the release kills velocity

### What happened

All releases look the same; three are traced per tick:

- 906.07: the button comes up and the lock ends `released`. At 906.14 the hook is `INACTIVE`, the traversal is still `reeling_in`, and the velocity is (17.2, 15.1, 7.2). At 906.20 it is (1.9, 1.1, 0.8). Then `on_foot`/no traversal for one tick, then `falling`, and the horizontal velocity decays to exactly zero by 906.48.
- 928.68: (−18.0, 3.0, 14.3) becomes (−1.9, 0.3, 1.5) at 928.74, still under the reel traversal.
- 2691.16: (1.3, −10.6, 2.2) becomes (0, 0, 0) at 2691.23.

The collapse is a uniform scale of about 0.105 on all three axes in a single tick (906: 0.11; 928: 0.105). It comes one world update after the retract, under the reel traversal. So it is not the falling state's air control (which then removes about 6 to 9 m/s² of horizontal speed), and it does not happen "while the lock is held", as world.md read it: the lock record is already gone at that tick.

A control case keeps everything. The canopy requested during a reel at 261.46 ends the reel by the game's own path: the velocity goes from (30.5, 11.8, −15.5) under the reel, to (29.4, 14.1, −14.9) on the one `on_foot` tick, to (28.4, 16.4, −14.3) under the parachute.

### Root cause

The lock lets go with `engine::let_go` → `GrapplingHook::RetractActiveWire` (`lock/engine.rs:142`). The reel task answers a detached far end on its next update with `NStateTask_ReelIn::CancelReelIn`. In the symbol dump, that function queues `NCharacter::ACT_GRAPPLE_CANCEL` and calls `CCharacter::ForceUpright`; `ForceUpright` touches only orientation. The 0.1 scale therefore comes from the state that `ACT_GRAPPLE_CANCEL` leads to in Rico's graph (hypothesis: an inherited-velocity factor on that state, or an on-ground/air locomotion state taking over the proxy for a tick after `NStateTask_ReelIn::OnExit` clears the proxy's game-controlled flag). The engine doc's claim that "the character keeps the velocity it had" (`docs/engine/gameplay/grapple-pipeline.md:179`) is wrong for this path and should be corrected.

The mod's two handbacks in `lock/mod.rs` both land before the scaling:

- `end`, at lines 693–713: `set_wanted_velocity` on the release tick, before the cancel exists.
- `lock_tick`, at lines 262–267: `release_velocity` on the next input tick, which runs before that tick's world update, in which the cancel's state transition applies.

### Fix

Preferred: do not leave the reel through the cancel. The canopy request proves the graph has an exit from the reel state that keeps velocity, logged as a 20 to 50 ms `jump` case in body.md. RE in `rico_base.afsmb` (decoded with `tools/anim_graph/decode_graph.py`; the converted xml is in `analysis/grapple/heli/`):

- the transitions out of the reel-in state, and which act leads to an airborne fall without the cancel state;
- what `ACT_GRAPPLE_CANCEL`'s target state does to velocity: find the 0.1 factor, a task property or a code constant.

Then end the lock by queueing that act (`Character::QueueAct` is in pyxis as part of the act machinery), and retract the wire after the state has changed.

Fallback with no RE: keep the retract, but hand the velocity back on the first tick the controller state reports `falling` (or on the first `UpdateIntegratingPhysics` after `OnExit`), for up to a few ticks until the measured velocity matches. Accept the fall's own air drag. Its rate (horizontal speed falls by about 6.5 m/s² in a fall with no stick) means a 30 m/s exit coasts for about 4 s. If that feels like an invisible brake, carrying the swing velocity is better done by the first option, or by adding the stick-free drag back for a short "momentum" window.

Files: `payload/src/grapple/physical/lock/mod.rs` (`end`, `lock_tick`'s handback, gated on the fall's state), `lock/engine.rs` (a new `leave_reel` beside `let_go`), and the pyxis defs for the act or state once found. Doc: `grapple-lock.md` "How it ends". This also fixes the reel endings the world review lists under "Not fixed".

## 802: landing breaks the grapple

At 800.55–801.23 Rico was reeled along the ground toward the parked jet. The lock took hold, and at 801.19 `supported` turned true. The lock ended `landed`, with the button still held until 802.0. The same happened at 804.30, 0.13 s after the lock took hold. This is designed behaviour: `lock/mod.rs:654–660` (`supported && active.airborne` → `LockEnd::Landed`) and the doc's "How it ends" table.

Fix (the reviewer's direction overrides the doc):

- A held swing does not end on support. While supported, keep the rope acting through the lock's velocity: the ground holds Rico up, and a taut line drags him (toward a moving plane, for example) or swings him off an edge.
- Two details make this work.
  - First, while supported and slack, the reel task's flying motion would make Rico skate. Let the lock hand off to an on-foot held line: the snatch's sling (`controllers/snatch/sling.rs`) is the same rope with the hand as the kinematic end, and it already runs with Rico on foot on a custom wire. The kinematic and dynamic ends need swapping (Rico becomes the dynamic end, driven through the on-foot character's velocity), which is the inverse of what the sling does now.
  - Second, take `held_floor` into account so walking toward the hook slackens the line instead of being blocked.
- Keep `Landed` for hangs, where landing on a floor below the ceiling should end it.

Files: `lock/mod.rs` (`ending`), and the doc. RE: none for the first step. A real on-foot tether needs a check on whether the character's on-foot wanted velocity can be offset as the reel's can (`CharacterInput::gameControledVelocity` is bound).

## 844.69–866.7: the upside-down hang, and jump does not release

### What happened

This hang worked as a hang: a pendulum of ±0.7 m/s with a period of about 2.6 s, tension 970–1010 N (Rico's weight), and the line at 1.5–1.9 m and 0.1 m of stretch. But:

- The line never paid out to `hang_length_m` (3.5 m). It went 1.8 → 1.9 → 1.5 m: it was taken in, not paid out.
- The frames (world.md: 846, 850, 858) have Rico's arm and body filling half the view, and the reviewer saw the body horizontal or inverted rather than hanging from the arm.
- Right A (jump) at 863.42 and 865.0 and left A at 864.47 did nothing. Left B (the grapple button) at 866.54 ended it `released`. So "can't release without regrappling" is the grapple button being the only exit.
- The left pistol dropped at 845.90 ("let go of", 0.5 m/s), and the long gun fell at 860.44 after a dozen "the animation forbids it" holster refusals. The reel's state forbids holstering (guns area; noted only).

### Root cause

- The pose: the hang is the reel task with the lock's velocity. `NStateTask_ReelIn::UpdateMotion` turns the character toward the reel direction every tick. For a hook straight overhead, that pitches Rico's facing up the vertical line, and the reel's flying clip (arm thrust along the line) is drawn rotated with him. Hypothesis, but consistent with the engine doc's description of `UpdateMotion` ("turns the character toward the reel's direction") and with 2634's pose.
- Jump: the lock's endings (`lock/mod.rs:629–635`) are the button, the yank, the switch, the far end, a fault, and support. No jump. In the reel state the game ignores jump.
- No pay-out: `Held::take_in` (`lock/model.rs:92–103`) clears `pay_out_to_m` on any positive take-in. `TakeIn::step` has no deadzone, so the first millimetre of the hand drifting back along the aim cancels the pay-out for good.

### Fix

- Pose: while the lock holds, intercept `NStateTask_ReelIn::UpdateMotion` (bound) for the local player and pass a level `target_dir` (the line's horizontal part, or the body yaw when the line is near vertical), so Rico stays upright. The body is then the firm ragdoll's job (below), with the grapple arm keyframed to the hand on the line.
- Jump: add `LockEnd::Released` on the jump action's press while locked (the bindings' jump, in `lock_tick`'s input). Optionally add a small upward impulse, as the game's own rope jumps do. Needs the jump press passed into `LockTickInput`.
- Pay-out: give `TakeIn` a deadzone (for example 2 cm of accumulated pull), and let pay-out resume when take-in stops, instead of cancelling for good.

Files: `lock/hooks.rs` (a new `UpdateMotion` detour), `lock/mod.rs`, `lock/model.rs`, `controllers/bindings/` (pass the jump), and the doc.

## 950 and 2539: frozen at the ceiling

### What happened

At 945.06 and 2538.24 the hang took hold 0.5–0.6 m from the hook, with a held length of 1.5–1.6 m. From the next tick, Rico's velocity is exactly (0, 0, 0) for 5.4 s and 2.1 s, the stretch is −0.85 to −0.94 m (slack), and the tension is zero. Gravity, which the lock adds every tick, never shows. The release (left B, 950.4 and 2540.4) dropped him from rest. The length shrank 1.6 → 1.5 m, so the pay-out was cancelled here too. The 844 hang, whose distance grew to 1.9 m within 0.35 s, swung normally.

### Root cause

This is not the spider state. The inverted attach is held off: `StartReelUpsideDown`'s detour (`lock/hooks.rs:37`) asked for the hang, and the lock took hold. In `NStateTask_ReelIn::Update` (symbol dump), for a static or generic far end, the arrival test is `(facing along the line && distance < m_ReelInArrivedDistance) || distance − travel < s_small_buffer`. On arrival the update calls `FinishReel` instead of `UpdateMotion`; `UpdateMotion` is what flags the proxy's velocity as game-controlled and sets the wanted velocity. The lock's `FinishReel` detour (`lock/hooks.rs:68–74`) only calls `SetWantedWorldVelocity` with the lock's last velocity. Without `UpdateMotion`, that velocity never moves the proxy, so Rico stays where the arrival found him. A frozen Rico never leaves the arrival distance, so the condition holds forever.

The 844 hang escaped because Rico started 1.2 m out, not facing the line, and fell out of the zone before freezing. The upside-down state never runs, which is why the inverted cling "never played".

### Fix

- While the lock holds, the arrival must still move the character. Either:
  - detour `UpdateMotion` (bound) to remember the last `(instance, properties, layer)` it was called with for the local reel, and call `UpdateMotion` from the `FinishReel` detour with the lock's velocity and a level direction (shared with the pose fix above); or
  - hold off the arrival test itself, which would need `NStateTask_ReelIn::Update` bound.

  The first needs no new RE beyond confirming that the instance pointer matches the `instance_properties` passed to `UpdateReelVelocity` that tick (it is the same `inst_p` in the dump).
- Start the hang's held length at `max(distance, hang_length_m)` minus the pay-out, so the line is never slack at the take. Fix the pay-out deadzone as above.

Files: `lock/hooks.rs`, `lock/mod.rs`, `lock/model.rs`. Doc: `grapple-lock.md` "The hang".

## 465–468: mashing the grapple at the car

From 464.49 Rico was reeling to a static point 78 m off. Three presses at 465.16, 466.17, and 466.51 each logged `the line locks`, then `the lock ends why="tapped"` 0.07–0.13 s later. No new line attached, and the reel ended at 467.29. The reviewer's taps meant "re-fire at the car under the cursor". The game would do that from the reel with the re-reel upgrade (`GrapplingHook::FireHookFromReelingIn`).

Cause: `controllers/bindings/grapple_button.rs:290–298` turns a release within `TAP_S` into `LockButton::Tapped`, and `lock/mod.rs:631` resumes the reel. Nothing is fired by design ("a fire onto the attached line can release it"). The design doc's own checklist item 6 says the reel "fires again from the reel at the aim", which the code never does.

Fix: on `Tapped`, if the grapple aim is valid and its point is more than a couple of metres from the current hook point (or names a different game object), pass a one-tick fire click to the game. The game's fire from the reel then retargets. Keep firing nothing when the aim is on the attached line. Optionally make the press itself not lock when the aim is on a new target, so the lock does not interrupt the reel for 0.1 s per tap. Files: `grapple_button.rs` (the `Locking` release branch needs the aim's target and the hook point in `GrappleContext`), `lock/mod.rs`, and the doc. This interacts with the hint-laser item: hiding the aim while grappled would hide the cursor used for this.

## 92: no shooting while grappled

The right pistol was in hand with its sight (`gun_sight`), and the right trigger was pulled at 94.33 and 94.70. That was during the 93.01–95.24 snatch line on a ragdoll (`GHS_CUSTOM_ACTIVE_WIRE`), and it did not fire.

Cause: `Inventory::UpdateInventoryPermissions` clears every permission while the hook has a live wire (`docs/engine/gameplay/weapon-firing.md:69`). The mod's waiver deliberately excludes live wires and reels (`controllers/weapons/permission.rs:43–47`, `waived`).

Fix: waive the permission for a gun held in the hand that does not hold the line, while the live wire is the mod's own: the snatch's sling, or the lock's held swing. The mod poses and fires the held gun itself, as for the airborne waiver. Consider the plain reel too: the game gates shooting while reeling on an upgrade, so either respect it or treat the VR hand as always allowed. Files: `permission.rs` (`AirState` gains `mod_line: bool` from `snatch` and `lock::holds()`), its tests, and `docs/mod/input/physical-guns.md` if it documents the waiver.

## 83: grappling a ragdoll

From 73.64 to 85.42 (and at 62.15 and 93.04) the snatch fired at a ragdoll (`target_kind="BODY"`) and went straight to `swinging on the held line`. It used the distance at the bite (7.6, 5.4, and 1.1 m), reeling toward `swing_length_m` (3 m) no faster than the body comes, and never pulled the body to the hand. With no strong hand motion, a 100 kg body on a line held at its distance does very little.

Fix (binding: yank toward you, and the locked-distance swing):

- Run the object pull phase for bodies too: the line reels at `SNATCH.line_speed_mps`, using the per-body spread pull the swing already has (`remote::pull_ragdoll`).
- End it in one of two ways: at the hand, as a ragdoll hold through the hands' character grip (`physical_hands::pickup::grips`, "took hold of a fallen character's ragdoll"); or, if the grip is still held and the player stops pulling, at a locked length, with the swing as now.
- The yank that fires is the pull's trigger. Swinging stays on the held line.

Files: `controllers/snatch/target.rs` (`ragdoll_offer`), `snatch/pull.rs`, `snatch/sling.rs`, and `docs/mod/input/grapple-snatch.md` (the table row "always swung").

## The grapple hint laser while grappled

`controllers/overlay/producers.rs:86–95` submits the gauntlet beam whenever `status.grapple` exists. A line already out does not matter.

Fix: skip the beam while the hook is not `GHS_INACTIVE`, or while the lock or the sling holds (`grapple::hook::local()` state). Conflict with 465: the re-fire from a reel needs a target cue. Keep the small ring (`grapple_highlight`) while grappled and drop only the beam.

## 1223–1244: grappling from the wingsuit

The left B was tapped 33 times, 11 of them with a valid aim, and no grapple action was injected.

Cause (from the side investigation): the wingsuit gesture deploy at 1175.2 spends the grapple press (`controllers/bindings/mod.rs:941`). The spent state clears only when `usable` holds, and `usable` includes `snatch::available_in` (`bindings/mod.rs:652`). That is false under the canopy and in the wingsuit (`controllers/snatch/mod.rs:120`). So after any gesture deploy the grapple stays dead until Rico is back on foot.

Fix: clear the spent press on any neutral button sample, whatever the mode. Add a test of a gesture deploy followed by a grapple tap in the wingsuit.

## 2926: the grapple cursor on a moving vehicle

Riding the vehicle's roof (stunt 2917.9–2933.0), the hand pointed down at the roof, 1.47 m away. In the record, the aim point relative to the head changes smoothly tick to tick (direction (−0.07, −0.99, 0.10) drifting to (−0.12, −0.99, −0.01) over 0.4 s). But the head moves 1.16 m per sim tick, and sim ticks per frame alternate between 0 and 1.

Cause: the overlay draws the beam to `Anchor::World(g.point)` from the last sim tick (`producers.rs:220`, submitted with `SIM_TTL`). The rendered view, interpolated within the tick (`camera.fraction`), moves on by up to a tick's travel before the point is refreshed. So the cursor slides back by up to a metre and snaps forward at each sim tick: the bounce. The ring has the same problem. Gun lasers do not, since they come from render-time hand poses.

Fix: either keep the last two sim ticks' grapple points and draw the lerp at the camera's render fraction (the same previous/current/fraction the camera uses), or anchor the point in the hit body's frame, read at render time. The lerp is simpler and also covers static targets seen from a moving vehicle. Files: `controllers/aim/mod.rs` (publish the previous point), `controllers/overlay/producers.rs` (`grapple_highlight` and `grapple_beam`).

## 3442 and 3518: pickups from the ground

- 3518 (C4): snatches at 8.0 m at 3515.4 and 3518.6 both bit, then failed after about 0.5 s with "the target snagged": the stall check moved under `stall_distance_m` (0.1 m) in `stall_window_s` (0.5 s) (`snatch/config.rs:218`). The one at 2.16 m at 3522.08 was caught in 0.24 s.
  - Hypothesis: the placed charge's body, restored by `PlantedExplosive::DropExplosive` (`snatch/charge.rs:59`), is not yet dynamic in the first half second, or lies pressed into the surface it was stuck to. At 2 m the catch zone and the hand's reach close the gap before the stall window ends.
  - The `bodies` events in `e3516.json` (scratch) can settle it: check whether the charge's body moves after the bite.
  - Fix: grace the stall window from the bite for placed charges, and loosen with `remote::loosen` if the body is still keyframed.
- 3442: in the frames a rifle lies on the road beside a soldier. No `snatch: armed` line falls between 3416.6 and 3446. The right hand took the soldier at 3441.66 and threw him into his ragdoll. So no grapple pickup was attempted in the log, which means the snatch never offered the rifle under the aim. With so little to go on, it needs the reviewer to say what "it" was. If it was the tripod minigun torn at 3401.7, it never left its mount (world.md), so the snatch could not offer it; that belongs to the mounted-gun item.

## Helicopters: 743–754, 1794, 2634, and the rail

From the side investigation, checked against the log:

- 743–754 was not a grapple. A fast travel (`spawn reset (travel)` at 739.8) put Rico on the rebel helicopter's skid in a scripted sequence (`m_InSequence` from 742.2). The sequence owns him, so stepping off did nothing until right A at 754.41 sent the jump.
- 1794 and 1800: six reels onto flying helicopters took the lock's hang. The violent swing has two causes:
  - The rope damps only along the line (`grapple/rope.rs`), so nothing damps the pendulum across it, and the airframe's turns keep pumping it.
  - The line held the reel's distance (4.9–12.3 m), not `hang_length_m`; the hang's pay-out never applies a shortening.
- Y at 1800.39 (right B on Index, Use) injected `[USE_ITEM, ENTER_VEHICLE, OPEN_WINGSUIT, BTN_Y]` (`PAD_Y_ON_FOOT`, `controllers/bindings/tables/declare.rs`). Mode was `on_foot`/`reeling_in`. The lock's `FinishReel` detour (`lock/hooks.rs:76–84`) keeps Rico out of the game's `S_IDLE_HANG_STUNT`, the only hang state that accepts `ACT_ENTER_VEHICLE`. So the entry was ignored and the wingsuit opened, which ended the reel.
- 2634: no Use press fell in 2629–2638. The left upper button at 2638.60 let go of the line. The wrong pose is the reel's flying clip turned toward an overhead hook (the same cause as 844).
- How the game does it: the helicopter hang is an interaction-graph attach with an animated pose. `CHangStuntPositionState` scrubs two clips by the helicopter's pitch and roll, with no constraint or physics. Rico's graph has a door-side family: `S_GRAPPLE_REEL_TO_HELI_DRIVER_SIDE`, `S_DRIVER_SIDE_OPEN_DOOR`/`ENTER`/`PULL_OUT_OCCUPANT`, `S_DRIVER_SIDE_TO_HANG`, and `S_HANG_TO_DRIVERSIDE`. It has no idle on the side, so it is the hijack path, not a standing state.
- Proposal for the binding (stand on the rail, IK, doors, Y enters, stepping off is a skydive without a jump):
  - A mod-owned rail state built on the moving-surface carry the on-foot body already uses on vehicles (the stunt-on-vehicle carry; see `docs/mod/input/stunt-positions.md`).
  - The reel onto a helicopter finishes at the nearest rail point. Rico stands on the airframe's support, the game's on-foot character on a moving surface, with a sticky pull toward the rail line (a horizontal spring in the airframe's frame, released past a step-off distance).
  - Doors open through the existing door-handle grabs (`docs/mod/input/vehicle-doors.md`).
  - While on the rail, a rail-mode Y table sends `ENTER_VEHICLE` only.
  - Stepping off is just losing support: Rico enters the fall with the airframe's velocity and no jump act.
- RE still needed:
  - how `FinishReel`/`CheckReelToVehicleRelease` chooses between the door side and the hang;
  - the helicopter's interaction-graph entries;
  - whether an on-foot `ENTER_VEHICLE` works from the airframe's skid, or needs the door-side state queued;
  - what keeps the on-foot character supported on a fast-moving rotorcraft (the vehicle-below flag).
- Until then: in the hang, damp across the line (relative velocity across, with a time constant of about 1 s), shorten to `hang_length_m`, and route Y to the game's own hang (let `FinishReel` through) when the player presses Use in a helicopter hang.

## 1384: freefalling but standing

The game ran its scripted drop (`S_JC_SEQUENCE_TO_FREEFALL` into `S_FREE_FALLING`) with exactly zero horizontal velocity. The mod saw mode `falling`, the body case "a fall past the jump's grace", and posture `standing`: the same presentation as stepping off a ledge, with the camera above Rico's head. Fix (hypothesis): classify a freefall state with no recent ledge as a skydive and present it as the wingsuit presents (prone body, view on the head).

## 1459.4: body swung left on the canopy opening

This was not a collision. The canopy opened out of a reel: horizontal speed fell from 20 to 0.4 m/s and overshot backward, the heading turned 39 degrees left within 0.25 s, and speed rebuilt to 29.6 m/s along the new heading. No stick input was injected. Likely cause (hypothesis): the parachute's velocity alignment (`m_RotateCharYawToVelocity`) turning toward a reversed, near-zero velocity. RE: `m_VelocityAlignMinSpeed`. Fix: suppress that yaw during the opening and below a speed in the existing parachute steering detour (`hooks/input/parachute.rs`), or carry the reel's velocity through the opening. The second shares a cause with the release item: the reel's ending dropping velocity.

## Parachute handles: reach, the grab column, and pitch

- 1153.93 and 1400: all 11 handle takes in the session came through the hidden 430 mm fallback reach, at 245–417 mm from the target; none landed within the visible 200 mm. The target is the `LeftLink`/`RightLink` translation (`controllers/vehicles/parachute_rig.rs:158`), and the restore puts the handle there (`:272`). That is the top of the riser, about 25–30 cm above the toggle's grip bar seen in the video. The docs and tests still describe a handle near the shoulder (`parachute.rs:3`, `:229`).
  - At 1400 the left controller was also untracked for about 90 percent of 1388–1399.9, and both grip presses landed while it was untracked. Hypothesis: reaching high behind the head lost tracking.
  - Fix: target the grip bar, which is the link plus the rotate joint's orientation times the pivot-to-bar offset. RE the offset from where `NParachuteObjectControl_SyncBones` puts a held handle (`LeftHandAttach2`/`RightHandAttach2`). Show the reach that actually takes presses (`begin.rs:282` marks it `Shown::Never`).
- The grab column: add `TargetReach::Capsule { a, b, radius }` beside `Sphere` (`grab/target.rs:135`). The resolver measures the distance to the segment (`resolve.rs:56`, `:269`). The column runs from the bar down by `full_pull_m` and is drawn with the overlay's `Line` primitive, which gives the reviewer the extruded column.
- Pitch (binding: both down brakes, up or forward dives, differential turns):
  - Brake: with both handles held (2818–2841) the handles wrote `MOVE_BACKWARD` up to 1.0, and horizontal speed fell from 12.2 to 2.0 m/s, so the brake works. Around 1153 the handles were never held together, so they only turned.
  - Dive: missing. `pull` reads only the vertical drop (`parachute.rs:174`), and `steer` returns `(right − left, −brake)` (`:200`).
  - The game pitches the parachute from the move stick's along axis (which also sets lift and drag through `m_Lift`/`m_DragX/Y/Z`), and the handles already write that axis (`write_move`).
  - Fix: `(right − left, dive − brake)`, where dive is the smaller of the two hands' rise or forward travel past a deadzone, written as `MOVE_FORWARD`. The handle anchor should follow the hand up and forward as well as down. RE: read the shipped lift and drag values to tune the deadzone and gain.

## 370.2: wingsuit arms bent in

The right arm was drawn straight (178 degrees) and the left bent (108 degrees). The head was 0.125 m left of the midpoint between the hands, and the frames show the player looking at the left arm.

The mod drives these arms, not the game (`controllers/body/solver/wings.rs:71–72`). It estimates the shoulders from the eye position, offset along the torso heading (`controllers/body/mod.rs:290`), so turning the head drags the shoulders toward the hand being looked at. That puts the left wrist about 0.41 m from its estimated shoulder, matching the drawn 0.40 m, so the elbow folds. The suit's cloth follows the arm. The suit does not constrain the IK target.

Fix: find the neck along the head's own facing before placing the shoulders, or, while the arms are spread, centre the shoulders on the wrists' midline.

## Firm ragdoll for swings, hangs, and the canopy (322–344, 1166)

### What the game offers

From the side investigation; names and shapes are confirmed in the release IDB unless marked:

- `CRagdollInstanceImpl` has two drive controllers per ragdoll: a motor controller (tau, damping, recovery velocities, max force) and a keyframe-hierarchy controller, plus a list of hard-keyframed bones.
- Any keyframed bone makes `IsDisplacementControlledByOwner` true, so `IsRagdolling` stays false. The character stays in its own state and the mod's case table never sees a ragdoll.
- `PostPhysicsUpdate` blends the physics pose into the drawn pose by `m_BlendWeight`. This is from the dump only.
- Presets in `ragdoll_params.brdd`, selected through `m_CurrentRagdollParamsID`/`DoSyncRagdollState`:
  - `GRAPPLE_HANG` keyframes the upper body and motor-drives the legs. `rico_base.afsmb` never references it.
  - `PARACHUTING`: the canopy already runs a firm ragdoll, and `UpdateParachuteRagdoll` drives its blend weight (decompiled).

### What the mod has

The character grabs and the ragdoll pull drive other characters' ragdoll bodies by velocity (`physical_hands::remote::pull_ragdoll`, `spread_pull`; `pickup::grips`) and read them (`physical_hands/characters`). The player's own body is pure HumanIK on the animated pose (`controllers/body/`), and there is no player-ragdoll path.

### Proposed design

- One mode, entered for the lock (swing and hang) and for the canopy, using the `GRAPPLE_HANG` and `PARACHUTING` presets at blend weight 1.
- The keyframe targets each tick are the mod's solved IK pose, rotated into the line's frame for the swing.
- The head and neck stay keyframed, so the camera stays on the head and there is no head-body-camera detachment.
- The grapple arm is keyframed to the IK hand on the line: the support.
- The other arm is keyframed to its controller.
- The pelvis, spine, and legs are motor-driven toward the IK pose, with gains tuned so the hip and thigh lag stays under the holster grab radius. That keeps holsters reachable; "firm enough to reach holsters" becomes a testable bound.
- The legs swing with the line's acceleration around the windmill, as the reviewer asks.
- Entry: on the lock's take, or on the canopy's open. Exit: on its end, blending the weight out over about 0.2 s.
- The upright `UpdateMotion` direction above keeps the proxy upright, so the ragdoll hangs from the arm instead of from a tilted capsule.

### RE blockers

- Whether `PrePhysicsUpdate` reads the pose after the mod's HumanIK solve, and where the blend lands relative to the presentation pass. If it reads before, the keyframe targets must be written into the ragdoll's own targets.
- Binding `m_BlendWeight` and the motor parameter fields; the side investigation's mapping of the preset's hashed field names to motor parameters is a guess.

Risks: motor-driven legs fighting the hull; the drawn pose and the hit volumes diverging from the proxy capsule; the canopy's own blend already being driven by the game, so a writer needs to win that each tick.

## Conflicts and cross-area notes

- The entry fix contradicts the design doc's "take", which was introduced because the 10 November brake stopped Rico dead. The ballistic engagement keeps the speed without redirecting it, so it answers both rounds' feedback, but a straight rush at an overhead hook now coasts up toward the hook instead of swinging forward. That is what the reviewer's model asks.
- Hiding the hint laser while grappled conflicts with re-firing from the reel (465). Keep a target cue.
- Keeping the line across ground contact (802) means the ragdoll-style hang should not end on ceiling-hang floors unintentionally. Keep `Landed` for hangs.
- The holster refusals and drops during hangs (845.9, 860.4) are the guns area's "the animation forbids it" problem. Firm ragdoll plus an upright reel orientation may change which animation state is current.
- The canopy roll at 262 (another reviewer's area): the reel drops to a jump first, so the opening is not in `REEL_OPENINGS` (`hooks/animation/canopy.rs:52`). It shares the reel-ending path with the release fix.
