# HumanIK: driving the upper body from an external target

JC3 wraps Autodesk HumanIK in `NAnimationSystem::CHumanIK` (bindings: `jc3gi::animation::ik::HumanIK`). Each `Character` embeds one solver and drives it every frame inside the pose-finalization pass. The sections below cover the solver's layout, its per-frame lifecycle, the point before which an effector target must be queued to be solved in the same frame, the effector-id model, and the parameter and coordinate-space semantics of the target calls.

The verified addresses, field offsets, and constants are in the pyxis defs: the solver in the [`animation::ik`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/module/animation::ik) module, and its host in [`Character`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character).

## Where it lives

The solver is [`Character::m_HIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_HIK), embedded by value in `CCharacter`. Every HumanIK call in `UpdateSecondaryHandIKPass` and `UpdatePassFinalizePose_Parallel` passes the address of that field. The layout of [`HumanIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK) depends on the build's `std::vector`, which carries the three MSVC pointers plus a trailing allocator slot. The pyxis [`Vector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/types::std_vector::Vector) models it that way.

The fields, in layout order:

| Field | Notes |
|---|---|
| `m_HIKCharacter`, `m_HIKCharacterState`, `m_HIKEffectorSetState`, `m_HIKPropertySetState` | opaque Autodesk HIK handles |
| [`m_PassInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_PassInfo) | one [`PassInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo) per pass: `[Pass::MAIN]` and `[Pass::SECONDARY]` |
| [`m_CurrentPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_CurrentPass) | set by `SetActiveIKPass` |
| [`m_HIKNodeAndBonePairs`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_HIKNodeAndBonePairs) | `Vector` |
| [`m_UsedHIKNodeIds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_UsedHIKNodeIds) | `int*`, `-2`-terminated |
| [`m_TQS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TQS) | `Vector` |
| [`m_EffectorIds`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_EffectorIds) | an [`EffectorIdTable`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::EffectorIdTable) (`THashTable<int,uint,1,ushort>`) |
| [`m_TargetPull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetPull) | interpolation destinations |
| [`m_TargetResist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetResist) | |
| [`m_TargetReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetReachT) | translation-reach target weight (written directly after queuing a position target) |
| [`m_TargetReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_TargetReachR) | rotation-reach target weight (written after a rotation target) |
| [`m_Pull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Pull) | current values, driven toward the targets |
| [`m_Resist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Resist) | |
| [`m_ReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachT) | |
| [`m_ReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachR) | |

The eight control-value arrays, from `m_TargetPull` to `m_ReachR`, each hold [`HumanIK::EFFECTOR_SLOTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::EFFECTOR_SLOTS) entries. Each `PassInfo` holds the pass's [`m_SolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_SolveStep), its position-target list [`m_EffectorTargetPositions`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_EffectorTargetPositions), and its rotation-target list [`m_EffectorTargetRotations`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::PassInfo##field-m_EffectorTargetRotations).

## Per-frame lifecycle

Everything runs in the SIM phase inside [`Character::UpdatePassFinalizePose_Parallel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePassFinalizePose_Parallel), after the animation graph finalizes the local pose and before the model-space pose is computed. The pass is gated by the global HIK enable [`Character_EnableHIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character_EnableHIK) (`CCharacter::m_EnableHIK`) and by the character not being in reduced LOD ([`CharacterLodFlags::REDUCED_LOD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::CharacterLodFlags##flag-REDUCED_LOD) clear in [`Character::m_LodFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_LodFlags)).

```
UpdatePassFinalizePose_Parallel(context):
  UpdateStateTasksInRange(dt, FINALIZE_POSE_STATE_TASK_RANGE)    <-- aim, look-at, loco, off-hand IK tasks
  ... UpdateGroundInfo / UpdateAttachmentTransforms ...
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
        UpdatePropTransforms(...)
    ResetSolveStep(m_HIK, PASS_MAIN)
    if ClearTargets(m_HIK, PASS_MAIN): ResetProperties(m_HIK)
    # ---- SECONDARY pass (hand / grip IK) ----
    UpdateSecondaryHandIKPass(this, dt, &ik_has_run)
      -> SetActiveIKPass(PASS_SECONDARY); AddEffectorTargetPosition(..., PASS_SECONDARY);
         (solve); ClearTargets(PASS_SECONDARY); ResetSolveStep(PASS_SECONDARY)
    ResetSolveStep(m_HIK, PASS_SECONDARY)
  if skip-subframe: UpdateGraphicsMatrix; SyncPoses; T0 = T1
  UpdateGraphicsMatrix; UpdateModelVisibility; UpdateMyselfTarget
  CInventory::UpdateInventoryPostFinalizePose
  CalculateModelSpacePose(poseProducer)              <-- model-space pose finalized here
  ...
  Character::UpdatePropEffects(this, dt)             <-- LAST call
```

The `HumanIK` calls in the listing are bound as [`HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets), [`SetActiveIKPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-SetActiveIKPass), [`DriveAllCurrentEffectorControlValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-DriveAllCurrentEffectorControlValues), [`CharacterToIKState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-CharacterToIKState), [`UpdateEffectorsFromTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-UpdateEffectorsFromTargets), [`Solve`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-Solve), [`IKToCharacterState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-IKToCharacterState), [`ResetSolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ResetSolveStep), [`ClearTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ClearTargets), and [`ResetProperties`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-ResetProperties). The last call is [`Character::UpdatePropEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePropEffects). `CCharacter::UpdateSecondaryHandIKPass` is bound as [`Character::UpdateSecondaryHandIKPass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateSecondaryHandIKPass). The state tasks the pass runs first, and why a blackboard key zeroed at the pass's entry is written again before the solves, are in [animation-layers.md](animation-layers.md#state-tasks-inside-the-pose-finalization-pass).

The MAIN solve is a separate function, `CCharacter::UpdateIKForTargets(this, dt)`, which runs the four `HumanIK` calls from `CharacterToIKState` to `IKToCharacterState`. The IDB mislabels it `CAnimationRuleSystem::GetState`. It is not bound in the defs. Inside the same branch, after the solve, `CInventory::UpdateAttachmentTransforms` (not bound) re-derives each held weapon's cached world transform from its attach bone, a child of the wrist, so a weapon on an IK-driven wrist follows the solved wrist within the same tick. `UpdateSecondaryHandIKPass` runs the same refresh after its own solve when that pass had targets.

Key ordering facts:
- The MAIN solve is **gated by `HasTargets(PASS_MAIN)`**. No targets ⇒ the entire body solve is skipped that frame.
- `ClearTargets(PASS_MAIN)` runs **after** the solve, so targets queued before the gate survive to be consumed the same frame; then they are invalidated — not removed — for the next (see "Target entry lifetime" below).
- `UpdatePropEffects` is the last call in this function. It runs after both IK passes and after `CalculateModelSpacePose`. A `SetJoint` made there acts on the finalized model-space pose, but an effector target queued there misses the current frame's solve.

## The injection seam

An effector target reaches the current frame's MAIN solve only if [`AddEffectorTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetPosition)`(m_HIK, …, PASS_MAIN, …)` (or a rotation variant) runs before the [`HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets)`(PASS_MAIN)` gate.

Nothing in [`Character::UpdatePassFinalizePose_Parallel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdatePassFinalizePose_Parallel) clears PASS_MAIN before the gate, and the whole HIK block sits well inside the function. A target queued at the function's entry therefore reaches the gate and is solved that frame. Function entry is the latest point with this property. The game's own aim IK queues its MAIN targets earlier, during animation-graph evaluation.

- `UpdatePropEffects` runs after the solve and after `CalculateModelSpacePose`, so a target queued there is not solved that frame.
- The pose read by `CharacterToIKState` is the one present when the solve runs, in the middle of the function. That is the freshly animated pose, which is the reference a model-space target is expressed against.
- The solve runs only when [`Character_EnableHIK`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character_EnableHIK) (`m_EnableHIK`) is set and the character is not in reduced LOD. A queued target has no effect on a frame where either gate fails.

## Effector-id model

[`HumanIK::GetEffectorIdFromBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-GetEffectorIdFromBoneIndex) maps a skeleton bone index to a HumanIK effector id below [`HumanIK::EFFECTOR_SLOTS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::EFFECTOR_SLOTS), or returns [`HumanIK::NO_EFFECTOR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::NO_EFFECTOR) if the bone has no effector mapping. The bone index is in the same integer space that the character's bone matrices and joints use (`AnimationController::GetBoneIndex` and `GetJoint`, and the value `Character::GetSafeBoneMatrix` resolves a `SafeBoneIndex` to). In `UpdateSecondaryHandIKPass` the hand bone indices come straight out of `Character::m_SafeBoneIndices` and are passed to `GetEffectorIdFromBoneIndex` unchanged. `Character::GetSafeIndex(SafeBoneIndex::HEAD)` therefore yields exactly the index to pass here.

That coincidence does **not** generalize. The table is keyed by the bone whose name matches the *HumanIK node's* name (see the `EffectorIdTable` def), a different naming scheme from the safe bones: the head happens to agree, but the game's shoulder safe bone is not the bone HumanIK calls a shoulder, and the lookup returns `NO_EFFECTOR`. The general reverse mapping — effector to the bone it drives — is a scan of `m_HIKNodeAndBonePairs`.

The mapping (`GetEffectorIdMapping`, node to effector) fixes the ids, bound as the [`Effector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector) enum:

| Effector | Body part |
|---|---|
| `HIPS` | Hips |
| `LEFT_ANKLE` / `RIGHT_ANKLE` | Left / Right ankle |
| `LEFT_WRIST` / `RIGHT_WRIST` | Left / Right wrist |
| `LEFT_KNEE` / `RIGHT_KNEE` | Left / Right knee |
| `LEFT_ELBOW` / `RIGHT_ELBOW` | Left / Right elbow |
| `WAIST` | Waist |
| `CHEST_END` | Chest end (`GetChestEndEffectorId`, constant) |
| `LEFT_FOOT` / `RIGHT_FOOT` | Left / Right foot |
| `LEFT_SHOULDER` / `RIGHT_SHOULDER` | Left / Right shoulder |
| `HEAD` | Head |
| `LEFT_HIP` / `RIGHT_HIP` | Left / Right hip |

Each effector's bone is the one named after its HumanIK node, and HumanIK names its nodes in MotionBuilder's convention (`HIKNodeNameFromNodeId` reads a fixed name table): the shoulder node is the upper-arm joint, so `LEFT_SHOULDER` drives the bone named `LeftArm`, `LEFT_ELBOW` drives `LeftForeArm`, and `LEFT_WRIST` drives `LeftHand`, and likewise on the right. The rig's `LeftShoulder` bone is the collar, HumanIK's separate collar node, which has no effector. The shoulder, elbow, and wrist effectors therefore span exactly the arm chain from the glenohumeral joint to the wrist.

The head effector is therefore [`Effector::HEAD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HEAD), provided the characterization uses a head node, which the humanoid rig does. The per-effector control arrays (`m_TargetReachT` and the others) are indexed by this effector id.

## Parameter and coordinate-space recipe

The prototype of [`HumanIK::AddEffectorTargetPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetPosition) is verified against the call sites:

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

**Coordinate space is character-model space**, not world. `UpdateSecondaryHandIKPass` builds `ws_to_ms = inverse(m_WorldMatrixT1)` and multiplies the world-space blackboard target through it *before* the call. A world-space head target therefore becomes `inverse(characterWorld) · desiredHeadWorld`. This is the same transform that converts a world transform into a model-space `Joint` (see `skeleton.md`).

After queuing a positional target, the engine also writes the reach weight: `m_HIK.m_TargetReachT[effector] = weight` (1.0 = full reach). Skipping this leaves the reach at zero and the target has no effect.

### The four control values, and which list drives which

Queuing a target is only half the recipe. [`HumanIK::UpdateEffectorsFromTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-UpdateEffectorsFromTargets) ends with a loop over all `EFFECTOR_SLOTS` effector slots. The loop pushes four per-effector control values into the Autodesk effector-set state:

| Call | Source array | Meaning |
|---|---|---|
| `HIKSetTranslationActive` | [`m_ReachT`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachT) | how strongly the positional target is reached |
| `HIKSetRotationActive` | [`m_ReachR`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_ReachR) | how strongly the rotational target is reached |
| `HIKSetPull` | [`m_Pull`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Pull) | how much reaching this effector **propagates into the rest of the body** |
| `HIKSetResist` | [`m_Resist`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##field-m_Resist) | how much this effector resists being displaced by *other* effectors' pull |

[`HumanIK::DriveAllCurrentEffectorControlValues`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-DriveAllCurrentEffectorControlValues) runs immediately before the solve. It populates those current arrays from the `m_Target*` ones, and it walks the pass's two target lists separately:

- The **position**-target list drives `m_ReachT[eff]` (eased by `SpeedLerp` when the target's `effector_interpolation`/`effector_blend_out` flags are set, snapped otherwise) and copies `m_Pull[eff] = m_TargetPull[eff]` **verbatim — no interpolation**.
- The **rotation**-target list drives `m_ReachR[eff]` the same way and copies `m_Resist[eff] = m_TargetResist[eff]` verbatim.

Two consequences that are easy to get wrong:

- **Pull rides the position target and resist rides the rotation target.** Writing `m_TargetPull` for an effector that has only a rotation target queued does nothing that frame, and vice versa.
- **Reach without pull is a local solve.** An effector with `ReachT` and no `Pull` arrives at its target by bending only the chain it terminates — a head target with pull 0 is absorbed by the neck and the shoulders, chest, and spine stay on the animated pose. The game's own foot/hip IK (`NStuntIKTask`) writes `m_TargetReachT` and `m_TargetPull` together for exactly this reason; the aim IK (`NRightArmAimIK`) writes only `m_TargetReachR`, which is why aiming turns the head without dragging the torso.

The distinction between the solve steps `FULL_BODY_NO_PULL` and `FULL_BODY` is the same one, expressed in the Autodesk solver bitmask: [`HumanIK::HIK_SOLVING_STEP_FULL_BODY_NO_PULL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_FULL_BODY_NO_PULL) is [`HumanIK::HIK_SOLVING_STEP_FULL_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_FULL_BODY) without the pull stages.

### Target entry lifetime and the `is_valid` flag

A pass holds at most one position entry and one rotation entry per effector: `AddEffectorTarget*` scans the list first and, on a hit, overwrites the entry in place and sets its `is_valid` flag rather than appending. `ClearTargets`, which runs after the solve, is the flag's other half: it does **not** remove a consumed entry — it clears `is_valid` and keeps the entry, removing it only once the effector's reach weight is zero *and* the entry is already invalid.

The consequences that matter to any external supplier:

- **An invalidated entry keeps steering.** `UpdateEffectorsFromTargets` applies every entry in the lists, valid or not, so a leftover keeps pushing its effector toward the (now stale) pose it was queued with while `DriveAllCurrentEffectorControlValues` runs its reach down — `SpeedLerp` toward zero at the blend-out rate when the entry carries the blend-out flag (the game's hand-pass default, `HumanIK::HAND_PASS_BLEND_OUT_RATE` per second, gives a tail of about 0.7 s from full reach), zeroed outright on the next driven frame without it. For valid entries the reach drive is the mirror image: `SpeedLerp` toward the target reach with the interpolation flag, snap without it — each flag governs only its own direction.
- **Presence in the list is not a claim; `is_valid` is.** Every system that queued recently has entries sitting in the lists mid-decay. Only a set `is_valid` means the supplier queued since the last solve — the game's own systems re-queue their active targets every frame, so "the game is driving this effector this frame" is exactly the set of valid entries at pose-finalization entry. Matching on presence alone mistakes leftovers (including one's own from the previous frame) for live targets.
- **Re-queuing per frame refreshes, never duplicates.** A supplier that re-adds each frame re-validates and overwrites the same entry before the gate, so its data never goes stale; with `interpolation=false` the reach also snaps back to the written `m_TargetReach*` each frame.

Defaults the game itself uses:

- The hand pass queues a position target from a world-space target with a per-hand `solve_step`, `pass=PASS_SECONDARY`, `interpolation=false`, the interpolation rate [`HumanIK::HAND_PASS_INTERPOLATION_RATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HAND_PASS_INTERPOLATION_RATE), `blend_out=true`, and the blend-out rate [`HumanIK::HAND_PASS_BLEND_OUT_RATE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HAND_PASS_BLEND_OUT_RATE), then writes `m_TargetReachT[eff]=weight`.
- The aim IK queues a rotation target on the body. [`RightArmAimIK::UpdateAimEffector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::arm_aim_ik::RightArmAimIK##func-UpdateAimEffector) calls `AddEffectorTargetRotationVector(m_HIK, eff, angle, &axis, SolveStep::UPPER_BODY, Pass::MAIN, interp=false, 0, blend_out=false, 0)` and writes `m_TargetReachR[eff]`. This is the concrete precedent for driving the upper body on PASS_MAIN. The step choice is a joint-mask choice. [`SolveStep::UPPER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep##variant-UPPER_BODY) admits the arms to the whole pass's solve, and [`SolveStep::SPINE_HEAD_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep##variant-SPINE_HEAD_ONLY) does not. A solver allowed to move the arms reaches a head target by swinging one, because pull does not arbitrate how a requirement is met. The step is therefore chosen from which joints may move, not from which effector is targeted.

`Solve` maps each [`SolveStep`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::SolveStep) to an Autodesk solver bitmask, bound as the `HumanIK::HIK_SOLVING_STEP_*` constants: [`SPINE_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_ONLY), [`SPINE_HEAD_ONLY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_HEAD_ONLY), [`ARMS`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_ARMS), [`UPPER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_UPPER_BODY), [`SPINE_HEAD_LOWER_BODY`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK::HIK_SOLVING_STEP_SPINE_HEAD_LOWER_BODY), `FULL_BODY_NO_PULL`, and `FULL_BODY`. A pass's effective step is the **max** of its queued targets' steps (with an arm-combining special case), so a single head target at `UPPER_BODY` pulls the whole upper body.

There are two `AddEffectorTargetRotation` overloads: the axis-enum variant [`HumanIK::AddEffectorTargetRotation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetRotation) and the explicit-axis-vector variant, bound as [`HumanIK::AddEffectorTargetRotationVector`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-AddEffectorTargetRotationVector).

### Recipe to drive the head

1. Queue before the `HasTargets(PASS_MAIN)` gate, at the latest at the entry of `UpdatePassFinalizePose_Parallel` (see "The injection seam").
2. `eff = GetEffectorIdFromBoneIndex(m_HIK, GetSafeIndex(HEAD))` (expect [`Effector::HEAD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HEAD)).
3. `posModel = inverse(characterWorldT1) · desiredHeadWorld`.
4. `AddEffectorTargetPosition(m_HIK, eff, &posModel, SolveStep::SPINE_HEAD_ONLY, Pass::MAIN, false, 3.0, false, 1.5)` — `SPINE_HEAD_ONLY` so the pass cannot recruit the arms to reach the head (see the step-as-joint-mask note above), and `blend_out=false` for a target re-queued every frame. During a blend-out tail the invalidated entry keeps steering toward the pose it was queued with (see "Target entry lifetime"). That suits a target that remains meaningful while it fades, and a target derived from the current frame's pose does not remain meaningful.
5. `m_HIK.m_TargetReachT[eff] = 1.0`.
6. `m_HIK.m_TargetPull[eff] = w` — without this the spine and shoulders do not follow (see "The four control values" above).
7. Optionally a rotation target for head orientation via `AddEffectorTargetRotationVector(..., Pass::MAIN)` + `m_TargetReachR[eff]`, and `m_TargetResist[eff]` alongside it.

The engine invalidates the pass's targets after solving, so re-queue every frame.

### Recipe to twist the torso

A head target does not reach the chest: reach is satisfied by the neck chain, and pull only propagates a *positional* requirement. To turn the torso, drive the chest-end effector directly. [`ReelInAimIK::Update`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::arm_aim_ik::ReelInAimIK##func-Update) does this to swing the chest toward the grapple hook:

```
chestEnd = GetChestEndEffectorId(m_HIK)
AddEffectorTargetRotation(m_HIK, chestEnd, yawRadians, RotationAxis::Y,
                          SolveStep::UPPER_BODY, Pass::MAIN, false, 2.0, true, 1.5)
m_HIK.m_TargetReachR[chestEnd] = weight
```

[`HumanIK::GetChestEndEffectorId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-GetChestEndEffectorId) returns the constant [`Effector::CHEST_END`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-CHEST_END). The angle is an **offset applied to the effector's current rotation**, not an absolute orientation: `UpdateEffectorsFromTargets` reads the effector's current state, builds a quaternion from the queued angle-axis, and multiplies it onto that. The shoulder effectors ([`Effector::LEFT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-LEFT_SHOULDER) and [`Effector::RIGHT_SHOULDER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-RIGHT_SHOULDER)) are children of the chest and follow it, so the chest-end target covers them for rotation.

To *displace* the torso rather than turn it, target the shoulder effectors positionally instead. A solver asked only to reach a moved head target has several ways to do it, such as bending the neck or swinging the arms, and pull does not choose among them; it only decides how far the requirement travels. Constraining the girdle directly removes the ambiguity: a positional target, `m_TargetReachT`, and `m_TargetPull` on `LEFT_SHOULDER` and `RIGHT_SHOULDER`, resolved via `GetEffectorIdFromBoneIndex` from the shoulder bones. This is the recipe `NStuntIKTask` uses for the feet and hips.

Where those shoulder targets should *go* is a rig question rather than a solver one, and the answer is a rotation: the spine articulates about its base, so a target derived by carrying each animated shoulder position through a rotation about the spine-base joint stays on the arc the rig can actually reach. Deriving it as a straight-line displacement instead asks the spine to change length, which it cannot, and cannot rotate the girdle at all when both shoulders take the same offset.

## Open risks

- **Coexistence with the aim IK.** The aim/reach IK also queues PASS_MAIN targets (chest/arm rotation). When an external caller and the aim IK both add targets in the same frame, both are solved. The pass step is the max of the queued steps, and targets are keyed by effector id, so a second add to the same effector updates the first. A head target ([`Effector::HEAD`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::Effector##variant-HEAD)) does not share an effector with the arm targets. Its interaction with a chest or spine target that the game adds in the same frame is untested.
- **Weight source.** `DriveAllCurrentEffectorControlValues` runs *before* the solve inside the pass; it eases `m_ReachT` toward `m_TargetReachT`. With `interpolation=false` the value snaps; with interpolation it ramps over several frames, so the body eases into the target rather than snapping. Choose per the desired feel.
- **`updateAllBones`.** `IKToCharacterState` takes `CCharacter::m_IkToCharacterUpdateAllBones`. How the flag changes which bones receive the solved result is not characterized. A `SetJoint` on the same bones later in the same frame, such as one at `UpdatePropEffects`, overwrites the solved joint and re-derives its descendants.
- **`hkaPose` pointer.** `CharacterToIKState`/`IKToCharacterState` take the character's `hkaPose*`; the engine invokes them inside the pass. An external caller therefore only queues targets. The engine runs the solve, and a direct call to `Solve` is not part of the sequence.
