# Trailer tooling

Tools for directing and capturing a first-person trailer from inside the headset. Two groups: directing tools in the debug menu (spawn, teleport, heat, time, cinematic mode), and an in-process capture path that records one eye at full resolution with audio. This doc is the working todo list for that work; each item becomes normal mod code and docs as it lands, and the item here is ticked off.

## Decisions

- Capture records the raw eye frame, uncropped, at the render resolution. Framing, FOV, and the vertical crop happen in post; recording the whole eye keeps that latitude. The per-frame HMD pose and projection go to a sidecar so post can also do a rotation-only reprojection stabilisation, which is a homography against ground truth rather than an optical-flow estimate.
- Capture is in-process. OBS against the desktop mirror is assumed not to work well enough, so it is not the fallback. The payload reads the eye back, converts it on the GPU, and streams raw frames to a host-side ffmpeg, which encodes, segments into 10-minute chunks, and muxes audio.
- Frames are recorded at the source rate (the Bigscreen Beyond at 72 Hz), with a timestamp per frame. Any temporal rescaling is decided later in post, not at capture time.
- Audio comes from the host side first (the game's pipewire stream), synced by a shared start marker. An in-process FMOD capture DSP is the fallback if drift or sync turns out unacceptable.
- No third smoothed spectator camera. The stabilisation sidecar covers the same need at no GPU cost.

## Todo

### Directing tools without RE

- [x] Time control in the debug menu: a time-scale slider and a pause toggle, with hotkeys. The rate goes through the slow-motion manager's normal game speed rather than the clock field, which that manager rewrites every frame.
- [x] Spawner: a tag field, presets (car, bike, boat, helicopter, jet, tank, soldier, barrel), a distance slider, and a "face the player" toggle. Fuel tanks are world geometry, not spawn definitions, so there is no preset for them; the mech is DLC content the shipped tables do not list.
- [x] Spawn definition picker: the shipped `settings/spawn_*_defs.bin` tables, extracted with `tools/spawn_defs/extract_spawn_defs.py` and embedded, listed by pool with a text filter. A definition's full tag set selects it outright.
- [x] Cinematic mode: one toggle that hides the HUD quad, the floating panel, and the overlays together, with a hotkey. With the HUD redirect off (flatscreen) the game's own HUD stays.
- [x] Take markers: a hotkey and a labelled button that append a line with the wallclock and the game-run time to `markers.jsonl` in the session folder. The capture path is to fold these into its sidecar.
- [x] Bindable keymap: `director.keymap` in the settings file ([settings.md](settings.md)), defaulting to the numeric keypad, fed from the window procedure. Controller chords are not bound yet; the in-headset panel covers that case.

### Directing tools that need RE

- [x] Teleport with bookmarks: the full world matrix saved under a name to `teleports.toml` in the configuration directory ([settings.md](settings.md)), jumped to through the game world's instant teleport ([`GameWorld::TeleportPlayerInstant`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayerInstant)) within 400 m (the mod's `INSTANT_RANGE_M`) and its streamed teleport ([`GameWorld::TeleportPlayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayer)) beyond, plus a coordinate entry.
- [x] Heat: set, clear, lock, and cap through the heat manager, with the lock and cap re-asserted every frame.
- [x] Actor safety: god mode and unlimited ammo through the character's own setters, re-asserted every frame. Unlimited tethers and vehicle damage are not covered; the tether count is an upgrade-system value and vehicle invulnerability lives on the vehicle's damageable, both still to map.

### Capture

- [x] Frame source: the chosen eye's capture packed on the GPU and copied into a ring of three staging textures, the oldest mapped with the no-wait flag two frames later, so the immediate context never stalls. A refused map repeats the previous frame.
- [x] GPU conversion: `i420_pack_ps` writes planar 4:2:0 YUV (BT.709, limited range) into one `R8` target whose rows are the frame's exact byte layout, compiled with `shadergen`.
- [x] Transport: a loopback TCP connection from a writer thread to the host listener, carrying `yuv4mpegpipe` (self-describing, so the host needs no dimensions). The per-frame index, wallclock, and game time go to the sidecar rather than into the stream, which keeps the stream a plain ffmpeg input.
- [x] Host script: `scripts/capture_host.sh` listens, encodes with the first hardware H.264 encoder ffmpeg reports, segments at ten minutes, and records the default sink's monitor with wall-clock timestamps on both inputs. Untested against a live stream so far.
- [x] Pose sidecar: `frames.jsonl` with the stream frame, the raw located pose and field of view of both eyes, the predicted display time, the wallclock, and the game time, plus `stream.json` with the stream parameters.
- [x] Debug menu and hotkey: the Director tab's Recording section (start, stop, eye, scale, rate, host, and the sent, repeated, and dropped counts) and a hotkey. The indicator is the tab itself; a headset-visible indicator outside the panel is still to do.
- [x] Post tool: `tools/capture/reframe.py` crops to a chosen field of view about the optical axis and stabilises the rotation from the sidecar by a per-frame homography (numpy and OpenCV). The projection math is checked; the tool has not yet run on real footage.
- [ ] Audio fallback, only if host-side audio fails: an FMOD capture DSP on the master channel group streaming PCM with the video's timestamps. FMOD Studio is the game's audio engine (`fmod_studio_F.dll`).

## Shot list this serves

From the draft script, the tools each shot needs:

- Cliff at golden hour, twice: time of day, weather pinned sunny, teleport bookmark, cinematic mode.
- Coast road convertible, bike, boat, tank: spawner, heat lock at zero.
- Wingsuit into chute into grapple onto a moving car: spawner for the car, actor safety, slow motion for the landing.
- Base assault, fuel tanks, chopper, sniper tower, gravity gun: spawner (fuel tanks, mech), heat lock, slow motion, actor safety.
- Jet barrel roll and wing walk: spawner, actor safety.
- UI beat: cinematic mode off for two seconds.
