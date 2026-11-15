# Body and camera: 14 November 02:05 take

All times are t (Rerun `recording` seconds; video = t − 1.864, `view-scan.md` uses about t − 1.82). The data comes from a full `tracking-synth dump --json` of `tracking.jc3trk` reduced to arrays (controller_state, pose, rig, the presentation's "drawn arm joints", body_state, and recenters), from `rig-eval --pelvis recorded` run twice over the take (with `--weights`, which replays the EgoPoser estimate the game ran, and without, as an A/B of the old yaw rules on the same head motion), from `estimate-body`, and from `view-scan.md`. The comparison baseline is the 10 November 17:07 take, re-read from the arrays extracted for its review. Scripts and frames are in the scratchpad (`session1114/body/`), not committed.

The rig ran the estimate for the whole take: its turn cause reads `estimate` from t 0.5 to 4387.4, and the replay with `--weights` matches the recorded legs to a mean of 5.6 degrees (14.4 without it). The estimator logged only its start (running on the game's device, 16-bit weights); its readouts are not recorded.

## Previous-session checks

### Head bob while jogging and sprinting: fixed

Measured as on 10 November: the camera's height above Rico's root less the tracked head's, band-passed to 1–4 Hz, medians over 1 s windows on foot.

| | this take | 10 Nov | 7 Nov |
|---|---|---|---|
| jog, first 2 s after a standstill (76 windows) | 0.2 mm vertical RMS (0.9 mm peak to peak) | 23 mm | 0.3 mm |
| jog, continuous (39) | 0.1 mm | 20 mm | 2.7 mm |
| sprint, first 2 s (63) | 0.1 mm | 20 mm | 0.3 mm |
| sprint, continuous (241) | 0.1 mm (0.5 mm peak to peak) | 20 mm | 0.2 mm |

Against Rico's animated head the vertical correlation is −0.01 with a gain of 0.08 (0.93 and 0.94 on 10 November). Laterally the camera moves 7.7 mm against the animated head's 33 mm, correlation 0.24 and gain 0.25 (0.97 and 1.07 before, 0.28 and 0.30 on 7 November). The camera no longer rides the walk cycle.

### Falls, landings, crashes, ragdolls, and get-ups: fixed on the ground, with a snap out of the wingsuit

Grouping the take's body states into 213 airborne, landing, ragdoll, and get-up spells: through jumps, falls, and landings the view's orientation stays within 0–7 degrees of the headset's and the camera stays 1.45–1.76 m over the root on touchdown (for example 343.7, 570.4, 1704.3, 2485.3). Every get-up holds the view upright to within 1 degree of the headset's tilt (298.2, 489.9, 596.5, 1058.9, 1249.4, 1821.2, 1940.8, 2101.1, 2998.6, 3080.9, 3477.5, 3848.3).

Ragdolls entered from the wingsuit or the air start with the wingsuit's tilted view and straighten in one step: at 593.13–593.30 the camera's tilt against the headset falls from 77 to 8 degrees in 0.17 s, and the same happens at 296.8 (55 degrees), 620.6 (47), 1244.4 (33), 2991.4 (22), 3761.3 (20), and 3846.0 (47). The ragdoll itself then holds.

### Dropping from a grapple: partly

Falls after a lock's release land held, as above. The reel's arrival on the ground still moves the view: at 21 of 24 reel endings onto the ground the camera sinks to 0.45–1.04 m over the root and rises to standing height within 0.2–0.3 s. At 1974.38 the camera's world height drops 0.97 m in the last 0.4 s of a 7 m reel and comes back 0.97 m in 0.25 s while the root does not move; 801.27, 918.88, and 2526.06 are the same. The reel is a traversal, which still hangs the view from the animated head, so this is the reel's own pose and arrival. It is the closest remaining case of the 122 s and 580 s feedback.

### The canopy's opening out of a reel: not fixed (the skip never fired)

The log has no `canopy: skipped the opening out of a reel` line and no warning from the hook, with `skip_reel_opening` on. Three canopies were asked for during a reel (261.46, 814.78, and 1813.87, each logging "a canopy is requested" while the line reeled): each time the reel ended, the case went to `jump` for 20–50 ms, and the canopy opened from there, so the game played an opening that is not one of the two reel openings the hook watches. All three put the view inside the harness and backpack: frames at 261.7–262.3, 815.4–816.0, and 1814.2–1814.8 are filled with dark harness and teal canopy surfaces.

The other canopy deploys, from falls and the wingsuit, are mixed. 1146.4, 1459.1, 2613.7, and 2964.8 are clear in frames; 1390.6–1390.8 and 2972.0–2972.3 show the backpack and harness filling the view; 1803.7–1804.1 has canopy fabric across half of it; at 1406.6 and 2806.3–2807.0 Rico's legs swing up through the view. `view-scan.md` puts none of these behind Rico's head (parachute: 24 flashes, 0 behind the head, against 21 of 28 on 10 November), so the absorber lag that caused the 10 November clips is gone, and what remains is the clips' own poses with the view on the animated head.

### Camera-in-body flashes: partly

`view-scan.md`: 304 flashes in 4387 s (4.2 a minute, 3.3 on 10 November) and 74 with the camera more than 0.12 m behind the head (1.0 a minute, the same as before). By mode the camera-behind cases are on foot 65, wingsuit 5, falling 4, and parachute 0. The on-foot ones cluster at high speed and after grapple arrivals (318.7 at 21.6 m/s, 535.1–535.6, 1103.6–1103.8 at 70–125 m/s, 1247.6–1247.8 at 2 m behind) and around ragdolls (297.3, 488.3, 595.5, 1058.1, 1820.6). The canopy clips above are the visible ones in this area.

### Seat lock yaw ease: works as designed, but vehicle entry turns the view first

The re-base is applied as about 38 small recenters over 1.0 s (2253.62–2254.64, 3604.67–3605.69, 3789.16–3790.00), the largest about 1.8 degrees in a frame, with no single-frame snap. The view turns against the headset by 34 degrees at 2253.6 (logged 36) and 49 at 3604.7 (logged −42), at up to 64 and 174 degrees a second; the two small re-bases (−5.6 and 3.6 degrees) are barely visible.

New: in the 2 s before every one of the 6 seat locks the entry itself turns the view 33–96 degrees against the headset, at 250–580 degrees a second, with the sticks idle. At 3872.84–3873.06 the player pressed B at a helicopter's handhold and the view swung 96 degrees in 0.2 s as the player's collision group changed (the entry's start); the frames show the cockpit window replaced by the open side in one step. 3788.33–3788.57 (65 degrees), 4087.0 (60), 4140.6 (48), 2252.9 (53), and 3603.2 (−33) are the same. Exits turn the view 100–133 degrees within 2 s at four of six (2471.1, 3822.4 at up to 980 degrees a second, 3913.5, 4129.1), but exits from a moving vehicle mix in the vehicle's own turn, so those are unconfirmed. Measured with the follow banks and seat recenters taken out of the headset's yaw: a follow bank steps the recorded headset yaw by the bank's angle while the camera's world yaw stays continuous (median largest frame step 0.8 degrees over 89 banks over 10 degrees), so a naive camera-minus-headset comparison shows false snaps at every bank.

### Swim stroke yaw: not exercised

3.4 s of swimming (4069.4–4072.8). The swim log shows the body heading turning from 47 to 121 degrees in 2 s, but with the swim gate open only briefly, nothing can be said about strokes.

## Tonight's new body work

### Learned body estimate turning the legs: works, but the legs turn constantly and small turns start the turn-in-place animation

The rig is "turning" with the estimate as the cause in 21% of all rig frames, and in 27–42% of standing frames in every 300 s of the take. Leg turns on foot, from the recorded rig:

| | this take (estimate) | the same head motion, old rules (rig-eval) | 10 Nov (old rules) |
|---|---|---|---|
| turns per grounded minute | 58 | 11 | 9.6 |
| median turn | 4.3 degrees | 32 degrees | 26 (moving), 64 (large) |
| standing turns per standing minute (merged within 0.5 s) | 54.5 | | 7.0 |
| of those, at least 10 / 20 / 45 degrees | 21.9 / 11.1 / 2.5 per minute | | 4.1 / 3.4 / 2.5 |
| legs moving faster than 3 degrees a second while standing | 57% of frames | | 2.8% |
| Rico's facing turned while standing | 996 degrees a minute | | 439 |

The head's mean angle from the legs is the same with and without the estimate (14.9 and 15.2 degrees in rig-eval).

Body turns: when the head turns and stays away for more than 3 s while standing (29 cases), the legs have followed a median 65% of the turn by then, against 20% with the old rules on the same motion. Head-only glances: of 32 glances of 20 degrees or more that return within 3 s while standing, the legs moved more than 5 degrees in all 32 (10 with the old rules), a median 23 degrees at their furthest (0 with the old rules), and ended a median 15 degrees from where they started. For glances of 30–60 degrees the legs went a median 31 degrees (old rules 1). The replayed estimate moves 64–84% of the way with these glances, above the 60% the tuning assumed, and the 9 degree band does not absorb that. Some of these "glances" are body turns and back, which the head alone cannot tell apart, so the glance figures overstate the error, but the legs clearly do not ignore glances.

Turn-in-place shuffles: the turn flag is the gate the act filter uses to let the game's turn-in-place acts through. Counting the animated (PreIk) feet's pivot steps while standing still (a foot turning more than 15 degrees and moving more than 4 cm in 0.6 s): 16.4 a minute against 10.1 on 10 November, and 83% of them now fall within 1 s of a rig turn (10% on 10 November). 21% of turns under 10 degrees are followed by one, 31% of 10–20 degree turns, and 43% of turns over 45 degrees, against 0–14% for small turns on 10 November. So small estimate turns do start the turn-in-place shuffle, a few times a minute. The drawn feet count 18.8 pivots a minute (16.4 before). The video shows the floor arrows (the estimate's green, the rig's magenta) close together throughout, but the feet are rarely in view, so the shuffles are not confirmed by eye.

### The arm remap's shoulders and forward-reach scale: partly verified

The drawn arms (the presentation's joints) on foot: the elbow is straighter than 170 degrees in 15.0% (left) and 19.1% (right) of frames, against 35.9% and 47.3% on 10 November. The commit's offline estimate was 8% and 14% at full extension, so the fall is about half what it predicted. The median wrist sits 30 cm (left) and 22 cm (right) below Rico's shoulder; the left is 2.5 cm lower and 4.7 cm further back than on 10 November, the right higher and further forward, which follows what the right hand held (guns for most of the take) more than the remap.

The clavicle pull is smaller: in the final pose Rico's shoulder joints sit a median 1 cm in front of his sternum, against 9 cm on 10 November (the animation's own is 4.5 cm behind on the left, 6 cm in front on the right). Frames with the arms in view (136.6, 705.3, 2046.5, 1851.0–1852.4) show natural forearms and gloves, no break at the wrist, and the grapple device on the forearm.

### Rico's head on the camera's neck pivot: fixed standing, not while moving

Rico's head bone against the camera's neck pivot (the eye point less the neck model's arm), in the camera's frame, on foot:

| | to the pivot, median | p90 | median offset (right, up, back) |
|---|---|---|---|
| standing (under 0.3 m/s, 50 278 frames) | 11 mm | 121 mm | 0, −1, −1 mm |
| walking | 70 mm | 209 mm | −3, −52, +1 mm |
| jogging | 102 mm | 176 mm | −6, −58, −25 mm |
| sprinting | 120 mm | 187 mm | −9, −50, −61 mm |

Standing, the head lands on the pivot as the commit estimated (1.0 cm offline), at any pitch (7–17 mm medians from looking down past 55 degrees to looking up). By the head's yaw against Rico's facing, the head's sideways error is now a median 3–6 mm at 30–90 degrees (p90 50–128 mm), against 70–84 mm on 10 November, and the head stays 20–24 mm from the pivot there: wide head turns no longer push the camera sideways into the head.

While moving, the head bone runs 5 cm lower and 2.5–6 cm further forward than the pivot, so the camera sits closer to the back of the skull. The camera is within 6 cm of the head bone in 1.1% of on-foot frames (0.03% on 10 November), almost all running and looking down: 172 spells, for example 604.45–606.78 (7 mm, 5.5 m/s, pitch −46), 2044.1–2045.5 (12 mm, pitch −67), 2202.4–2205.7, and 4287.1–4287.9 (2 mm). The head is hidden in the view, so these do not show in the frames checked (605.5, 2204.5, 4287.4 show the ground, the legs, and the held gun), but they are where a camera-inside-the-head look would come from.

## Other findings

1. The canopy openings asked for during a reel bypass the reel-opening skip and play a clip that puts the view in the harness (above).
2. The vehicle entry turns the view 33–96 degrees in a fraction of a second before the seat lock, at all six entries (above).
3. The reel's arrival dips the view about 1 m for 0.3 s at most ground arrivals (above).
4. A crash out of the wingsuit straightens a tilted view in one step (above).
5. The estimate drives the legs far more often than the old rules and with it the turn-in-place shuffles (above).

## Cross-area notes

- The reel-arrival dip and the canopy-from-reel case both start in the grapple's ending, which the world review covers.
- The arms' full extension figures come from the presentation's joint diagnostic; the final pose (before the presentation) shows the elbow straighter than 170 degrees in only 5–7% of frames in both takes, so the remap's effect is only visible in the drawn arms.
