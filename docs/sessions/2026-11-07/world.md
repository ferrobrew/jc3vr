# World, vehicles, grapple, flight: 7 November take

Times are recording seconds (rec) and video mm:ss (video = rec - 0.996 s). Slow motion (time scale 0.5) runs 1646-2037 and 2989-end. Whole-session timeline: `world/timeline.txt`; annotated log: `world/log_annot.txt`.

## Timeline (my area)

- 303-356 (05:02-05:55): roadster doors torn and carried two-handed (both lost), 9 snatch attempts at the torn door on the ground, a 10 kg prop snatched and lost at once.
- 360-530 (05:59-08:49): grapple traversal, three gesture wingsuit openings, a canopy cut, swimming 376-398.
- 535-582 (08:54-09:41): modernlimo, parachute bail at 581.6.
- 1430 (23:49): first wingsuit take-off. 1453-1476 (24:12-24:35): armoured jeep door pulled open, torn, carried 13 s.
- 1546-1824 (25:45-30:23): grapple locks that hold (four), canopy cut with an SMG drawn mid-fall.
- 1891-1922 (31:30-32:01): helicopter. 1992 and 2804: parachute handles.
- 2002.8, 2183.5, 2305.6: three deaths and respawns.
- 2461-2582 (41:00-43:01): racingsupercar, four seat-grapple tethers. 2736-2755 (45:35-45:54): vintagesuper, failed bail.
- 2888.7: placed crate snatched. 3038 and 3095: take-offs.
- Clip `video-explosion-to-remember.mp4` is video 50:53.0-51:24.0 (rec 3054-3085; matched by frame fingerprint). Rico, with a pistol and an SMG, shoots the pumps at a gas station. It blows up at about 51:15 (rec 3076) and the canopy comes down on him.
- The session ends with a first-chance access violation in `cudart64_55.dll` under `gfsdk_waveworks` at rec 3141, 14 s after the recording stopped. The log stops 0.12 s before it. There is no shutdown or uninject.

## Changes

- Settings migration: exercised, worked. There is a `settings.toml.v0-2026-11-07_01-58-54_943` copy, the 3 expected warns (back_bend 20, moving_deadzone 15, keymap), 2 unknown keys skipped, and the file is rewritten sparse (579 B). The weapon wheel is still on (`[controllers.weapons.wheel] enabled = true` in the new file and in the recording's settings). By design, 02d4c140 no longer migrates switches. That contradicts the plan's "the wheel stays off".
- Human hull: one line at inject, `narrowed=true body=598 migrated_bodies=106`, and no change after three respawns. There was no uninject, so no restore is logged. Nothing in the log points to getting stuck. Not checked against the video.
- Heavy carries stay held: partly. The 41 kg jeep door was carried 13 s two-handed and let go by the grip (1462.6-1475.7). Both 75 kg roadster doors were lost 1.5-3.5 s after their tear (308.0; 348.6/350.4). The tear on the latched jeep door at 1458.2 came at the hinge's limit, which is by design.
- Doors on the ground: not exercised by hand. The snatch on one failed (see the anomalies).
- Snatch: worked for a placed crate (2888.7), 5 placed C4 charges, weapons, and grenades. Right-hand catches work: 8 of 17 weapon catches went to the right hand (725.6, 799.3, 900.7, 946.3, 1225.1, 1702.1, 1848.7, 2355.6). Two failures: the torn door, and a loose prop that was lost on arrival. A late squeeze cannot be confirmed from the log. There were 4 missed catches (714.7, 721.6, 792.1, 1217.9).
- Grapple lock: exercised, works when held. 11 locks: 1546.5 (1.3 s), 1552.9 (1.8 s), 1585.4 (28.5 s, released), and 1813.6 (a vehicle, 10 s). The other 7 ended "tapped" within 0.1-0.2 s.
- Seat grapple: worked. 4 fire-and-tether from the supercar (2561.1-2572.2). Double tap and tighten not seen.
- Vehicle exit with long A: the plain exit (below the bail speed) was not exercised. The bail worked once (581.1) and failed once (2754.9, see anomaly 2). The summary heading never flickered.
- Motorcycle grips: not exercised (no bike).
- Hands on the wheel: worked. All 11 releases followed the squeeze opening; there were no drift losses.
- Nitrous button: not evidenced. `USE_VEHICLE_MOD` was only a right B tap in the helicopter (1898.2).
- Wingsuit from the game's jump: mostly works. Jumps opened at 0.68 s (461.5) and 0.50 s (2385.2). 373.6 opened on the retry at 0.94 s, and 1927 on the third click. 2958.8 never opened.
- Wingsuit after a canopy cut: 496.7 → 497.45 (0.76 s, with the grapple hand blocking until 497.17, so 0.28 s after that cleared).
- Wingsuit with a drawn gun: 1764.3 cut, SMG drawn at 1765.4, opened 1767.0 (slow motion) without the arms lowered. Worked.
- Take-off toward the gaze: rig body yaw versus the logged head heading at wind-up was 9° at 1430 and under 3° at 3038 and 3095.
- Parachute handles: worked. Both handles taken in the same tick (1992.0, 2804.2), the right one at 324 mm.

## Anomalies

Confirmed:

1. The snatch cannot hook a torn-off door lying on the ground. 9 of 9 attempts at 311-342 (05:10-05:41) failed with "the hook missed": a HEAVY OBJECT, 74.8 kg, 3.4-3.9 m away. The game's wire goes invalid about 0.1 s after firing and never attaches (`snatch/mod.rs:760`, `engine.rs:94-101`). Frame 05:12 shows the door with the target ring on it. Likely cause: torn parts are offered as targets (1ac63572), but the game's hook does not attach to them.
2. A long A at speed can do nothing. At 2754.32 the right A was held 0.72 s. The bail latched at 0.5 s and held OPEN_PARACHUTE for only 0.23 s before the release, so the game did not bail. No plain exit either. The player then tapped A three times and ended up on the roof (2755.4). The bail at 581 worked because A was held 0.88 s (a 0.40 s hold). Cause: `bindings/bail.rs` holds OPEN_PARACHUTE only while the button stays down, so the bail needs about 0.9 s in total, not 0.5. Keeping the hold for the game's hold time once latched would fix it.
3. 75 kg door carries tear from twist, not snag. At 307.96 the right hand's lag was 0.05 m while the left grab point sat 0.79 m below its target (lag +y), marked liftable and shared. That reads as the door pivoting about the right hand, and the strain judged it a tear (`grasp.rs:150-177`, break 0.6 m / 0.4 s). The second door shows the same pattern (right hand +y 0.79 m at 348.6). The two-hand lift assist makes it count as liftable (`hold.rs:96-123`), so sag and pivot count as strain, while the per-hand torque (30 Nm) cannot hold the door level.
4. A snatched prop is lost on arrival. 356.4: a 10 kg OBJECT came into the left hold at 11.2 m/s and was torn away 0.07 s later, likely past the 1.0 m tear distance.
5. A gesture wingsuit click is ignored early in a jump: 373.58, 1927.29, 1927.84, and 2958.83. The 0.5 s retry rescued the first two cases. At 2958 the gesture went idle before retrying and no suit opened.
6. Re-tapping B during a reel makes a 0.1 s lock with its brake and lock haptic: 7 times, five of them at 2786-2792. Possibly felt as a hitch on chained zips (`bindings/grapple_button.rs:248-266`).

Suspected:

- Snatch misses rose to 4 of 21 weapon snatches, from 1 of 23 on 6 Nov. Not attributed to a hand.
- Three deaths (one 5 s after own C4, one 4 s after a parachute landing). Heat was locked at 5.

## Before/after against 6 Nov

- Grapple locks that held: 0 of 2 → 4 of 11.
- Heavy carries kept: 0 → 1 of 3 torn doors kept (the jeep door, 13 s).
- Right-hand weapon catches: almost none → 8.
- Wheel or bar drift releases: many → 0 of 11.
- Canopy cut to suit: 2.3 s → 0.76 s.
- Cliff jump opening: never → 2 of 4 jumps at 1x on the first click, 1 on a retry, and 1 failed (2958). The helicopter drop (1927, at 0.5x) opened on the third click.
- Take-off skew: 15-20° → 9° / 3° / under 1°.
