# Original gameplay criteria audit gaps

## Scope

The audit against the original video feedback classifies 32 criteria as implemented and six as partial at source checkpoint `3384b0bf`. Implemented means a connected production path, not reproduced incident resolution or game/headset acceptance. The six partial criteria require the corrective work below. The earlier correction record remains in [gameplay-review-remaining.md](gameplay-review-remaining.md).

| Criterion | Missing implementation | Required result |
| --- | --- | --- |
| B3: support-aware gait and terrain | The live fit always receives no support and bypasses the old ground/gait branch. Its support-dependent stepping and touchdown code cannot run. | Connect observed native support and bounded terrain evidence to the committed fit and foot realization. Preserve support identity and moving-frame transport. Missing evidence releases inferred contacts; low head height or dwell does not fabricate knee/pelvis support. |
| A4: recorded locker grenade source | Native grenade displays are discoverable independently of ammo eligibility, but the recorded shelf's owner/model is unknown. Static decoration has no source-specific world region. | Identify the shipped shelf entity/model and native owner/query. Add the actual source form at its real transform, routed into independent safe items. A chest-pouch draw or a different pickup entity is not the recorded source fix. |
| P1: global discovery and practical restoration | Current-player/future settings adoption misses distinct settings already used by NPCs. Enclosing bounds can reject ordinary ground contact even when the original hull fits. | Discover affected live human settings, including late injection. Use a verified original-shape clearance operation with explicit unsupported/error handling. Restore the whole policy transactionally or defer while normal gameplay remains active. Successful retry/eject/reinject remains a live test; restart is not restoration. |
| A5: real paired source forms | Independent members currently require an unlimited rack-state offering with exactly two native gun records and separate-pistol handling. Coverage of the recorded locker and other pair-granting forms is unestablished. | Inspect shipped placement/give data and native ownership/ammo behavior. Support qualifying real pair-granting source forms without duplicating ordinary single guns, consuming the neighbour, or requiring carry space for the second source member. |
| W6: ready-gun physical racking | The action candidate is offered only when chambering or completing the action is needed. A ready chambered gun cannot ordinarily be racked. | Offer supported manual actions regardless of need, preserving exclusive support/extraction/transfer claims. Eject/feed rounds correctly, including aborts and repeated cycles, without free ammunition or refunds. |
| C1: restart and outgoing blend evidence | Exact named states are connected, but production supplies no coherent clip-time or native contribution evidence. Same-state restarts and outgoing blend tails remain uncovered. | Capture matching native motion identity/time/contribution where required, detect restarts and valid transitions, and retain tracked-head composition and discontinuity resets. Do not label the mod-side handback as a native animation weight. |

## Connected corrections

The corrective series adds the following production paths. These implementation records do not establish game or headset acceptance.

| Criterion | Connected implementation | Remaining runtime evidence |
| --- | --- | --- |
| B3 | Native support and per-foot terrain enter the committed fit, contact transitions, and foot realization. Moving support carries contacts once. | Slopes, stairs, edges, support changes, platform turning, and body-follow behavior. |
| A4 | Shipped Massos static model collections register grenade shelf sources at copied instance transforms. | Collection-name matching during loading, late injection, stream-out/reload, and both-hand safe-item use. |
| P1 | Live body-owner discovery adopts existing human settings. Native convex queries and bounded terrain-leaf traversal preflight transactional restoration. | Existing and streamed NPCs, clear flat-ground restoration, wall deferral, retry, eject, and reinject. Unsupported or large terrain can defer. |
| A5 | Finite pairs stage one empty native copy and divide actual source ammunition. The original remains until the final native pickup. | Both grab orders, native callback races, partial-source stand-down/resume, streaming, and reset. |
| W6 | Supported ready guns expose manual actions. Rear-stop ejection and completed feeding conserve loaded rounds. Gun release remains pending during a genuine action hold. | Ready and dry cycles, aborts, both hands, pump support at home, release during an action, and vehicle travel. |
| C1 | Native clip time and contributions identify restarts and outgoing blends. Fresh completion requires camera-bone coverage and the native producer buffer exchange. | Exact-state restart, interrupted/outgoing blend, skipped evaluation, tracked composition, and handback. |

Cumulative review required corrections for deferred gun release, camera completion eligibility, partial finite-source consumption across stand-down, and terrain-rejected contacts during constraint recovery. The corrective commits and closure reviews address all required findings. A separate read-only check confirms that the repaired terrain fixture still exercises runtime constraint recovery and final pose realization.

## Automated verification

The integrated corrective source through `8102597f` passes formatting, all-target/all-feature Windows Clippy with warnings denied, the full Wine suite, and `scripts/xwin_build.sh`. The payload suite reports 3,104 passed and one ignored. The tracking-format suite reports 46 passed and one ignored; synthesis reports 35 passed. Rust jobs are capped at 16, and test threads at four. No game or headset run is recorded.

The rebuilt debug artifacts are `target/x86_64-pc-windows-msvc/debug/jc3vr_payload.dll` and `target/x86_64-pc-windows-msvc/debug/jc3vr_injector.exe`. The payload SHA-256 is `11f5339474ebdad08bc8336416e380fb403a844a40a59398f74177b75965dc22`. The injector SHA-256 is `7bc1152cc6ed60b6d52873618ac2826d2417f3ea9b507bf13df59f2be0ef5a6b`. [The runtime test guide](../mod/body/runtime-testing.md) and [per-criterion protocols](../../testing/protocols/gameplay-review.md) define the remaining live checks.

## Research and tooling

Inspect native bodies and shipped assets where they determine implementation. Build small reusable extraction, graph-inspection, or shape-query tools when repeated manual work would otherwise obscure the evidence. Tools must preserve source provenance and patch precedence, avoid whole-archive extraction, and keep game addresses/layouts in verified pyxis definitions. No invented native ABI or guessed lifetime is permitted.

## Delivery

Use disjoint subagent implementation scopes and centralized shared-file integration. Commit major coherent changes and review cumulative batches every few commits. Keep aggregate Rust compilation at `CARGO_BUILD_JOBS=16` and Wine tests at `RUST_TEST_THREADS=4`; freeze source writers before shared-target checks. Publish verified pyxis changes before parent pointers and regenerate bindings through the build. No main-repository push is authorized.

Update `testing/protocols/gameplay-review.md` so current procedures no longer describe active features as absent. Preserve historical attribution and per-ID oracles. Produce a rebuilt payload and injector with passing formatting, all-target/all-feature Windows Clippy, and full Wine tests. Runtime paths must be connected for testing; isolated models and additional diagnostics alone do not complete these gaps. Report game/headset acceptance and real-capture performance separately from implementation.
