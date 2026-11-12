# Analysis of the 10 November 2026 17:07 session

Take: `sessions/2026-11-10_17-07-47_621/recordings/2026-11-10_17-08-16_047/`. The session started at 16:07:47.632 UTC and ended at 16:49:21.484 in the known WaveWorks crash (CONTRIBUTING.md), 48.9 s after the recording stopped. Tracking started at 16:08:16.047, the video at 16:08:17.047, and the recording stopped at 16:48:32.572, so the take is 2416.5 s of tracking. It has the video, `tracking.jc3trk`, `frames.jsonl`, `take.rrd` (exported by the capture relay), and `video-grapplesnatch.mp4`, a clip the reviewer saved of t 1720.0 to 1735.0. The build string is `2026-11-10 14:38:46`, which includes the stunt and window-handle changes committed minutes later (88093ab0, feac2409, 808489ff).

Times are the Rerun `recording` timeline's, which is the recording's own clock and starts at the tracking start: a time `t` is the UTC log time minus 16:08:16.047, and the video time is `t` minus 1.0 s. The reviewer's feedback uses the same `t`.

The verdicts below come from the tracking data and video frames, not only from the log: a log line shows that the mod requested or observed an action, not that it looked right. Each area has its full report beside this one, with the measurements and the frames it used:

- [body.md](body.md): the body, the camera, the drawn arms, and the stunt positions.
- [guns.md](guns.md): guns, holsters, reloading, grabs of weapons, C4, and the HUD.
- [world.md](world.md): the debug UI, doors, ragdolls, mounted guns, cockpits, the grapple, flight, vehicles, and frame timing.
- `timeline.txt` and `log_annot.txt` (not committed): what the reviewer did, and the session log with `t` and the video time on each line.

## The previous round's feedback

The items from [the 11:32 feedback](../2026-11-10_11-32/philpax_feedback.md), with the changes [its README](../2026-11-10_11-32/README.md) describes.

### The debug UI started visible: fixed

Nothing opened the panel at startup. Its only records are the controller chord that opened it at t −2.07 and closed it at t 1.23: the reviewer opened it to start the recording from Tools > Video.

### The panel could not be used until the window was refocused: fixed at startup, the background case not exercised

On its first open the panel read "Editing input: effective", and a virtual-pointer click (`menu.pointer_select` on the right trigger, ending at t −0.03) started the recording at t 0.00. The window never lost the foreground before t 2444.8, after the recording, so the virtual cursor never ran with the window in the background; drag and scroll were not used.

### Holsters dropped weapons that should have gone in: partly fixed

The wider sockets are in effect (attraction 0.375 m at the hips, 0.675 m at the shoulders; draws in from 0.188 to 0.375 m). Of the 21 weapons released into the world, 12 were deliberate drops or throws, 3 are unclear, 1 fell in a two-handed stow (t 2252.80), and 5 were holsters that missed:

- t 263.65 (left hip, while the right hand holstered at the same moment) and t 417.19 (right hip, the wrist 0.35 to 0.54 m from the socket, in front of the hip or past the attraction's edge).
- t 1576.91: the right hip already held a pistol. The ring was red; hips never swap.
- t 2266.19 and 2273.17: the right shoulder already held a gun. A taken back socket swaps only through the attraction, and `attract()` treats a release faster than 2.5 m/s as a throw (`controllers/weapons/docks/attract.rs`, `ATTRACTION.max_speed_mps`); the hand let go at 3.9 and 3.6 m/s, a natural toss over the shoulder.

At t 263.65, 312.34, and 2252.80 a dock started (the socket's click) and the gun dropped 80 ms later; a dock that fails after starting logs only at debug level (`controllers/weapons/finish.rs` around 885), so the cause is unconfirmed. A simultaneous dock by the other hand is the common factor.

### Grapple-snatch refused weapons on the ground: fixed

Of 18 ground-weapon yanks, 17 caught and came into the hand, from 0.84 to 15.5 m. One 25 kg weapon was caught and then refused by the table at t 2095.27 ("the table did not take the weapon into the hand"). The saved clip shows a 40 kg crate snatched (t 1721.1 to 1722.2), a pistol off the ground into the left hand (t 1723.22 to 1723.66), a handover to the right, and a second pistol pulled from 7.9 m (t 1728.60 to 1729.22), all working.

### The camera bobbed with the walking animation: not fixed

While jogging and sprinting the camera bobs about 20 mm RMS vertically (60 to 86 mm peak to peak) and 35 mm laterally in the 1 to 4 Hz band, against 0.2 to 2.7 mm on 7 November; the tracked head moves 1 to 2 mm in that band. The bob follows Rico's animated head (correlation 0.93 vertically, 0.97 laterally), from a standstill and in continuous movement alike.

Cause: f05925ce stores the neutral head in `Geometry::camera_head` and uses it for the camera's neck delta and eye arm (`headpose/mod.rs` around 299, `hooks/camera/mod.rs` around 449), but the headset's per-frame pose is composed in `vr/frame.rs` (around 215 and 241) from `pair[1].head` and `pair[0].head`, the animated head, and published every display frame (`publish_pair`, around 294). The neutral head never reaches the displayed pose.

### The left hand could not draw from the back: fixed

The left hand drew the launcher off the left shoulder at t 183.06 (0.181 m, the "Left back" hint showing), and the right hand stowed a gun on the left shoulder at t 247.83.

### A weapon snapped to the hand from too far: fixed for handles and foregrips; world takes are not capped

The three handle handovers (t 10.88, 1646.86, 1725.58) were at 0.09 to 0.14 m and the foregrip grabs at 0.015 to 0.195 m, all inside the 0.20 m cap; none was refused. The cap does not cover weapons taken from the world: t 2135.83 took a gun out of an NPC's hands at 0.328 m, and t 1660.51 at 0.257 m. Snatch arrivals still effectively snap, because the slide's time follows the angle rather than the distance: 0.28 m was closed in 0.018 s at t 1729.22.

### No feedback when inserting into a loaded well: fixed

Five insert refusals appear as `pouch refused` haptics (t 209.10, 215.01, and 241.74 on the launcher; t 2145.53 and 2166.66 on the UP-39, 0.5 to 0.6 s before the knock-out), and the frames show the red ring each time. The dry-fire click is a sound the take does not record. Every knock-out reload now buzzes "refused" on the way in, since the fresh magazine reaches the occupied well before it knocks the old one out.

### The head was detached in the plane stunt position: fixed in steady state, except a deep crouch

The camera stays 111 to 160 mm from the drawn head in steady state in all 11 stunt windows, close to seated (103 mm) and on foot (169 mm); the skeleton itself stays rigid, and frames at t 366, 369, 376, 1962, and 2372 show the body whole under the view. The plane stunt is t 364.6 to 379.3, standing on the parked private plane. A deep crouch in a stunt still drops the view through the body: at t 372.3 to 373.7 the tracked head went down 0.89 m while the stunt legs stayed straight, and the view sat 0.42 to 0.45 m below the drawn head. The last entry's 874 mm error (t 2415.25) is the supercar overturning in a crash with the reviewer's head 1.3 m out of the seat, not a stunt fault.

### The door grab target was too small: fixed

All three latched-door grabs took: the private plane's front-left door at 151 mm (t 332.75), which the old 0.15 m limit would have refused, torn off at t 334.87; a saloon's trunk at 133 mm (t 1144.6), torn off at t 1154.5; a roadster's trunk at 105 mm (t 1206.1).

### The C4 detonate button and hint showed with nothing placed: fixed for the button

The button and its ring show while a charge is placed and vanish at the detonation (t 650.65) with the hand still raised; every detonator approach came with a charge out. A thin white bar by the right index fingertip remains after the button clears (t 651.6 to 651.9), from an unknown source. The controls summary's detonate hint was not checked in frames.

## Earlier feedback this session exercised

- Ragdolls (9 November and 14:18): grabbing an NPC still ragdolls them (t 76.36, 153.26, 2080.27, 2113.50), but every held ragdoll was torn from the hand within 0.08 to 0.35 s (t 76.62, 153.34, 2082.55), so nothing was carried or thrown. The held bone reaches 15 to 60 m/s before the tear. Suspected cause: the ragdoll joint drives the hand with the whole body's mass and an unclamped pull on one bone (`controllers/physical_hands/physics/state.rs` around 437 to 450), until the strain passes the 1.0 m tear distance (`grasp.rs` around 179). Dragging a ragdoll on the snatch line works.
- Mounted guns (9 November and 14:18): the armoured jeep's turret MG was gripped by both handles and fired twice (t 1973.1 to 1977.3), but was never torn off. The grip chose the gun's body as a loose prop over the handle 8 mm farther, and the runtime logged one hand where two held it. No minigun or helicopter appears.
- Cockpit controls (9 November and 14:18): fixed for the fighter (t 719 to 1087). The virtual stick and throttle are drawn over the modelled ones with the throttle's full rail shown, and stick twist drives the rudder (1572 rudder writes with the thumbsticks idle). The helicopter was not flown.
- The grapple swing (7 November and 14:18): the lock now takes at the press and brakes the reel within 0.15 s, across eight locks. The swing's speed is still lost on release: the horizontal speed falls to about zero within 0.4 s of every release (24.9 to 0.1 m/s at t 1264.7). Suspected cause: `grapple/physical/lock/mod.rs` around 631 to 633 prefers the game's wanted velocity for the character, which reads near zero while the lock drives the reel, over the lock's own last velocity.
- The wingsuit from a canopy (9 November and 14:18): partly fixed. The canopy deploy at t 2303.4 opened the suit at t 2303.7. At t 1467.6 the canopy was cut and the suit never opened; the retry was blocked for 2 s as "the left hand grips something" with both grips at zero, and the reviewer fell into trees at t 1471.2. Cause: `controllers/bindings/flight.rs` around 487 to 493 counts the palm's passive surface contact (the fingers' `surface` source, flat palm) as a grip. No deploy from a grapple hang was attempted.
- The drawn arms (9 November): elbow flips per minute on the drawn arms, now recorded each frame, are 3.9 on foot, 7.3 under the canopy, and 23 in the wingsuit (12 flips, a thin sample), matching the offline predictions for the rule-B pole (4, 10, and 22); HumanIK's own elbow in this take flips 5.9, 38.8, and 19.4 times a minute. The upper arm keeps its length and the drawn wrist sits within 1 to 2 mm of the late correction's wrist. The forearm reaches its 1.5x stretch cap in 2 to 4 percent of on-foot frames and is pinned at it for 56 percent of the wingsuit flight on the right arm (t 662 to 675). No break was visible in frames with the arms in view.
- The body (9 November): steady spells of 3 s or more end more than 15 degrees off the head 3 of 45 times; looking down past 55 degrees no longer turns the legs (0 of 30 look-downs, against 11 of 60 on 7 November), though one look-down hold left the legs 80 degrees off the head (t 1573.8 to 1577.7). Leg reversals while moving are 2.9 a minute.
- Camera clipping (9 November): vehicle entries are clean, but at seat lock the view yaw snaps 20 to 24 degrees in a single frame (t 338.90, 358.26, 1940.50; the log's "re-based the cockpit's yaw at the seat lock"). The camera clips into the body for 0.2 to 0.6 s on one of 7 vehicle exits (t 1088.5), three of 10 canopy deploys (t 535.6, 700.1, 2288.2), and two of 4 wingsuit openings (t 581.0, 1286.9).

## New problems

1. Revolvers do not reload. With two revolvers from a locker at 0/32, six reload presses (t 1678.04 to 1691.80), dry fire, and belt pushes until t 1703 produced no eject, no seat, and no load; the reviewer confirmed it. Pistols reloaded before and after. The cause is unknown: the reload's internal steps are not logged.
2. Turret hijacks seat the player instead of removing the gunner (t 1939.2, 1945.4, 1957.2). Each pull clicks the vehicle entry (`controllers/physical_hands/pickup/grips.rs` around 224, `hijack_clicks`), and the game picks the empty driver seat while the gunner stays.
3. After two yank-boosted ground reels (95 to 117 m/s), the on-foot player re-accelerates from about 21 to 63 to 77 m/s half a second after the reel ends (t 1493.98, 2350.09), shooting past a car and up a bank. Cause unknown.
4. A grapple-snatched placed C4 charge was caught and then refused ("planting is not allowed now", t 428.76); the charge stayed placed.
5. A heavy weapon (25 kg) was snatched and caught but not taken into the hand (t 2095.27).
6. Socket swaps at a taken shoulder fail for a natural over-the-shoulder release (above), and a two-handed stow to one shoulder drops the second hand's gun (t 2252.80).

Suspected: a large red and white ring hanging inside the roadster's open trunk (t 1221.5); the white bar by the fingertip after a detonation; a gun taken off a locker logged "taken off the ground" by the loadout and "taken out of a locker" by the hold (t 1660.58).

## Frame hitches

Twenty-eight frames of 0.19 to 0.61 s (22 of 0.27 to 0.33 s, 5 of 0.60 to 0.61 s), each in `sim_steam` inside the sim tick's tail, on a 61 s wall-clock beat with two phases 25.9 and 35.1 s apart. The skip in [periodic-sim-tick-hitches.md](../../issues/periodic-sim-tick-hitches.md) moved the stall from the logged-on query into `SteamAPI_RunCallbacks` unchanged, which points at the native Steam client being busy under Proton, so that whichever Steam call comes first blocks. The next step is to time each dispatched callback and check the Linux Steam client's logs for 61 s periodic work.

## Not exercised

The panel with the window in the background; the loadout editor; weapon hints on a loading screen; the seat grapple and seated tether retraction; the motorcycle; the helicopter; the wingsuit from a grapple hang; any minigun.

## For the next analysis

- The reload, a dock that fails after starting, and the "the animation forbids it" refusal do not log enough to diagnose: the reload's steps are silent, the failed dock logs at debug level, and the refusal names neither the hand nor the socket (`controllers/weapons/hold/mod.rs` around 444). Logging them would let the next take answer the revolver and holster questions directly.
- The presentation's "drawn arm joints" diagnostic carries the presentation's own sequence counter (`controllers/presentation/mod.rs` around 293), not the controller frame's; it lags by 12 to 26 frames, so pair it with the frames by time.
- The "magazine pouch: not placed, the skeleton was not read" diagnostic marks a one-tick state change, not a press that took nothing.
- The reel-end log's `boosts` field is the session total (`grapple/physical/mod.rs` around 261).
