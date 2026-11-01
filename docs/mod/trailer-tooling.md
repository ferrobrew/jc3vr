# Trailer tooling

Tools for directing and capturing a first-person trailer from inside the headset. Two groups: directing tools in the debug menu (spawn, teleport, heat, time, the clean view), and a capture path that records the desktop mirror window with audio on the host. This doc is the working todo list for that work; each item becomes normal mod code and docs as it lands, and the item here is ticked off.

## Decisions

- Capture records the desktop mirror window from the screen, on the host, with gpu-screen-recorder. An in-process recorder that read the full eye render back from the GPU and streamed it to a host-side ffmpeg was tried and replaced: in real sessions it dropped frames, recorded no sound, and its colour conversion broke, while a screen recording of the window was smooth. The video is what the mirror shows at the window's size, so framing is set by the mirror view (F10) and by the window, and post works within that.
- The per-frame HMD pose and projection, and the mirror's layout, go to a sidecar so post can crop about the optical axis and do a rotation-only reprojection stabilisation, which is a homography against ground truth rather than an optical-flow estimate. The sidecar and the video line up by wall clock.
- The sidecar has a line per rendered frame at the source rate (the Bigscreen Beyond at 72 Hz); the video is recorded at 60 frames per second with a variable frame rate. Any temporal rescaling is decided later in post, not at capture time.
- Audio comes from the host side, the default output, recorded by the same gpu-screen-recorder process as the video. An in-process FMOD capture DSP is the fallback if that turns out unacceptable.
- No third smoothed spectator camera. The stabilisation sidecar covers the same need at no GPU cost.

## Todo

### Directing tools without RE

- [x] Time control in the debug menu: a time-scale slider and a pause toggle, with hotkeys. The rate goes through the slow-motion manager's normal game speed rather than the clock field, which that manager rewrites every frame.
- [x] Spawner: a tag field, presets (car, bike, boat, helicopter, jet, tank, soldier, barrel), a distance slider, and a "face the player" toggle. Fuel tanks are world geometry, not spawn definitions, so there is no preset for them; the mech is DLC content the shipped tables do not list.
- [x] Spawn definition picker: the shipped `settings/spawn_*_defs.bin` tables, extracted with `tools/spawn_defs/extract_spawn_defs.py` and embedded, listed by pool with a text filter. A definition's full tag set selects it outright.
- [x] Clean view: one toggle (F4) that hides the mod's overlays at a scope picked in the Director feature: the desktop debug overlay alone, or everything, the HUD quad, the floating panel, and the in-world overlays included. With the HUD redirect off (flatscreen) the game's own HUD stays.
- [x] Take markers: a hotkey and a labelled button that record the wallclock, the game-run time, and a label. While the video recorder runs they go into its sidecar before the next rendered frame; otherwise they are appended to `markers.jsonl` in the session folder. F2 drops one from the keyboard, and F1 starts a session recording, the video and a tracking recording under one stamp ([capture.md](capture.md#session-recording)).
- [x] Bindable keymap: `director.keymap` in the settings file ([settings.md](settings.md)), defaulting to the numeric keypad, fed from the window procedure. Controller chords are not bound yet; the in-headset panel covers that case.

### Directing tools that need RE

- [x] Teleport with bookmarks: the full world matrix saved under a name to `teleports.toml` in the configuration directory ([settings.md](settings.md)), jumped to through the game world's instant teleport ([`GameWorld::TeleportPlayerInstant`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayerInstant)) within 400 m (the mod's `INSTANT_RANGE_M`) and its streamed teleport ([`GameWorld::TeleportPlayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayer)) beyond, plus a coordinate entry.
- [x] Heat: set, clear, lock, and cap through the heat manager, with the lock and cap re-asserted every frame.
- [x] Actor safety: god mode and unlimited ammo through the character's own setters, re-asserted every frame. Unlimited tethers and vehicle damage are not covered; the tether count is an upgrade-system value and vehicle invulnerability lives on the vehicle's damageable, both still to map.

### Capture

- [x] Host-side recording: `scripts/capture_host.sh` runs a relay (`tools/capture/relay.py`) that serves every take of a session in turn. For each it finds the game window under niri, records the window's region with gpu-screen-recorder and the default output's audio into the take's folder, and writes the first frame's wall clock and the region to `stream.json` ([capture.md](capture.md)). Smoke-tested against a real window with a stand-in client; not yet run against the game.
- [x] Control protocol: a versioned line protocol over TCP from the payload's take thread to the relay, naming the take's folder and the game window, with the host's confirmation, error, and stop replies.
- [x] Pose sidecar: `frames.jsonl` with a line per rendered frame (the raw located pose and field of view of both eyes, the predicted display time, the wallclock, and the game time), a line per mirror layout, and the take markers.
- [x] Debug menu and hotkey: Tools > Video (start, stop, the capture host's address, its state and the recorded region) and a hotkey. The indicator is the tab itself; a headset-visible indicator outside the panel is still to do.
- [x] Post tool: `tools/capture/reframe.py` crops to a chosen field of view about the optical axis and stabilises the rotation from the sidecar by a per-frame homography (numpy and OpenCV), using the mirror layout to place the eye image in the video. Run on a screen recording with a synthetic sidecar; not yet on real footage.
- [ ] Audio fallback, only if host-side audio fails: an FMOD capture DSP on the master channel group streaming PCM with the video's timestamps. FMOD Studio is the game's audio engine (`fmod_studio_F.dll`).

## Shot list this serves

From the draft script, the tools each shot needs:

- Cliff at golden hour, twice: time of day, weather pinned sunny, teleport bookmark, clean view at the everything scope.
- Coast road convertible, bike, boat, tank: spawner, heat lock at zero.
- Wingsuit into chute into grapple onto a moving car: spawner for the car, actor safety, slow motion for the landing.
- Base assault, fuel tanks, chopper, sniper tower, gravity gun: spawner (fuel tanks, mech), heat lock, slow motion, actor safety.
- Jet barrel roll and wing walk: spawner, actor safety.
- UI beat: clean view off for two seconds.
