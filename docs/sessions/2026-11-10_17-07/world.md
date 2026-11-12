# World, vehicles, grapple, ragdolls, flight, frame timing: 10 November 17:07 take

Times are Rerun t (log UTC minus 16:08:16.047); video time is t minus 1.0. Frames and contact sheets are in this folder (`frames/`, `sheets_*.png`). `timeline.txt` and `log_annot.txt` are the shared timeline and annotated log.

## Previous feedback in this area

**Debug UI started visible (session-state panel).** Fixed. Nothing opened the panel at startup: its only records are the controller chord opening it at t -2.07 and closing it at t 1.23. At video 00:00.0 it shows on Tools > Video with "Stop session recording", so the player opened it to start the take.

**Panel unusable until the window was refocused (77fa3567).** Partly verified. The initial-state half works. On its first open the panel reads "Mode: Editing … Editing input: effective" (frame v 00:00.0). A pointer click started the recording: the self-check logged `menu.pointer_select` on the right trigger for 14 frames ending at t -0.03, and the recorder started at t 0.00. The background half was not exercised. The window never lost the foreground before t 2444.8, after the recording, so the virtual cursor never ran with the window in the background. Drag and scroll were not used.

**Door grab target doubled (3c96ca19).** Fixed. There were three latched-door grabs, and all succeeded. The private plane's front-left door took at 151 mm at t 332.75, which the old 0.15 m limit would have refused, and was torn off at t 334.87. The saloon trunk took at 133 mm at t 1144.6 and was torn off at t 1154.5. The roadster trunk took at 105 mm at t 1206.1 and swung shut at t 1222.1. No press refused a door within reach.

**Ragdoll grab, carry, throw (d48f2174, 556fb596).** Not fixed: grabbing works, carrying and throwing never happen. Grabbing a standing NPC yanks them into ragdoll at t 76.36, 153.26, 2080.27, and 2113.50. Each of the three ragdolls held was torn from the hand within 0.08 to 0.35 s (t 76.62, 153.34, 2082.55), and there were no ragdoll throws. The `held` reports show the joined bone running away. At t 2082.26 the lag starts at 0.06 m, the load reaches 1.0 within 80 ms, and the bone hits 54 to 60 m/s before the tear. At t 76.4 the bone reaches 15 m/s with a lag of 0.99 m. In the frames the body leaves view within 0.15 s. Suspected cause: the ragdoll joint drives the 3 kg hand with the whole 100 kg body's mass and an unclamped pull (`physics/state.rs:437-450`, `holding_tuning`), applied to one bone. That overdrives the bone until `Strain::torn` passes the 1.0 m tear distance (`grasp.rs:179`). Dragging a ragdoll on the snatch line still works (t 156.5 to 162.4).

**Mounted guns gripped, fired, torn off (939f0210, af243a10, ca67f620).** Partly fixed. The armored jeep's turret MG was gripped by both handles and fired twice (t 1973.1 to 1975.1 and 1975.4 to 1977.3), with muzzle flash at t 1974.4 and 1976.5. It was never torn off; each grip release let it go. The grip chose the gun's body as a loose prop over the handle 8 mm farther, the loadout logged "#41 (a prop) … taken off the ground", and the runtime logged `hands=[true, false]` with both hands on it. No minigun or helicopter appears in the take.

**Cockpit and virtual controls (e5521bb8).** Fixed for planes. In the fighter (t 719 to 1087) the virtual stick and throttle are drawn over the modelled ones, and the throttle shows its full rail (sheets_cockpit1, sheets_cockzoom, t 721.5 and 729). All 1572 rudder writes above 0.2 came while both thumbsticks were under 0.15, so stick twist drives yaw. The helicopter cyclic was not exercised. The private plane was never flown.

**Grapple lock at the press, swing speed carried out (e4b4c797).** Lock fixed, carry-out not fixed. The lock takes at the press and brakes the reel rush from about 30 to 10 m/s within 0.15 s, across eight locks. The swing's speed is lost on release: the camera's horizontal speed falls within 0.4 s of every release (9.8 to 0.2 m/s at t 461.8, 24.9 to 0.1 m/s at t 1264.7), then the player drops straight down.

Suspected cause: `lock/mod.rs:631-633` captures `engine::wanted_velocity(character)`, which reads near zero when the lock drives the reel. It prefers that reading over the lock's own `last_velocity`, so it hands back roughly zero.

**Wingsuit from a canopy or a grapple hang (d21c930d, 02c5e3c7).** Partly fixed. The canopy deploy at t 2303.4 works (suit at t 2303.7). At t 1467.6 the canopy was cut but the suit never opened, and the player fast-fell into trees at t 1471.2 (sheets_ws1467). The retry was blocked from t 1467.8 to 1469.8 as "the left hand grips something" while both squeezes read 0. Fingers had both hands' grip source flicking to `surface` (flat palm) in free fall. Cause: `bindings/flight.rs:487-493` counts any current grip, including the palm's passive `Surface` source, as a gesture claim.

No deploy from a grapple hang. The deploy at t 1286.2 came from a fall after a lock release.

**Seat grapple and seated tether retraction.** Not exercised: none of the 21239 seated samples has a grapple.

**Vehicle entry and exit camera.** Partly fixed. The entry animations follow the head, and no exit moved the camera more than 0.2 m in one frame, apart from the mid-air bail-out at t 1087.5. At seat lock, though, the view yaw snaps in one frame: 22.2 deg at t 338.90, 23.8 at t 358.26, 20.6 at t 1940.50, and 5.6 at t 2380.32. These match the log's "re-based the cockpit's yaw at the seat lock" lines (20.8, 22.8, 20.0, and 5.5 deg). The 338.85 to 338.95 and 1940.45 to 1940.55 frame pairs show the turn (sheets_seat338).

**Motorcycle control.** Not exercised.

**Stunt entries (11).** All are "standing on a vehicle", and the body reads as one piece in the frames I checked:

1. t 352.9 (0.3 s) and t 364.6 (14.6 s): on the parked private plane, the nose and then the wing.
2. t 713.1 (3.5 s): on the fighter in its hangar, before climbing in.
3. t 1936.1 to 1967.0, six windows of 2.7 to 7.3 s: on the armored jeep's roof. Three end in a turret hijack that seats the player in the empty driver seat; in one the player shoots the gunner.
4. t 2368.3 (10.4 s): riding the supercar's roof. The last, at t 2415.3, is the supercar overturning in the crash.

**sim_steam hitches.** Confirmed. frames.jsonl has 28 frame gaps over 150 ms: 22 of 0.27 to 0.33 s, 5 of 0.60 to 0.61 s, and one of 0.19 s. Each matches a frame_timing window whose maximum is `sim_steam`, inside `sim_tail`, with `sim_steam` self time of 4 to 31 ms against about 0.15 ms normally. They keep the old 61.0 s wall-clock beat, two phases 25.9 and 35.1 s apart, with some cycles skipped (t 106.3, 132.1, 167.3, 193.1, …).

The skip remedy in `docs/issues/periodic-sim-tick-hitches.md` removed the beat from `sim_online`'s `BLoggedOn` call, and it reappeared unchanged in `OnlinePlatformSystemSteam::Update` (`SteamAPI_RunCallbacks`). That points at the native Steam client being busy under Proton, so whichever Steam call comes first blocks; skipping more game calls would likely just move the stall. Only keeping Steam calls off the game thread in that window would avoid it, and moving the callback pump is unsafe because the callbacks touch game state. Next: time each dispatched callback, and check the Linux Steam client logs for 61 s periodic work.

**Crash.** This is the known WaveWorks CUDA fault, after the recording ended.

## Other anomalies

Confirmed:

1. **Turret hijacks seat the player instead of removing the gunner** (t 1939.2, 1945.4, 1957.2; sheets_aj1 to aj3). Each pull clicks vehicle entry (`pickup/grips.rs:224`, `hijack_clicks`), and the game picks the empty driver seat.
2. **Re-acceleration on foot after boosted reels.** Yank-boosted ground reels reach 95 to 117 m/s, against about 40 m/s unboosted. After two of them the character, supported and on foot, goes from about 21 back to 63 to 77 m/s 0.5 s after the reel ends (t 1493.98 and 2350.09). At t 2350.15 the player shoots past the car and up the bank (sheets_fast2). Cause unknown.
3. **The reel-end log's `boosts` field is the session total**, not the reel's own count (`grapple/physical/mod.rs:261`).

Suspected:

4. A large red and white tether-style ring hangs inside the roadster's open trunk at t 1221.5 (sheets_ring). Its source is unknown.

## Errors in the preliminary README

- Its t values are video times, 1 s early (the door grab is at t 5:32.7, not 5:31.7).
- The panel was used: its button, clicked with the virtual pointer, started the recording.
- The first two stunt entries did not come while a plane was being flown: the private plane was parked, and the player stood on it.
- The three "hijacks of the vehicle's occupant" failed: the gunner stayed, and the player was seated in the driver seat.
- The 874 mm last entry is the supercar tumbling in a crash.
- There are 28 hitches, not 27, on the 61.0 s two-phase wall-clock beat, not "roughly every 1.5 minutes".
