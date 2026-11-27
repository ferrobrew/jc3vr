# Trailer tooling

The directing tools stage footage from inside the headset: time control, actor safety, heat control, teleport bookmarks, a spawner, the clean view, take markers, and a keymap that drives them without opening the debug panel. They live in `crates/jc3vr-tooling/src/director/` and in the debug UI's Director feature; recording the footage is [capture.md](capture.md), and time of day and weather are [environment.md](environment.md).

Everything is applied from the director's `update`, which runs on the game's main thread at the top of the frame, so the engine setters that assert the main thread (the heat manager, the teleports) are always called from the right place. The debug UI and the hotkeys only edit the `director` settings or queue a request; the per-frame pass reconciles the live game against them, which is what makes them survive the game's own resets (a respawn clearing god mode, a mission reset unlocking heat). When a switch turns off, the pass restores the game's default once and then leaves the game's own state alone, so a sequence that sets god mode or a mission that sets a heat cap is not fought.

## Time

A time-scale slider, a slow-motion toggle, and a pause. The rate is written to the slow-motion manager's normal game speed ([`SlowMotionManager_NormalGameSpeed`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/slow_motion::SlowMotionManager_NormalGameSpeed)) rather than the clock's update speed, which that manager rewrites every frame, so the game's scripted slow-motion effects keep blending against the directed rate. The pause holds the engine clock: the world freezes while rendering continues. Neither the rate nor the pause is saved.

## Actor safety and heat

God mode and unlimited ammo go through the character's own setters ([`Character::SetGodMode`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetGodMode) and [`Character::SetUnlimitedAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-SetUnlimitedAmmo)), re-asserted every frame because a respawn recreates the character with them clear. Unlimited tethers and vehicle invulnerability are not covered. Heat can be set, cleared, locked, and capped through the [`HeatManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/heat_manager::HeatManager); the lock and the cap are re-asserted every frame because the manager's own reset clears them.

## Teleport bookmarks

A bookmark saves the character's full world matrix under a name to `teleports.toml` in the configuration directory ([settings.md](settings.md)), so the facing comes back too and a shot's framing survives a relaunch; a coordinate entry jumps to a typed position. The jump goes through the game world's teleports rather than a transform write, since the engine's teleport also resets the animation state and the camera and fires the events the HUD and streaming listen for. Within `director.instant_teleport_range_m` of the tooling's tuning ([tuning.md](tuning.md)) it uses the instant teleport ([`GameWorld::TeleportPlayerInstant`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayerInstant)), which needs the destination resident; beyond it, the streamed one ([`GameWorld::TeleportPlayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game_world::GameWorld##vfunc-TeleportPlayer)).

## Spawner

The spawner takes a tag set, a distance along the view, and whether the spawned object faces the player. It lists the shipped spawn definition tables (extracted by `tools/spawn_defs/extract_spawn_defs.py` and embedded) by pool with a text filter, and its presets are the full tag sets of single definitions. The spawner and its table are `jc3vr_weapons::controllers::spawn`, since the inventory's gives use the table too; the director queues requests and services them. Fuel tanks are world geometry rather than spawn definitions, and DLC content such as the mech is not in the shipped tables.

## Clean view and markers

F4 toggles the clean view at the scope picked in the Director feature: the desktop debug overlay alone, or everything the mod draws ([hud.md](hud.md)). Take markers are described with the session recording in [capture.md](capture.md#session-recording).

## Keymap

The keymap is `director.keymap` in the settings file, fed from the window procedure so it respects focus and key repeat like the built-in function keys. The defaults sit on the numeric keypad, which nothing else uses and which is findable by touch under a headset; the Director feature lists the effective bindings and can reset them. An action a saved map does not bind takes its default chord unless the map binds that chord to something else. Controller chords are not bound: the in-headset debug panel covers the controller-only case.
