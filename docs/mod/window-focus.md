# Window focus during a VR session

The game freezes itself whenever its desktop window loses the foreground ([engine/window-focus.md](../engine/window-focus.md)). In VR that is wrong: the desktop window's focus says nothing about where the player is looking, and it is taken often (a focus-follows-mouse window manager, an overlay such as WayVR, a terminal). Left alone, a focus change mid-session stops `Game::Update`, so the headset view, the head pose, and the controllers freeze until someone clicks the window.

`payload/src/hooks/window_focus.rs` detours [`LoseFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::LoseFocus) and [`GainFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::GainFocus), the two functions the main loop calls on a foreground change. Those are where the suspension is applied and lifted, so skipping them keeps every system the suspension touches running together: the frame, the audio, and the fullscreen mode. The window procedure is not the hook point, since the engine never reads the activation messages.

## While a session runs

The session predicate is `vr::is_running_published`, an atomic the game thread writes after each `vr::update`. The focus functions run in the main loop outside `Game::Update`, where taking the VR runtime lock is not allowed ([threading.md](threading.md)).

- A `LoseFocus` while a session runs and the game is not already suspended is skipped. [`SuspendGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::SuspendGame) stays clear, so the main loop keeps calling `Game::Update`, the audio keeps playing, and the window keeps its fullscreen mode. The first skipped loss of a background stretch logs a `window` line.
- The keyboard and mouse still follow the desktop focus: the detour clears [`m_IsInFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##field-m_IsInFocus) as the engine would. The mouse is a background DirectInput device, so leaving the flag set would let a mouse moved or clicked in another window turn the camera or fire. Gamepads and the OpenXR controllers do not read the flag, so head-tracked gamepad play and motion controls are unaffected.
- The `GainFocus` that ends a skipped loss is skipped too, and only sets `m_IsInFocus` again. Run on a game that is not suspended, the engine's function would apply a stale [`WasFullscreen`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::WasFullscreen). The pairing holds even when the session has ended before the window comes back.

A suspension can also predate the session: the game keeps updating while suspended outside gameplay, so a window that lost focus on the main menu, followed by a session starting, would freeze at the end of the load. `window_focus::resume_for_vr` runs at the top of each `Game::Update`; when a session runs and `SuspendGame` is set, it calls the engine's own `GainFocus` to lift the suspension, then sets `m_IsInFocus` from the real foreground window. When the window is still in the background, the next `GainFocus` the main loop makes is paired as above.

## Without a session

Both functions go through unchanged, and `resume_for_vr` does nothing. A session that ends while the window is in the background leaves the game running until the next foreground change, which suspends it as usual.

## Unverified in the headset

- That nothing else in the frame depends on focus: the `pause_game_checked` event the skipped `LoseFocus` would have sent has no located subscriber.
- That `GainFocus` from `resume_for_vr`, which restores a saved fullscreen mode, behaves on a window that is still in the background in exclusive fullscreen. Under Proton the game normally runs windowed or borderless.
