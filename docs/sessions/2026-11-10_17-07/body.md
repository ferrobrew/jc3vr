# Body, camera, arms, and stunts: 10 November 17:07 take

All times are t (recording seconds; video = t − 1.0). The data comes from `dump.jsonl` (controller_state, pose, rig, and the presentation's "drawn arm joints"). The scripts are in this folder (`extract.py`, `bob*.py`, `headtorso.py`, `arms.py`, `dvt3.py`, `clip.py`). The 11:32 take is no longer on disk (`sessions/` only holds this session, and a filesystem search found nothing), so the comparison baseline is the 7 November take, re-extracted from `take1107/dump.jsonl`.

## Previous-feedback items

### The camera bobs with the walking animation: not fixed

The camera still rides the walk cycle. Vertical bob is the camera's height above Rico's root, band-passed to 1–4 Hz, after taking out the tracked head's own height. Lateral bob is the same measure across the direction of travel, after taking out the tracked head offset turned into the world. Medians over 1 s windows on foot:

| | this take | 7 Nov |
|---|---|---|
| jog, first 2 s after a standstill | 23 mm vertical RMS (86 mm peak to peak) | 0.3 mm |
| jog, continuous | 20 mm | 2.7 mm |
| sprint, first 2 s | 20 mm | 0.3 mm |
| sprint, continuous | 20 mm (60 mm peak to peak) | 0.2 mm |
| tracked head in the same band | 1–2 mm | 1–2 mm |

The camera's bob follows the animated pre-IK head bone almost one to one. Vertically the correlation is 0.93 with a gain of 0.94, against 0.01 and 0.07 on 7 November. Laterally it is 0.97 and 1.07 (35 mm against the head's 34 mm), against 0.28 and 0.30. In `bobseg.py t10 270 272.5` the camera minus the root swings ±40 mm at about 3 Hz, in step with `head_pre_y`. Starting from a standstill and walking continuously bob the same way. The player almost never walked slowly (one walk window), so the walk numbers are jog and sprint.

Cause: f05925ce put the neutral head in `Geometry::camera_head` and taught `HeadPoseState::camera_head` (`payload/src/headpose/mod.rs:299`) and the camera's neck delta and eye arm (`payload/src/hooks/camera/mod.rs:449`) to use it. The headset's per-frame pose, though, is composed in `payload/src/vr/frame.rs:215` and `:241` from `pair[1].head` and `pair[0].head`, which are the anatomy (animated) head. It is then published by `publish_pair` (`:294`) every display frame, so the base position the camera hangs from is still the animated head. The neutral-head capture works as intended, but its result never reaches the headset pose.

### The head detached from the body in the plane stunt position: fixed in steady state, with one remaining case

The skeleton itself stays rigid in all 11 windows (head–neck 80–91 mm, neck–sternum 109–120 mm). Detachment is therefore the camera against the drawn head. That distance was measured at the same tick (lag 0 checked on fast frames, median 110 mm), and the baselines are 103 mm seated, 122 mm under the parachute, and 169 mm standing on foot. The stunt windows from the session log, with the camera's distance to the drawn head as a steady-state median (after the first 1 s) and its maximum:

- 352.9–353.2: 0.3 s, 174 mm.
- 364.6–379.3, the plane stunt: 113 mm, p90 202.
- 713.1–716.7: 143 mm.
- 1936.1–1939.3: 126 mm, 495 mm at the exit.
- 1941.9–1945.5: 128 mm.
- 1950.2–1953.0: 111 mm.
- 1953.6–1957.3: 121 mm, 410 mm at the exit.
- 1959.0–1966.3: 111 mm.
- 1967.0–1969.8: 160 mm.
- 2368.3–2378.7: 132 mm. The 595 mm handover error at entry settles to about 150 mm within 1 s.
- 2415.25 to the end of the take: about 1 m.

Frames at 366, 369, 376, 1962, and 2372 show the body whole under the view.

The remaining case is a deep crouch in a stunt. At 372.3–373.7 the tracked head dropped 0.89 m and the drawn head bottomed out at 1.19 m, because the hips stay at 1.01 m on the clip's legs. The camera went 0.42–0.45 m below the drawn head. The frame at 372.9 (video 06:11.9) has the view down at chest level, with a boot just under it.

The last entry (874 mm) is not a stunt-body fault. The camera was already 0.8–1.0 m from the drawn head in the seat from 2413, while the car crashed and the player's tracked head moved 1.3 m sideways in the cockpit (head_cockpit x from −0.99 to +0.33 m). At 2415.4 (video 40:14.4) the view is inside the car's shell.

### The drawn arms (9 November: arm, forearm, and hand detachment): partly verified

The elbow flips on the drawn arms (a swivel turn of more than 60° within 0.1 s, both arms, matched to the mode by time):

| | drawn | prediction | HumanIK Final, this take | animation (PreIk) |
|---|---|---|---|---|
| on foot (25.8 min) | 3.9/min | 4 | 5.9 | 12.7 |
| canopy (2.9 min) | 7.3/min | 10 | 38.8 | 19.2 |
| wingsuit (0.5 min, 12 flips) | 23/min | 22 | 19.4 | 15.5 |

All three match the offline study. The wingsuit sample is thin.

Separation at the joints:

- The upper arm is rigid, 251–253 mm.
- The forearm stretches past its 239 mm bind length, up to the 1.5× cap of 359 mm, in 2–4% of on-foot frames, more than 1% of canopy frames, and 56% of wingsuit time on the right arm (662–675 s at the cap). The stretch is designed behaviour (`stretch_forearm`), but it is where a stretched or detached look would show.
- The drawn wrist sits on the late correction's render wrist (`latest_wrist`) within 1–2 mm (p90) in 96–99% of frames in every mode. On foot while moving and when falling, 2–4% of frames are 0.5 m off, which looks like pairing noise around ticks. Wingsuit frames have no late data.

Mesh seams themselves cannot be seen in joint data, and no visible break was found in the frames where the arms are in view (369, 376, 1150.5).

### The body's general alignment: improved

- Steady spells of 3 s or more: 3 of 45 end more than 15° off (4 of 90 on 7 November). 11 of 14 spells that start more than 15° off settle within 8° (median 2.2 s).
- Looking down: past 55° pitch the legs no longer turn at all (0 of 30 look-downs turned them more than 15°, against 11 of 60 on 7 November). The exception is the 1573.8–1577.7 spell, which ended with the legs 80° off the head at pitch −65°. The hold can also leave the body facing away.
- Leg reversals while moving: 2.9/min (2.7 on 7 November).
- Drift: 108 entries in 1485 s of rig time (210 in 2300 s), unchanged.

### Camera clipping into the body (9 November): partly

On vehicle entry, none of 7 entries brings the camera within 111 mm of the torso. The video confirms clipping at:

- One of 7 vehicle exits: 1088.4–1088.6, where Rico's leg fills the view.
- Three of 10 canopy deploys: 535.4–535.8, 699.9–700.4, and 2288.1–2288.5, inside the harness and backpack (`strip_535.4.png`, `s_both.png`, `strip_2287.8.png`).
- Two of 4 wingsuit openings: 580.9–581.2, and 1286.9 going into the water.

Each lasts 0.2–0.6 s, and the camera is 0.3–1.1 m from the drawn head during them.

### The frame hitches attributed to sim_steam: confirmed

28 one-second windows have a frame over 150 ms (0.19–0.61 s), and each one is `sim_steam` under `sim_tail`. They fall into two groups, 22 of 250–330 ms and 5 of 580–610 ms, and the gaps between them are mostly multiples of about 30 s (26, 35, 61, 87, 96, 122, and 183 s), which points to a periodic Steam call. The mean frame is 22.5 ms.

## Other anomalies

Confirmed:

1. Beyond the items above (the bob regression and the deploy and exit clipping): In a stunt, a deep crouch drops the view through the body, because the stunt legs do not crouch (372.3–373.7).
2. The diagnostic trap: the "drawn arm joints" `sequence` field is the presentation's own counter (`payload/src/controllers/presentation/mod.rs:293`), not `controller_state.frame_sequence`. It lags by 12–26 frames, and pairing by it gives false wrist errors of 0.2–0.4 m and a false 25% detachment rate. Pair by time instead.
3. The right forearm is pinned at the stretch cap for most of the wingsuit flight (662–675).

Suspected:

4. The look-down hold can leave the legs 80° off the head (1573.8).
5. The view went inside the car shell after the crash at 2415.4, with the player's head out of the seat. This is end-of-take behaviour and may be expected.

## What the preliminary README gets wrong

- The time basis is off by 1 s throughout: the 874 mm entry is at t 2415.25, not 2414.3, and the plane stunt is at t 364.6–379.3.
- It treats the 874 mm last entry as a possible stunt-body problem. The camera was already 0.8–1 m off in the seat, because the player's real head left the seat in a crash.
- It says the bob "needs the recording". The recording shows the bob is present and also shows why.
- The hitches are right, but it misses the periodicity and the bimodal durations.
