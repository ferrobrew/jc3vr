# Pre-analysis of the 7 November 2026 session

Take: `sessions/2026-11-07_01-58-54_932/recordings/2026-11-07_01-59-02_529/` in the jc3vr repo (52 minutes, Index on Monado, build 0afbe301). Recording seconds = log UTC minus 00:59:02.529; the video runs about 1.0 s behind the recording. `take.rrd` was exported automatically at the take's end (5 minutes). Comparisons are against the 6 November take (`sessions/2026-11-06_04-34-15_069/`), whose pre-analysis lived in the session scratchpad.

Files in this folder:

- `body.md`: the body, arms, camera, swimming, and flight body (full area report).
- `guns.md`: guns, reloading, inventory, holds, C4, and the HUD (full area report).
- `world.md`: the world, vehicles, grapple, and flight (full area report).
- `timeline.txt`: what the player did, with recording and video times.
- `log_annot.txt`: the session log with recording and video times on each line.

The light event dump and the full JSON dump are in the session scratchpad (`take1107/`) and can be regenerated with `tracking-synth dump --json`.

## What the fixes changed (6 November to 7 November)

| Problem | 6 November | 7 November |
|---|---|---|
| Gun releases with the hand still closed (Index force sag) | 18 of 25 | 0 of 22 |
| Dry-fire clicks per dry pull | 92 for 40 | 38 for 38 |
| "Held gun turned in its hand" warnings | 20 | 0 |
| Steady still spells ending more than 15 degrees off the head | 14 of 64 | 4 of 90 (median settle 2.4 s) |
| Leg reversals while walking | 7.4 per minute | 2.7 per minute (small pairs 36 to 4) |
| Idle hips and chest off the gaze | 24 and 18 degrees | 2.5 and 2 degrees |
| Camera-to-body gap (drift) | 8.3 cm | 5.9 cm |
| Drawn hand from the real hand, as the eyes see it, standing | about 19 cm | about 13 cm |
| HumanIK elbow flips, on foot and under canopy | 13.7 and 57 per minute | 9.7 and 30 per minute (the drawn rule-B elbow is not recorded; offline it predicts about 4 and 10) |
| Rico's wrist swing while walking with a rifle | 9.5 to 12.6 cm/s | 8.5 cm/s (real hands 3 cm/s) |
| Swim stroke speed | 0.01 to 0.2 m/s | surges to 2 to 4 m/s, glides to a stop |
| Grapple locks that held | 0 | 4 of 11 (a 28.5 s swing, 10 s on a vehicle) |
| Wingsuit open after a canopy cut | 2.3 s | 0.76 s; jumps 0.5 to 0.94 s |
| Take-off heading off the gaze | 15 to 20 degrees | 9 degrees once, within 3 degrees twice |
| Wheel and bars losses from drifting hands | 9 of 21 losses | none (all 11 releases followed the hand opening) |
| Frame rate | 49 fps | 40 fps (regression; see below) |

Also confirmed working: no cocking (every seat fired without a rack); the seat thud and double pulse; the per-side belt push with two guns (13 seats, none while holstering, no fire ever held at the belt); knocked magazines at 0.6 to 2.5 m/s; the UPU-210 (5 seats) and Fire Leach (4); caught-weapon labels; the ammo counter over the gun; icon hints with socket labels; the detonator double-press guard (4 buzzes); placed C4 taken back by snatch (5); drawing and firing in a car's driver seat; the settings migration (v0 copy kept, sparse rewrite, 3 warn lines); the human hull active (106 bodies narrowed, `narrowed=true`); the seat grapple tethering from a supercar (4); the snatch on a placed crate and 8 right-hand weapon catches; the torso held procedural through all 120 jumps and canopy toggles; the wingsuit and parachute arms not bending on a glance; drawing a gun mid-fall then opening the suit; both parachute handles grabbed together.

## Open problems, ranked

1. Frame rate fell from 49 to 40 fps across the whole take, with six frames of 100 ms or more (worst 162 ms at 2003.7 s), all in the debug UI and menus between 1973 and 2311 s. The game's own work slowed by the same 30 to 60 percent, which points at scene or machine load rather than one mod system; the only new mod cost found is `hand_bodies` at 0.35 ms. The human hull, now running for the first time, deserves a direct measurement. (body.md)
2. The Hydra's pod usually does not catch the well: 148 to 202 s (three pouch takes) and 605 to 636 s (four) never caught; 957.8 s worked after 8 s. The video shows the pod held at the rear of the housing inside the gold rings. Likely the launcher catch tolerance or the Hydra's well position and axis in `tools/sights/wells.json`. All eight "the magazine is out" diagnostics are Hydra dry pulls. (guns.md)
3. Torn-off doors cannot be snatched: all nine tries at a door on the ground (311 to 342 s) ended "the hook missed"; the game's line never attaches. (world.md)
4. The 75 kg roadster doors still drop 1.5 to 3.5 s after the tear (308, 348, 350 s): the door pivots about one hand, the other grab point ends 0.8 m below its hand, and that reads as a tear. The 41 kg armoured-jeep door held. A snatched 10 kg prop arriving at 11 m/s was torn away 0.07 s later (356.4 s). (world.md)
5. The long-A bail needs about 0.9 s of holding, not 0.5: at 2754 s a 0.72 s hold gave 0.23 s of parachute input, and the game neither bailed nor exited. `controllers/bindings/bail.rs` holds the parachute input only while the button is down, and the game wants it held 0.3 s past the long-press point. (world.md)
6. Inventory edits made while the debug panel is open queue up and all apply when it closes: at 695.88 s three gives landed for one socket at once, two were refused, and stray launchers were left in the world; at 2345 s one socket was cleared twice. `controllers/weapons/inventory/restore/give.rs`: the queue (around line 87) never merges repeat changes for one socket, and every give for a socket shares one tag (around line 360). (guns.md)
7. A holster refused with "the animation forbids it" leaves the gun in an open hand (1258.44, 1496.96 s); after the second the gun was regripped and then dropped at 1499.4 s. (guns.md)
8. Hands while crouched sit 24 to 43 cm from the real ones (12 cm standing); just as bad on 6 November. Under canopy 41 cm, in the wingsuit 56 cm, in vehicles 22 cm, partly by design (handles, wheel). (body.md)
9. The export's remaining 13 cm wrist error is flat across reach and mostly sideways, so it is not the reach clamp, and the eye registration is fixed (the published head sits 0.1 cm from the rendered camera). Suspected: the chest-based shoulder in the arm remap (2a4cd377). (body.md)
10. Looking down between 55 and 70 degrees (where most holster glances land) still lets a fast glance turn the legs (for example 3031.5 s). (body.md)
11. An automatic recenter at 2200.1 s set the height baseline 16 cm below the standing head until the manual recenter at 2422.7 s. The player recentered five times mid-session (1164.8, 2422.7, 2973.6, 2976.8, 3048.6 s), against once on 6 November. (body.md)
12. Stopping a walk now triggers the game's turn-in-place on 35 percent of stops (27 percent before); the heading turns a median 18 degrees at a stop. (body.md)
13. Wingsuit gesture opens early in a jump were sometimes ignored; the retry recovered most, but the jump at 2958 s never opened. (world.md)
14. Re-tapping the grapple during a reel produces a 0.1 s lock seven times (five at 2786 to 2792 s), which may feel like a hitch. (world.md)
15. The rule-B elbow is predicted offline to flip more than HumanIK in the wingsuit (22 against 15 per minute). (body.md)
16. Suspected: missed snatch catches rose to 4 of 21 (1 of 23 before); in the car the pistol was passed to the left hand and holstered on the right hip within 50 ms three times (2518.9, 2527.4, 2531.6 s); a gun holstered and redrawn in one tick (1623.49 s); guns drawn on the grapple got their physics bodies only on landing, 17 s later.

## Notes

- The weapon wheel is still on in the rewritten settings file (`enabled = true`). Since 02d4c140 the migration no longer drops switches, so the old "on" stayed; the player intends it to be off, which is a one-line change in the settings file or the debug UI.
- The game hit an access violation in NVIDIA's water library (`cudart64_55.dll` under `gfsdk_waveworks`) 14 s after the recording stopped; the log ends there with no shutdown or uninject line. The same first-chance exception appeared at the end of earlier sessions.
- Not exercised this session: the single-gun centred pouch; the Negotiator, shotguns, revolvers, Automat 12, and Splitter; the golden gun give; the stand-down resume with a gun in hand; the passenger seat, motorcycle, and reloading while seated (unlimited ammunition was on from 1652 s); taking placed C4 back by hand; the nitrous button; the plain long-A exit below bail speed; picking a door up off the ground by hand; swimming underwater.
- The player's clip `video-explosion-to-remember.mp4` is video 50:53 to 51:24 (recording 3054 to 3085 s): Rico shoots the pumps at a gas station with a pistol and an SMG, it explodes at about recording 3076 s, and the canopy comes down on him.
