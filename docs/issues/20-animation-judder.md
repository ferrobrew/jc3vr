# Animation judder from the 30 Hz tick (issue #20)

Open. The game simulates at a fixed 30 Hz and renders every frame, so animation, physics, and AI advance once per tick while the headset displays at 75 to 90 Hz. The engine interpolates between the last two ticks at render time, which hides the steps for anything that moves linearly over a tick, but an animation's pose is still sampled at the tick rate. Two parts of the player are already drawn at the render rate: the view, whose camera transform pair the mod fills with the head pose as of the previous and current ticks, with the headset's own motion the same on both sides so it passes through without lag ([head-and-body.md](../mod/body/head-and-body.md)), and Rico's body, hands, and held items, which the presentation layer re-poses each rendered frame from the latest tracking ([presentation-layer.md](../mod/rendering/presentation-layer.md)). What remains is everything else's animation cadence. Changing the tick rate globally is not an option: the game's systems assume 30 Hz.

This page holds the engine's interpolation path, which any fix builds on.

## The decoupled loop

`CGame::Update` runs a fixed-step simulation and a variable-rate render. Each frame it picks an interpolation method: [`Game::INTERPOLATION_METHOD_ACCUMULATED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game::INTERPOLATION_METHOD_ACCUMULATED) while a cutscene plays, [`Game::INTERPOLATION_METHOD_SINGLE_STEP`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game::INTERPOLATION_METHOD_SINGLE_STEP) otherwise, and the console override when it is not [`Game::INTERPOLATION_OVERRIDE_NONE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game::INTERPOLATION_OVERRIDE_NONE).

- Single step, the gameplay path, runs `CGame::UpdateGame` at most once per render frame and accumulates the fractional timestep in `m_RunningFrameFraction`.
- Accumulated, the cutscene path, runs as many updates as the accumulator holds, subtracting one per update.
- [`Game::INTERPOLATION_METHOD_TIME_BASED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game::INTERPOLATION_METHOD_TIME_BASED) derives the update count from the wall clock and can run several updates in a slow frame.

Every method stores the fraction of a tick, from 0 to 1, in [`UpdateContexts::m_RenderContext`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::UpdateContexts##field-m_RenderContext) as [`GameObjectRenderContext::m_Dtf`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObjectRenderContext##field-m_Dtf): the interpolation weight from the previous tick (T0) to the current one (T1).

## Where the fraction is consumed

`CGameStateRun::UpdateRender` passes the render context's `m_Dt` and `m_Dtf` on, and `CCharacter::UpdateRender` uses the fraction twice:

1. It lerps the character's world transform from `m_GraphicsWorldT0` to `m_GraphicsWorldT1`, which places the root, not the bones.
2. Unless the character is on lightweight skinning, it runs the inlined `CCharacter::UpdateSkinning`, which calls [`AnimationController::UpdateSkinning`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-UpdateSkinning) with the fraction. That calls [`PoseProducer::MakeSkinningPalette`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##func-MakeSkinningPalette), which queues a skinning job that blends the producer's `m_PoseT0` and `m_PoseT1` model-space joints by the fraction into the double-buffered skinning palette the vertex shaders read. The IDB labels `MakeSkinningPalette` as `CPoseProducer::Update`, and the function it labels `CCharacter::UpdateSkinning` is an unrelated fade-alpha update.

[`Character::UpdatePassFinalizePose_Parallel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePassFinalizePose_Parallel) finalizes T1 in the tick, and the previous T1 becomes T0. When `m_SkipSubframeInterpolation` is set, which happens when the ragdoll's orientation changes sharply between ticks, it copies T1 over T0 and syncs the pose buffers, so that one frame does not interpolate across the discontinuity. Weapons interpolate the same way in their own render update ([weapon-attachment-and-arm-ik.md](../engine/character/weapon-attachment-and-arm-ik.md#where-the-weapons-world-transform-is-computed-per-frame)), and the camera lerps its own transform pair by the fraction ([rendering.md §2.2](../engine/rendering/rendering.md#22-sim-path-where-m_activecameram_view-is-produced-once-per-frame)).

The mod forces `Game::UpdateRender` to run every frame by patching the update-flags gate in `Game::Update`, without touching the decoupled loop that computes the fraction, so the fraction it passes is the engine's. The debug UI's camera panel shows the last fraction the active camera received (`hooks::camera::last_dtf`), and the mod's render-time placements read the same value.

## What a fix would act on

The interpolation between ticks is linear in the joints' model-space transforms, so it smooths motion that is linear over a tick and cannot add animation detail between ticks. Smoother animation for other characters and objects would have to sample their animation between ticks, at a CPU cost per character per rendered frame, or extrapolate from the last two ticks, which overshoots on reversals. Neither is investigated.
