# Native canopy targets

The live canopy targets use the native rig's link positions. `controllers/vehicles/parachute_rig.rs` copies the engine's existing joint reads during [`NParachuteObjectControl_SyncBones`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NParachuteObjectControl_SyncBones). It adds no joint evaluation calls. A scoped detour on [`AnimationController::GetBoneMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneMatrix) marks each re-derivation read, and a detour on [`AnimationController::GetJoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetJoint) copies the later handle-placement reads before the engine modifies the link and handle.

The link translations use the parachute object's model space. The callback copies that object's current transform through [`GameObject::GetTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObject##vfunc-GetTransform) and converts the translations to world-space points before publication. The input tick converts those copied world-space points into its current cockpit frame. The character root is not used as a substitute.

## Input and lifetime

`vehicles::begin_tick` requests samples while the canopy controls act. The request identifies the local character and an input generation. A sample contains copied world-space points, a timestamp, a sequence, and comparison-only rig identities. It contains no borrowed engine object. The native callback uses a thread-local capture scope, ends engine borrows before the original sync, and reconstructs short-lived borrows only for synchronous prop restoration. A nested sync for the same controller invalidates the outer restoration.

Normal and wider-reach free targets use the same native point. Missing, stale, retired-request, or incomplete native data supplies no free target until another valid native callback publishes. There is no shoulder-derived fallback. Held targets retain their grip-derived placement and pull origin. Releasing a handle returns it to a fresh native target.

The visual hook preserves the existing held-handle behavior. For an unheld side, it restores copied link, rotation, and handle joints in the sync callback and puts the handle translation on its link. The copies come from the engine's own reads after rig re-derivation, rather than extra reads before the sync.

## Verification

Pure tests cover incomplete placement capture, placement after scale and re-derivation, and unheld-side selection. The existing test-only native-rest model covers owner, session, sequence, timestamp, transform, and coordinate-conversion cases. These tests do not establish live alignment or engine callback serialization across threads.

The native producer and its immediate synchronous caller are checked in the IDB. The integrated adapter still requires the central build, tests, and a controlled game test. The controlled test must check free-handle alignment, left and right ownership, taking and releasing either handle, wider reach, stale-input stand-down, and opening a replacement canopy.
