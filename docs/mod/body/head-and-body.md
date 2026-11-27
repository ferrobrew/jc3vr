# Head and body movement in VR

The player's tracked head owns where they look; the turn input, the vehicle, or the flight path owns where the character faces. The grounded body rig is [rig.md](rig.md), and the steady on-foot view is [locomotion.md](locomotion.md#the-steady-view).

## Design

VR sickness comes mostly from the eyes reporting a rotation the inner ear never felt, so the question is how much of the character's yaw comes from the player's real head and body and how much from artificial input. Where the visible torso points is a separate question of presentation, not comfort. JC3's realistic VR audience has strong VR legs, so the defaults lean immersive: smooth turning and no vignette.

- On foot the head is decoupled and one to one, never clamped. The body's yaw comes from the stick, smooth or snap, and the body also turns under the view to face the player without turning the view ([rig.md, "The yaw"](rig.md#the-yaw)). Movement is relative to the view, not the torso.
- In vehicles the root is fixed and head-look is one to one and unclamped. Recentering is a manual button with no automatic snap-back, and steering stays on the stick, so looking out of a side window never turns the vehicle.
- In the wingsuit and the parachute the body's yaw is locked to the flight direction, since steering is the body banking. The camera rolls with the wingsuit's bank, a known sickness source chosen for immersion.

The control topology follows Boneworks: the head is ground truth, the body hangs from it as kinematic IK on the authored skeleton, and the body's facing derives only from the head and the hands, never from the travel direction. Four constraints follow:

- No synthetic neck model on 6DoF: the headset's translation already contains the player's real neck pivot.
- No smoothing on the path from the headset to the camera. The body's yaw can be smoothed; the head path stays direct.
- The headset owns the head bone's orientation and roomscale position, and the game's head animation applies only below the neck.
- Roomscale head translation is never hard-collided, since freezing the view while the real head moves is itself nauseating. The view is held at walls the body cannot pass and a head in geometry fades ([rig.md, "Walls"](rig.md#walls)).

## HMD pose to camera

The root applies outermost, so roomscale translation is in the body's local frame:

    world_camera_eye = body_root · neck/head_attach_offset · hmd_head_pose · (±IPD/2 along head-local X)
    view_eye         = inverse(world_camera_eye)      (paired with that eye's asymmetric projection)

The body's yaw comes from the stick or the vehicle or flight root; the head's pitch, roll, and residual yaw come only from the headset, and never feed back into the body. When the body turns to face the rig's yaw on foot, the view frame turns back by the same amount. In a car's driver seat, the driving view's per-vehicle correction sits between the attach point and the headset pose, a constant for the length of the seat entry ([steering-wheel.md, "The driving view"](../input/steering-wheel.md#the-driving-view)).

### Gameplay and loading

In gameplay ([`GameState::E_GAME_RUN`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameState##variant-E_GAME_RUN)) the mod owns the camera position outright, from Rico's head anchor plus the offsets. Outside gameplay it must not: during a load the engine parks the active camera at the teleport destination and streams the world from it (`CGameCameraManager::ResetCamera`, then `SetActive(0)` while `PushRenderContext` re-stamps the parked transform), and writing the sim camera or driving Rico's IK there hangs the load.

So outside `E_GAME_RUN` the camera and character hooks write nothing, and only the render copy ([`GraphicsEngine::m_RenderCamera`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::GraphicsEngine##field-m_RenderCamera)) is head-tracked: the `SetupRenderCamera` hook composes the cockpit-frame headset pose onto a frozen upright base, the last gameplay viewpoint's position and yaw (`hooks::camera::LOADING_BASE`). The loading panel's world lock and this head tracking share one predicate, `hooks::camera::vr_loading_view_active`, so they never disagree. On resume the camera snaps back onto the anchor, which kept being captured, and the automatic recenter re-neutralizes.

### The camera write

In gameplay `camera_update_render` (`crates/jc3vr-view/src/hooks/camera/`) writes the previous and current head transforms to [`Camera::m_TransformT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_TransformT0) and [`m_TransformT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::camera::Camera##field-m_TransformT1) before calling the original, which lerps them by `dtf` into `m_TransformF`, so the engine's own lerp smooths the tick-rate head pose across frames. Writing here also bypasses `PushRenderContext`'s jitter filter, which quantizes head motion into visible steps. `CameraTree::UpdateRenderContexts` patches the control contexts with the same pair, which also feeds `GameCameraManager::GetInputMatrix`, so movement follows the head pose. The engine's vehicle camera route is in [rendering.md](../../engine/rendering/rendering.md#28-vehicle-camera-path).

## Hiding the head

The head mesh (face, eyes, hair, and gear weighted to facial bones) is hidden from the colour passes only, so Rico's shadow keeps its head; reflections inherit the hide. The code is `crates/jc3vr-render/src/hooks/graphics_engine/scene/render_block.rs`, fed by the view crate's `hooks::graphics_engine::player_draw`; the render-block types are in `rendering/render_block.pyxis`. Three engine facts force a per-bone, per-draw design: every pass draws the same render blocks, branching on the shadow bits of [`RenderContext::m_RenderStatus`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::graphics_engine::RenderContext##field-m_RenderStatus); the skinning palette is built once per frame ([`PoseProducer::MakeSkinningPalette`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##func-MakeSkinningPalette)) and shared by every pass, and rebuilding it mid-frame races the sim thread; and the skin material packs face, arms, and hands into one batch.

The `Draw` and `DrawZ` hooks of `RenderBlockCharacter` and `RenderBlockCharacterSkin` raise a thread-local flag while one of the player's blocks draws outside a shadow pass, and the `SetMatrixPalette` hooks upload a thread-local copy of the bones with the head and facial bones collapsed to a zero rotation at the neck's model-space position, so their vertices contract inside the collar. The real palette is never modified. The invariants:

- Ownership is an exact identity test on the draw's `info` argument against the instance info of the player's body model (the first slot of [`AnimatedModel::m_ModelInstances`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##field-m_ModelInstances), embedded at [`AnimatedModel::MODEL_INSTANCE_RBI_INFO_OFFSET`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel::MODEL_INSTANCE_RBI_INFO_OFFSET)). A radius test matches NPCs and loses the player at speed, and matching every slot crumples the canopy, which has its own skeleton.
- The collapse set is the head plus the named facial bones from the live skeleton, with the root and implausible indices filtered out. The flag and copy are thread-local because character draws run on several render workers at once.
- A batch whose non-root bones are mostly facial is face gear and collapses entirely, root included, so it does not dangle mid-body; mixed batches collapse selectively and keep the hands.
- The target is the neck joint's position from the skeleton, published after `CalculateModelSpacePose`, not read from the palette, whose layout differs per block. The hook detects the layout (column or row translation) by scoring both readings for orthonormality, since writing through the wrong one crushes geometry.

## The head pose

`crates/jc3vr-view/src/headpose/mod.rs` is the single source of truth: the current and previous `HeadPose` (for T0 and T1) and the head-bone anchor, written by the active source through `set_pose` and read through `query` and `query_prev`. The sources are the VR source (`headpose::xr`) and the flatscreen simulation (`headpose::sim`). `body_relative_rotation()` gives the head's rotation with the body frame divided out, the factor each source composes as `body × … × this`; anything that poses the head bone reads it, since the bone lives in model space and neutral is exactly the identity.

### The flatscreen simulation

Within a cone the simulated head moves freely, and past it the body's yaw tracks the head; the sim's yaw is body-relative, which gives cockpit-relative free-look in vehicles. A post-call hook on `InputDeviceManager::Update` consumes the `LOOK_*` effector values so the game's camera never sees them, and runs the whole sim step there, on the engine's tick, so the published pose pair is in phase with its `dtf` reset.

Mode detection reads the on-foot evaluator's liveness (`ModeLiveness` in `crates/jc3vr-body/src/controllers/body_frame.rs`), which advances every on-foot tick including idle, unlike the move and aim tasks, and is compared once per input tick. Body-relative yaw past a threshold twists the neck bone too.

### The head bone and the eye point

The head-bone override runs at the end of pose finalization (`crates/jc3vr-body/src/hooks/character/head.rs`, after `Character::UpdatePropEffects`), where a `SetJoint` reaches both the camera and the skinning ([skeleton.md, "Frame order, and where to override"](../../engine/character/skeleton.md#frame-order-and-where-to-override)). It composes the body-relative rotation onto the animated model-space orientation rather than writing an absolute one, since the bone's rest frame does not match the model axes.

The anchor is captured earlier, in the pre-IK hook, from the freshly animated joint: the bone matrices hold last frame's override, and reading them closes a loop that freezes the position. The camera hangs its eye point from the anchor through the head-to-neck delta and a neck-to-eyes arm measured from the eye bones. Under the sim the arm turns with the head (a synthetic neck); under VR it turns only with the body, since the tracked pose already contains the neck. The whole placement comes from the tick-spaced pose pair, because per-frame bone reads step at the sim rate while the mesh interpolates.

### The aim direction

`GetCameraMatrix` reads a sim-phase context frozen at injection time ([aim-pipeline.md, "The camera getters"](../../engine/gameplay/aim-pipeline.md)), so a post-call hook overwrites it with the head pose and every sim-side aim consumer follows the gaze. That would let the game turn the body toward the head through `NStateTask_LocoUtil::GetAimMoveAngle` and the `track_face_dir` blackboard, so for the local player on foot, idle, decoupled, and not aiming, the `GetAimMoveAngle` detour returns zero and the `force_face_camera` shim forces `track_face_dir` off (`crates/jc3vr-body/src/hooks/input/locomotion.rs`). The grapple's rotations are filtered separately ([grapple.md, "Comfort"](../grapple/grapple.md#comfort)).

## Body IK via HumanIK

The body hangs from the head through the engine's HumanIK solver ([humanik.md](../../engine/character/humanik.md)), so the spine and neck bend toward the gaze as the game's aim IK bends them. The code is `crates/jc3vr-body/src/hooks/character/body_ik/`, tuned by the `body_ik` table of `crates/jc3vr-body/tuning/character.toml`; the debug UI's Animation and IK status and a once-per-second debug log line show the targets and name the gate when nothing is queued. On foot and in the cases the procedural torso takes ([procedural-body.md](procedural-body.md), [procedural-states.md](procedural-states.md)), the torso targets below stand down once it has full weight; this construction poses the torso everywhere else.

### The head target

A pre-call hook on `Character::UpdatePassFinalizePose_Parallel` queues head position and rotation targets on the `MAIN` pass at the `SPINE_HEAD_ONLY` step, before the engine's `HasTargets` gate. The step limits which joints the pass may move, and a head target that admits the arms is met by swinging them, so the head alone stays at the spine and neck; the torso targets go at `UPPER_BODY` only on frames where the hand pins hold the arms. The target is the head bone's place under the head pose, the eye point less the headset's rotation of the neck-to-eyes arm (`controllers::body::procedural::head::head_bone_world`), so a look down turns the eyes about the neck rather than dragging the head forward.

The anchor capture runs in the same pre-IK hook, so it never chases the solved pose. The override at the end of the pass composes onto a pre-IK orientation snapshot, so it does not double the yaw the solver applied, and its roomscale position add stands down whenever a head target is queued, since both would displace the head (visible in the shadow as a detached head).

### The torso

A head target cannot move the shoulders: pull acts only on position, and turning the head, the dominant motion, gives it nothing. So the torso is driven directly as one rotation about a pivot between the spine's base and the hips ([`Effector::HIPS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HIPS)), since a sideways lean bends at the spine and a forward lean hinges at the hips. The rotation combines a share of the swing carrying the animated head to the player's head with a share of the head's body-relative heading; while the rig is grounded the yaw is the rig's chest heading against Rico's facing ([rig.md, "The yaw"](rig.md#the-yaw)), and in a vehicle the torso takes the full head-to-body angle. The lean is capped so the pose saturates rather than flips as the head nears the pivot's height.

The rotation goes through two channels that cannot disagree: shoulder position targets ([`Effector::LEFT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-LEFT_SHOULDER), [`RIGHT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-RIGHT_SHOULDER)) at their animated positions carried through it, and one rotation target on the chest-end effector (`GetChestEndEffectorId`). The shoulder bones are found by scanning the solver's `m_HIKNodeAndBonePairs`, because the safe-bone names differ from the HumanIK node names there and the targets would silently never queue.

Moving the shoulders drags the arms, so each wrist ([`Effector::LEFT_WRIST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-LEFT_WRIST), [`RIGHT_WRIST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-RIGHT_WRIST)) is pinned at its animated position with zero pull, and a zero-angle rotation target holds its animated orientation so the hand, and any weapon on it, does not spin. A deep crouch is left to the game's crouch capsule ([rig.md, "Posture"](rig.md#posture)).

### The game's own targets

A second target for the same effector replaces the first, so the shoulders, hands, and chest stand down wherever the game holds a live target (aiming, gripping, swinging toward a hook); the head is the exception. While [`Character::m_AimFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_AimFlags) reports weapon or grapple aiming only the head target queues, because the weapon-aim IK needs the rest of the chain free to swing the arms past the body's facing. A live target is a valid entry: `ClearTargets` leaves consumed entries decaying with `is_valid` cleared ([humanik.md, "Target entry lifetime"](../../engine/character/humanik.md)), and treating those as claims would make the mod defer to its own last-frame pins. For the same reason its targets queue with `blend_out` off, since they are animation-anchored and wrong a frame later.

## Suppressing idle motion

The idle fidget (`S_IDLE_ONE_OFF`, the weight shift and look-around) would move the torso independently of the player, so the idle-task hook in `crates/jc3vr-body/src/hooks/input/locomotion.rs` holds [`Character::m_IdleFidgetTimer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_IdleFidgetTimer) positive for the local player; holding the timer, rather than swallowing the queued act, keeps the task's later walk and start checks. The breathing in the base `S_IDLE` clip plays as authored: freezing the clip would be a bigger change, and no additive layer is active there to suppress it.
