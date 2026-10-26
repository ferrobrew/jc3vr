# Authored camera continuity

## Controlled runtime scope

The authored-camera adapter runs in production when the normal tracked camera is enabled. It admits only the exact boosted ground-takeoff states `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO` and `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND`. The current body rule-state hash comes from [`AnimatedModel::m_RuleSystems`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##field-m_RuleSystems) through the existing `active_state_type_id` reader. Generic jumping, the broad ground-takeoff flag, ordinary idle, walking, and running cannot admit the adapter.

The adapter is a controlled runtime path, not complete support for every requested animation case. Landing, explosion recovery, button-based floor pickup, and hanging retain the normal tracked view. The known pickup and hanging state names report specific refusals. Other unsupported states report `OrdinaryOrUnsupportedState`. A missing state, channel, tracked head, or valid transform also retains normal tracking. There is no default-off authored-camera switch.

The Camera debug page displays `authored::status()`. `BoostedGroundTakeoff` includes the entry or live phase and `ExactStatesOnlyTreeSampledNoBlendTail`. `NormalTracked` includes the refusal. The status is an admission result, not evidence that the current view was inspected in a live game.

## Native channel and root treatment

The adapter resolves the named `CharacterCam1` channel with [`AnimationController::GetBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneIndex). The checked takeoff asset gives the numbered camera channels equal full TRS curves. The adapter does not alias the safe `CAMERA` slot: that slot can fall back to the root when its unnumbered bone name is absent. A root result, an out-of-range result, a name-table mismatch, a missing skeleton resource, a mismatched pose skeleton, or missing pose arrays rejects capture.

[`AnimationController::GetBoneMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneMatrix) returns the current hierarchy-composed model-space matrix. It already includes the `Reference` ancestor. The adapter retains scale, orientation, and translation together, rejects non-finite or singular transforms, and rejects matrices that cannot be represented by the TRS model without shear.

The camera channel's raw parent-local displacement is not an additive eye offset. The takeoff channel counteracts movement in its parent and extracted root. The adapter therefore uses the hierarchy-composed model-space matrix and keeps the live character world root in the view base. It does not compose the world root into the channel delta again. It captures the first accepted sample as the run baseline; it does not treat the channel's near-zero origin as eye height or use a scratch asset curve as runtime data.

With column-vector matrices, model-space camera channel `L`, accepted entry sample `L_b`, world-root-derived view base `B`, and local tracked head `H`, composition is `D = inverse(L_b) * L` and `V = B * D * H`. The production path calls the same `Pair::deltas` and `compose_view` functions as the pure fixtures. `RootTreatment::SeparateFromAuthoredLocal` records that the world root is supplied by `B`. The root-compensated alternative remains a separate tested operation; the runtime adapter does not select it for an already model-space channel.

## Tracking and animation sampling

Capture runs immediately after the original camera-tree update, before the mod replaces its contexts. The native tree's attachment modifiers read the same current model-space bone API at this phase. The adapter reads no new raw pointer offsets and retains no native pointer after capture. It publishes owned root/channel TRS values, a source identity, and a selected camera pair behind a mutex.

The adapter recognizes new samples from the exchange of [`PoseProducer::m_PoseT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT1). Repeated tree calls with the same buffer reuse history and do not manufacture two animation times. Entry supplies one sample and leaves the normal tracked owner active. A second observed buffer forms the first authored pair. The into-to-follow-on handoff preserves the entry baseline. Character, controller, producer, skeleton, or channel replacement clears history. A large root-position discontinuity also rejects the pair. Loading, disabled placement, source loss, and unsupported states clear history and the selected camera result.

This sampling is sufficient for the bounded controlled test, but it is not an engine finalization generation counter. More than one pose update between tree reads can reuse a buffer address and escape detection. A time-only update can leave the buffer unchanged. There is no bound current clip-time reader, so the adapter cannot verify a clip restart inside an unchanged exact state. The debug status names this tree-sampled limitation. A later finalization publication should replace buffer-exchange observation without adding another pose writer.

The view base uses the sampled world root and the accepted entry's body-local eye offset. It holds that animated offset fixed during ownership, suppressing normal animated head and neck bob. The tracked input retains the published pose's translation relative to its matching head anchor and its orientation relative to the root. `compose_view` applies that local tracked transform once after the authored delta. The adapter does not write the skeleton, publish a second headpose, or run a second HumanIK solve.

## Camera consumers and transitions

The tree hook writes the selected previous/current full matrices to the camera contexts, render contexts, listener transforms, and alternate-aim transforms. The render hook consumes that same owned pair rather than sampling again or applying another authored transform. `always_use_t1` collapses the selected pair only after composition. The simulation-camera cache publishes the selected current matrix. Source loss clears it before normal tracking republishes its current matrix. The scoped grapple aim override retains precedence in the camera getter.

The first accepted sample uses normal tracking. The first authored pair starts at the captured baseline, so its previous authored delta is identity. The normal tracked view resumes immediately when the exact state ends. The adapter does not guess the outgoing blend contribution. `S_JC_WINGSUIT_IDLE` reports `IdleTailWeightNotBound`, even when the previous takeoff clip can still contribute to the native pose. Interrupted destinations also return to normal tracking. Entry/exit comfort, abrupt handoffs, the residual root/channel cancellation, and timing under skipped tree samples require live testing.

The existing policy still supports a measured outgoing blend in its pure input contract. Native exact-state frames provide neither a fabricated weight nor fabricated clip time. A `WingsuitIdle` frame without current source-weight evidence is rejected. A real tail needs the accepted source's contribution for the same finalized pose. No wall-clock timeout or broad wingsuit flag substitutes for that evidence.

## Binding and integration follow-up

The executable exact-state path uses existing bindings. It needs no new function ABI, numeric state hash, safe-bone alias, pointer offset, or generated binding edit. Engine-owned name strings are passed through the existing native hash helper, matching the existing rule-state reader.

Full completion needs typed bindings for the body driver's current and outgoing animation instances, the current source's finalized contribution weight, and absolute clip time. These fields require focused verification before binding. The finalized-pose hook also needs a shared publication call that captures the named channel and its matching root once at a known eligible completion. That hook belongs to the character/body writer and is not duplicated by this adapter. No unverified address or layout is proposed for these missing items.

## Fixtures and verification

Composition fixtures cover hierarchy/root inverse cancellation, full TRS and noncommuting rotation order, a nonidentity baseline, one tracked-head composition, unknown root semantics, invalid transforms, identity changes, and mixed-time pseudo-pairs. Temporal fixtures cover entry, internal handoff, measured idle-tail behavior, exact-state refusal of an unmeasured tail, ordinary locomotion exclusion, interrupted exits, source loss, teleports, and stale weights. Runtime fixtures cover repeated tree samples, the native input policy path, and tracking without animated-anchor bob. Classifier fixtures admit only the exact known ground sequence.

No build, test runner, formatter, staging, or commit command runs as part of this change. The coordinator owns the frozen validation batch. A source review and `git diff --check` do not establish compilation or live camera behavior.
