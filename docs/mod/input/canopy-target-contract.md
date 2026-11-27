# Native canopy targets

The parachute handles' free targets ([flight.md, "Parachute handles"](flight.md#parachute-handles)) sit where the game's own rig places the handle props. The code is `crates/jc3vr-vehicles/src/vehicles/parachute/rig/mod.rs`.

## Capture

The positions come from the reads the engine already makes during [`NParachuteObjectControl_SyncBones`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::parachute::NParachuteObjectControl_SyncBones); the mod adds no joint evaluation. A scoped detour on [`AnimationController::GetBoneMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetBoneMatrix) marks each re-derivation read, and a detour on [`AnimationController::GetJoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::AnimationController##func-GetJoint) copies the later handle-placement reads before the engine moves the link and handle. The link translations are in the parachute object's model space, so the callback copies that object's transform through [`GameObject::GetTransform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObject##vfunc-GetTransform) and publishes world-space points. The character root never stands in for it.

## Requests and lifetime

`vehicles::begin_tick` requests samples while the canopy controls act (`parachute::rig::centres`), naming the local character and an input generation. A sample holds copied world-space points, a timestamp, a sequence, and comparison-only rig identities, and no borrowed engine object. The input tick converts the points into its current cockpit frame. The native callback uses a thread-local capture scope, ends its engine borrows before the original sync runs, and reconstructs short-lived borrows only to restore the props synchronously; a nested sync for the same controller invalidates the outer restoration.

The normal and the wider-reach free targets use the same native point. Missing, stale (`parachute_rig.stale_after_ms`), retired-request, or incomplete data supplies no free target until another valid callback publishes, and there is no shoulder-derived fallback. A held handle keeps its grip-derived placement and pull origin, and a released one returns to a fresh native target.

## The unheld props

The game draws both handle props in Rico's hands wherever the hands are. For a side no hand holds, the post-call part of the sync detour restores the copied link, rotation, and handle joints and puts the handle back on its link, as the game does for a hand it does not place; a held handle stays native, in the hand. The copies are the engine's own reads after the rig's re-derivation, not extra reads before the sync. While the handles do not act, the rig is the game's.
