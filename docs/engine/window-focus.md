# Window focus and suspension

The game suspends itself when its window loses the desktop foreground. The suspension is driven from the main loop, not from the window procedure, and during gameplay it stops the whole frame.

## How the game notices

[`WndProc`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::WndProc) does not handle `WM_ACTIVATEAPP`, `WM_ACTIVATE`, `WM_SETFOCUS`, or `WM_KILLFOCUS`; they reach `DefWindowProcA` and change nothing in the game. Its only focus-related case is `WM_CANCELMODE`, which sets [`AltTab`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::AltTab).

[`AVAMainLoop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::AVAMainLoop) drains the message queue, then calls `GetForegroundWindow` every iteration and compares the result with the window it saw on the previous iteration. Only a change of foreground window (or a set `AltTab`) acts:

- If the new foreground window is the game's own ([`WindowHandle`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::WindowHandle)) and `AltTab` is clear, it calls [`GainFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::GainFocus).
- Otherwise it calls [`LoseFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::LoseFocus), telling it whether any window holds the foreground.

It then refreshes the cursor clip and clears `AltTab`. Because the trigger is a change, a window that stays in the background keeps whatever state the last call left, and a window that never changes the foreground never suspends.

## What a focus loss changes

`LoseFocus` acts only when [`SuspendGame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::SuspendGame) is clear. It:

- sets `SuspendGame`;
- tells the sound library the window lost focus through [`SetWindowFocused`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/audio::sound_library::SetWindowFocused), whose flag [`WindowFocused`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/audio::sound_library::WindowFocused) makes the post-sim sound update silence the master bus;
- broadcasts the `pause_game_checked` event (its subscribers are not located; unverified);
- saves the fullscreen mode in [`WasFullscreen`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/window::WasFullscreen) and switches to windowed mode, when some window holds the foreground and the graphics engine exists;
- clears the input device manager's [`m_IsInFocus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_device_manager::InputDeviceManager##field-m_IsInFocus).

`m_IsInFocus` is passed to every device's object-state update. The keyboard and mouse read their DirectInput state only while it is set and zero it otherwise; gamepads ignore it. The keyboard is acquired with a foreground cooperative level, but the mouse is acquired non-exclusively in the background, so for the mouse this flag is the only thing that stops motion and clicks from another window reaching the game.

`GainFocus` undoes all of this when `SuspendGame` is set: it clears the flag, tells the sound library the window has focus, sets `m_IsInFocus`, restores the fullscreen mode from `WasFullscreen` and clears `WasFullscreen`, and broadcasts `pause_game_clear`. Called when the game is not suspended, it only applies the fullscreen mode in `WasFullscreen`, which after a full lose-and-gain cycle holds windowed mode, not the player's setting.

## What suspension stops

While `SuspendGame` is set and the game state is [`E_GAME_RUN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##variant-E_GAME_RUN), `AVAMainLoop` skips [`Game::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-Update) and the `SleepEx` yield after it. The loop spins on the message queue and the foreground poll; simulation, rendering, the input poll, and present all stop until `GainFocus` runs. Frame time spent there is outside `Game::Update` entirely.

In any other game state (the frontend, loading) the game keeps updating while suspended, with the audio muted and the keyboard and mouse dropped, and freezes as soon as it reaches gameplay.
