# Body, arms, camera, swim, flight: 7 November take

Times are recording seconds (video = rec − 1.0 s; video start 00:59:03.525 UTC). 6 November numbers were recomputed here with the same scripts on that take, so some differ from the brief's (method noted). Scripts and outputs are in this folder (`armcmp.txt`, `werr2.py`, `quiet3.py`, `flips.py`, `stopturn.py`, `square2.py`).

## Timeline

0-374 on foot, rifles in hand 111-294 (braced at the handguard 213-280). 376-400 swim (00:06:15-00:06:39). 461-523 wingsuit/parachute/canopy toggles. 535-582 car. 605-1135 on foot with long guns most of the time; 646-695 debug UI. 1164.8 manual recenter. 1365-1435 fall, wingsuit. 1568-1582 parachute. 1767-1824 wingsuit then canopy. 1891-1923 car, 1928-1943 wingsuit/parachute. 1950-2113 mostly debug UI/menus (auto-recenters on resume at 2015.8, 2200.1, 2313.7). 2422.7 manual recenter. 2461-2583 and 2737-2755 vehicles. 2800-2816 parachute. 2973.6 and 2976.8 manual recenters. 3048.6 manual recenter after a landing. 3097 wingsuit.

## Changes

- Hands on the real hands (cockpit from eyes): exercised, worked for the registration. `head_world` now sits 0.1 cm from the rendered camera (12.5 cm on 6 Nov), and the residual has no constant head-frame bias (≤4 cm, as on 6 Nov). The export's 0.133 vs 0.119 is neither the eye registration (the export used the same head as the cockpit both times, so the eye offset cancelled then and still does) nor the reach clamp: the error is flat across reach (12-13.5 cm at every band, short reach included) and mostly sideways (radial −2 to −5 cm, tangential 8-11 cm). It grew uniformly (still and moving, any head-chest yaw), the left hand's sideways part most (6 → 9.5 cm). Suspected: the remap's new chest-frame shoulder (2a4cd377, `hands/reach.rs`). What the player sees improved: 6 Nov's ~19 cm (export + eye offset) is now ~13 cm.
- Legs settle on the head when still: worked. Steady spells (≥3 s, gaze within 8°, measured against the head's yaw): 14 of 64 ended >15° off on 6 Nov, 4 of 90 now; 31 of 36 spells that started >15° off settled within 8° (median 2.4 s). The 4 left (746, 773, 982, 2903 s) hold rifles: the yaw target itself sits off the head, i.e. the hand bias.
- Looking down: past 70° it holds (legs 2 deg/s against a head sweeping 75 deg/s, chest 1%). In the 55-70° band, where most holster glances fall (122 s against 74 s past 70°), legs still turn at 15% of head rate (9% on 6 Nov) and a fast glance there still fires a large turn (e.g. 3031.5: legs 24° → −38° in 0.25 s at pitch −62; 2635.3; 65.0). 11 of 60 look-down spells turned the legs >15° (14 of 67).
- Leg flips while walking: worked. Leg-yaw reversals within 1.5 s while moving: 50 in 404 s (7.4/min) on 6 Nov, 33 in 742 s (2.7/min) now; small 8-25° flip pairs 36 → 4. The remainder are deliberate 60-80° head turns (e.g. 1326-1334).
- Drift slip correction: exercised, working. Drifting phase entered 210 times (134 s) against 55 (27 s); stepping 48 against 6. Still-standing camera-root gap p50 8.3 → 5.9 cm (p99 unchanged, ~31 cm).
- Squared hips and chest: worked. Steady, level gaze: hips −23.8° → −2.5° from the head, shoulders −18.1° → +1.9°.
- Square idle feet: exercised; the foot IK result is not in the pose events (the Final pose's feet are identical to 6 Nov), but sampled frames looking down (1177, 1517 s) show the feet side by side. Stop turn-in-place: not every time, but more often: 79 of 228 walk stops entered `S_RUN_TO_STOP_TURN_0`/`S_WALK_TO_STOP_TURN_0` (35%, against 40 of 150, 27%), and the character heading now turns a median 18° within 1.2 s of a plain stop (0° on 6 Nov).
- Spine fold cap: exercised (24 still look-downs past 72°, some to −90°); no fold in sampled frames. Not measurable from the rig rows.
- Rifle pumping: partly fixed. Walking with a rifle, Rico's wrist-to-head distance swings 8.3-8.7 cm per second (median) against 9.5-12.6 on 6 Nov; the real hands swing 3 cm. Model-space wrist acc p50 6.4 → 3.6 m/s², jerk mean 104 → 86; shoulder acc p50 9.5 → 7.9, jerk 142 → 130 (the walk clip's shoulder bob stays). targets_check: all 41.2 / on foot 35.6 / parachute 53.9 m/s² wrist p99 (6 Nov 45.6/38.0/66.5).
- Rule-B elbow pole: the drawn elbow is not recorded (pose events stop at HumanIK). HumanIK's own elbow flips dropped (13.7 → 9.7/min overall, 11.7 → 8.7 on foot, 57.2 → 29.7 parachute). Rule B run offline on this take's targets predicts 4.0/min on foot and 9.6 under the canopy, but 22/min in the wingsuit, worse than HumanIK's 14.6 (spread, nearly straight arms).
- Torso hold through short flights and canopy toggles: worked. Every jump, fall past the grace, and parachute↔fall change keeps `torso=1.0` (120 jumps, 17+7 canopy toggles). Grapple pulls still hand over to the animation (`torso=0.0`, 37 times).
- Wingsuit arm heading: no glance coupling. Wrist-midpoint lateral vs head yaw: corr −0.03 wingsuit, +0.05 parachute (+0.30 on 6 Nov).
- Swim strokes: worked. Stick at 0 throughout; strokes surge to 2-4.2 m/s and glide down over ~5 s (380-392, 393-398), against 0.01-0.2 m/s on 6 Nov. Only 23 s of swimming; underwater not seen.
- Frame timing: regressed, see anomaly 1.
- Recenters: 5 player recenters mid-session (1164.8, 2422.7, 2973.6, 2976.8, 3048.6) against 1 on 6 Nov, plus 3 auto on resume. Camera snaps: none unexplained; the root-camera mismatch flags in jumps and grapples occur at the 6 Nov rate (409 vs 352 frames).

## Anomalies

Confirmed:

1. Frame rate fell from 49 to 40 fps for the whole take (display every second refresh 52% → 83%; 3+ refresh frames 0.07% → 1.8%; six frames ≥100 ms, worst 162 ms at 2003.7, all at 1973-2311 during debug UI and menus, in `engine_update_render`). The slowdown is uniform across modes and across game-only slots (`engine_update_render` 1.95 → 3.17 ms, GPU drains +60%, sim slots +30%), so it looks like scene or machine load rather than one mod system; the new cost is `hand_bodies` 0.01 → 0.35 ms. Render settings are unchanged between takes.
2. Crouched, the drawn hands sit far off the real ones: 24 cm at 15-35 cm crouch, 39-43 cm deeper (12 cm standing). Same on 6 Nov (35-41 cm).
3. Under the canopy and in the wingsuit the drawn hands are 41 and 56 cm off (target-to-drawn 24 and 29 cm: the late correction does not reach the target); in vehicles 22 cm. Partly by design (handles, wheel).
4. An auto-recenter on resume (2200.1) took a baseline 16 cm below the standing head, and the head stayed 14-27 cm above baseline until the manual recenter at 2422.7. The camera height comes from the floor remap, so the effect may be small, but it may explain that recenter.

Suspected:

5. Wrist error +1.3 cm sideways, uniform: the chest-frame shoulder in the remap (2a4cd377).
6. Glances at 55-70° still turn the legs (anomaly in the look-down change above).
7. Wingsuit elbows may flip more with the pole than without (offline prediction only).
8. Stop turn-in-place more frequent, heading turns on stop (likely the square-hips follow, 9c7cf4ba, steering the root at a stop); check the video at 976-1061 s, where 16 stop-turns fall in 85 s.
