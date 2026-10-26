# Final gameplay audit corrections

## Scope

The fresh audit of all 38 original criteria at `26a88a9a` finds 34 connected source implementations, four partial criteria, and no unattempted criteria. The authority is the original feedback and agreed per-criterion acceptance, not historical completion claims. The detailed source evidence is retained in `target/gameplay-review-20261024/final-audit/criteria.md`. No game or headset acceptance is established by that audit.

| Criterion | Confirmed partial | Required correction |
| --- | --- | --- |
| A4: static grenade shelves | Static source registration occurs only after collection load. A qualifying shelf already loaded at injection has no discovery path. | Discover existing qualifying collections through verified native ownership and lifecycle. Reuse source identity, model validation, retirement, and address-reuse handling. Do not use an unverified pointer sweep or assume native weapon displays cover static instances. |
| I3: vehicle canopy/stunt button | The three vehicle default sets use dominant-hand lower face. Left dominance moves the contextual action off the approved physical-right A button. | Fix the default sources to physical Right, preserve explicit user bindings, and test both dominance choices and contextual routing. Native seat eligibility remains a separate live check. |
| C1: grapple-hanging camera | Selected attached/stunt-hanging cases are covered, but the native grapple-hanging state is explicitly excluded. | Verify the actual hanging state and finalized camera channel, then connect its precise authored-camera ownership and transitions. Do not infer camera data from a broad hanging predicate or claim a recorded inversion fix without matching runtime evidence. |
| W1: independent chamber accounting | Loaded grip-and-pull extraction is connected, but all native loaded rounds enter the removed magazine. Chamber readiness does not retain an independent cartridge with the magazine absent. | Track logical chamber and magazine ammunition independently across extraction, insertion, firing, manual action, and ownership/loss transitions. Preserve button ejection and abort behavior without duplication or reserve refunds. A separate native cartridge object is not required. |

The weapon-part audit does not establish a missing non-hand correction. Non-hand guns and parts retain native placement and interpolation. Invalid body-fit withdrawal also remains intentional. Neither case authorizes a return to old defaults or an additional transform without evidence.

## Native findings and connected corrections

The vehicle stunt defaults now use physical right A, with the land handbrake on physical left A to avoid a left-dominant collision. Explicit user bindings remain authoritative. The loaded-shelf provider walks `GameLocationSystemContext::m_LoadedLocations` in the game-thread input phase, verifies the archive provenance and `Model::m_Id`, and copies current transforms. Offer and completion share discovery, source generations, and collection retirement.

The shipped Rico graph has no `S_GRAPPLE_HANG` state. Its actual inverted-grapple sequence is `S_GRAPPLE_PRE_UPSIDEDOWN` followed by `S_GRAPPLE_ATTACH_UPSIDE_DOWN`. Installed archive mappings and bound camera tracks verify animated entry and the matching idle endpoint. The camera adapter admits those precise states, retains their forward baseline, and continues to reject the absent legacy name. Broad hanging flags do not establish authored camera data. The recording's exact hanging context remains unidentified.

The ammunition ledger partitions the existing native loaded total. Retained extraction moves only magazine-owned rounds; a live chamber stays on the weapon and can fire once with the magazine absent. Native reload refund calculation, insertion, and manual actions use the same ownership. Cumulative review found stale policy after settings changes, pair-to-single chamber ownership, chamber-inclusive input capacity checks, and duplicate-finalization camera handoff defects. The corrective commits explicitly deactivate policy, transfer chambers through the native pair split, unify capacity checks, and separate raw camera observations from admitted handoff state. Regression tests and the closure review approve all required findings.

## Verified build

The integrated source through `f8852fa3` passes formatting, all-target/all-feature Windows Clippy with warnings denied, the full Wine suite, and the payload/injector build. Payload tests report 3,137 passed and one ignored; tracking format reports 46 passed and one ignored; synthesis reports 35 passed. Native-definition documentation and Rust/C++ checks pass, and the generated documentation commit is pulled before parent integration. Rust jobs are capped at 16 and test threads at four. No game or headset run is recorded.

The payload artifact is `target/x86_64-pc-windows-msvc/debug/jc3vr_payload.dll`, SHA-256 `0e52cdbbbf42fb099346105b1ff60362bcbd87065da650898ece406cff2ab00a`. The injector is `target/x86_64-pc-windows-msvc/debug/jc3vr_injector.exe`, SHA-256 `7bc1152cc6ed60b6d52873618ac2826d2417f3ea9b507bf13df59f2be0ef5a6b`.

The four source partials have connected corrective implementations. This is not a new claim that all 38 original incidents pass live. The original audit remains a checkpoint record; native callback timing, actual chamber/insert/fire behavior, collection streaming, contextual seat eligibility, inverted-camera comfort, and representative recording measurements remain controlled-test work.

## Delivery

Use disjoint implementation scopes, local commits for major coherent corrections, and cumulative review after the batch. The coordinator serializes shared-target formatting, compilation, and tests after every writer freezes. Keep aggregate Rust jobs at 16 and test threads at four. Native definitions require release-IDB verification and the normal published-definition/regeneration workflow. No generated binding edits or main-repository push are authorized.

Final delivery requires formatting, all-target/all-feature Windows Clippy with denied warnings, the full Wine suite, and rebuilt payload/injector artifacts. Record regression and review outcomes separately from live acceptance. Preserve the original per-ID protocols and add the existing-shelf injection, both-dominance vehicle route, precise hanging-state/channel, and independent chamber/magazine cases.
