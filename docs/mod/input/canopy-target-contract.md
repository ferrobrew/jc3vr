# Native canopy target contract

The live canopy targets retain their existing shoulder-derived placement. A test-only native-rest model in `controllers/vehicles/parachute_native_model.rs` specifies a possible replacement, but no live adapter is installed.

The native rig's link joints use the parachute object's model space. The current object transform is available through [`GameObject::GetTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObject##vfunc-GetTransform). The sync hook, [`NParachuteObjectControl_SyncBones`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NParachuteObjectControl_SyncBones), uses that object's transform rather than the character root. The proposed target therefore requires copied link positions paired with the same object's transform from one callback, followed by conversion into the current cockpit frame. The character root is not a substitute.

## Model coverage

The pure model records owner, session, rig, sequence, and time identities. Tests reject stale, future, crossed-owner, invalid-transform, and retired-rig samples. Independent quarter-turn coordinate oracles check transform order and reject character-root substitution. Normal and wider-reach candidates use the same native point. Held candidates remain grip-derived, and release returns to a fresh native rest sample. These tests do not establish native callback ordering, safe joint restoration, or live alignment.

## Adapter blocker

A live implementation must end Rust engine-object borrows before calling the original native sync and reconstruct short-lived borrows afterward. It must also exclude superseded or overlapping restoration before writing saved joints. A mailbox decision followed by an unlocked native write does not establish that exclusion. Blocking a nested native callback can deadlock or suppress required engine work; allowing it can invalidate saved restoration data.

The attempted live adapter is removed because that ordering contract is unproven. Session retirement must cover shared stand-down paths, and native weak-reference acquisition and release must occur outside the outer input mutex as well as local vehicle and sample locks. The published transform binding supplies none of those guarantees. Existing live behavior remains unchanged, and native target alignment remains open.
