# Skeleton, pose, and bone override

JC3 characters are posed through a Havok `hkaPose`. This page covers how the pose is stored, the skeleton's bones, how to read and override a bone, and where in the frame a change reaches the camera and the rendered mesh.

## Bone store: hkaPose, model-space

The animation controller ([`AnimationController`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController)) holds the skeleton resource ([`m_Skeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_Skeleton)) and a pose producer ([`m_PoseProducer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##field-m_PoseProducer)) that owns a tick-spaced pair of Havok poses: [`m_PoseT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT0), the previous tick's, and [`m_PoseT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT1), the current one. The pose update swaps the two pointers and blends the layers' clips into the new `T1` ([animation-layers.md, "The pose job"](animation-layers.md#the-pose-job)); producer-level asynchronous jobs (`PoseUpdate_fragment` on a CPU fragment worker) are a separate mechanism. So the buffer being written held the tick before last, and the previous tick's result, with everything written into it afterwards (the HumanIK solve, `SetJoint` overrides), becomes `T0` as it was left. The skinning job blends `T0` to `T1` by the sub-frame fraction, so a bone written once per sim tick renders smoothly between ticks.

Each [`HkaPose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaPose) keeps a local-space (parent-relative) and a model-space (character-root-relative) array of `hkQsTransform` (translation, rotation quaternion, scale; [`QsTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::QsTransform)), a per-bone flag array, and a pointer to its skeleton. The flags keep the two spaces lazily consistent: reading a stale model-space transform walks up to the nearest clean ancestor and recomposes the chain down, and a model-space write marks the descendants it propagates to.

At the API bones are model-space: a bone's world transform is `characterWorld · boneModelSpace`, where the character world matrix `m_WorldMatrixT0`/`T1` has its translation at the feet (the base of the collision capsule). Placing a bone therefore takes one `inverse(characterWorld)` multiply, not a walk of the chain.

## Read and write API (Joint)

The controller's model-space `Joint` API, in the bindings:

- `AnimationController::GetBoneMatrix` builds a `Matrix4` from the model-space joint; `Character::GetSafeBoneMatrix` resolves through a hash-to-index table to it.
- `AnimationController::GetJoint` reads the model-space `Joint`.
- `AnimationController::SetJoint` writes a model-space `Joint` into the current pose (`T1`), marks the bone local-dirty, and recomputes the model space of all its descendants, but not its ancestors. Writing parent-first along a chain therefore works, each write carrying the children not yet written.

`Joint` is `{ Translation; Orientation; Scale }`, model-space, with the quaternion in Havok order (x, y, z, w). Overriding a bone is a `SetJoint`, not a buffer patch: to place the head at a world transform, `SetJoint(HEAD, inverse(characterWorldT1) · desiredHeadWorld)`, which leaves the neck as the animation set it and carries the facial children along, `fLeftEye` and `fRightEye` included (verified in-game).

Bone rest frames are not aligned with the model axes, so an absolute model-space orientation written to a bone does not describe a neutral pose; an orientation change is composed onto the animated orientation instead.

## The skeleton

The skeleton is a Havok [`HkaSkeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton), reached through [`Skeleton::m_HavokSkeleton`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::Skeleton##field-m_HavokSkeleton): the bones and their names ([`m_Bones`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_Bones)), each bone's parent ([`m_ParentIndices`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_ParentIndices), parents before children), and the parent-relative reference pose ([`m_ReferencePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::HkaSkeleton##field-m_ReferencePose)) the pose job samples the first layer over. The resource also keeps each bone's hashed name ([`m_BoneNameHashes`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::skeleton::Skeleton##field-m_BoneNameHashes)), which [`AnimationController::GetBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneIndex) searches.

The generic male skeleton (`animations/jc_skeletons/characters/gen_male.bsk`, a Havok 2014 packfile) has 73 bones:

- `Reference` (the root), with `Offset` under it, and with `Reference`'s other children the targets and markers: `CharacterCam1`, `TargetRef1`, `TargetRef2`, `AimTarget`, and the IK targets `LeftFootIKTarget`, `LeftToeIKTarget`, `LeftHandIKTarget`, `RightFootIKTarget`, `RightToeIKTarget`, `RightHandIKTarget`.
- `Offset` → `Hips`, whose two children are the spine (`Spine` → `Spine1` → `Spine2` → `Sternum`) and `Pelvis`, which carries the legs. The legs hang from a sibling of the spine's root, not from it.
- `Sternum` → `Neck` → `Head` → `offset_facialOrienter` → the facial bones (`fJaw` with the lips and mouth corners, `fNose`, `fMidUprLip`, `fUprLids`, `fLwrLids`, and the brows).
- `Sternum` → `LeftShoulder` (the clavicle) → `LeftArm` → `LeftForeArm` → `LeftHand`, and the right side alike. `LeftArm` carries a twist bone (`df_LeftArmRollA01`), `LeftForeArm` another (`LeftForeArmRoll`), and `LeftHand` the attach point `LeftHandAttach` and three-bone thumb, index, and ring fingers.
- `Pelvis` → `LeftUpLeg` → `LeftLeg` → `LeftFoot` → `LeftToeBase`, and the right side alike. `RightUpLeg` carries `RightHolsterAttach`, and `Spine2` carries `BackAttach`.

Rico's skeleton (`rico.bsk`, packed inside `main_character.ee`) adds at least the eye bones (`fLeftEye` and `fRightEye`, children of `Head`, looked up by name hash) and two back dock bones under `sm_BackAttachParent`, which hangs from `sm_UprBackHarness`, a child of `Sternum`: `BackAttach` on the right of the back below the shoulder, and `BackAttach02` on the left, a little higher. Which a holstered weapon rides is in [`WeaponBase::UpdatePostFinalizePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-UpdatePostFinalizePose). Its other additions are to be read from a live dump.

### The camera markers in Rico's clips

Rico's skeleton has four camera markers, `CharacterCam1` to `CharacterCam4`, each a direct child of `Reference` with an identity rest pose; there is no bone named `CharacterCam` or `CAMERA`. Only the split lower-body clips carry tracks for them. Across the clips Rico's base graph binds (read from the `.ban` files with the packfile reader in `tools/door_handles`, states mapped to clips through the base animation set's state-to-animation records), `CharacterCam1` falls into three classes:

- In most clips that move it, its model-space transform is the inverse of the clip's extracted root motion at every frame, to within a few millimetres and degrees: the marker stays where the clip started while the body travels. The locomotion landings, the boosted takeoff from the ground, the wingsuit and parachute landing slides, the ledge and climb clips, and the vehicle stunt and exit clips (the upside-down rotations and the wingsuit barrel rolls included) are of this class.
- In many it holds identity or a constant offset: the hard and idle landings, the get-ups, the idle floor pickup, the reeled-in grapple idles, and `rico_grpl_newhang_idle`.
- In a few it moves beyond the root motion. `rico_grpl_newhang_land` (bound by `S_GRAPPLE_PRE_UPSIDEDOWN`, with no root motion) pitches the marker a quarter-turn back to identity while raising it about 1.4 m over its first 0.7 s, and the newhang idle holds that endpoint. The boosted takeoff from water adds about 20° of pitch to its inverse root motion, the boosted underwater frog kick a half-turn of roll, one grapple reel onto a motorbike a few degrees of roll, and the wingsuit flinches a sideways sway of under 10 cm.

Thirty base-graph states bind no clip through their animation node's direct children and modifiers, among them the full and crash ragdolls, `S_JC_WINGSUIT_TO_FALL`, `S_JC_PARACHUTE_RUN_LAND`, and the left, right, and alternate hang-stunt idles; whether they reach clips through nested blend nodes is unverified. How the engine consumes the marker is not established.

## Frame order, and where to override

The pose is finalized in the SIM phase and consumed in the RENDER phase:

    SIM:    UpdatePassFinalizePose_Parallel
              -> HumanIK pass, SyncPoses, CalculateModelSpacePose   => model-space pose finalized
              -> Character::UpdatePropEffects                       <- last call
    RENDER: GameCameraManager::UpdateRender
              -> CameraTree::UpdateRenderContexts
              -> Camera::UpdateRender
            ... KickSkinningJob -> render submit

A `SetJoint` made when `Character::UpdatePropEffects` returns therefore reaches both RENDER-phase consumers of the model-space buffer, the camera update and the skinning job. HumanIK blends its solve with the animated pose, whereas a `SetJoint` after finalization replaces the joint outright.

### Who reads the pose before `UpdatePropEffects`

The finalize pass's full order is in [humanik.md, "Per-frame lifecycle"](humanik.md#per-frame-lifecycle). Before `UpdatePropEffects` it runs the IK solve and the weapons' attachment refreshes, the character's target point (what other characters aim at), the attached effects, the AI entity's view of the character, and the ragdoll proxy phantom's shapes (`CRagdollProxyPhantom::SyncShapeTransformWithPose`, the bodies bullets hit). So a pose written before [`HumanIK::HasTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::ik::HumanIK##func-HasTargets) returns reaches all of those as well as the camera and the skinning, while one written in `UpdatePropEffects` reaches only the last two: bullets and AI still see the pose as it stood before, and an effector target queued there misses the frame's solve.

## Matrix basis convention

The engine matrices are D3D-style: row-major and row-vector (`clip = p · M`, `VP = View · Proj`). A transform's basis vectors are its rows: `data[0..2]` is right (+X), `data[4..6]` is up (+Y), `data[8..10]` is the +Z basis (forward is -`data[8..10]`), and `data[12..14]` is the translation. The space is right-handed and Y-up ([rendering.md, "The VP-rebuild recipe"](../rendering/rendering.md#26-the-vp-rebuild-recipe)). An orientation built from a look direction takes `right = normalize(cross(up, fwd))` and `trueUp = cross(fwd, right)`, and places `right`, `trueUp`, `-fwd`, and `pos` in rows 0 to 3.
