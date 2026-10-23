# Skeleton, pose, and bone override

JC3 characters are posed via a Havok `hkaPose`. This covers how the pose is stored and updated, how to read and override individual bones, and where in the frame a change must happen to reach both the camera and the rendered mesh.

## Bone store: hkaPose, model-space

The animation controller ([`AnimationController`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController)) holds the skeleton resource ([`m_Skeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_Skeleton)) and a pose producer ([`m_PoseProducer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_PoseProducer)). The producer owns two Havok poses, a tick-spaced pair: [`m_PoseT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT0) is the previous tick's and [`m_PoseT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT1) the current one. Kicking the pose job swaps the two pointers and queues `PoseUpdate_fragment` on a CPU fragment worker, which blends the layers' clips into the new `T1`. So the buffer the job writes held the tick before last, and the previous tick's result, with everything written into it after the job (the HumanIK solve, `SetJoint` overrides), becomes `T0` as it was left. The skinning job blends `T0` to `T1` by the sub-frame fraction, which is why a bone written once per sim tick still renders smoothly between ticks.

Each [`HkaPose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaPose) keeps a local-space (parent-relative) and a model-space (character-root-relative) array of `hkQsTransform` (translation, rotation quaternion, scale; [`QsTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::QsTransform)), a per-bone flag array, and a pointer to its skeleton. The flags make the two spaces lazily consistent: reading a bone's model-space transform whose flag marks it stale first walks up to the nearest clean ancestor and recomposes the chain down from it, and a model-space write marks the descendants it propagates to.

Bones are model-space, not parent-local, at the API: a bone's world transform is `characterWorld · boneModelSpace`.

## Read / write API (Joint)

The controller exposes a model-space `Joint` API, already in the bindings:
- `AnimationController::GetBoneMatrix` builds a `Matrix4` from the model-space joint. `Character::GetSafeBoneMatrix` resolves through a hash→index table to this.
- `AnimationController::GetJoint` reads the model-space `Joint`.
- `AnimationController::SetJoint` writes a model-space `Joint` into the current pose (`T1`) and propagates to all descendants (recomputes their model space); it marks the bone local-dirty. Writing parent-first along a chain therefore works: each write carries the children not yet written, and the later writes overwrite them.

`Joint` is `{ Translation; Orientation; Scale }`, model-space; the quaternion is Havok order (x, y, z, w).

So overriding a bone is a `SetJoint`, not a buffer patch. To place the head at a desired world transform relative to the body: `desiredHeadModel = inverse(characterWorldT1) · desiredHeadWorld`, then `SetJoint(HEAD, desiredHeadModel)`. `SetJoint` re-derives descendants but not ancestors — fine for a head/eyes override (the neck above is left as the animation set it).

## Hierarchy and root

The root for "head relative to body" is the character world matrix `m_WorldMatrixT0/T1`, whose translation is the feet (the base of the collision capsule). Because everything is model-space, you don't walk the chain to place a bone: one `inverse(characterWorld)` multiply suffices.

The skeleton is a Havok [`HkaSkeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton), reached through the resource's [`Skeleton::m_HavokSkeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::Skeleton##field-m_HavokSkeleton): the bones and their names ([`m_Bones`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_Bones)), the parent of each ([`m_ParentIndices`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_ParentIndices), parents before children), and the reference pose ([`m_ReferencePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_ReferencePose), parent-relative), which the pose job samples the first layer over. The resource also keeps each bone's hashed name ([`m_BoneNameHashes`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::Skeleton##field-m_BoneNameHashes)), which [`AnimationController::GetBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneIndex) searches.

The generic male skeleton the game ships (`animations/jc_skeletons/characters/gen_male.bsk`, a Havok 2014 packfile) has 73 bones. Its body, read from the file:

- `Reference` (the root), with `Offset` under it, and with `Reference`'s other children the targets and markers: `CharacterCam1`, `TargetRef1`, `TargetRef2`, `AimTarget`, and the IK targets `LeftFootIKTarget`, `LeftToeIKTarget`, `LeftHandIKTarget`, `RightFootIKTarget`, `RightToeIKTarget`, `RightHandIKTarget`.
- `Offset` → `Hips`. `Hips` has two children: the spine, `Spine` → `Spine1` → `Spine2` → `Sternum`, and `Pelvis`, which carries the legs. The legs therefore do not hang from the spine's root but from a sibling of it.
- `Sternum` → `Neck` → `Head` → `offset_facialOrienter` → the facial bones (`fJaw` with the lips and mouth corners, `fNose`, `fMidUprLip`, `fUprLids`, `fLwrLids`, and the brows).
- `Sternum` → `LeftShoulder` (the clavicle) → `LeftArm` → `LeftForeArm` → `LeftHand`, and the right side alike. `LeftArm` also carries a twist bone (`df_LeftArmRollA01`), `LeftForeArm` another (`LeftForeArmRoll`), and `LeftHand` the attach point `LeftHandAttach` and three-bone thumb, index, and ring fingers.
- `Pelvis` → `LeftUpLeg` → `LeftLeg` → `LeftFoot` → `LeftToeBase`, and the right side alike. `RightUpLeg` carries `RightHolsterAttach`, and `Spine2` carries `BackAttach`.

Rico's skeleton (`rico.bsk`, packed inside `main_character.ee`) has at least the eye bones besides (`fLeftEye` and `fRightEye`, children of `Head`, looked up by name hash), and two back dock bones under `sm_BackAttachParent`, which hangs from `sm_UprBackHarness`, a child of `Sternum`: `BackAttach` on the right of the back below the shoulder, and `BackAttach02` on the left, a little higher. The generic male skeleton has only `BackAttach`. Which of them a holstered weapon rides is in [`WeaponBase::UpdatePostFinalizePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-UpdatePostFinalizePose). Which other bones Rico adds is to be read from a live dump.

Bone rest frames are not aligned with the model axes. An absolute model-space orientation written to a bone therefore does not describe a neutral pose, and an orientation change is composed onto the animated orientation instead. `SetJoint(HEAD)` carries the facial children along, including `fLeftEye` and `fRightEye`, as verified in-game.

## Frame order, and where to override

The pose is finalized in the SIM phase and consumed (camera, then skinning) in the RENDER phase:

    SIM:    UpdatePassFinalizePose_Parallel
              -> HumanIK pass, SyncPoses, CalculateModelSpacePose   => model-space pose finalized
              -> Character::UpdatePropEffects                       <- last call
    RENDER: GameCameraManager::UpdateRender
              -> CameraTree::UpdateRenderContexts
              -> Camera::UpdateRender
            ... KickSkinningJob -> render submit

`Character::UpdatePropEffects` is the last call in `UpdatePassFinalizePose_Parallel`. It runs in the SIM phase, after the model-space pose is built. A `SetJoint` made when it returns therefore reaches both RENDER-phase consumers of the model-space buffer: the camera update and the skinning job.

### Who reads the pose before `UpdatePropEffects`

`UpdatePropEffects` is last, so a write there reaches the camera and the skinning but nothing the pass runs before it. In order, the pass does:

1. The state tasks of the finalize range (the aim, look-at, foot, and off-hand IK tasks queue their targets), then the ground information ([`Character::UpdateGroundInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-UpdateGroundInfo)) and the weapons' first attachment refresh from the attach bones. While the character is off its support, the ground information casts a scene ray down from the root ([`Character::CalculateHeightOverGround`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CalculateHeightOverGround), through [`DoGroundInfoRaycasts`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-DoGroundInfoRaycasts)) through [`PhysicsSystem::CastRay`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-CastRay), with its hit record and filter on the stack. The post-physics pass queues this pass for the parallel update of every character, so these scene queries run on the engine's worker threads after the physics step.
2. Inside the HIK gate (see humanik.md), [`HumanIK::HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets) on the `MAIN` pass. It has this one caller in the binary. When it reports targets, the pass drives the effectors' control values, solves, and refreshes the weapons' attachments again from the solved wrists.
3. The graphics matrix, the character's own target point (what other characters aim at), the inventory's post-finalize update, and `CPoseProducer::CalculateModelSpacePose`, which syncs both poses' model space.
4. The attached effects (the parachute, the wingsuit, stunts, water), the AI entity's view of the character, and the ragdoll proxy phantom's shapes (`CRagdollProxyPhantom::SyncShapeTransformWithPose`, the bodies bullets hit), each from the current pose.
5. `UpdatePropEffects`.

So a pose written before `HasTargets` returns reaches the solve, the weapons, the hit shapes, the AI, and the effects as well as the camera and the skinning, while one written in `UpdatePropEffects` reaches only the last two: bullets and AI still see the pose as it stood before.

## Matrix basis convention

The engine matrices are D3D-style: row-major and row-vector (`clip = p · M`, `VP = View · Proj`). A transform's basis vectors are therefore its rows: `data[0..2]` is right (+X), `data[4..6]` is up (+Y), `data[8..10]` is the +Z basis (forward is -`data[8..10]`), and `data[12..14]` is the translation. The space is right-handed and Y-up (rendering §2.6). An orientation built from a look direction takes `right = normalize(cross(up, fwd))` and `trueUp = cross(fwd, right)`, and places `right`, `trueUp`, `-fwd`, and `pos` in rows 0 to 3.

## Relation to HumanIK

The engine's HumanIK pass consumes effector targets inside `UpdatePassFinalizePose_Parallel`, before `CalculateModelSpacePose`. A target queued at `Character::UpdatePropEffects` is therefore too late for the current frame's solve. The HumanIK solve blends with the animated pose, whereas `SetJoint` after finalization replaces the model-space joint outright. The solver, its gate, and the effector ids are in `humanik.md`.
