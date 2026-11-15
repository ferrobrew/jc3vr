# World, vehicles, grapple, flight, frame timing: 14 November 02:09 take

Times are Rerun t (log UTC minus 01:09:08.957); video time is t minus 1.864. `timeline.txt` and `log_annot.txt` beside this file are the shared timeline and annotated log. The build is `2026-11-14 02:04:28` (up to 6947b3ad). Speeds are from the rendered camera's motion over 0.1 s windows unless named otherwise, and "the character's velocity" is the controller state's `character.velocity_mps`.

## The checks from next-session.md

### Steam stalls (before playing): fixed

There are no `online: the Steam callbacks blocked the game thread` lines, `sim_steam` never takes more than 9.2 ms in any frame-timing window (t 732.1, on the fast travel's load), and the 61 s two-phase beat of 0.27 to 0.61 s hitches is gone. frames.jsonl has 13 frame gaps over 80 ms in 4387 s, against 28 over 150 ms in the 10 November take's 2416 s. None of them is on a beat, and every one but the first falls on a loading screen or a fast travel (see "Frame timing" below).

### Debug UI: works, one cross-area side effect

Before the recording (t -166 to -49) the chord opened the panel with the move stick deflected and the panel could be neither used nor closed until F8; that is diagnosed and fixed in 304ae765 and 1daf4598. In the take the chord opened and closed the panel three times without trouble: t -4.5 to 3.4 (to start the recording), t 2496.7 to 2506.8 (Tools > Video, in Falco's hangar, frames t 2497.5 to 2506), and t 4380.5 to 4390.0 (stopping the recording from Tools > Video at t 4387.6). Each time the bindings stood down and resumed in the same tick. The last opening dropped the left hand's SMG ("the gun left the hand" at t 4380.46) and the closing docked it to the left hip at t 4391.0, which the guns review may want to look at.

### Shadows: not exercised

The settings record holds only the session's initial snapshot, so neither `stereo.shadow_anchor_to_fit` nor `stereo.shadow_fit_unpadded` was flipped, and the only mid-take panel visit was to Tools > Video. No reel toward a parked car in sun was found; the frames of a fast reel past parked vehicles at Falco (t 1990.6 and 1991.0) are in the shade of the elevated roads and settle nothing.

### Human hull steps: fixed, with a suspected lift-and-drop

Fourteen step-ups logged `human hull: the narrowed player steps up a step the authored hull would`, with steps of 0.23 to 0.42 m (narrowed cone 0.21 m, authored 0.44 m). A search for on-foot stalls (supported, left stick past 0.7, camera under 0.5 m/s for over a second) finds two, t 930.4 (1.2 s, walking into a pillar, frame t 930.8) and t 2598.1 (1.1 s, into a wall, frame t 2598.5); the 10 November take stalled at a stone step for 16 s. In four step-ups (t 2746.2, 2769.1, 3160.8, and 3426.8) the camera rose 0.25 to 0.6 m and was back at or below its start within about 0.6 s; at t 2746 that is a step onto a crate at a truck's tail (frames t 2746.0 and 2746.5). The lift may be raising the player onto edges they then slide off. Suspected, not confirmed.

### Grapple swing: entry fixed, exit not fixed

Entry. The lock now keeps the rush. Across the 25 locks that were not hangs, the speed 0.3 s after the take is 61 to 126 percent of the speed 0.1 s before it, except where the lock ended in a landing within 0.3 s (t 804.2, 918.5, 921.3). Short lines keep less, as the take's cap intends: 49 to 75 percent on 7 to 15 m lines (t 2484.4, 2489.5, 2525.4, 3053.6). At t 885.97, 19 m/s across and 23 m/s up becomes 30 m/s across and 10 m/s down: a swing, not a stop.

Exit. Not fixed. All 18 `released` endings lose the swing's speed. At t 928.75 the camera goes from 37 m/s to 4.8 m/s in about 70 ms, at t 1416.46 from 26 to 2.3 m/s, and at t 2691.28 from 31 to 2.4 m/s; the character's velocity in the first falling sample is (-1.3, -0.5, 1.1) m/s at t 928.79 and exactly zero across at t 2691.30, after which Rico drops straight down, gaining 1.1 m/s each 0.1 s. The best exit (t 1830.87) keeps 11 of 33 m/s. The velocity collapses while the lock is still held, on the frame the grapple button comes up (button up at t 928.68, 1416.37, and 2691.15 in the recorded inputs; the character's velocity reads 0.4 m/s across at t 2691.10 with `lock` still set), before the release's hand-over of the swing's velocity can act. So whatever zeroes it runs at the button's release, ahead of the lock's ending; the lock's exit velocity reaches neither the reel nor the fall.

### Tap and hold as the hook lands: fixed

Eleven locks took within 0.12 s of the reel starting (t 319.2, 800.6, 838.5, 886.0, 905.2, 918.5, 1767.3, 1829.4, 2484.4, 2489.5, 2525.4), so the held press of a tap and hold now becomes the lock's at the reel's first tick.

New, for the player to confirm: four reels had bursts of lock-and-tap pairs, each lock ending `tapped` 0.06 to 0.17 s after it took: three at t 465.2 to 466.6, two at t 838.5 to 839.2, five at t 1276.8 to 1278.5, and four at t 3504.7 to 3505.6. No new line attaches during any burst, so if these were re-fire taps (the game's re-reel), the lock swallowed them; the doc's quick release resumes the reel and fires nothing onto the attached line.

### Tethering a barrel and swinging it: not exercised

The three `swinging on the held line` snatches (t 62.2, 73.7, 93.0) are ragdolls (`target_kind="BODY"`); no object snatch reached a swing. The game's own tethers were used at t 1916 to 1951 and 3026 to 3047.

### No re-acceleration after boosted reels: fixed as far as exercised

Five yank-boosted reels (t 358.5, 1777.6, 2249.3, 2594.0, 2613.1). Only one ended on a surface: at t 2249.7, peaking at 102 m/s onto a fighter jet's wing, after which the camera never passed 4.6 m/s in the next two seconds. The others ended in falls, a wingsuit, or a canopy. The 75 m/s readings after the reels at t 1091.6 and 1093.6 are Rico standing on a fighter jet in flight (frame t 1128).

### Ceiling hangs: not fixed

Four reels to a ceiling took the hang (`hang=true` at t 844.7, 889.9, 945.0, 2538.3), and the inverted cling never played. But the line never paid out to the 3.5 m `lock.hang_length_m`: the held length stayed 1.50 to 1.90 m in all four (the lock record's `length_m`).

- t 844.7 to 866.7: Rico hangs about 1.6 to 2.0 m below the hook with the tension near his weight (900 to 1100 N), the camera 1.6 to 2.0 m under the ceiling. In the frames the view is upright, but Rico's arm and body fill half the view (t 846, 850, 858).
- t 889.9 to 895.1, 945.0 to 950.4, and 2538.3 to 2540.5: Rico is held motionless 0.56 to 0.65 m below the hook, with the line slack (stretch -0.87 m, tension 0), the character's velocity exactly zero for seconds, and the camera 0.29, 0.28, and -0.20 m below the hook, at the ceiling or in it (frames t 891, 947, and 2539; at t 947 an arm fills the view). Gravity is not acting, so something other than the lock holds him: possibly the reel's arrival pinning him, or the hang lock not running its swing.

Each ended on a release, and Rico fell straight down from rest (the exit problem above).

### Helicopter hangs: partly fixed

Six reels onto a flying helicopter took the hang (`kind="vehicle"`, 15.9 t, `hang=true`). Rico rides below it upright in the frames (t 1784, 1787). The held length was the reel's distance, not 3.5 m: 11.5 to 12.3 m at t 1782.9, 4.9 to 5.3 m at t 1794.0, 6.3 to 6.6 m at t 2629.7. Two ended at once: t 1778.4 broke overstretched within 0.15 s on a 5.9 m line, and t 3531.5 ended with `reel ended` in 0.03 s. From the hang at t 1800.1 the wingsuit opened at t 1800.5, which is the wingsuit-from-a-grapple-hang case the previous take never reached. At t 2636 the view is inside Rico's arms against the airframe.

The helicopter seat's stunt press (fe34be31) was not exercised: both helicopter exits (t 3822.4, 3913.5) used the exit action.

### Canopy out of a reel: partly fixed

Five canopies opened out of a reel (t 261.5, 814.9, 1459.1, 1813.9, 2613.6). Two opened level (camera tilt never past 3 degrees beyond its pitch at t 1459 and 2613). Three rolled the view over: the camera's up axis reaches 164 degrees from vertical at t 262.52 with the pitch level, 168 degrees at t 815.79, and 154 degrees at t 1814.74. The frames at t 261.7 to 263.8 show the roll: the inside of the harness at t 262.1, looking up at the canopy at t 262.5, the ground upside down past the body at t 262.9, upright at t 263.8. The skip in `hooks/animation/canopy.rs` evidently misses some of the openings out of a reel; which state these three entered is not in the recording (`rule_state_name` is unnamed for them).

### Side grapple under the canopy: not judged

Lines fired from under the canopy at t 2627.5 and 2969.9 only; no clean side shot to judge the turn.

### Wingsuit gestures: fixed

Every deploy gesture opened the suit: from falls (t 294.8, 358.7, 592.0, 609.8, 756.7, 1142.5, 2594.4), from the canopy (t 1175.2, 1404.1, 2892.8, 2970.5), and from a helicopter hang (t 1800.5). No spread-arm pose was refused for a grip: every `unavailable` line with both lateral spans over 0.2 is a mode change, a requested canopy, a ragdoll, or a sample check. The deploy at t 358.7 took four gestures over 1.4 s before the suit came at t 360.2, each refused as "the mode is changing". No superspeed: after the deploy at t 2594.4, which followed a 63 m/s boosted reel, the suit flew at 23 m/s.

### The grapple device on the forearm: spot-checked

In the reel frames looked at (t 261.7, 1990.6, 2244.3) the line leaves the device on the drawn forearm. Not measured; the body review covers the drawn arms.

## Vehicles

### Seat lock yaw: fixed as designed; the entry and exit animations still whip the view

The re-base at each seat lock is now applied as 39 to 49 small `SeatTransition` re-bases over a second (at most 1.8 degrees in one frame; the plane's 36 degrees ran t 2253.62 to 2254.64, the car's 42 degrees t 3604.67 to 3605.69), and the camera's yaw against the vehicle's and the cockpit head's never jumps at the lock (t 2253.7, 3604.8, 3789.3, 3874.6, 4087.4). The re-bases were 36.0, -42.2, -5.6, and 3.6 degrees. The previous take's one-frame snaps of 20 to 24 degrees are gone. Whether a 36 or 42 degree eased turn is acceptable is for the player.

The fast turns are now elsewhere. On entry, before the seat lock, the view follows the entry animation's swing toward the seat with the head still: 82 degrees in 0.3 s into the helicopter (t 3788.2 to 3788.5, up to 12 degrees a frame; frames t 3788.1 to 3788.65 go from beside the seat to seated facing forward), 94 degrees in 0.2 s into the second helicopter (t 3872.84 to 3873.03, up to 15.7 degrees a frame), 60 degrees in 0.2 s into the boat (t 4085.63 to 4085.81), about 95 degrees over 1.0 s into the plane (t 2250.9 to 2251.9), and 55 degrees in 0.2 s onto the scooter (t 182.75 to 182.93). Exits do the same: 57 degrees in 0.1 s off the scooter (t 251.73 to 251.82, 18 degrees a frame) and about 100 degrees in 0.5 s out of the helicopter (t 3822.5). This is the `Ride` seat transition letting the view ride the animation, by design, but the rates are well past what the seat-lock ease was written to avoid. The previous take's review did not measure the entry swings, so whether this is new is unknown.

### No blackout near cockpit glass: fixed

The head fade never passed 0.09 while seated (t 3789.2, during the helicopter's seat lock); across the plane's 217 s it stayed at zero.

### Akrobat yoke: not exercised

No `privatepropeller` in the take. The plane flown (t 2253.7 to 2471.1) is a fighter jet with the stick and throttle drawn over the cockpit (frames t 2256, 2262, 2470).

### Throttle and stick haptics: exercised, working

In the plane the left hand felt 78 throttle steps, 13 zero stops, 10 reverse-gate crossings, and 6 afterburner-gate entries or exits; the stick gave 23 limit bumps and 17 centre ticks. The game received 4182 thrust-up and 1123 thrust-down writes, so reverse was used. The helicopter's sticks gave 11 stops and 4 neutral ticks over its two flights. How it feels is for the player.

### Turrets and mounted guns: gunner and seat fixed, tear-off partly

- Yanking the gunner: fixed. At t 532.1 the right hand pulled the turreted jeep's gunner out of the seat (the frames at t 531.9 to 533.2 show him gone and the hatch empty), the player stepped into the empty gunner seat at t 534.2, and manned the gun at t 535.4 (frame t 535.6). The armoured vehicle's empty gunner seat was entered the same way at t 1742.7 to 1743.9.
- Aiming from the seat by the handles: both hands took the handles repeatedly (t 536.7 to 551.4, 1745.8 to 1750.4).
- Tearing from the seat: the first pull at t 543.9 asked for the exit, the player stayed seated, and the tear was given up at t 547.9. The second (t 550.7) tore the gun off at t 552.05; the table refused it, the use-item fallback took nothing, and the gun lies at the road's edge in the frame at t 552.3. At t 1750.4 the tear was given up again with the player still seated. A stale "did not leave the gunner seat" line followed at t 554.8, after the gun was already gone.
- Tearing from outside: the gunboat at t 4055.1 and the standalone tripod minigun at t 3401.7 both logged `the pull tore the gun off`, the table refused both, and in the frames both guns are still on their mounts (t 4057 and 4058.5; t 3402.6 and 3403.5). At t 3402.1 the use-item fallback took a dead soldier's rifle off the ground into the right hand instead.
- Whether an NPC mans a gutted seat: not seen.

So the tear reaches its detach in every case, but the gun reaches the hand in none, and on two mounts it does not visibly leave the mount.

### Hijacks, the seat grapple, and the motorcycle

- The scooter (t 182.8 to 251.8) was ridden on the handlebars, with 17 throttle-grip and 19 brake-grip presses, up to 19.5 m/s; the previous take never exercised a motorcycle.
- The seat grapple was tried from the boat three times (t 4118.2, 4118.6, 4118.9), each `seat grapple: refused why="fire refused"`. That refusal comes from five different checks in `grapple/seat/engine.rs` (no tuning, the hook not in its custom wire state, no wire, no active wire, no device proxy after the setup), so the log cannot say which failed. Logging which check refused would settle it next time.
- No car hijack from the roof this take; the car was entered after riding its roof (t 3592 to 3603), and the car and plane exits were stunt jumps onto the roof (t 3642.4, 2471.0).

## Doors and ragdolls as world objects

The pickup's latched front-left door was gripped at t 456.07 (146 mm), the latch gave at t 456.56, a yank tore it off at t 460.07, and it came into the right hand as a prop at t 460.08 until let go at t 462.1. An armoured truck's rear door was snatched open with the line at t 2706.8 (frames t 2706.5, 2707.3). Both work.

Ragdolls, briefly, for the grabs review: 36 holds of a character or ragdoll lasted 0.25 to 7.7 s (t 1578.5 held 7.7 s, t 3298.4 held 7.1 s), against 0.08 to 0.35 s in the 10 November take, but the held bone's lag often reaches 0.9 to 1.0 m, the tear distance, before the hold ends.

## Frame timing

Hitches. 13 frame gaps over 80 ms in frames.jsonl: one of 546 ms at t 0.91, when the capture host started the screen recorder (the stall is in `vr_lock` and `submit`, 523 ms), and the rest of 81 to 145 ms on loading screens and fast travels: t 579.7 (death), 735.3 to 742.3 (four, the fast travel to Falco), 1380.3 and 1380.6 (fast travel), 3751.8 and 3752.1 (death), 4158.2, 4158.3, and 4162.7 (death). The longest frames there are in `engine_update_render` (up to 99 ms) and `sim_tick` (up to 46 ms). There are no hitches in play.

Frame rate. The median 10 s window's frame time is 24.5 ms (41 fps), against 20.5 ms in the 10 November take, and it never fell below 21.5 ms in any five-minute stretch. The rise is mostly `engine_update_render`, whose mean per frame doubled from 2.3 to 4.2 ms, plus 0.2 ms more in the mod's own share. This build adds the learned body estimator, running on the game's Direct3D 11 device (`body estimator: running on the game's device ... precision=F16` at t -211.1), which is the obvious suspect; the scenes also differ, so it is not established.

The crash after the recording is the known WaveWorks CUDA fault.

## New problems in this area

1. The swing's exit still drops Rico dead: the velocity collapses on the frame the grapple button comes up, while the lock still holds (t 928.75, 1416.46, 2691.28, and every other release).
2. Ceiling hangs neither pay out to 3.5 m nor, in three of four, let Rico hang: he is held motionless 0.6 m under the hook with the line slack and the camera at the ceiling (t 889.9, 945.0, 2538.3). The fourth (t 844.7) hangs on a 1.9 m line.
3. Three of five canopies opened out of a reel still roll the view over, to 150 to 170 degrees, through the harness (t 262.5, 815.8, 1814.7).
4. Mounted-gun tears never put the gun in the hand; on the gunboat and the tripod minigun the gun stays on its mount although the tear is logged, and the use-item fallback took a nearby rifle instead (t 3401.7, 4055.1). From the gunner seat the tear is often given up with the player still seated (t 547.9, 1754.4).
5. Vehicle entries and exits whip the view 55 to 100 degrees in 0.1 to 0.5 s, at up to 18 degrees a frame (helicopter t 3788.3 and 3872.9, boat t 4085.7, scooter t 182.8 and 251.8, helicopter exit t 3822.5).
6. The frame time rose from 20.5 to 24.5 ms median, with `engine_update_render` doubling; the body estimator on the game's device is the suspect.

Suspected: bursts of lock-and-tap pairs on one reel swallowing what may be re-fire taps (t 465, 839, 1277, 3505); step-ups that lift the player and drop them back (t 2746.2, 3160.8); the seat grapple's `fire refused` naming no check.

## Not exercised

The shadow A/B; the barrel tether swing; the Akrobat and its yoke; the helicopter seat's stunt press; an NPC manning a gutted seat; a side grapple under the canopy clean enough to judge; a car hijack.
