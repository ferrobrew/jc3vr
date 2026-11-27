# Haptics

Every vibration the mod sends to the controllers is one of a few named primitives, played at a strength within that primitive's range, and every source goes through the same mixer. A tick always feels like a tick and a thud like a thud, whatever raised it, and a hand that several sources reach at once feels the strongest of them clearly rather than a saturated buzz.

The data and the mixer are pure and live in the input crate, `crates/jc3vr-input/src/haptics/`: `vocabulary.rs` (the primitives), `cues.rs` (every source and its category), `mixer.rs` (what each hand plays), `output.rs` (the one shared mixer), and `tuning.rs` (the shapes, the mix, and each source's cue). The tuning file is `crates/jc3vr-view/tuning/haptics.toml` ([tuning.md](../tuning.md)). The sources' entry points are in the body crate's `controllers::haptics` (`crates/jc3vr-body/src/controllers/haptics/`), with `rumble.rs` forwarding the game's own rumble and `shot.rs` marking the shot in progress.

## The vocabulary

Seven primitives (`Primitive`). Five play once (one-off cues) and two follow a level that their source sends every tick it lasts (sustained cues). Each has a soft and a full amplitude, a length, and a frequency hint, in the tuning's `vocabulary` tables. The one-off primitives are listed from the lowest priority to the highest.

| Primitive | Kind | Feels like |
| --- | --- | --- |
| tick | one-off | a hand crossing into a volume, a detent, a selection moving |
| click | one-off | a button, a catch, a round, a release |
| bump | one-off | a grip taken, a hand meeting a surface, a stop reached |
| buzz | one-off | no: a refused grab, a press that does nothing |
| thud | one-off | a shot, a hook taking hold, a body caught, a detonation |
| hum | sustained | the game's rumble, the reel's speed |
| tension | sustained | a pull, a press, a hang |

A cue's strength places it between the primitive's soft and full amplitudes; a source whose cue follows how hard something happened (a palm's speed, a climb's fling, a recoil kick) scales that strength by its share (`play_scaled`). A sustained cue's amplitude is its strength times the level its source sends, so a level of zero is still. The length is how long a one-off cue plays, and for a sustained one how long a level holds without being sent again. A cue whose length is `Long` plays `long_factor` times its primitive's length. The shot plays for the length the game's fire rumble sets, capped at the controls tuning's `rumble.max_shot_s`.

The frequency is a hint (OpenXR's `XrHapticVibration::frequency`), which a runtime may ignore. Every vibration carries its primitive's hint.

A shape is set once in the tuning, so a change to it changes every cue built on it together.

## Sources

Every source is an `Event` (one-off) or a `Sustain` (sustained level) in `cues.rs`, whose doc comments say when each is raised. The tuning's `events` and `sustains` tables give each its primitive, its strength, and for an event its length; every source must be present, and no other key is accepted. A few sources share a cue on purpose and stay separate so that a log or recording names which it was: `MagazineSeat`, `MagazineKnock`, and `BeltSeat`.

Each source belongs to one category (`Category`), fixed in code, which the readout counts its cues under:

| Category | Sources |
| --- | --- |
| Contact | the palms and the hand bodies meeting, pressing, and striking things, taking hold and letting go, a door's latch, the snatch's catch |
| Weapons | shots and kicks, dry fire, the sockets and pouches, the foregrip, the reload's steps, grenades, C4, the weapon wheel |
| Vehicles | the physical controls' stops, detents, and gates, the cockpit buttons, the mounted gun's handles and recoil |
| Traversal | climbing, the stunt positions, swimming strokes, the grapple, the line lock, and the snatch |
| Interface | the controller chords (the recenter and the debug panel) |
| Game | the game's own rumble |

The pressure and tension sources are sustained levels that sum, not short pulses resent every tick.

## The mixer

OpenXR plays one vibration per controller at a time, and a new one replaces whatever is playing. The sources queue their cues from any thread, and the VR input update asks the mixer once per rendered frame what each hand plays (`jc3vr_input::haptics::output::mix`, called from the view crate's input sampling). The mixer holds per hand the one-off cues queued since the last flush, the cue playing and when it ends, and each sustained source's level and when it expires.

- Sustained sources sum. The sum passes a soft limiter: unchanged up to the tuning's `mix.sustain_knee`, then bent smoothly toward `mix.sustain_ceiling`, which sits below full strength. However many sustained sources stack, the hand never reaches full strength from them, and a one-off cue stands out above them. With no cue playing, the hand plays the limited sum for `mix.refresh_s`, refreshed at every flush. A source that sends a level of zero ends at once; one that stops sending ends when its hold runs out.
- One-off cues play at the next flush. The cue of the highest priority (then the strongest) among the frame's arrivals leads and sets the length and the frequency. Each other arrival, and a cue still playing, adds `mix.stack_share` of its amplitude, and the mod's sustained level adds `mix.duck_share` of itself beneath the lead. The total passes the soft limiter at full strength with its knee at `mix.knee`, so a single cue below the knee plays exactly as shaped and a pile of them never clips. The cue plays for the longer of its own length and what remained of the one it cut into.
- The game's rumble is left out of a one-off cue's onset entirely (it ducks under the mod's cues), and every sustained source is held off a hand while a one-off cue plays there, resuming at the first flush after it ends.
- An arrival weaker than the cue playing and of a lower priority is masked: dropped rather than cutting a heavier cue short (a holster tick during a long burst of fire).

The global strength (`intensity`) scales the mixer's output as it is sent, so it scales every cue the same way and never changes the mix.

## Settings

`controllers.haptics` (`HapticsConfig`, in the view crate's `vr::input::config`; [settings.md](../settings.md)) has `enabled`, `intensity`, and `game_rumble`, all on or full by default, which the pause menu's `VR: CONTROLS` page shows as `HAPTICS`, `HAPTIC STRENGTH`, and `GAME RUMBLE`. A cue plays only while the controllers are on, the haptics are on, and the controller scheme is in effect, so a gamepad player gets the gamepad's own rumble. Every category of the mod's own cues always plays; the game's rumble, including the shot and the recoil kick, also needs `game_rumble`.

## The game's rumble

The game's rumble reaches the controllers through two pre-call detours (`rumble.rs`), so a vehicle's continuous engine and road rumble is covered as well as every one-off effect:

- [`RumbleManager::SpawnRumble`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/rumble_manager::RumbleManager##func-SpawnRumble), which every one-off rumble goes through. Inside a shot of a gun held in a hand, the weapon's own fire rumble plays on that hand as `Event::Shot`, at the game's strength raised by the shot's recoil kick. The weapon handling marks the shot around the weapon's fire call (`shot::begin` and `shot::end`) so the detour knows the firing hand. A shot of a weapon with no fire rumble plays `Event::Kick` instead, scaled by the kick. The detour always lets the game's call through, since the effect it starts still feeds the game's per-frame sum below.
- [`Gamepad::SetMotorValue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::gamepad::Gamepad##func-SetMotorValue), which sees that sum: the manager's per-frame, decayed, and distance-attenuated motor strength covering every active rumble, including a vehicle's own rumble, which never goes through `SpawnRumble`. The low motor plays on the left hand and the high motor on the right as `Sustain::GameRumble`. The detour forwards every motor value, zero included, so the rumble stops on the controllers the frame it stops on the pad, and it passes zero through to the gamepad so the physical pad stays still.

## Threading

`haptics::play`, `play_hands` (one cue on several hands), `play_scaled`, and `sustain` are called from the input tick, the hand bodies' callbacks and frame tick, the weapon hooks, and the rumble detours. Each reads the settings, then takes the mixer's lock, a leaf held only to queue the cue ([threading.md](../threading.md)). The input update takes the same lock once per frame to mix.

## Readout and log

The debug UI's Feedback feature opens with the `Haptic cues` and `Impact sounds` status rows, then shows the cues sent per category, the mixer's played and stacked counts, the amplitude each hand was last sent, and the last dozen one-off cues with their primitive, hand, amplitude, and length. The live controller frame has a test button per primitive for each hand, which plays it at full strength (`haptics::test`). The Status page's `Haptic cues` row, under FEEDBACK, has the cues sent, masked, and limited.

The cues themselves are not logged, since they are too frequent; the events that raise them are, by their own subsystems.

## In the headset

1. Use the debug UI's test buttons on each hand: tick, click, bump, buzz, and thud should feel distinct in weight and length, and hum and tension like a held vibration. If the five one-off primitives feel the same, the runtime is probably ignoring the frequency.
2. Reach into a holster and out again: a faint tick on entering. Draw: a click. `Haptic cues` counts up.
3. Fire a pistol: a thud on the gun hand only. Fire a long burst while reaching into a pouch with the other hand: the burst stays clean on the gun hand, and the pouch tick plays on the other.
4. Fire a burst while the reel or a vehicle's rumble runs: the shots stand out above the rumble, and the rumble resumes between shots.
5. Press a palm into a crate and push: a tension that rises with the push. Do it with both hands while the game rumbles nearby: the hands buzz together but never at full strength, and the row's limited count goes up.
6. Take a car's wheel and turn to the lock and back through centre: a bump at the lock and a tick at centre.
7. Detonate C4: a long thud on both hands.
8. Turn `GAME RUMBLE` off: landings and vehicles no longer rumble, and shots no longer pulse. Lower `HAPTIC STRENGTH`: everything weakens together.

## Open questions

- Whether the runtimes in use honour the frequency hint, and whether the low hint of the thud and the hum feels weaker than the runtime's default on the Index's actuators.
- Whether the sustained ceiling leaves the game's strongest rumbles (a landing from height, a close explosion) strong enough, since they sum under it.
