# Guns, holsters, reloading, inventory, C4, HUD: 10 November 17:07 take

Times are Rerun t (log UTC minus 16:08:16.047). Where I looked at frames, video time is t − 1.0. Frame files are in this folder.

## Previous-feedback items

### Holsters 1.5x larger: partly
The wider radii are live (0.375 m hip and 0.675 m shoulder attraction; draws in at 0.188 to 0.375 m, e.g. t 15.07, 247.56, 993.91). There are 18 `hold: docked` lines plus one dock at t 412.34 that has only a loadout line, so 19 docks. I sorted the 21 releases using the wrist's model-space position against the wrist positions at the docks, the socket haptics, the release speed, and frame strips (`rel_<t>.png`):
- Missed holsters (5): t 263.65 (L hip; both hands holstered pistols together, the right docked at 263.70 and the left fell; wrist about 0.17 m from the hip, 0.03 m/s, attraction tick at 263.27; frames show both hands lowering to the hips and a pistol on the ground after). t 417.19 (R hip, empty; the hand turns the pistol down onto the hip with the yellow ring showing, and it drops at the feet; wrist 0.35 to 0.54 m, so probably in front of the hip or past the 0.375 m edge). t 1576.91 (R hip, but already holding pistol #1; the socket marker is red in the frames, and hips never swap). t 2266.19 and 2273.17 (R shoulder already holding #47; frames show the gun lifted over the right shoulder with a "Right back" hint; wrist 0.05 to 0.10 m from the socket).
- Lost in a stow with both hands (1): t 2252.80. Both hands stowed long guns, both were attracted to the right shoulder, the right hand's gun swapped in (`socket swap`), and the left hand's gun fell.
- Deliberate drops or throws (12): t 306.77/306.84 (both opened in front, the start of the ground-snatch test), 312.34 and 319.48 (inside the drop and snatch test run), 1606.39 (4.3 m/s), 1649.68 (9.6), 1717.47/1717.50 (the start of the clip), 1896.61, 1934.42, 2099.02, 2260.37, and 2276.53 (it frees the right shoulder: #2 docks at 2281.70).
- Fumbles or unclear (3): t 9.32 (the right pistol rolls out during a cross-body move), and t 1660.27 (a revolver taken off the locker falls 0.24 s later, then is re-grabbed at 0.257 m).

Cause of the two shoulder misses: a taken back socket swaps only through the attraction path, and `attract()` throws out any release faster than 2.5 m/s over 0.1 s (`docks/attract.rs`, `ATTRACTION.max_speed_mps`). The hands let go at 3.9 and 3.6 m/s, a natural toss over the shoulder, so the releases counted as throws. Socket reach (`over`) can't help either, because a taken socket doesn't fit (`finish.rs:794-818`). At t 263.65 a left `draw, holster, or foregrip` click (VolumeGrab, `finish.rs:882`) fired 80 ms before the gun was dropped by the ArmedInHand route. The same click-then-drop pattern shows at 312.34 and 2252.80. A dock that fails after it starts is logged only at debug level (`finish.rs:885`), so the cause is unconfirmed; I suspect the simultaneous right-hand dock reconciling the shared dual-wield slot.

### Grapple-snatching weapons off the ground: fixed
There were 18 ground-weapon yanks. 17 caught and came into the hand, at 0.84 to 15.5 m, catch 0.07 to 0.42 m. One failed: at t 2095.27 a 25 kg weapon was caught, then `the table did not take the weapon into the hand`, with a `snatch refused` buzz. In the frames the weapon flies in and the hand ends up empty. `video-grapplesnatch.mp4` is t 1720.0–1735.0 (video 28:39–28:54), matched to the box-flight frame. It shows a 40 kg crate snatched with `OBJECT` (t 1721.1–1722.2), then pistol #1 off the ground into the left hand (armed 1723.22, caught 1723.66, `gs_a.png`), a handover to the right (1725.66), and pistol #4 pulled from 7.9 m into the left hand (1728.60 → 1729.22, `gs_b.png`), with the "5/32" counter on it. Every snatch in the clip works.

### Left hand drawing from the back: fixed
At t 183.06 the left grip took `Socket(Shoulder(Left))` at 0.181 m and drew the launcher (#3). Frames (`backdraw.png`) show the "Left back" hint and the launcher coming over the left shoulder. This is the only off-hand back draw. The cross-body stow at 247.83 also worked.

### Snapping from too far (cap 0.20 m): fixed for handles and foregrips; world takes uncapped
Handle handovers: t 10.88 (0.138 m), 1646.86 (0.091), 1725.58 (0.104). Foregrips: 0.015 to 0.195 m (t 187.53 at 0.195). No handle or foregrip grab was refused. The slide is too small to see in frames (`handover.png`). World-weapon takes are not capped: t 1572.02 (0.194), 1660.51 (0.257), 1922.38 (0.189), and 2135.83 at 0.328 m (out of an NPC's hands; frames show a disarm, but whether it snapped can't be seen). Snatch arrivals can effectively snap: the slide time follows the angle, not the distance, so 0.28 m was closed in 0.018 s at t 1729.22, 0.12 m in 0.026 s at 2209.39, and 0.06 m in 0.011 s at 2278.96.

### Feedback on a loaded well: fixed (the click is not recorded)
The haptic events have 5 insert refusals (`pouch refused` from `reload/steps.rs:744`): t 209.10, 215.01, and 241.74 (launcher #3, well occupied after a load), then 2145.53 and 2166.66 (the UP-39 reloads, each 0.5 to 0.6 s before the knock-out). Frames show the red ring at the well: t 2145.4/2145.6 (`ring1/2.png`, `ringcrop.png`), 2165.6 (`ring3.png`), 209.0, and 241.6 (launcher at 1/5). The third reload (2179) knocked the old magazine out before reaching the capture radius, so it had no refusal. The dry-fire click is a sound, which the take doesn't record. The preliminary README says no refusal was recorded; it is in the haptics. UX note: every knock-out reload now plays the "refused" buzz on its way in.

### C4 button and hint hidden with nothing placed: fixed (button); hint not checked
All three `detonator near` ticks (t 644.32, 1908.54, 2063.88) came with a charge placed. Frames at t 649.7–651.9 (`c4s.png`, `c4b.png`) show the lit button and ring until the detonation at 650.65, then both gone with the hand still raised. With a charge out, the button shows (`c4_644.3.png`, `c4_2063.9.png`). One suspected leftover: a thin white bar by the right index fingertip stays after the button clears (t 651.6–651.9). It doesn't look like part of the button, and I didn't find its source. I didn't check the summary's detonate hint in frames.

## Other diagnostics asked about

- "Magazine pouch: not placed, the skeleton was not read" (t 10.37, 193.37, 411.83, 1657.32, 1875.48, 2145.29). This is reported when the per-tick pouch state changes into NoBody (`reload/mod.rs:302`), not on a press. Only t 10.37 coincided with a press, and that left hand held a pistol, so the pouch was unavailable anyway; at 2145.29 the hand already held a magazine. No take was lost.
- The seven "the animation forbids it" refusals. Six are at t 419.22–419.38 (the preliminary's 6:58). The left hand opened on its pistol over the left hip. The dock refused every tick for 160 ms, the gun stayed in the hand (StowFallback::Keep), and at 419.42 the hip drew it in from 0.188 m (docked 419.57). There was no grip press, so these were stow retries, not draw attempts. It probably followed the right hand's draw at 415.98 and drop at 417.19. The log line (`hold/mod.rs:444`, called from `hold/act.rs:155/199`) names neither the hand nor the socket. The seventh, t 472.31, came in a grapple sequence with no gun event.
- Ammo counters: text only, no box, sitting on the gun ("6/45" over the pistols at t 262, "5/32" in the clip, "0/5" and "1/5" on the launcher, red at zero). I didn't measure any lag.
- Weapon hints on loading screens: not exercised. The only load was the spawn reset at t −28; the tracking gaps are jet flight, not loads.
- Loadout editor publishing live: not exercised. The debug panel was open only from t −3.1 to 0.2, and nothing was given.

## Other anomalies

Confirmed:
1. Revolver reload does nothing. With two locker revolvers at 0/32 (`rvc.png`), RELOAD was injected and applied six times (t 1678.04–1691.80), along with dry fire and belt pushes until 1703, and there was no eject, belt seat, or load. The player dropped both at 1717.5. Pistol reloads before and after this work. Cause unknown; the reload status events that would say why aren't logged.
2. Over-shoulder swap needs a slow hand (see holsters).
3. A grapple-snatched placed C4 charge was refused. At t 428.76 the charge was caught, then `c4 pouch outcome=refused: planting is not allowed now`, with a pouch buzz. In the frames (`c4snatch.png`) the hand ends up empty and the charge stays placed: the detonation at 450.0 had charges=1.
4. A heavy weapon snatch caught the gun and then failed (t 2095.27, above).
5. A 1.5 m/s socket swap at t 2252.72 dropped the shoulder's gun (#2) as designed, while the left hand's gun in the same stow fell too.

Suspected:
- The snatch slide follows the angle rather than the distance (above).
- At t 1660.58 the loadout says "taken off the ground" while the hold line says `source="taken out of a locker"`.
- The white bar near the fingertip after a detonation.

## Errors in the preliminary README

- Its times are 1.0 s early (6:58 is t 419.2; 4:07.8 is t 247.8, and so on).
- It says 19 docks and 18 draws, and there are 18 of each in `hold:` lines; the 19th dock (t 412.34) has only a loadout line.
- It says no insert refusal was recorded; there are 5 `pouch refused` haptics, and red rings in the frames.
- It counts 19 ground yanks, 18 fired, and every one caught. Of the 18 fired, 17 caught and 1 failed at the table (2095.27).
- It says the pouch diagnostic marks six grip presses that took nothing. It's a per-tick state transition, and no press lost anything.
- It says the refusal burst was draw attempts. They were dock retries on a release.
- It says the only gun take was within the cap. There were three handle handovers, all within it, and world takes at up to 0.328 m, which the cap doesn't cover.
