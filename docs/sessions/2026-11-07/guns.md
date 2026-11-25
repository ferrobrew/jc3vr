# Guns, reloading, inventory, holds, C4, HUD: 7 November take

Times are recording seconds, with video mm:ss in brackets (video = recording − 1.0 s, from stream.json `video_start_unix_ms` against the take start). The weapon names come from the loadout hashes mapped through `tools/sights/wells.json`. Work files are in this folder: `log_annot.txt`, `filtered.txt`, `reload_haptics.txt`, `frames_haptics.txt` (trigger, squeeze, and haptics from the jc3trk), and `gr1106_frames.txt`.

## Timeline

- 11-83 (00:10-01:22): two pistols (Pozhar). Dry pulls, ejects, and belt seats on each side's pouch at 55, 57, 66, 67, 72.6, and 73 (00:54-01:12).
- 110-294 (01:49-04:53): U-39 rifle. Hydra at 148-202 (02:27-03:21): shot once, then about 50 s of failed pod insertion and 6 "magazine is out" dry pulls before docking it. U-39 again: a loose magazine picked up and seated (217.96), then 4 knock reloads (238.6, 248.4, 278.4, 289.0).
- 605-636 (10:04-10:35): Hydra again. 2 more "magazine is out" pulls, 4 pouch takes, no seat, docked.
- 645-695 (10:44-11:34): debug panel, inventory edits applied when it closed (Fire Leach, UPU-210, Spectre).
- 710-967 (11:49-16:06): snatched launchers. UPU-210 seated at 754 and 778, Fire Leach 4 times (838-883), UPU-210 3 times (911-932), Hydra once (957.8).
- 971-1134 (16:10-18:53): U-39 knocks at 997 and 1049. Two U-39s (one per hand) with 7 per-side belt seats (1079-1132).
- 1307-1345 (21:47-22:24): Pozhar plus Spectre; left belt seat at 1324.
- 1488-1520: rack Wraith 2000 SMGs. 1504-1510 and 2439-2448: placed C4 snatched back, and once returned to the pouch.
- From 1652 (27:31): unlimited ammo ON, so no reload was needed for the rest of the take.
- 2461-2582 (41:00-43:01): driver's seat of a supercar. Pistol drawn and 65 shots; Wraith drawn left and 31 shots.
- 2345 (39:04): inventory view cleared four sockets.

## Changes in this area

- **No cocking:** exercised, works. There are no rack, detent, chambered, or action-cycled events all session. Every U-39 seat fired afterwards without a rack (for example, the seat at 239.57 and shots from 242). The bolt rifle and the pump were never used.
- **Seat feedback:** exercised, works. All 17 magazine seats and 15 belt seats are a 1.0/70 ms thud, followed by two "ready to fire" bumps at +0.16 and +0.27 s. The sound is logged only at debug level, so it is unverifiable.
- **Belt push, two guns:** exercised, works. There were 13 per-side seats with two pistols or two rifles, and none landed near a holster. No "gun is in the belt pouch" refusal all session. A Wraith fired 1.6 s after a hip draw (2864).
- **Belt push, one gun (centred pouch):** not exercised. No belt seat happened with a single gun.
- **Gold ring:** seen in video around the pouch and well (172.0 [02:51]).
- **Per-weapon checklist:**
  - Used: Pozhar, Spectre, U-39, Wraith 2000, Prizrak (no reload), Fire Leach, UPU-210, and Hydra.
  - Worked: UPU-210 (5 seats) and Fire Leach (4 seats).
  - Hydra: 1 seat in 3 attempts (see anomaly 1).
  - Not used: the Negotiator, any shotgun (no shell, no Automat 12 borrowed magazine), any revolver, and the Splitter.
- **Knocked magazines:** exercised, believable. In 6 knocks the magazine body leaves at 0.6-2.5 m/s, falls freely, and stops on landing within about 0.6 s. The one at 997 left at 5.7 m/s, probably with the player's own motion. The "body not readable" fallback never fired.
- **One dry click and the named reason:** works. 38 dry clicks for 38 trigger press edges, at most one click per pull (6 Nov: 92 for 40). All 8 diagnostics read "the magazine is out", and all are the Hydra's (168-192, 606, 612). Other dry pulls are the game's own empty-magazine click, with no diagnostic.
- **Left-hip pistol and the Docked guns view:** the left-hip draws worked 14 times (socket found at 92-113 mm). Visibility and the green ring are not confirmed from frames.
- **Closed-hand holds (Index):** works. All 22 gun releases happened with the squeeze falling to 0.5 or below. Guns stayed held for long stretches with the force press off and the squeeze at 1.00. See the numbers below.
- **Stand-down resume:** not exercised with a gun held. Both hands were empty at all 6 debug-panel opens and in the pause-menu stretch (2045-2113).
- **Inventory as a live editor:** exercised, and the edits apply. They take effect only when the panel closes, and the queue duplicates requests (anomaly 2).
- **Golden gun give:** not attempted.
- **Caught-weapon label:** works. Caught weapons read "taken off the ground" or "taken from a character's hands". The only "taken off a rack" lines (1488, 1492) were a real rack.
- **Grip watch:** works. 0 "held gun turned" warnings (6 Nov: 20).
- **Ammo counter over the gun:** seen centred over the pistol's slide (54.5 [00:53.5]).
- **Hints:** an icon-only hint with a "Right hip" label was seen (8, 14 s). Two hints at once were not checked.
- **Picking placed C4 back up:** exercised, but only through the grapple snatch. All 5 takes have the snatch line; a direct in-reach take is not evidenced. A snatched charge went back into the pouch at 1509.9.
- **Detonator double-press guard:** works. 4 presses during a detonation (1203.8, 2778.1, 2938.9, 2939.3) gave a buzz and no detonation.
- **Seated guns:** drawing and firing in the driver's seat works. The passenger seat and the motorcycle were not used. Reloading while seated was not exercised (unlimited ammo).

## Anomalies

Confirmed:

1. **The Hydra pod often will not catch.**
   - 148-202 (02:27-03:21): 3 pouch takes over 25 s, no well catch.
   - 605-636 (10:04-10:35): 4 pouch takes, no well catch.
   - 949.7-957.8: one success after 8 s.
   - Frames at 165, 173, and 185 s show the pod held at the rear of the housing inside the rings, with the counter at 0/5.
   - The well is the pod's rear mouth with axis (0,0,-1) and a 0.34 m part (`wells.json` w144). Likely cause: the launcher capture (`reload/insert.rs`, launcher radius and angle) is too strict for a long pod entering from the rear, or the mouth or axis sign is off. It needs a look at the well marker against the frames.
2. **Inventory edits queue up and multiply.**
   - With the panel open, the input tick is not ready, so every dropdown change waits in `QUEUE` (`inventory/restore/give.rs:87`, drained at `:193`).
   - On closing at 695.88, three gives landed for the left shoulder: Fire Leach twice, then the UPU-210. They share one token, `SpawnToken::Give{generation, socket}` (`give.rs:360`). One was carried; the other two were "Refused(NoSocket)" and left lying in the world. The player then snatched those strays at 710 and 725.
   - At 2345.21 the left shoulder was cleared twice: "cleared", then "not cleared: nothing ... on the socket".
   - Likely fix: coalesce requests per target and apply them live.
3. **A refused holster leaves the gun in an open hand.**
   - Twice: 1258.44 [20:57] and 1496.96 [24:56], both "hold: refused: the animation forbids it".
   - At 1496.96 the left gun docked in the same tick, so both hips docked at once. The right Wraith stayed in the open hand until a re-grip (1498.97), then dropped to the ground (1499.40).

Suspected:

4. **Handover then dock in the car seat.** At 2518.89, 2527.35, and 2531.56 the right-hand pistol was "handed over" to the left hand and docked on the right hip from the left hand 30-50 ms later. The end state is right; the path looks like the left hand grabbing it mid-holster.
5. **Dock and redraw in one tick.** At 1623.49 [27:02.5] the Wraith docked on the right hip and was redrawn in the same tick, which suggests the draw latch did not wait for the grip to reopen.
6. **Gun bodies deferred on the grapple.** Guns drawn while on the grapple (1595-1596) got their gun bodies only on landing (1613.97), 17 s later.
7. **Unused pouch takes.** Pouch takes with unlimited ammo on (2426.4, 2630.7) did nothing visible.

## Before and after (6 November → 7 November)

| Measure | 6 Nov | 7 Nov |
|---|---|---|
| Guns released with the hand still closed (squeeze ≥ 0.9 at release) | 18 of 25 | 0 of 22 |
| Dry-fire haptics per trigger pull | 92 for 40 | 38 for 38 |
| "Held gun turned" warnings | 20 | 0 |
| Caught weapons mislabelled locker or rack | 2 | 0 |
