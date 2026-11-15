# Analysis of the 14 November 2026 02:05 session

Take: `sessions/2026-11-14_02-05-36_243/recordings/2026-11-14_02-09-08_957/`. The session started at 01:05:36.248 UTC and ended at 02:22:32.8 in the known WaveWorks crash (CONTRIBUTING.md), 16 s after the recording stopped. Tracking started at 01:09:08.957, the video at 01:09:10.821, and the recording stopped at 02:22:16.534, so the take is 4387.6 s of tracking. It has the video, `tracking.jc3trk`, `frames.jsonl`, `take.rrd` (exported by the capture relay), and `view-scan.md` (copied here). The build string is `2026-11-14 02:04:28`, which includes everything up to 6947b3ad: the learned body estimator, the skeleton shoulders and forward-reach arm remap, and Rico's head on the camera's neck pivot all ran in a headset for the first time. The estimator ran on the game's device with the 16-bit weights.

Times are the Rerun `recording` timeline's, which starts at the tracking start: a time `t` is the UTC log time minus 01:09:08.957, and the video time is `t` minus 1.864 s.

This is the initial harvest, written before the reviewer's feedback. The verdicts come from the tracking data and video frames, not only from the log. Each area has its full report beside this one:

- [body.md](body.md): the body, the camera, the learned estimate, the arm remap, and the head pivot.
- [guns.md](guns.md): guns, holsters, reloading, grenades, C4, grabs of objects and characters, and the snatch.
- [world.md](world.md): the grapple, flight, vehicles and mounted guns, doors and ragdolls, the hull, the debug UI, and frame timing.
- [view-scan.md](view-scan.md): 304 flashes, 74 with the camera more than 0.12 m behind Rico's head.
- `timeline.txt` and `log_annot.txt` (not committed): what the reviewer did, and the session log with `t` and the video time on each line.

## The previous round's checks

From [the 10 November 17:07 test plan](../2026-11-10_17-07/next-session.md), in its order.

Fixed:

- The Steam stalls: no `online:` stall lines, `sim_steam` peaks at 9.2 ms, and the 61 s beat is gone.
- Head bob: 0.1 to 0.2 mm RMS vertically while jogging and sprinting, against 20 to 23 mm, and no longer correlated with the animated head. A 7.7 mm lateral sway remains.
- Get-ups on the ground hold the view upright to within 1 degree.
- The narrowed hull steps up 0.23 to 0.42 m kerbs and steps (14 step-ups, no stalls).
- Ragdoll grabs mostly hang: of 36 ragdoll holds, 21 ended at the grip's release (12 of them throws) and 11 were torn after 0.23 to 6.97 s, against every hold torn within 0.35 s before.
- Grenade snatches (6 of 6), car doors by the handle, and the door coming to the hand.
- C4: the right hand's first press takes a charge 86 percent of the time (50 percent before); the detonator replaces a held charge and shows on the pouch with none left.
- The launcher's fresh pod shows at once.
- The fast magazine knock-out works (9 times), though 5 of the 12 pushes that were too slow came at 0.81 to 0.99 m/s, just under the 1.0 m/s threshold.
- The swing entry keeps 61 to 126 percent of the incoming speed; tap and hold as the hook lands took 11 locks within 0.12 s.
- Wingsuit gestures: every deploy worked, none was refused for a grip, and no superspeed. The suit opened from a helicopter hang (t 1800.5).
- The seat lock's yaw re-base is spread over about a second (at most 1.8 degrees a frame), and nothing blacked out near cockpit glass.
- The fighter's throttle and stick haptics all fire (78 throttle steps, 13 zero stops, 10 reverse-gate crossings).
- Yanking a turret gunner out, and entering the gunner seat.

Partly fixed:

- Holsters: hip draw misses fell from 30 to 17 percent, but docks still succeed only 75 percent of the time. Eleven drops follow "the animation forbids it", where the attraction path has no keep fallback; six guns fell mid-travel with no refusal logged; a two-handed hip holster lost the right gun 3 times in 4.
- Swaps onto a full socket: 2 of 3 worked, and the hip swap's hint reads old to new. The failed shoulder swap at t 2657 put both guns on the ground.
- Grenade priming: the pin bump, the quickening clicks, and the green-to-yellow ring work, but the trail and its sound never start in the hand (`effect=false sound=false` on all 14 primes). None went off early.
- Dropping from a grapple: 21 of 24 reel endings on the ground sink the view about 1 m and spring back within 0.3 s (for example t 1974.4).
- Helicopter hangs ride upright but on the reel's own length (5 to 12 m), and two ended at once.
- Mounted guns: the gun never comes to the hand. The gunboat's gun and the tripod minigun stay mounted although the tear is logged, and the pickup took a dead soldier's rifle instead (t 3401.7, 4055.1); tears from the gunner seat are often given up with the player still seated.

Not fixed:

- The swing exit: all 18 releases drop the speed to about zero within some 70 ms (37 to 4.8 m/s at t 928.75), on the frame the grapple button comes up, and Rico falls straight down.
- Ceiling hangs: the line never pays out to 3.5 m and stays at 1.5 to 1.9 m. In three of four (t 889.9, 945, 2538) Rico hangs frozen 0.6 m under the hook with the line slack and the camera at the ceiling, inside Rico's body.
- The canopy opening out of a reel: the reel-originated deploys (t 261.5, 814.9, 1814.0) end the reel with a 20 to 50 ms jump first, so the roll skip never fires; all three roll the view 150 to 170 degrees through the harness.
- The launcher's burst is still two rockets, then the pod is thrown; the game's own reload start is never logged.

Not exercised: swim stroke yaw (3.4 s of swimming), the Akrobat yoke, the shadow A/B, a tethered barrel swing, the helicopter seat's stunt press, the revolver, two rifles to one shoulder, and a fence carry.

## Tonight's body changes

- The learned estimate turns the legs (d305e9c4, 3fcd73c8). After a head turn held more than 3 s the legs have followed 65 percent of it, against 20 percent under the old rules. But they also follow every glance (median 23 degrees), turn about 55 times a standing minute, and move in 57 percent of standing frames (2.8 percent before). The small turns start turn-in-place shuffles: 16.4 animated foot pivots a minute against 10.1, 83 percent of them following a rig turn. The estimate swings 64 to 84 percent of the way with a head-only glance, more than the 60 percent the tuning assumed, so the 9 degree band is too narrow for it.
- The arm remap (2b54ac38, 6947b3ad): the drawn elbow is near full extension in 15 percent (left) and 19 percent (right) of on-foot frames, against 36 and 47 percent before and the 8 and 14 percent the commit predicted. The shoulders sit about 1 cm ahead of the sternum (9 cm before). A hand at rest now hangs on its hip, so the "Left hip" hint shows while walking.
- Rico's head on the neck pivot (c48ab372): standing, the head bone lands 11 mm from the pivot and wide head turns err 3 to 6 mm sideways (70 to 84 mm before). Jogging and sprinting, the head runs 10 to 12 cm off the pivot, lower and further forward, and the camera comes within 6 cm of the head bone in 1.1 percent of on-foot frames, mostly running while looking down.

## New problems

1. The estimate-driven legs turn on glances and shuffle the feet (above).
2. Vehicle entry and exit animations swing the view 33 to 100 degrees in 0.1 to 0.5 s with the sticks idle, up to 18 degrees a frame (helicopters at t 3788.3 and 3872.8, the scooter at t 182.8 and 251.8). The seat-lock ease does not cover them.
3. A crash out of the wingsuit straightens the tilted view in one step (77 degrees in 0.17 s at t 593.1, and six other cases).
4. Frame time: the median rose from 20.5 to 24.5 ms against the previous take, and `engine_update_render` doubled. The estimator runs on the game's device and is the obvious suspect, but the scenes differ and its own GPU time measured about 0.12 ms offline, so the cause is not established; an A/B with the estimator switched off would settle it.
5. A refused C4 planting loses the charge (t 906, 1939, 2100), and at t 575 to 578 ten presses logged detonations with no charge going off.
6. Belt pushes with a magazine still in do nothing and block the trigger (9 pushes, t 1110 to 1122).
7. About 40 percent of the 64 character grabs left the hand empty, and allied soldiers do not go limp although the log says they did (t 3256 to 3261).
8. Opening the debug panel at t 4380 dropped the left hand's SMG.

Before the recording (t −166 to −49), opening the debug panel with the controller chord while the move stick was tilted left Rico walking under the panel, and the panel could be neither used nor closed for two minutes until F8. The walking is fixed by 304ae765; 1daf4598 logs what holds such a wait, since the second half is not yet explained.

Suspected, not confirmed: bursts of lock-then-tap pairs on one reel that may swallow re-fire taps; step-ups that lift the player and drop them back; the seat grapple's "fire refused", which does not say which check failed.

## Frame hitches

None in play. One 546 ms frame at t 0.91 came with the screen recorder starting; the other 12 gaps of 81 to 145 ms fall on loading screens and fast travel.
