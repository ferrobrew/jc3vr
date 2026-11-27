# Roadmap

jc3vr renders Just Cause 3 in stereo for an OpenXR headset, drives the camera and Rico's body from the player's head and hands, floats the game's HUD as a panel in the world, and maps motion controllers onto the game across on-foot play, vehicles, the grapple, the wingsuit, and the parachute. A gamepad with head tracking stays a supported way to play.

## Where the project stands

The OpenXR runtime, multipass and single-pass stereo, FSR 2 at native resolution as the anti-aliaser, the floating panel with world-anchored markers, the VR pages in the pause menu, and the motion-controller scheme (physical hands and weapons, the grip model, two-handed inventory, roomscale, the body rig, vehicles, the physical grapple, and the wingsuit and parachute controls) are built and in headset playtesting. [architecture.md](architecture.md) describes how the pieces fit, and the newest folder in `docs/sessions/` records the latest playtest.

## Open work

Open work, including FSR upscaling below panel resolution, per-eye depth submission, foveated rendering, cutscene presentation, and the open rendering bugs, is tracked as GitHub issues rather than here. Investigations behind specific issues are in `docs/issues/`.

## Out of scope

- A full first-person mesh rework: the third-person assets were not built for it.
- Multiplayer: the mod targets the single-player game.
