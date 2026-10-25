# Authored camera continuity

## Availability

Selective authored-camera following remains inactive. `payload/src/hooks/camera/authored/mod.rs` returns `Status::Unavailable`. The composition and temporal ownership modules compile only under `cfg(test)`. No native capture, camera-owner activation, configuration switch, state hash, or safe-bone alias is added.

The bounded candidate is boosted ground takeoff. The current asset contains full local camera motion that nearly cancels when composed with its hierarchy and extracted root. That result does not establish an eye origin, a runtime channel, or the root treatment used by the running game. Scratch asset curves are not runtime samples. The other requested animation cases remain unsupported.

The original camera ownership contract is retained: ordinary locomotion does not select authored following, unavailable source data leaves normal head tracking active, and the tracked head transform is composed exactly once. Source loss does not pause animation, freeze the view, or substitute the complete game-camera framing transform.

## Full-transform composition

The pure model accepts two distinct finalized samples. Each sample contains a full root TRS and full authored-local TRS, including the camera channel's local hierarchy. Each component carries a character identity, a continuity epoch, and a finalized sample identity. The root and local identities must match within each sample. The previous and current samples share the character and continuity epoch, and their finalized identities must increase. Equal transform values at different sample identities remain valid. A single current bone attached to two root endpoints does not form a pair.

The baseline belongs to the explicitly accepted animation run and uses the same character and continuity epoch. It is not a measured eye position. Baseline changes invalidate the temporal pair. Character replacement, continuity changes, teleports, missing samples, invalid transforms, and reversed sample order also invalidate the pair.

`DeltaRequest` provides separate operations. With column-vector matrices, root `R`, hierarchy-composed local channel `L`, and baseline suffix `b`, the operations are:

- `AuthoredLocalRelativeToBaseline`: `D = inverse(L_b) * L`. This requires `SeparateFromAuthoredLocal` root semantics.
- `RootCompensatedRelativeToBaseline`: `D = inverse(R_b * L_b) * (R * L)`. This requires `ComposeWithAuthoredLocal` root semantics.

`RootTreatment::Unknown` rejects both requests. A caller cannot select an operation merely because a decoded camera curve exists. These operations preserve the order of full matrices, including scale, translation, and noncommuting rotations. The compensated operation permits root/local inverse cancellation rather than treating the raw camera-local displacement as an additive eye offset.

`compose_view` produces `V = B * D * H`, where `B` is the normal view base without the tracked head, and `H` is the local tracked head transform. A later adapter must establish that frame relationship and supply an appropriate baseline. No claim is made that an existing world-space headpose value can serve as `H` unchanged.

## Temporal ownership

The pure policy requires an explicit `AcceptedRun` for boosted ground takeoff. It does not infer acceptance from a ground-takeoff bit. `GraphState::GroundInto` and `GraphState::GroundFollowOn` represent the exact graph identities `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND_INTO` and `S_JC_WINGSUIT_BOOST_TRANSITION_FROM_GROUND`. They are semantic inputs, not bound native state identifiers. No production reader for them exists.

Entry records a single valid sample and leaves the normal tracked owner active. A second valid sample forms the first authored pair. The forward internal handoff preserves the accepted run, source, baseline, and absolute clip time. A clip-time restart invalidates the pair. The checked asset pair uses `ContinueAbsolute`; its internal handoff does not restart the clip.

Normal completion can enter `WingsuitIdle` while the accepted takeoff source still contributes to the finalized pose. That state represents `S_JC_WINGSUIT_IDLE`, not arbitrary idle or locomotion. The checked data specifies an outgoing idle blend, so clearing the takeoff state bit cannot end ownership by itself. Every tail sample must retain the accepted run and source and supply that source's finite positive weight for the same finalized pose. A zero weight clears ownership and history. A later positive weight in idle cannot restart an expired run. The temporal fixture contains a one-second idle-tail shape; the policy has no wall-clock tail timeout and does not claim a measured runtime duration.

The full local transform is assumed to come from the finalized blend. The supplied contribution weight proves the accepted source still contributes; the model does not multiply an already blended transform by that weight again. A future reader must demonstrate those semantics. A raw per-clip curve paired with a guessed weight does not satisfy the input contract.

Ordinary locomotion and unknown destinations reject ownership even when an old takeoff source is reported. Interrupted exits invalidate history because their blend contracts are not characterized. Missing data, a changed source, an unrelated accepted run, an unknown root treatment, and a mismatched finalized weight sample also return the normal tracked owner and reset history. Re-entry requires a new entry sample before another pair can be formed.

## Native consumer gate

Production remains unavailable until a later change proves all of the following together:

- A non-fallback runtime channel with established baseline and eye-frame semantics.
- The actual native root treatment for that channel.
- An owned root/local full-transform snapshot captured at a safe native finalization phase, with distinct matched previous/current samples and explicit character, continuity, and finalized identities.
- Exact accepted graph identity and current finalized blend-source weight through entry, the continuous internal handoff, the normal idle tail, and every supported interruption.
- Serialized publication and reader proof across the tree contexts, render endpoints, and simulation camera getter.

A future adapter must consume one ownership result in `camera_tree_update_render_contexts`, `camera_update_render`, and `game_camera_manager_get_camera_matrix`. A gate only around render-camera placement leaves the earlier context replacement active. `LAST_CAMERA_WORLD` must contain the selected owner's current transform with a matching identity, or the getter must invalidate or bypass it. The listener and alternate-aim transforms require the same explicit ownership contract. The scoped grapple aim override keeps precedence.

Final native game-camera capture belongs at `GameCameraManager::PushRenderContext`, after the tree and special cameras, if a later verified capture needs that full framing transform. It remains distinct from the authored-local source. The existing native endpoint initializer can copy one transform to both endpoints, so endpoint field names do not establish authored sample history. No native capture is added here.

## Fixtures and integration

The synthetic composition fixtures cover root/local inverse cancellation, full TRS and noncommuting rotation order, a nonidentity baseline, one head composition, unknown root semantics, invalid transforms, identity changes, and mixed-time pseudo-pairs. The temporal fixtures cover entry, a continuous internal handoff, the ordinary-idle blend tail, zero-weight completion, ordinary locomotion exclusion, interrupted exits, source loss, teleports, identity changes, and stale blend weights. These fixtures validate the contract only. They do not validate a native source or activate the candidate.

The parent integration registers `pub(crate) mod authored;` beside the existing camera submodules. No consumer hook changes are required or justified while `status()` is `Unavailable`.
