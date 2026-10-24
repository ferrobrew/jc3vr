# jc3vr

> [!WARNING]
> jc3vr is an unfinished mod that I'm still designing and developing. Early builds do not represent the experience I intend to deliver.
>
> I appreciate the enthusiasm, and I'm glad that it's being enjoyed, but I'd prefer that you wait until it's ready. Please don't package it for other people. I have much more planned before the grand reveal!

jc3vr is a work-in-progress VR mod for Just Cause 3. It renders the game in stereo for an HMD via OpenXR, with a separate floating panel for the UI.

The mod is a DLL injected into the running game. Inside the process, it hooks the engine's rendering, camera, input, and post-processing to drive an OpenXR session: an off-axis per-eye projection into the game's own renderer, a body/head model for on-foot locomotion, and reprojection to the headset.
