# The game's look, move, and mouse input

The mod drives the head and body, so the game's look input must feed the mod's turn scheme instead of the camera, the move input must steer the body without the game's aim switch, and the mouse must keep working on the redirected HUD. The controller bindings are [bindings.md](bindings.md); the engine's input and UI pipelines are [engine/gameplay/input.md](../../engine/gameplay/input.md).

## Tapping the look input

The post-call on `InputDeviceManager::Update` (`crates/jc3vr-gameplay/src/hooks/input/look.rs`, the bindings' poll stage: [bindings.md, "Injection timing"](bindings.md#injection-timing)) reads the four look effectors off `LocalPlayerActionMap`, clears them so the camera sees no look, and feeds the deltas to `headpose::sim::on_input_tick` on the engine's fixed-rate tick. Clearing at the effector is safe because the look effectors feed only camera code, and aim assist derives from the resulting camera. The camera's own consumer, `InputToOrbitModifier::CalculateInputDeltaAngles`, carries an anti-tamper codemarker ([engine/gameplay/input.md](../../engine/gameplay/input.md)), so it is no hook point.

## Tapping the move input

Move is handled in the locomotion tasks (`crates/jc3vr-body/src/hooks/input/locomotion.rs`), not by clearing effectors. The tasks choose run-and-steer or strafe from `Character::m_AimFlags`, which also drive the weapon raise, the reticle, and auto-aim, so the mod leaves those flags alone ([hands-and-aiming.md, "The aim-state pin and the mental-state cuts"](hands-and-aiming.md#the-aim-state-pin-and-the-mental-state-cuts)) and steers through the orientation and displacement evaluators that build on `NStateTask_InputLocoSetTargetDirTask::SetupTargetDir` ([head-and-body.md](../body/head-and-body.md), [locomotion.md](../body/locomotion.md)).

## The mouse on the redirected HUD

The HUD redirect points the movie's viewport at the mod's offscreen texture ([hud.md](../hud.md)), so window pixels no longer map to it. The `SendMouseEvents` detour (`crates/jc3vr-hud/src/hooks/ui/mouse.rs`, mapping in `crates/jc3vr-hud/src/hud/cursor.rs`) normalizes the cursor against the live client rect from the `WndProc` detour (the back buffer can differ under Wine scaling), maps it through the movie's live `ViewportMatrix`, and hands Scaleform the whole state each frame through [`MovieImpl::NotifyMouseState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/ui::scaleform::MovieImpl##vfunc-NotifyMouseState), which also sidesteps the DirectInput delta gate. The mod draws its own cursor quad whenever the game's cursor policy shows one. The map converts the mouse itself through fields the redirect bypasses, so a second detour gives it the same normalization.
