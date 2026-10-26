# Authored camera continuity

## Runtime admission

The authored-camera adapter runs when the normal tracked camera is enabled. Only exact shipped body rule states admit takeoff, landing, recovery, floor-pickup, and hanging runs. Broad movement flags, generic jumping, ordinary idle, walking, and running do not admit a run. There is no default-off authored-camera switch.

| Case | Exact states | Continuity |
|---|---|---|
| Boosted ground takeoff | `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND` | Into establishes entry; the ordered follow-on retains its baseline. A fresh follow-on is refused. |
| Landing | `S_LAND_FAST_FALL`, `S_LAND_HARD`, `S_LAND_IDLE_0`, `S_LAND_IDLE_90L`, `S_LAND_IDLE_90R`, `S_LAND_IDLE_180L`, `S_LAND_IDLE_180R`, `S_LAND_RUN_0` | Each distinct landing establishes a new run. |
| Recovery | `S_GETUP_POSEMATCHING`, `S_GETUP_BLENDIN`, `S_GETUP` | Ordered pose-matching, blend-in, and get-up handoffs retain entry. Reverse handoffs restart. |
| Floor pickup | `S_IDLE_PICKUP` | The exact idle pickup establishes entry. Heavy-weapon pickup is excluded. |
| Hanging | `S_GRAPPLE_ATTACH_0`, `S_IDLE_HANG_STUNT`, `S_IDLE_HANG_STUNT_120L`, `S_IDLE_HANG_STUNT_120R`, `S_IDLE_HANG_STUNT2` | Distinct hanging motions establish new runs. Grapple flags are not admission. |

The shipped AFSM and explicit ASB state-to-animation records supply these names. Landing records bind idle, hard, fast-fall, and split running-landing leaves. Recovery records bind directional get-up clips across the ordered states; this identifies recovery after ragdoll, not the cause of ragdoll. Full ragdoll and crash-ragdoll are unsupported. Pickup binds the idle pickup leaf. The grapple-attach record binds reeled-in split idle leaves. The inspected reeled-in base and rifle-aim leaves have identity camera and Reference channels; an identity channel remains an active pair without manufacturing motion. Complete stunt camera curves are not independently characterized.

## Coherent native evidence

The synchronous [`AnimationController::UpdateAnimations`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-UpdateAnimations) hook calls `authored::animation_update` after the original evaluation. It copies owned evidence and advances a publication serial. The original freeze policy and native call are unchanged. The time-only [`AnimationController::UpdateAnimationsTime`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-UpdateAnimationsTime) hook invalidates eligibility after its original call: advancing time without a new pose cannot certify a matching pose.

The snapshot follows the existing first body part through [`AnimatedBodyPart::m_AnimationDriver`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::AnimatedBodyPart##field-m_AnimationDriver). At the camera seam, that driver's rule system must match the character's first body rule system. The snapshot records current graph state, playing clips, and the blender's live samplers, including easing-out clips. Each clip has its instance and shared-owner identity, animation resource handle, current control binding, exact native time, duration, loop mode, overflow and underflow counts, total control weight, and camera-track mask. The reader rejects missing owners or bindings, malformed arrays, non-finite or out-of-range values, duplicate sampler identities, and playing clips absent from the live sampler set.

The source generation also includes controller, pose producer and its owner, skeleton resource handle, skeleton, driver, rule system, blender, and camera channel. No native pointer survives a read. At the existing camera-tree seam, the adapter requires the current source, pose buffer, and full animation snapshot to match the synchronous publication. It checks again after reading the matrix. A driver, time, binding, or blend change after pose evaluation is refused rather than attached to the old pose. Missing or time-only evidence is reported explicitly.

A new publication serial, rather than a changed buffer address, identifies a new sample. Repeated camera reads of one serial reuse owned history. Two completed updates that reuse a pose buffer remain distinct. A same-state clip-source replacement or unexplained native-time decrease resets the baseline even if neither graph state nor buffer changes. A backwards time is accepted as a forward loop only on the same source and duration, with increased native overflow count and unchanged underflow count. Counter reset, reverse playback, invalid time, or changed binding is not treated as a wrap. Ordered graph handoffs can select different clips without pretending their local clocks are one absolute clock.

This publication covers the synchronous update path, not arbitrary asynchronous pose completion. The existing camera seam supplies serialized native access; the owned snapshot and serial checks are not a lock for engine worker writes. An asynchronous-only path has no eligible synchronous observation. A path that advances driver time after evaluation must invalidate eligibility. These constraints require live verification under skipped updates and loading; the adapter does not introduce a second pose writer or a speculative async hook.

## Native contribution and outgoing tails

The accepted playing clip set establishes source identity for a run. On exit from an admitted state, the adapter follows those exact sources in the blender's current sampler list, even across further native interruptions. It does not admit the new unsupported state or adopt its clips. Outgoing ownership continues only while at least one accepted source has positive native total control weight and a positive bound camera-track mask. Missing membership or zero camera contribution completes the native tail. Same-source time reversal during a tail is refused.

Total control weight is not a normalized per-bone contribution. The camera-track map and mask establish presence, and the native weight establishes that the sampler remains active. The adapter does not sum them into a fabricated native blend weight, infer weight from an ASB timeout, or multiply an already blended matrix by the weight. The hierarchy matrix contains the actual engine blend, including the incoming pose, normalization, and reference filling. This bounds the claim: source-presence evidence controls lifetime, while the native blended camera channel controls motion. A separately isolated outgoing camera transform and a complete per-layer normalized contribution are not measured.

`S_JC_WINGSUIT_IDLE` cannot start a run. It can retain the preceding takeoff only through the same accepted-source evidence as every other outgoing tail. With no prior admitted source it remains unsupported. A native tail is distinct from the mod-side camera handback described below.

## Native channel and root treatment

The adapter resolves `CharacterCam1` with [`AnimationController::GetBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneIndex). It does not alias the safe `CAMERA` slot, whose unnumbered name can be absent and fall back to root. Root fallback, range failure, name-table mismatch, missing resource, mismatched pose skeleton, or absent arrays rejects capture.

[`AnimationController::GetBoneMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneMatrix) returns the current hierarchy-composed model-space matrix, including the Reference ancestor. Full translation, rotation, and scale are retained; non-finite, singular, or sheared transforms are rejected. The world root remains separate. Parent-local camera displacement is not added to eye height.

For column-vector matrices, with model-space channel `L`, entry `L_b`, world-root-derived base `B`, and local tracked head `H`, the adapter uses `D = inverse(L_b) * L` and `V = B * D * H`. The native blend is already in `L`. The first accepted sample leaves normal tracking active; the second creates the first authored pair. The body-local eye offset is frozen at entry, suppressing animated head-anchor bob, while the current root and tracked displacement remain live. The tracked transform is composed once after the authored delta. The adapter neither writes the skeleton nor publishes another head pose.

## Consumers and handback

The tree hook writes the selected previous/current matrices to camera contexts, render contexts, listeners, and alternate aim. The render hook consumes the same owned pair without resampling or applying another authored transform. `always_use_t1` collapses only after composition. The simulation-camera cache publishes the selected current matrix, and scoped grapple aim retains precedence.

After native contribution ends, handback starts from the current applied correction over the live normal tracked view. Translation, scale, and rotation interpolate to identity with a smooth endpoint. This is a mod-side transition, not an outgoing animation weight. Character or source replacement, teleport epoch change, a large root discontinuity, invalid transforms, tracking loss, loading, and read failure clear both authored history and handback immediately. The continuity guard remains active throughout handback and updates its checked root each capture.

The Camera debug status reports entry, live, or native tail with `SynchronousSourceVerifiedNativeBlend`; `Handback` names the previous case and exit refusal. `NormalTracked` reports missing, invalid, unsupported, or incoherent evidence. Runtime status does not establish live visual acceptance.

## Fixtures and validation limits

Composition fixtures cover hierarchy/root cancellation, full TRS, noncommuting rotation order, nonidentity baseline, one tracked head, invalid transforms, and mixed-time pairs. Policy fixtures retain explicit scalar-blend tests separately from native contribution gates. Adapter fixtures cover every named family, repeated same-state restarts, same-buffer new serials, counter-proven loop wrap versus reset, binding replacement, outgoing source membership across interruptions, zero camera masks, native weight tails without double weighting, head-once composition, invalid and missing evidence, stale serials, ordered handoffs, and handback epoch/source/root guards.

No Rust build, test runner, formatter, staging, or commit command runs in this task. The coordinator owns binding publication, shared-hook integration, and the frozen validation batch. Source inspection and `git diff --check` do not establish compilation, live camera behavior, complete per-clip blend characterization, or visual comfort in the five cases.
