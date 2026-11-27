# Paired rack weapons

A native weapon in the dual-wield slot holds two visible guns sharing one actor and one magazine. On a rack, each native gun is a separate `RackMember` grab target carrying the source actor, the observation generation, and the native member index. A grab takes only its member; the other stays at the source, also when the other hand is full. The rest of taking a weapon off a rack is in [physical-weapons.md, "Taking a weapon off a rack"](physical-weapons.md#taking-a-weapon-off-a-rack).

## Source qualification

`hold::rack::paired` accepts a live, offered native actor in the rack state and the dual-wield slot with exactly two non-null native gun records, while new single pistols are made (`dual_wield::single::active`) and the loadout table runs. A spawn reset and mission-held inventory exclude new sources. Weapons already single do not split again, and weapons with one gun or outside the rack state take the ordinary pickup path.

The inspected Guardia Massos locations hold a `WeaponRack` rigid object with `Slot.OneHanded` placement-rule children whose `in_rack` property is on and whose infinite and ordinary pickup properties are off; their one-handed selector resolves to the Wraith SMG or the Spectre pistol. The selector is not an exact spawn tag, so the runtime reads the selected actor's spawn tag instead. Qualification reads the native actor's slot and gun records, so it accepts any finite placement-rule form with that runtime structure; it never infers a pair from the selector, the model name, or the cabinet geometry.

## Finite sources

A finite source keeps the original actor at the placement rule and stages one additional native-initialized actor through [`Inventory::GivePlayerWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GivePlayerWeapon), with the live source's exact spawn tag, an empty magazine, and no equip, extra-ammunition transfer, or replacement drop, through the slot-and-tag callback fence. A source never requests another actor after its request fails or times out (`hold.rack.paired.copy_wait_ms`) within that observation.

[`Inventory::WeaponSpawned`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-WeaponSpawned) initializes the staged actor with the inventory occupant preserved and its magazine at the requested amount, without the extra-ammunition reserve write. The staged actor then leaves the inventory, becomes a single pistol, and stays hidden at the source with its spawned stored ammunition cleared. Staging neither calls the original's pickup nor credits any ammunition.

Either gun can be grabbed first. On an accepted first take, the copy receives the selected member's share of the source's actual magazine and the original keeps the rest; rounds above the two single pistols' combined capacity stay as stored ammunition on the original. No reserve refund happens during the split. A rejected take restores the original's magazine, stored ammunition, and pair designation, and returns the empty staged actor to waiting.

After the first accepted take, the original is a single pistol: its first native model is drawn at the remaining member's transform, its extra model hidden, and the surviving `RackMember` keeps the remaining native member index. Retiring the registry entry keeps the original's single designation and reduced ammunition, so re-observation cannot create another pair, and an ordinary pickup can still take it.

The final grab transfers the original through the table's pickup. [`WeaponBase::PickupStart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupStart) runs on a finite source at most once, when the final member leaves the rack: it leaves the rack state and its native pickup event notifies the placement rule. The table's pickup credits the original's stored ammunition once. No third actor remains to despawn or restock. A refusal before the native pickup leaves the remaining gun on the rack; if the native pickup ran but the final carry fails, the original becomes a loose single pistol, and the consumed placement rule is not restored.

On the rack every gun of a weapon takes the placement rule's one transform, so a pair's two pistols are drawn on top of each other and the shelf shows one, and the hidden copy appearing on the first grab would look like the second pistol being made on the spot. While the copy is ready and neither pistol is claimed, the second pistol and its parts are drawn `hold.rack.paired.stack_m` along the pistol's own up axis at render time (`rack::paired::finite::stack_offset`), with its grab points moved with it, so both pistols show. Once one is claimed the offset ends.

## Unlimited sources

An unlimited source initializes an actor for each member: the native pickup supplies the pair's magazine allocation, and the give detour splits it between the staged members. Each grab claims only its cached actor. A finite source never takes this path.

## From range

The grapple's snatch takes one pistol of a pair from beyond the hands' reach ([grapple-snatch-targets.md, "Racks and lockers"](grapple-snatch-targets.md#racks-and-lockers)). The render pass notes the rack's weapon, and the snatch offers each of its two guns. Firing at one observes the pair as a hand's reach does (`paired::prepare`), so the copy or members are requested through the same fence, and picks the member: the aimed gun while it is still at the source, else the other (`paired::member_for`). The snatch waits until that member can be claimed, claims it with the same admission and claim a hand's take makes (`paired::claim`), and settles the claim as accepted at once, since the pistol has left its rack: a finite pair's first claim commits the ammunition split, and its last starts the original's native pickup once. The claimed pistol is shown alone and dropped into the world where it hung, for the line to pull in; a missed catch leaves it lying with its share, and the other pistol stays on the rack.

## Lifetime

The finite registry bounds the observed sources and owns at most one staged actor per source. An unclaimed observed source holds a strong reference and retires after `hold.rack.paired.source_grace_ms`. A partially claimed source releases its strong reference after that grace period and keeps a weak reference and its single designation; the single-pistol watch exempts these live weak records from untracked-weapon pruning, re-observation reacquires the original through the weak reference without requesting another copy, and an expired weak reference removes the record and the designation. A reset or a stand-down retires unclaimed staging. Cancelled arrivals still pass through the member callback fence and are discarded when their source generation no longer waits for them.

The pure tests cover ammunition conservation with stored and overflow ammunition, both grab orders, a rejected first take, repeated claims, invalid member indices, and the snatch's choice of member. Unverified: the native placement-rule callbacks, the gun transform updates, streaming in a live game, and whether a single pistol's dropped bodies match the one pistol that shows.
