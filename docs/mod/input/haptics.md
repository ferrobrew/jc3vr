# Haptics

Every vibration the mod sends is one of a few named primitives, played at a strength within that primitive's range, and every source goes through one mixer. A tick always feels like a tick and a thud like a thud, and a hand that several sources reach at once feels the strongest clearly rather than a saturated buzz.

The vocabulary and the mixer are pure and live in `crates/jc3vr-input/src/haptics/` (`vocabulary.rs`, `cues.rs`, `mixer.rs`, `output.rs`, `tuning.rs`), tuned by `crates/jc3vr-view/tuning/haptics.toml` ([tuning.md](../tuning.md)). The game-facing entry points are in `crates/jc3vr-body/src/controllers/haptics/`.

## Vocabulary and sources

The seven primitives (`Primitive`) are five one-off cues, from the lowest priority to the highest tick, click, bump, buzz (a refusal), and thud, and two sustained cues, hum and tension, which follow a level their source sends every tick. Each has a soft and a full amplitude, a length, and a frequency hint, set once in the tuning so a change applies to every cue built on it. The frequency is OpenXR's `XrHapticVibration::frequency`, which a runtime may ignore.

Every source is an `Event` or a `Sustain` in `cues.rs`, whose doc comments say when each is raised, and the tuning maps each to its primitive and strength; every source must be present and no other key is accepted. A source whose cue follows how hard something happened scales its strength by that share (`play_scaled`). Each source belongs to a `Category` (contact, weapons, vehicles, traversal, interface, or game), which the readout counts under.

## The mixer

OpenXR plays one vibration per controller at a time, a new one replacing the last, so sources queue cues from any thread and the VR input update asks the mixer once per rendered frame what each hand plays (`output::mix`).

- Sustained levels sum through a soft limiter whose ceiling sits below full strength, so however many stack, a one-off cue still stands out above them.
- The one-off cue of the highest priority among a frame's arrivals leads; the others, and a cue still playing, add a share of their amplitude beneath it, and the total passes a soft limiter whose knee leaves a single cue exactly as shaped.
- Sustained sources, including the game's rumble, are held off a hand while a one-off cue plays there.
- An arrival weaker and of lower priority than the cue playing is dropped rather than cutting it short.

The global `intensity` scales the output as it is sent, never the mix. The mixer's lock is a leaf held only to queue or mix ([threading.md](../threading.md)).

## Settings

`controllers.haptics` (`HapticsConfig`; [settings.md](../settings.md)) has `enabled`, `intensity`, and `game_rumble`, on the pause menu's `VR: CONTROLS` page. Cues play only under the controller scheme, so a gamepad player gets the gamepad's own rumble. The game's rumble, including the shot and the recoil kick, also needs `game_rumble`.

## The game's rumble

Two pre-call detours (`rumble.rs`) carry the game's rumble to the controllers:

- [`RumbleManager::SpawnRumble`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/rumble_manager::RumbleManager##func-SpawnRumble) sees every one-off rumble. Inside a shot of a gun held in a hand, which the weapon handling marks around the fire call (`shot.rs`), the weapon's fire rumble plays on that hand as `Event::Shot`, raised by the recoil kick, or as `Event::Kick` when the weapon has none. The game's call always goes through, since it still feeds the per-frame sum.
- [`Gamepad::SetMotorValue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::gamepad::Gamepad##func-SetMotorValue) sees that sum, including a vehicle's own rumble, which never goes through `SpawnRumble`. The low motor plays on the left hand and the high on the right as `Sustain::GameRumble`, zero included so the rumble stops when the pad's does, and zero is passed to the physical pad.

## Readout

The debug UI's Feedback feature shows the cues per category, the mixer's counts, each hand's last amplitude, and the recent one-off cues, and the live controller frame has a test button per primitive and hand (`haptics::test`). The cues are not logged; the events that raise them are.

Open questions: whether the runtimes honour the frequency hint, which is what makes the one-off primitives feel distinct (the test buttons show it), and whether the sustained ceiling leaves the game's strongest rumbles strong enough.
