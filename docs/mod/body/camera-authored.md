# Authored camera continuity

## Controlled runtime scope

The authored-camera adapter runs in production when the normal tracked camera is enabled. It admits named takeoff, landing, recovery, floor-pickup, and hanging cases from the current body rule-state hash. The hash comes from [`AnimatedModel::m_RuleSystems`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimatedModel##field-m_RuleSystems) through the existing `active_state_type_id` reader. Generic jumping, broad movement flags, ordinary idle, walking, and running cannot admit the adapter. There is no default-off authored-camera switch.

Admission uses the following exact shipped AFSM names and their explicit ASB state-to-animation records. The names are hashed through the existing native helper, rather than stored as numeric hash literals.

| Case | Exact states | Runtime treatment |
|---|---|---|
| Boosted ground takeoff | `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO`, `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND` | The into state establishes entry. The follow-on retains its baseline. A fresh follow-on without entry is refused. |
| Landing | `S_LAND_FAST_FALL`, `S_LAND_HARD`, `S_LAND_IDLE_0`, `S_LAND_IDLE_90L`, `S_LAND_IDLE_90R`, `S_LAND_IDLE_180L`, `S_LAND_IDLE_180R`, `S_LAND_RUN_0` | Each exact landing motion establishes its own entry baseline. A different landing motion starts a new run. Ordinary running remains excluded. |
| Recovery | `S_GETUP_POSEMATCHING`, `S_GETUP_BLENDIN`, `S_GETUP` | The observed first recovery state establishes entry. The ordered pose-matching, blend-in, and get-up handoffs retain the baseline. A reverse handoff starts a new run. |
| Floor pickup | `S_IDLE_PICKUP` | The explicit idle pickup state establishes entry. Heavy-weapon pickup and other interactions remain excluded. |
| Hanging | `S_GRAPPLE_ATTACH_0`, `S_IDLE_HANG_STUNT`, `S_IDLE_HANG_STUNT_120L`, `S_IDLE_HANG_STUNT_120R`, `S_IDLE_HANG_STUNT2` | Each exact hanging motion establishes its own entry baseline. Different hanging motions start new runs. Grapple flags and unverified state names do not substitute for these states. |

The landing ASB records bind the idle, hard, fast-fall, and split running-landing clips. The recovery records bind the same directional get-up clip set across the ordered AFSM handoffs. The get-up sequence is the recovery path after ragdoll, including explosion-triggered ragdoll; it does not identify the original cause. Full ragdoll and crash-ragdoll states remain unsupported. Recovery and landing use the current native model-space camera channel without claiming a separately measured outgoing clip contribution or a fully characterized blend interval.

The floor-pickup ASB record binds the idle pickup clip. Its idle entry uses the weapon-pickup action and interaction verification. The hanging ASB record for `S_GRAPPLE_ATTACH_0` binds the reeled-in split idle clips, while the stunt states have their own explicit animation records. The inspected reeled-in base and rifle-aim leaves encode identity camera and Reference channels. An identity channel still forms an active runtime pair and suppresses ordinary animated-anchor bob; it does not manufacture authored camera motion. The stunt leaves' complete camera curves are not independently characterized.

Unrecognized states report `OrdinaryOrUnsupportedState`. `S_JC_WINGSUIT_IDLE` reports `IdleTailWeightNotBound`. A missing state, channel, tracked head, or valid transform retains normal tracking. The Camera debug page displays `authored::status()`: `NamedCase` includes the case, entry or live phase, and `ExactStatesOnlyTreeSampledNoBlendTail`; `Handback` includes the previous case and exit refusal; `NormalTracked` includes the refusal. Status reports runtime admission, not evidence that the view was inspected in a live game.

## Native channel and root treatment

The adapter resolves the named `CharacterCam1` channel with [`AnimationController::GetBoneIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneIndex). The checked takeoff asset gives the numbered camera channels equal full TRS curves. The adapter does not alias the safe `CAMERA` slot: that slot can fall back to the root when its unnumbered bone name is absent. A root result, an out-of-range result, a name-table mismatch, a missing skeleton resource, a mismatched pose skeleton, or missing pose arrays rejects capture for every case.

[`AnimationController::GetBoneMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneMatrix) returns the current hierarchy-composed model-space matrix. It already includes the `Reference` ancestor. The adapter retains scale, orientation, and translation together, rejects non-finite or singular transforms, and rejects matrices that cannot be represented by the TRS model without shear.

The camera channel's raw parent-local displacement is not an additive eye offset. The takeoff channel counteracts movement in its parent and extracted root. The adapter therefore uses the hierarchy-composed model-space matrix and keeps the live character world root in the view base. It does not compose the world root into the channel delta again. It captures the first accepted sample as the run baseline; it does not treat the channel's near-zero origin as eye height or use a scratch asset curve as runtime data.

With column-vector matrices, model-space camera channel `L`, accepted entry sample `L_b`, world-root-derived view base `B`, and local tracked head `H`, composition is `D = inverse(L_b) * L` and `V = B * D * H`. The production path calls the same `Pair::deltas` and `compose_view` functions as the pure fixtures. `RootTreatment::SeparateFromAuthoredLocal` records that the world root is supplied by `B`. The root-compensated alternative remains a separate tested operation; the runtime adapter does not select it for an already model-space channel.

## Tracking and animation sampling

Capture runs immediately after the original camera-tree update, before the mod replaces its contexts. The native tree's attachment modifiers read the same current model-space bone API at this phase. The adapter reads no new raw pointer offsets and retains no native pointer after capture. It publishes owned root/channel TRS values, a source identity, and a selected camera pair behind a mutex.

The adapter recognizes new samples from the exchange of [`PoseProducer::m_PoseT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::pose_producer::PoseProducer##field-m_PoseT1). Repeated tree calls with the same buffer reuse history and do not manufacture two animation times. Entry supplies one sample and leaves the normal tracked owner active. A second observed buffer forms the first authored pair. The takeoff into-to-follow-on handoff and the ordered recovery handoffs preserve the entry baseline. A changed case or a distinct landing or hanging motion clears the previous run, even when the pose buffer address is unchanged.

Character, controller, producer, skeleton, or channel replacement clears history. A large root-position discontinuity also rejects the pair. Every history clear resets the graph state, baseline, pair, and selected result, so a later takeoff can enter after an earlier follow-on. Loading, disabled placement, tracking loss, invalid transforms, and source loss clear the result immediately.

This sampling is sufficient for the bounded controlled test, but it is not an engine finalization generation counter. More than one pose update between tree reads can reuse a buffer address and escape detection. A time-only update can leave the buffer unchanged. There is no bound current clip-time reader, so the adapter cannot verify a clip restart inside an unchanged exact state. A same-state restart without an observed intervening exit therefore remains a limitation. The debug status names this tree-sampled limitation. A later finalization publication should replace buffer-exchange observation without adding another pose writer.

The view base uses the sampled world root and the accepted entry's body-local eye offset. It holds that animated offset fixed during ownership, suppressing normal animated head and neck bob. The tracked input retains the published pose's translation relative to its matching head anchor and its orientation relative to the root. `compose_view` applies that local tracked transform once after the authored delta. The adapter does not write the skeleton, publish a second headpose, or run a second HumanIK solve.

## Camera consumers and transitions

The tree hook writes the selected previous/current full matrices to the camera contexts, render contexts, listener transforms, and alternate-aim transforms. The render hook consumes that same owned pair rather than sampling again or applying another authored transform. `always_use_t1` collapses the selected pair only after composition. The simulation-camera cache publishes the selected current matrix. Source loss clears it before normal tracking republishes its current matrix. The scoped grapple aim override retains precedence in the camera getter.

The first accepted sample uses normal tracking. The first authored pair starts at the captured baseline, so its previous authored delta is identity. When an exact state exits to an unsupported state, the adapter hands back from its current applied correction over the live normal tracked view. Translation and scale interpolate to identity, and rotation interpolates to identity, with a smooth endpoint curve. The handback does not hold the old world matrix or suspend head tracking. It is a mod-side camera transition, not a claim about outgoing animation weight. Teleports, source changes, invalid transforms, loading, and tracking loss do not retain this correction.

The adapter does not guess the outgoing blend contribution. `S_JC_WINGSUIT_IDLE` remains unsupported even when the previous takeoff clip contributes to the native pose; its exit can use the same camera handback. The existing pure policy supports a measured outgoing takeoff blend, but native exact-state frames provide neither fabricated weight nor fabricated clip time. A `WingsuitIdle` frame without current source-weight evidence is rejected. A real animation tail needs the accepted source's contribution for the same finalized pose. The camera handback duration and broad movement flags do not substitute for that evidence.

Entry comfort, transitions between different accepted motions, the residual root/channel cancellation, get-up pose-matching behavior, landing variants, and timing under skipped tree samples require live testing. Admission of these named cases is implemented; complete per-clip blend characterization and live camera acceptance are not established by the unit fixtures.

## Binding and integration follow-up

The executable exact-state path uses existing bindings. It needs no new function ABI, numeric state hash, safe-bone alias, pointer offset, or generated binding edit. Engine-owned name strings are passed through the existing native hash helper, matching the existing rule-state reader.

More precise animation-tail support needs typed bindings for the body driver's current and outgoing animation instances, the current source's finalized contribution weight, and absolute clip time. These fields require focused verification before binding. A finalized-pose hook also needs a shared publication call that captures the named channel and its matching root once at a known eligible completion. That hook belongs to the character/body writer and is not duplicated by this adapter. No unverified address or layout is proposed for these missing items.

## Fixtures and verification

Composition fixtures cover hierarchy/root inverse cancellation, full TRS and noncommuting rotation order, a nonidentity baseline, one tracked-head composition, unknown root semantics, invalid transforms, identity changes, and mixed-time pseudo-pairs. Temporal fixtures cover entry, takeoff handoff, measured idle-tail behavior, refusal of an unmeasured tail, ordinary locomotion exclusion, per-case admission mismatch, distinct landing and hanging identities, ordered recovery handoffs, interrupted exits, source loss, teleports, and stale weights.

Runtime fixtures exercise the production native-sample-to-policy path for all named families, including an active identity hanging channel. They cover repeated tree samples, repeated takeoff after a follow-on and clear, same-buffer entry after a changed motion, recovery baseline continuity, handback from the current correction with continuing tracking, and tracking without animated-anchor bob. Classifier fixtures cover the exact admitted names and reject ordinary locomotion, full ragdoll, unrelated interactions, unverified grapple names, and unsupported landing transitions.

No build, test runner, formatter, staging, or commit command runs as part of this change. The coordinator owns the frozen validation batch. Source inspection and `git diff --check` do not establish compilation or live camera behavior.
