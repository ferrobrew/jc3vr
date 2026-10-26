# Paired rack weapons

A native weapon in the dual-wield slot can contain separate visible guns while sharing one actor and magazine. Physical grabs expose each native gun as a separate `RackMember` target. Each target carries the source actor, observation generation, and native member index. A grab takes only its selected member. The other member remains at the source, including when the other hand is full.

## Source qualification

`controllers::weapons::hold::rack::paired` accepts a live, offered native rack actor in the dual-wield slot with exactly two non-null native gun records. The single-weapon setting and the loadout table must be active. Spawn reset and mission-held inventory exclude new sources. Already designated single weapons do not split again. Weapons with one gun and weapons outside the rack state keep the ordinary pickup path.

The inspected Guardia Massos locations contain a `WeaponRack` rigid object with `Slot.OneHanded` placement-rule children. Their `in_rack` property is enabled, their infinite and ordinary pickup properties are disabled, and their one-handed selector resolves to the Wraith SMG or the Spectre pistol. The selector is not an exact weapon spawn tag. The runtime reads the selected actor's spawn tag instead. Qualification uses the native actor's slot and gun records, so it also accepts other finite placement-rule forms with that same verified runtime structure. It does not infer a pair from the selector, model name, or cabinet geometry.

## Finite sources

The finite path retains the original actor at the placement rule and stages one additional native-initialized actor through [`Inventory::GivePlayerWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GivePlayerWeapon). The request uses the live source's exact spawn tag, requests an empty magazine, and disables equip, extra-ammunition transfer, and replacement drops. The existing slot-and-tag callback fence remains in use. A source never requests another actor after its request fails or times out within that observation.

[`Inventory::WeaponSpawned`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-WeaponSpawned) initializes the staged actor with the inventory occupant preserved. Its magazine is set to the requested amount without the extra-ammunition reserve write. The staged actor then leaves the inventory, becomes a designated single weapon, and remains hidden at the source. Its spawned stored ammunition is cleared. Staging does not call the original actor's pickup function or credit any ammunition.

Either native gun can be grabbed first. Before that take, the source owns all of its original ammunition. On an accepted first take, the copy receives the selected member's share of the actual source magazine. The original retains the remaining share. Rounds above the combined single-weapon magazine capacity remain as stored ammunition on the original, together with its existing stored ammunition. No reserve refund occurs during the split. A rejected take restores the original magazine, stored ammunition, and pair designation, and returns the empty staged actor to its waiting state.

After the first accepted take, the original is a designated single weapon. Its first native model is displayed at the remaining native member's transform, and its extra model stays hidden. The surviving `RackMember` still uses the remaining native member index. Registry retirement preserves the original's single designation and reduced ammunition, so re-observation cannot create another pair. An ordinary single-weapon pickup can still take that remaining actor after retirement.

The final grab transfers the original through the existing table pickup. [`WeaponBase::PickupStart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupStart) runs on this finite source at most once, when the final member leaves the rack. Its state leaves the rack state and its native pickup event notifies the placement rule. The table pickup establishes player ownership and credits the original's stored ammunition once through the existing pickup accounting. The original becomes the final held single weapon; no third actor remains to despawn or restock.

A refusal before the native pickup leaves the remaining gun on the rack. If the native pickup event runs but the final carry fails afterward, the remaining original becomes a loose single weapon. The implementation does not restore a consumed placement rule or send its pickup event again.

## Unlimited sources

Unlimited sources keep their existing member initialization path. Native pickup supplies the pair's magazine allocation, and the give detour splits that allocation between the staged members. Each explicit grab claims only its cached actor. A finite source never enters this path.

## Lifetime and verification

The finite registry bounds observed sources and owns at most one staged actor per source. An unclaimed observed source has a strong reference and retires after the existing rack grace period. A partially claimed source releases its strong reference after that grace period and retains a weak reference and its single designation. The normal single-weapon watch exempts these live weak records from untracked-weapon pruning. An expired weak reference removes the record and designation. Reset and stand-down discard staged actors and observation generations. Canceled arrivals still pass through the existing member callback fence and are discarded when their source generation no longer waits for them.

Pure tests cover ammunition conservation with stored and overflow ammunition, both grab orders, a rejected first take, repeated claims, and invalid member indices. They do not verify native placement-rule callbacks, gun transform updates, or streaming in a live game. Runtime checks must cover both weapon types, both grab orders, a full other hand, rejection before and after native pickup, leaving and re-entering interaction range after the first take, streaming out, and spawn reset while a copy is pending. The expected successful sequence contains one native copy request, two accepted member takes, one original pickup event, and no additional reserve credit during staging.
