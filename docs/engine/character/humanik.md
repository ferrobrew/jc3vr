# HumanIK: driving the upper body from an external target

JC3 wraps Autodesk HumanIK in `NAnimationSystem::CHumanIK` ([`HumanIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK), in the [`animation::ik`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/animation::ik) module). Each `Character` embeds one solver and drives it every frame inside the pose-finalization pass. This page covers the solver's layout, the pass order (which this page owns for the whole finalize pass), the effector ids, and the semantics of the target calls.

## Where it lives

The solver is [`Character::m_HIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_HIK), embedded by value in `CCharacter`; every HumanIK call in `UpdateSecondaryHandIKPass` and `UpdatePassFinalizePose_Parallel` passes its address. The layout depends on the build's `std::vector`, which carries the three MSVC pointers plus a trailing allocator slot, as the pyxis [`Vector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::std_vector::Vector) models it. The fields, in layout order:

| Field | Notes |
|---|---|
| `m_HIKCharacter`, `m_HIKCharacterState`, `m_HIKEffectorSetState`, `m_HIKPropertySetState` | opaque Autodesk HIK handles |
| [`m_PassInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_PassInfo) | one [`PassInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo) per pass, `[Pass::MAIN]` and `[Pass::SECONDARY]`: the pass's [`m_SolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_SolveStep) and its target lists [`m_EffectorTargetPositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_EffectorTargetPositions) and [`m_EffectorTargetRotations`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_EffectorTargetRotations) |
| [`m_CurrentPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_CurrentPass) | set by `SetActiveIKPass` |
| [`m_HIKNodeAndBonePairs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_HIKNodeAndBonePairs), [`m_TQS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TQS) | `Vector`s |
| [`m_UsedHIKNodeIds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_UsedHIKNodeIds) | `int*`, terminated by [`HumanIK::USED_HIK_NODE_IDS_END`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::USED_HIK_NODE_IDS_END) |
| [`m_EffectorIds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_EffectorIds) | an [`EffectorIdTable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::EffectorIdTable) (`THashTable<int,uint,1,ushort>`) |
| [`m_TargetPull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetPull), [`m_TargetResist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetResist), [`m_TargetReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetReachT), [`m_TargetReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetReachR) | the destinations; the translation reach is written directly after queuing a position target, the rotation reach after a rotation target |
| [`m_Pull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Pull), [`m_Resist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Resist), [`m_ReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachT), [`m_ReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachR) | the current values, driven toward the destinations |

The eight control-value arrays each hold [`HumanIK::EFFECTOR_SLOTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::EFFECTOR_SLOTS) entries, indexed by effector id and shared by both passes.

## Per-frame lifecycle

Everything runs in the SIM phase inside [`Character::UpdatePassFinalizePose_Parallel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePassFinalizePose_Parallel), which the post-physics pass queues for the parallel update of every character, so it runs on the engine's worker threads after the physics step. The IK block is gated by the global HIK enable [`Character_EnableHIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character_EnableHIK) (`CCharacter::m_EnableHIK`) and by [`CharacterLodFlags::REDUCED_LOD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::CharacterLodFlags##flag-REDUCED_LOD) being clear in [`Character::m_LodFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_LodFlags).

```
UpdatePassFinalizePose_Parallel(context):
  UpdateStateTasksInRange(dt, FINALIZE_POSE_STATE_TASK_RANGE)    <-- aim, look-at, loco, off-hand IK tasks
  UpdateGroundInfo                                               <-- off support: CalculateHeightOverGround ray
  CInventory::UpdateAttachmentTransforms                         <-- weapons placed from the attach bones
  if m_EnableHIK && not-reduced-LOD:
    # ---- MAIN pass (body IK: aim, reach) ----
    if HasTargets(m_HIK, PASS_MAIN):                             <-- THE GATE
        SetActiveIKPass(m_HIK, PASS_MAIN)
        DriveAllCurrentEffectorControlValues(m_HIK, dt)
        CCharacter::UpdateIKForTargets(this, dt):
            CharacterToIKState(m_HIK, pose)
            UpdateEffectorsFromTargets(m_HIK, dt)
            Solve(m_HIK)
            IKToCharacterState(m_HIK, pose, updateAllBones)      -> writes solved pose back
        UpdatePropTransforms(...)                                <-- weapons re-placed from the solved wrists
    ResetSolveStep(m_HIK, PASS_MAIN)
    if ClearTargets(m_HIK, PASS_MAIN): ResetProperties(m_HIK)
    # ---- SECONDARY pass (hand / grip IK) ----
    UpdateSecondaryHandIKPass(this, dt, &ik_has_run)
      -> SetActiveIKPass(PASS_SECONDARY); AddEffectorTargetPosition(..., PASS_SECONDARY);
         (solve); ClearTargets(PASS_SECONDARY); ResetSolveStep(PASS_SECONDARY)
    ResetSolveStep(m_HIK, PASS_SECONDARY)
  if skip-subframe: UpdateGraphicsMatrix; SyncPoses; T0 = T1
  UpdateGraphicsMatrix; UpdateModelVisibility; UpdateMyselfTarget  <-- the point other characters aim at
  CInventory::UpdateInventoryPostFinalizePose                    <-- attach-bone tables rewritten
  CalculateModelSpacePose(poseProducer)                          <-- both poses' model space synced
  attached effects (parachute, wingsuit, stunts, water); AI entity's view;
  CRagdollProxyPhantom::SyncShapeTransformWithPose               <-- the bodies bullets hit
  Character::UpdatePropEffects(this, dt)                         <-- LAST call
```

The `HumanIK` calls are bound as [`HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets) (this is its one caller in the binary), [`SetActiveIKPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-SetActiveIKPass), [`DriveAllCurrentEffectorControlValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-DriveAllCurrentEffectorControlValues), [`CharacterToIKState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-CharacterToIKState), [`UpdateEffectorsFromTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-UpdateEffectorsFromTargets), [`Solve`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-Solve), [`IKToCharacterState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-IKToCharacterState), [`ResetSolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ResetSolveStep), [`ClearTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ClearTargets), and [`ResetProperties`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ResetProperties). The ground information is [`Character::UpdateGroundInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateGroundInfo): off its support, the character casts a scene ray down from the root ([`Character::CalculateHeightOverGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CalculateHeightOverGround), through [`DoGroundInfoRaycasts`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-DoGroundInfoRaycasts)) through [`PhysicsSystem::CastRay`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-CastRay), with its hit record and filter on the stack. The finalize-range state tasks are in [animation-layers.md](animation-layers.md#state-tasks-inside-the-pose-finalization-pass), the attachment calls in [weapon-attachment-and-arm-ik.md](weapon-attachment-and-arm-ik.md#the-attach-bone-table-is-rewritten-every-pass), and the last call is [`Character::UpdatePropEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePropEffects).

The MAIN solve is [`Character::UpdateIKForTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateIKForTargets), which the IDB mislabels `CAnimationRuleSystem::GetState` (its body and its sole call site identify it). After it, `CInventory::UpdateAttachmentTransforms` (not bound) re-derives each held weapon's world transform from its attach bone, a child of the wrist, so a weapon on an IK-driven wrist follows the solved wrist within the tick. [`Character::UpdateSecondaryHandIKPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateSecondaryHandIKPass) does the same after its own solve when it had targets, through [`Character::UpdateSecondaryHandIKForTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateSecondaryHandIKForTargets), which skips `CharacterToIKState` when the MAIN solve ran this frame and reuses that pose copy.

The consequences for anyone queuing a target:

- The MAIN solve is gated by `HasTargets(PASS_MAIN)`: with no targets the body solve is skipped that frame.
- A target reaches the current frame's MAIN solve only if [`AddEffectorTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetPosition)`(…, PASS_MAIN, …)` (or a rotation variant) runs before the gate. Nothing in the pass clears PASS_MAIN before the gate, so the entry of `UpdatePassFinalizePose_Parallel` is the latest point that works. The game's own aim IK queues earlier, in the finalize-range state tasks.
- `ClearTargets(PASS_MAIN)` runs after the solve, so targets queued before the gate are consumed that frame and then invalidated, not removed ("Target entry lifetime" below).
- `CharacterToIKState` reads the pose present mid-pass, the freshly animated one, which is the reference a model-space target is expressed against.
- A queued target does nothing on a frame where the HIK enable or the LOD gate fails.
- A pose written before the gate reaches the solve, the weapons, the hit shapes, the AI, the effects, the camera, and the skinning. `UpdatePropEffects` runs after both IK passes and `CalculateModelSpacePose`: a `SetJoint` there acts on the finalized model-space pose and reaches only the camera and the skinning ([skeleton.md](skeleton.md#frame-order-and-where-to-override)), and an effector target queued there misses the frame's solve.

## Effector-id model

[`HumanIK::GetEffectorIdFromBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-GetEffectorIdFromBoneIndex) maps a skeleton bone index to an effector id below [`HumanIK::EFFECTOR_SLOTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::EFFECTOR_SLOTS), or [`HumanIK::NO_EFFECTOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::NO_EFFECTOR) for a bone without one. The bone index is in the integer space the bone matrices and joints use (`AnimationController::GetBoneIndex` and `GetJoint`, and what `Character::GetSafeBoneMatrix` resolves a `SafeBoneIndex` to); `UpdateSecondaryHandIKPass` passes hand indices from `Character::m_SafeBoneIndices` unchanged, and `Character::GetSafeIndex(SafeBoneIndex::HEAD)` yields exactly the head's index.

That does not generalize: the table is keyed by the bone whose name matches the HumanIK node's name (see the `EffectorIdTable` def), a different scheme from the safe bones. The head agrees, but the game's shoulder safe bone is not the bone HumanIK calls a shoulder, and the lookup returns `NO_EFFECTOR`. The reverse mapping, effector to bone, is a scan of `m_HIKNodeAndBonePairs`.

The mapping (`GetEffectorIdMapping`, node to effector) fixes the ids, bound as the [`Effector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector) enum: `HIPS`, `LEFT_ANKLE`/`RIGHT_ANKLE`, `LEFT_WRIST`/`RIGHT_WRIST`, `LEFT_KNEE`/`RIGHT_KNEE`, `LEFT_ELBOW`/`RIGHT_ELBOW`, `WAIST`, `CHEST_END` (`GetChestEndEffectorId`, a constant), `LEFT_FOOT`/`RIGHT_FOOT`, `LEFT_SHOULDER`/`RIGHT_SHOULDER`, `HEAD`, and `LEFT_HIP`/`RIGHT_HIP`.

HumanIK names its nodes in MotionBuilder's convention (`HIKNodeNameFromNodeId` reads a fixed name table), so the shoulder node is the upper-arm joint: `LEFT_SHOULDER` drives the bone `LeftArm`, `LEFT_ELBOW` drives `LeftForeArm`, and `LEFT_WRIST` drives `LeftHand`, and likewise on the right. The rig's `LeftShoulder` bone is the collar, HumanIK's separate collar node, which has no effector. The shoulder, elbow, and wrist effectors therefore span the arm from the glenohumeral joint to the wrist. The head effector is [`Effector::HEAD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HEAD), the humanoid rig's characterization having a head node.

## Target calls

The prototype of [`HumanIK::AddEffectorTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetPosition), verified against the call sites:

```
AddEffectorTargetPosition(
    m_HIK,
    effector,                    # i32, e.g. Effector::HEAD
    pos,                         # *const Vector3, CHARACTER-MODEL SPACE
    solve_step,                  # SolveStep
    pass,                        # Pass (PASS_MAIN for the body pass)
    effector_interpolation,      # bool
    effector_interpolation_rate, # f32
    effector_blend_out,          # bool
    effector_blend_out_rate)     # f32
```

The position is in character-model space: `UpdateSecondaryHandIKPass` multiplies the world-space blackboard target by `inverse(m_WorldMatrixT1)` before the call, the same transform that makes a model-space `Joint` ([skeleton.md](skeleton.md)). After queuing, the engine writes `m_TargetReachT[effector] = weight` (1.0 is full reach); without it the reach stays zero and the target has no effect.

There are two rotation overloads: the axis-enum [`HumanIK::AddEffectorTargetRotation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetRotation) and the explicit-axis [`HumanIK::AddEffectorTargetRotationVector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetRotationVector). A rotation target's angle is an offset applied to the effector's current rotation, not an absolute orientation: `UpdateEffectorsFromTargets` reads the effector's current state, builds a quaternion from the angle-axis, and multiplies it on.

### The four control values, and which list drives which

[`HumanIK::UpdateEffectorsFromTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-UpdateEffectorsFromTargets) ends with a loop over all effector slots that pushes four per-effector values into the Autodesk effector-set state:

| Call | Source array | Meaning |
|---|---|---|
| `HIKSetTranslationActive` | [`m_ReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachT) | how strongly the positional target is reached |
| `HIKSetRotationActive` | [`m_ReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachR) | how strongly the rotational target is reached |
| `HIKSetPull` | [`m_Pull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Pull) | how much reaching this effector propagates into the rest of the body |
| `HIKSetResist` | [`m_Resist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Resist) | how much this effector resists being displaced by other effectors' pull |

[`HumanIK::DriveAllCurrentEffectorControlValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-DriveAllCurrentEffectorControlValues), just before the solve, fills those arrays from the `m_Target*` ones, walking the pass's two target lists separately. The position list drives `m_ReachT[eff]` (eased by `SpeedLerp` when the entry's interpolation or blend-out flag applies, snapped otherwise) and copies `m_Pull[eff] = m_TargetPull[eff]` verbatim; the rotation list drives `m_ReachR[eff]` the same way and copies `m_Resist[eff] = m_TargetResist[eff]` verbatim. So:

- Pull rides the position target and resist the rotation target: writing `m_TargetPull` for an effector with only a rotation target does nothing that frame, and vice versa.
- Reach without pull is a local solve: the effector reaches its target by bending only the chain it terminates. A head target with pull 0 is absorbed by the neck, and the shoulders, chest, and spine stay on the animated pose. The foot and hip IK (`NStuntIKTask`) writes `m_TargetReachT` and `m_TargetPull` together; the aim IK (`NRightArmAimIK`) writes only `m_TargetReachR`, which is why aiming turns the head without dragging the torso. Pull propagates only a positional requirement, so a head target never turns the chest; the game turns it with a chest-end rotation ([`HumanIK::GetChestEndEffectorId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-GetChestEndEffectorId), the constant [`Effector::CHEST_END`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-CHEST_END)), as [`ReelInAimIK::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::arm_aim_ik::ReelInAimIK##func-Update) does with `AddEffectorTargetRotation(m_HIK, chestEnd, yawRadians, RotationAxis::Y, SolveStep::UPPER_BODY, Pass::MAIN, false, rate, true, blendOutRate)` and `m_TargetReachR[chestEnd]`. The shoulder effectors ([`LEFT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-LEFT_SHOULDER), [`RIGHT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-RIGHT_SHOULDER)) are children of the chest and follow its rotation; displacing the girdle takes positional targets with reach and pull on them, the recipe `NStuntIKTask` uses for the feet and hips.

### Solve steps

`Solve` maps each [`SolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep) to an Autodesk solver bitmask, bound as the `HumanIK::HIK_SOLVING_STEP_*` constants: [`SPINE_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_ONLY), [`SPINE_HEAD_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_HEAD_ONLY), [`ARMS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_ARMS), [`UPPER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_UPPER_BODY), [`SPINE_HEAD_LOWER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_HEAD_LOWER_BODY), [`FULL_BODY_NO_PULL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_FULL_BODY_NO_PULL) (`FULL_BODY` without the pull stages), and [`FULL_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_FULL_BODY). A pass's effective step is the maximum of its queued targets' steps (with an arm-combining special case), so a single head target at `UPPER_BODY` admits the whole upper body. The step is a joint mask: [`SolveStep::UPPER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep##variant-UPPER_BODY) admits the arms and [`SolveStep::SPINE_HEAD_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep##variant-SPINE_HEAD_ONLY) does not, and a solver allowed to move the arms may reach a head target by swinging one, because pull does not arbitrate how a requirement is met.

### Target entry lifetime and the `is_valid` flag

A pass holds at most one position entry and one rotation entry per effector: `AddEffectorTarget*` scans the list and, on a hit, overwrites the entry in place and sets its `is_valid` flag rather than appending. `ClearTargets`, after the solve, clears `is_valid` and keeps the entry, removing it only once the effector's reach weight is zero and the entry is already invalid. So:

- An invalidated entry keeps steering. `UpdateEffectorsFromTargets` applies every entry, valid or not, so a leftover pushes its effector toward its stale pose while `DriveAllCurrentEffectorControlValues` runs its reach down: `SpeedLerp` toward zero at the blend-out rate with the blend-out flag (the hand pass's default, `HumanIK::HAND_PASS_BLEND_OUT_RATE` per second, gives a tail of about 0.7 s from full reach), zeroed on the next driven frame without it. For valid entries the interpolation flag likewise governs the ramp toward the target reach; each flag governs only its own direction.
- Presence in the list is not a claim; `is_valid` is. The game's systems re-queue their active targets every frame, so the set of valid entries at pose-finalization entry is exactly what the game is driving this frame. Matching on presence mistakes leftovers, including one's own from the previous frame, for live targets.
- Re-queuing each frame refreshes and never duplicates; with interpolation off the reach snaps back to the written `m_TargetReach*` each frame.
- The reach values are shared by both passes. An invalidated entry on one pass is never removed while the other pass keeps that effector's reach up, so it keeps its stale pose and pulls the effector back to it whenever its own pass solves for another target. A `SECONDARY` hand target queued once and left behind by a `MAIN` supplier on the same wrist is the case that matters.
- A second add to the same effector, from another supplier, replaces the first; targets on distinct effectors share the solve at the maximum step. How a head target interacts with a chest or spine target the game adds in the same frame is untested.

The game's own calls:

- The hand pass queues a position target with a per-hand `solve_step`, `pass=PASS_SECONDARY`, interpolation off with [`HumanIK::HAND_PASS_INTERPOLATION_RATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HAND_PASS_INTERPOLATION_RATE), and blend-out on with [`HumanIK::HAND_PASS_BLEND_OUT_RATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HAND_PASS_BLEND_OUT_RATE), then writes `m_TargetReachT[eff]`.
- The aim IK ([`RightArmAimIK::UpdateAimEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::arm_aim_ik::RightArmAimIK##func-UpdateAimEffector)) calls `AddEffectorTargetRotationVector(m_HIK, eff, angle, &axis, SolveStep::UPPER_BODY, Pass::MAIN, false, 0, false, 0)` and writes `m_TargetReachR[eff]`, the game's precedent for driving the upper body on PASS_MAIN.

## Open questions

- `IKToCharacterState` takes `CCharacter::m_IkToCharacterUpdateAllBones`; how the flag changes which bones receive the solved result is not characterized. A later `SetJoint` on the same bones, such as one at `UpdatePropEffects`, overwrites the solved joint and re-derives its descendants.
- `CharacterToIKState` and `IKToCharacterState` take the character's `hkaPose*` and are invoked only inside the pass, so code outside it queues targets and leaves the solve to the engine.
