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

## Research and tooling

Inspect native bodies and shipped assets where they determine implementation. Build small reusable extraction, graph-inspection, or shape-query tools when repeated manual work would otherwise obscure the evidence. Tools must preserve source provenance and patch precedence, avoid whole-archive extraction, and keep game addresses/layouts in verified pyxis definitions. No invented native ABI or guessed lifetime is permitted.

## Delivery

Use disjoint subagent implementation scopes and centralized shared-file integration. Commit major coherent changes and review cumulative batches every few commits. Keep aggregate Rust compilation at `CARGO_BUILD_JOBS=16` and Wine tests at `RUST_TEST_THREADS=4`; freeze source writers before shared-target checks. Publish verified pyxis changes before parent pointers and regenerate bindings through the build. No main-repository push is authorized.

Update `testing/protocols/gameplay-review.md` so current procedures no longer describe active features as absent. Preserve historical attribution and per-ID oracles. Produce a rebuilt payload and injector with passing formatting, all-target/all-feature Windows Clippy, and full Wine tests. Runtime paths must be connected for testing; isolated models and additional diagnostics alone do not complete these gaps. Report game/headset acceptance and real-capture performance separately from implementation.
