# The two-handed inventory

Mostly not built yet: this is the design and the plan of record. The pure core is built (["The core as built"](#the-core-as-built)), a live table of it is the only writer of where the player's weapons are (["The live table"](#the-live-table)), every gun in a hand fires from its own hand (["The per-hand tick"](#the-per-hand-tick)), and the game's slots follow the table (["The slots as a projection"](#the-slots-as-a-projection)); the grabs still decide as the old handling did, and everything else is design. It replaces the mechanisms described in [vr-inventory.md](vr-inventory.md) and the second weapon and separate pistols of [physical-weapons.md](physical-weapons.md), and the [grapple snatch](grapple-snatch.md) is built on top of it.

The local player's two hands are independent. Each holds its own weapon or object, items pass between the hands, weapons dock on the body's sockets and are drawn from them, and the player carries what the body holds rather than what the game's slots allow. Boneworks and Half-Life: Alyx are the guiding light: the player does everything with their own body, even where that adds friction, and nothing docks, draws, or switches by itself.

## Why the game's inventory is replaced

The game wields exactly one weapon by construction ([engine: weapon-ownership-and-despawn.md](../../engine/gameplay/weapon-ownership-and-despawn.md), [engine: item-pickup.md](../../engine/gameplay/item-pickup.md)). Its eight slots are a fixed count, `Character::GetWieldingWeapon` returns the selected slot's weapon, and nearly every consumer reads that one weapon: the fire tick, the reload paths, the track messages, the animation graph, aim, the HUD, the AI's threat rating, the camera, the vehicles' and traversal's holstering, the saves, and the missions. The mod has so far kept that model and worked around it (a second weapon fired by the mod, a carry set beside the slots, pistols split from their pair, and takes that reshuffle the slots after the game's own pickup), and every workaround meets every other: a take with the free hand goes through the game's pickup, whose equip takes the other hand's weapon away, and the second weapon lacks the foregrip, heft, sights, scope, wall collision, and handover that the wielded one has.

So the mod owns weapon handling for the local player. The game keeps three jobs: owning the weapon objects (their spawn, despawn, and references), simulating the weapons themselves (projectiles, effects, damage), and holding the ammunition reserves. Everything else that reads the wielded weapon is replaced by the mod, patched to work per weapon, or fed with the primary hand's weapon.

## The model

### The loadout table

One table holds every item the player has: each is in exactly one place, the left hand, the right hand, a body socket, or the world (kept near the player). Items are weapons, the props a hand picks up, and a grenade or a C4 charge while a hand holds one. Both hands are the same type: a gun in either hand has the foregrip, the heft, the sights, the scope, the wall collision, the reload, and the pips, and a heavy weapon held in one hand is simply very heavy and lifts only so far.

The primary hand is the dominant hand when it holds a weapon, otherwise the other hand. The primary follows from the hands and is never set on its own.

The grapple hand is a hand like the other: it holds anything the other can. The grapple is not an item: it stays a device on the grapple hand's wrist, worked by its button whatever the hand holds. A hand that holds something is not pinned to the line while hanging from it, and the grapple hand's trigger retracts the tethers only while its held item, if any, does not use the trigger.

### The game's slots are a projection

The game's slots are made to match the table, never the other way round:

- the primary hand's weapon is in its slot, selected, and wielded, for the consumers the mod feeds rather than patches;
- before the game stores its save, and before a mission or challenge caches the player's weapons, the slots are rewritten to a sensible loadout of what the table holds, and the mod's sidecar records the rest: which hand or socket each weapon is in, and the weapons beyond the slots ([settings.md](../settings.md), the inventory sidecar). The save stores only the slots and the ammunition reserves, so the projection covers the game's half;
- a held heavy weapon is never projected into the heavy slot outside a store or a cache: a weapon in that slot replaces the body's idle and strafe with the minigun's, and the base layer drops it, holstered or not, on grapple fire, grenade throws, falls, the parachute's and the wingsuit's opening, vehicle entry, and swimming.

The table takes a snapshot of its own at each mission checkpoint, since the mission manager caches the slots at every checkpoint and a death, failure, or checkpoint restart restores that cache: a slot of another weapon type is replaced (the old object despawned), a slot the cache had empty is emptied, and the occupied slots' ammunition reserves are zeroed first. Weapons outside the slots are kept by a restore, never removed or duplicated, but share reserves with the slots.

Every spawn reset (death and checkpoint restarts, fast travel, a challenge's start, retry, or abort, a mission's start, loading a save, the frontend) replaces every slot weapon with a new object of the same type, keeping the magazines and re-equipping the selected slot. The table re-keys its slot weapons across a reset, and brings the weapons outside the slots, which the reset keeps where they were, to the player.

The game's word on ownership is final. A mission or challenge that removes the player's weapons removes them from the table, the hands' included; a weapon the game gives goes to an empty hand if one is free, otherwise to a socket that fits it. On death and respawn the game's rules decide what the player owns, and the hands come back empty with everything else on its socket.

### No automatic moves

Nothing moves an item but the player:

- Every take (from the ground, a rack or locker, a character's hands, a mission's give, the grapple snatch) lands in the table and then in the grabbing hand. The game's equip never runs, so a take never touches the other hand.
- Entering a vehicle, gliding, opening the parachute, falling, and swimming leave the hands as they are. The base layer's state tasks that holster or hide the wielded weapon (on vehicle entry, the wingsuit, free fall, diving, the ragdoll, and death), stop its use, or drop a heavy weapon are patched out for the local player, and the neutral state the game forces on every teleport, revive, and cutscene setup, which unequips the wielded weapon, is undone by the reconcile. A hand holding a gun cannot take the steering wheel or a control, so the player drives one-handed or docks first; a gun in a hand in a seat fires, since the mod ticks it rather than the game's wielded tick, which skips non-motorcycle seats.
- The game's weapon cycling, the slot selects, the weapon wheel, and the automatic switch away from an empty weapon become requests on the table or go away.
- Ammunition pickups credit the reserve for any weapon in the table: the auto-pickup's weapon check (`Inventory::HasWeaponWithAmmoType`) is patched to see the table, and its only caller credits the ammunition itself.
- The game's own automatic moves are routed through the table or refused: the combat mental state's draw, the relaxed state's holster, the drop of an empty power-core weapon or a spent single-use launcher, and the store's reselection on close.

A heavy weapon does not restrict anything yet: the player can grapple, throw, glide, and swim while holding one. Making the player-initiated actions refuse while a heavy weapon is held, as its weight would suggest, is left for later.

### Letting go

A weapon a hand lets go of stays where it falls, protected from the despawn while the player is near. It returns to a free socket that fits it only when it is lost rather than dropped:

- out of the world: it fell through the terrain or below the world's floor, or sank out of reach in water;
- left behind at speed: it left the hand while the player travelled fast (the wingsuit, a vehicle, the parachute, or a fall), so the player never chose to leave it there.

Swapping a weapon on the ground for another, or walking away from one, never brings it back. Retrieving a weapon left behind is the grapple snatch's job.

## Slots

The sockets are today's ([vr-inventory.md, "Carrying more than the slots"](vr-inventory.md#carrying-more-than-the-slots)), on Rico's skeleton ([body/rig.md](../body/rig.md#the-bodys-volumes-on-ricos-skeleton)): the hips for pistols, three on the back for long guns and heavy weapons, the chest, and the pouches. Each socket takes its size class or smaller, mirrored for handedness. A held weapon shows before the hand opens whether the socket under it fits, and docking confirms with a haptic cue. Props have no socket and no backpack: letting go of one drops it. Grenades and C4 stay consumables drawn from their pouches.

## Ammunition and reload

One reserve per ammunition type, shared by both hands: two pistols of one calibre draw on one stock. The universal belt pouch hands out what the gun in the other hand takes ([reload.md](reload.md)). Every reload is the mod's true reload, per weapon: the game's reload paths are refused for the local player, and each gun's magazine, chamber, and action are its own. Each gun's own ammunition check asks the character to reload the wielded weapon whenever its magazine runs low, and a gun that does not reload by magazine (the pump shotgun, the timed reloads) cancels the character's reload every tick; both requests are refused for the local player, so one hand's gun never reloads or interrupts the other's.

## The HUD

Each hand shows its own weapon's state; the per-hand ammunition pips do this today, and a fuller per-hand HUD is the goal. Until then, the game's ammunition counter shows the primary's.

## Firing

Each hand's gun has the player as its user and its holder, and the mod ticks its update (both the parallel and the serial halves) with the hand's own inputs and aim point. The fire path keys everything on the weapon's user rather than its state or the selected slot: the heat, the gunshot event the AI hears, the bullets' owner and the player's bullet flag, the muzzle flash and sound, the recoil, the spread, the rumble, and the camera shake all follow. A gun with no user does not update at all, and the holder gives the bullets the bodies they ignore, the player's own and the vehicle's. The shooting permissions gate only the inputs the game's own fire tick passes down, so the mod checks them for its own inputs, as the second weapon does today.

Each gun keeps its own aim mode, which the mod drives per hand: the game's aim-mode setter reaches only the selected weapon. Starting to use a gun drops the player out of precision aim when that gun has none, and the camera hints are global and keyed by type, so two guns of one type would remove each other's hint when one stops being used: the mod owns the hints.

## Animation

The arms are the mod's IK, and the game's weapon acts (the draw, the holster, the switch, the reload) are cut. Moving the primary takes no act: the old weapon is unequipped and the new one equipped (`WeaponBase::OnUnequip`, then `OnEquip`), which is what the draw clip's own equip message does; a draw, holster, or switch clip still playing is aborted first, since its messages equip and unequip by slot when they fire.

The body never takes the combat stance. With a weapon wielded, the animation graph's base layer switches the locomotion, the walk starts, the turns, and the stunt turns to their combat families, whose default clips are the rifle's whatever the weapon and with no modifier set. The weapon-wielded condition reads false for the local player's base layer, so the body keeps the relaxed families whatever the hands hold, and the procedural body and the rig own the stance ([body/procedural-body.md](../body/procedural-body.md), [body/rig.md](../body/rig.md)). The weapon's class reaches the graph only through the character modifiers, which the mod already strips. No weapon condition guards a vault, a climb, the parachute, the wingsuit, swimming, or vehicle entry.

The game has no per-hand hold layer: its hand poses are per weapon class, one clip covering both hands' fingers, with right and left versions for rifles and one for pistols. A hand's fingers can take its weapon's class pose by sampling that clip once with the engine's own loader and blender, caching it, and copying that hand's finger bones at the pose seam each tick. That comes after the core, as polish to judge in the headset, beside the grip poses that cover the fingers today ([grip-poses.md](grip-poses.md)).

## Every consumer of the wielded weapon

| Consumer | Treatment |
| --- | --- |
| The fire tick (`Inventory::UpdateWeapons_Parallel`, `_Serial`) | Replaced: the mod ticks every hand's weapon with its own inputs and aim point, as it ticks the second weapon today; the game's tick gets no inputs for the player's guns |
| The reload paths (`Character::StartReloadingWieldingWeapon` and the reload acts, the refill-from-empty, the empty check's switch) | Replaced by the true reload, per weapon |
| Track messages | Not needed: no reload or equip clip plays |
| The weapon acts (draw, holster, switch, reload) | Cut; the IK places the arms and the guns |
| Selection, cycling, the wheel, the automatic switch | Requests on the table, or cut |
| The game's pickup (`WeaponBase::PickupItem`) | Detoured into the table's take |
| The HUD's ammunition and reticle | The mod's pips and reticle; the game's counter shows the primary meanwhile |
| The camera's recoil and hints | Already the mod's |
| The placement and visibility of slot weapons | The mod's: the IK in the hands, the sockets on the body |
| Ammunition auto-pickup (`HasWeaponWithAmmoType`) | Patched to see every weapon in the table |
| Firing as the player from a hand (heat, the gunshot stimulus, stealth, player-fired damage) | Follows the weapon's user and holder, whatever its state (see "Firing") |
| The shooting permissions (`GetAnimationAllowsShooting` and the inventory's) | Bypassed for the mod's tick |
| The aim mode with two weapons in use (`WeaponBase::StartUsing`, `Player::SetAimMode`) | Patched so both have a user without contention |
| Vehicles' and traversal's holstering and tick swaps | Patched out for the player: the table's rules apply |
| The animation graph's weapon-wielded condition and the combat families | Patched: reads false for the player's base layer (see "Animation") |
| The base layer's holster, stop-use, and heavy-weapon drop tasks, and the forced neutral state | Patched out, or undone by the reconcile (see "No automatic moves") |
| Each gun's ammunition check's reload and cancel requests | Refused for the local player (see "Ammunition and reload") |
| The camera hints | The mod's (see "Firing") |
| The AI's threat rating, Rico's lines, the level of detail, the button hints, the store | Fed by the primary |
| The save, the mission caches and restores, the challenges | Fed by the slot projection and the sidecar |

## The architecture

The table is the one source of truth for the player's items, owned by one module, and nothing else holds any of that state.

- The core is pure: the table, its places, and its transitions, with its invariants in the types, tested without the game. It is built; see ["The core as built"](#the-core-as-built).
- The hooks are thin adapters. A detour decides nothing: it turns what the game wanted (a pickup, a give, a removal, a save about to be stored, a mission cache) into an event for the table, or reads the table's published snapshot. One reconcile step per tick makes the game match the table: the slot projection, each weapon's state and user, its placement and visibility, and its tick.
- The readouts are one: a Status row and a debug UI section show the whole table, and the log records every transition with its cause, so a bug is either the table being wrong or the reconcile not applying it.

The existing pieces fold into it: the second weapon's tick becomes every hand's, the carry set's keeping, placement, and end of pass become the table's, the despawn protection, the true reload, the sidecar, and the store and mission detours carry over, and the second weapon, the carry set as a separate store, the split pistols as an exception, and the pickups' reshuffles go.

Switching the control scheme away from the controllers (or turning the mod's weapon handling off) projects the table back into the game's slots, so nothing is lost; switching back rebuilds the table from the slots and the sidecar. Gamepad play is expected to be rough meanwhile.

## The core as built

The code is `payload/src/controllers/loadout/`. It is pure, apart from the object address an item is bound to. The debug UI's Controllers tab has a `Loadout` section whose `Sandbox` header drives a table of its own with made-up objects, showing the items, the primary, the slots in play, the last store's sidecar, and the latest transitions and refusals.

### Items and their identity

An item is a weapon (its spawn tag's hash, its slot, and its size class), a prop, a grenade, or a C4 charge. Its identity is the table's own, assigned when it enters the table and never reused in a session. Beside it the table keeps the object the item is now: a live address, or nothing while it waits for one, either for its slot's new object across a spawn reset or for a spawn of its type after a restore. The address is the one place the core meets the game; the hooks and the reconcile turn it into an object.

### The places and their invariants

There are six sockets: each hip, each shoulder (where the game docks its two-handed slot's weapon, on the right, and its special slot's, on the left, whatever the dominant hand), a third place across the back, and the chest. The size classes are, smallest first, the pistol, the submachine gun, the long gun, and the heavy weapon. The hips and the chest take submachine guns and smaller, and the back's three places take everything, the heavy weapons included. How many places the back and the hips have, and whether there is a chest socket, follow the settings and the dominant hand, as the carried weapons' sockets did; a socket a smaller layout lacks keeps the weapon already on it until the player draws it.

The table's layout carries the invariants. An item is a value that cannot be copied, owned by exactly one container: a hand, a socket, or the world. Each hand and socket holds at most one. A socket's item can only be made through a check that it fits, and only a weapon can lie in the world, so a prop, a grenade, or a charge let go of leaves the table. No two live items share an object, and the primary is never stored: it is the dominant hand when that holds a weapon, otherwise the other hand when it does, otherwise none.

### The transitions

Each transition applies whole and returns its changes (an item entered, moved, left, was bound to a new object, or lost its object), each with its cause for the log, or is refused with a typed error and leaves the table as it was:

- a take into an empty hand, from the ground, a rack, a rack's copy, a locker, a character's hands, a mission's give, the grapple snatch, a pouch, or out of the air; each source brings only its kinds (a pouch a grenade or a charge, a catch a grenade, the ground a weapon or a prop, the rest weapons), and a weapon lying in the world taken again keeps its identity;
- a dock of a hand's weapon on a socket the body has, which is free and fits it; a draw from a socket into an empty hand; a hand-over to an empty other hand;
- a let-go, recorded as at an ordinary pace or at speed;
- a loss of a weapon lying in the world: out of the world whatever its departure, or left behind only when it was let go of at speed. It returns to the first free socket that fits it, or, with none free, to the player's feet, so that the body being full never loses a weapon;
- a game's give, to an empty hand (the dominant first), else a free socket that fits it, else the player's feet;
- a game's removal of one object or one item from wherever it is, or of every weapon on the body, the hands included; the weapons lying in the world stay, since the game takes what the player holds;
- a game's drop of an object the player has, wherever it is: a weapon lies in the world as one the player dropped, and anything else leaves;
- a weapon the mod spawned to stand for one the player has (one of the sidecar's after a load, or the second pistol of a pair split in two), onto its recorded socket when that is free and fits it, otherwise where a weapon goes when the player does not choose;
- a restored item's object spawned.

Where a weapon goes when the player does not choose (a give with the hands full, a loss, a respawn, a restore) is the socket the game docks its slot on, then the snuggest socket that fits it, the dominant side first.

### The spawn reset and the caches

At the reset's start every weapon the play projection puts in a slot waits for its slot's new object; a slot already waited for stays as it is. At its end the weapons in the world leave the table, and each new slot object re-binds the item waiting for that slot with the same type, so it keeps its identity and its place. A new object nothing waits for goes onto the body, and an item whose slot the game emptied or gave another type leaves. A respawn also empties the hands: their weapons go onto the body and anything else leaves. A travel keeps the hands, and a load starts the table again from the slots.

At each of the game's caches (a checkpoint, a mission taking the player's weapons, a challenge) the table snapshots the weapons on the body with their places and the slots the store projection gave them. A checkpoint's restore returns the table to the snapshot: what was gained since leaves, and every item comes back to its socket, all taken out before any goes back so that two that swapped places swap back. A mission's or a challenge's restore brings the snapshot's weapons back beside what the player holds since. Either way the hands come back empty, the restored slots' objects re-bind the snapshot's items by slot and type, and a snapshot's weapon whose object is gone waits to be spawned again.

### The projection

Each weapon slot holds one weapon of that slot from the body: the primary hand's, then the other hand's, then the one on the socket the game docks the slot on, then the other sockets' in order, the oldest first among equals. Only a weapon bound to a live object goes into a slot, and one in the world never does. The primary hand's weapon is selected. In play the heavy slot stays empty; for a store or a cache it holds a heavy weapon too, and the sidecar records every weapon on the body with its hand or socket and its slot, or none for one beyond the slots. The consumables' slots stay the game's.

### The events

A hook or the hands send one event per thing that happened: a take, a dock, a draw, a hand-over, a let-go, a loss, a give, a spawn, a removal, a game's drop, a weapon restored onto the body, a store about to happen, a cache, a restore, a reset beginning, and a reset completing with the slots the game now holds. The fold applies each whole or refuses it, and returns the transitions and, for a store or a cache, the slots and the sidecar the game should be given before it goes on.

## The live table

The live table is the one `Loadout` of the local player's items (`loadout::live`), behind one lock, and published with its snapshot at the end of each input tick. It is the only writer of where the weapons are: the grabs, the takes, and the game's own draws and holsters send it events, and the reconcile makes the game's slots, its wielded weapon, and the weapons armed in the hands follow it (["The slots as a projection"](#the-slots-as-a-projection)). The table runs while the VR inventory is on, takes the weapons of the slots the old inventory tracks (the pistols, the two-handed, and the special weapons) and a heavy weapon while a hand holds it, and has every socket whatever the carry settings.

The table says which hand holds what, and the placement follows it ([physical-weapons.md, "Either hand"](physical-weapons.md#either-hand)). It publishes each hand's weapon as one object address per hand, which the placement detours compare against before any lock, and the place of every item on the body behind a leaf lock of its own (`loadout::live::published`), after every fold that can move an item: at the end of the input tick, and at once for the moves the hands make between ticks. The wielded weapon's attach bones follow its hand in the table, the second weapon's too, and the carry set places each carried weapon where the table has it: on its socket's bone and offset, or in a hand. The holder of the wielded weapon is gone: a weapon the game wields lands in a hand of the table's (`loadout::live::expect`), the hand a grab left expecting it (a draw, a pickup, a take from a character or a rack, for up to three seconds), or for the game's own draw the dominant hand when it is empty, otherwise the other. The expectation is per-hand runtime state beside the table, not an item's place.

What feeds it:

- On its first input tick the table starts from the slots, as a load does.
- The hand moves. The calls that move a weapon send the move at once (a draw off a holster or a socket, a pickup or take the mod makes, a handover, the second weapon taken, put back, or let fall, a holster or a drop without the game's act, a dock onto a socket), and the reconcile follows. The input tick sees the game's own moves: a weapon the table has in a hand that the game no longer wields and that is neither armed there nor kept by the carry set (the game holstered or dropped it) docks on the socket the game docks its slot on, or is let go of when it left the slots; then the wielded weapon lands in a hand when the table has it in none: drawn off its socket, taken off the ground, or, for a weapon the table does not know, taken from where the expecting hand's grab says it came from (a pickup, a character, a rack, or a give).
- The weapons the game no longer keeps: each tick, every weapon of the table's that is in no slot, the carry set, or a hand of the game's is asked what became of it through the inventory's weak reference to it (`inventory::world`, [vr-inventory.md, "The weapons the inventory watches"](vr-inventory.md#the-weapons-the-inventory-watches)). One lying in the world that the table still has on the body is a game's drop, and one another character holds or whose object is gone is a removal. A weapon the pins left to the game is removed as forgotten.
- The spawn reset: its pre-reset event begins the reset and removes the carry set's weapons, which the inventory releases and the reset then removes; the first input tick after its post-reset event completes it with the slots it left. The reset is a respawn when the player's health was spent at its begin, a load when the game loaded the player's save data since, and a travel otherwise. That classification is unverified. A load of the save data also completes a load on its own, so a load that does not reset first still starts the table from its slots.
- The store of the save data is a store; the table's projection is kept for the readouts.
- A mission's weapon cache ([`NodeWeaponCache::CacheAllWeaponSlots`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graph_script::NodeWeaponCache##func-CacheAllWeaponSlots)) is a cache before the game's, and with its removal takes every weapon on the body after it. The give-back has no seam of its own, so as in the old handling it is the tracked slots holding a weapon again after they were seen empty, and it restores the cache with what they hold. The inventory stashes the carried weapons for the mission and keeps their objects, so each item the restore leaves waiting for a spawn is bound to a kept weapon of its type.
- A sidecar weapon the restore respawns onto a socket, and the second pistol of a pair split in two, are restored onto their sockets.

The readouts are a Status row (`LOADOUT TABLE`: where the items are, and how many events were refused) and the debug UI's `Loadout` section (the table, the last store, and the latest transitions and refusals). Until step 6 a divergence readout compared the table with the old handling's slots, carry set, and hands; with the table the only writer it went.

The weapons lying in the world are the table's alone. The pins read them from the table (`inventory::pins`, [vr-inventory.md, "Keeping dropped weapons"](vr-inventory.md#keeping-dropped-weapons)): each one's distance comes from its fate, a pin is a strong reference the inventory holds beside the weak one, and a weapon forgotten leaves the table. The registry that mirrored the slots, the hands, and the world beside the table is gone, and so are its log lines; the table's transitions are the log.

Decisions taken while wiring it:

- The hand moves are sent from the call that makes them where the call knows the move, and otherwise from the input tick's view of the two weapons in the hands. The table leads the hands: a weapon the game wields lands in a hand of the table's rather than the table following a hand the old handling chose, so the placement has one source. A move the table already shows sends nothing, and a hand the table disagrees about is left as the table has it.
- A weapon the game wields lands only in a hand the table has empty. With both hands full (a stale item the dual-write could not dock, since no socket that fits it was free), it keeps the game's layout until a hand empties, rather than being refused anew every tick.
- The hold needs the table: with the VR inventory off, the game's own layout stands, as with `weapons.hold.either_hand` off.
- A heavy weapon is the table's while a hand holds it, so its hand is the table's like any other's. The heavy slot stays the game's: the game drops a heavy weapon it holsters, which the table sees as a let-go.
- In step 4 a slot's weapon the carry set moved onto another socket without a hand (the weapon a pickup replaced, the slot's weapon a carried one drawn or promoted into the slot put out) stayed on its slot's socket in the table, since the core has no socket-to-socket move; step 6 deleted those moves, so the table shows the new model's answer.
- The game's drop is a core event of its own. The old handling and the game drop weapons the player has without any hand letting go (a pickup's replaced weapon, the carry set switched off, the game's own drops), and the plan routes the game's automatic moves through the table: the game's word on ownership is final, so the weapon lies in the world, where the pins keep it.
- A removal of every weapon takes the weapons on the body and leaves those lying in the world: a mission takes what the player holds, not what lies near them.
- A weapon the game gives without the hands (a mission's give into a slot) is unknown to the table until a hand draws it, and is taken then as a give. Until then it diverges, and one the game then drops is not in the table's world. Phase 4 binds the give's seam.
- A weapon another character takes leaves the table, as the core decided, and is never the player's again until a hand takes it back: the inventory watches it while that character keeps it near (for the readouts, and so a single pistol stays one pistol), but one the character drops again is not pinned, where the registry pinned it as the player's again.
- Until step 6 the old model diverged from the table in places of its own: it carried a weapon beside each slot's weapon on the same back bone, which the table, with three places on the back, put elsewhere; and the dual-wield pair's two guns ride both hips as one item.

## The per-hand tick

Every gun in a hand fires from that hand's own trigger and aims at that hand's own point (`weapons::hold::tick`). The fire path keys on the weapon's user, so a gun the game does not wield fires as the player's from the holstered state.

- A weapon the table holds in a hand that the game does not wield is armed there (`weapons::hold::armed`): its guns' holstered attach-bone entries name the hand's bone, its holstered scale is one, and the player uses it ([`WeaponBase::StartUsing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-StartUsing)); disarming puts the entries and the scale back and stops using it. Its holder stays the player, so its bullets ignore the player's own bodies. This is per-hand runtime state beside the table, keyed by the hand: the table says what the hand holds, and the armed state what the mod did to the object to hold it there. The rigid grip, the gun aim, the per-shot seam, the docks, and the render-time late update treat an armed weapon as held. Today the second weapon is the one weapon armed.
- After the inventory's own weapon updates, the mod updates each armed weapon through its parallel and serial update virtuals with its hand's fire input and aim point, the input given only while the shooting permission allows it and the true reload does not hold the gun.
- The wielded weapon is still updated by the inventory's fan-out, but with its hand's fire input in place of the game's own: the gun aim's per-weapon detour hands it over, so both hands fire the same way and the fire action reaches no gun. The dual-wield pair keeps its split's inputs.
- Each gun's ammunition check asks the character to reload the wielded weapon when its own magazine runs low, and a gun that does not reload by magazine cancels the character's reload every tick. Inside the local player's checks both requests are refused (`weapons::empty`, on the detours of [`Character::StartReloadingWieldingWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-StartReloadingWieldingWeapon) and [`Character::CancelReload`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-CancelReload)).
- Each armed gun takes the wielded weapon's aim mode, which is the player's, since the game forwards the player's mode to the selected weapon alone. With the controller scheme refusing precision aim, that is normal aim.
- A weapon's use hint is keyed by hash in the camera's hint table, added at its start of use only when absent and removed at its stop only when present ([`GameCameraManager::HasHint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/camera::game_camera_manager::GameCameraManager##func-HasHint)), so two guns of one type share one reference and the first to stop takes it away. A stop of a local player's weapon is noted, and the next input tick puts the hint back while a gun in a hand still in use carries it.
- The true reload acts on each hand's gun at once ([reload.md](reload.md)): its chamber, its action, its belt push, and its magazine are its own, and its record is keyed by the table's item, so a weapon keeps its chamber across the spawn reset's re-key and a handover. The focus that turned the reload between the wielded and the second weapon is gone.

The per-hand tick is behind `weapons.hold.per_hand_tick` (on), in the debug UI's Controllers tab. Off, the wielded weapon fires from the game's own fire input, an armed gun fires only while its magazine holds a round (its ammunition check would otherwise reload the wielded weapon), and nothing is refused, set, or put back; the arming and the per-hand reload stay, since they are not the tick's.

Decisions taken here:

- The game's fan-out keeps updating the wielded weapon, and only its input is the hand's: updating it a second time from the mod would advance its timers twice a frame, and the fan-out's own choice of what to update (the mounted gun, the boost wingsuit's guns, the grenade) stays the game's. A seat that skips the wielded weapon's update still skips it; that is phase 5's.
- The requests are refused only inside the ammunition check, so the reload input's own reload start still goes through to the game. With the true reload off, an empty gun no longer reloads on its own; its reload input does.
- The armed guns' aim mode follows the wielded weapon's rather than reading the player's, which the bindings do not reach from the character; the two are the same whenever a weapon is wielded, and normal aim otherwise.

## The slots as a projection

From step 6 of phase 3b the table is the only writer, and the game's slots are made to match it each input tick (`weapons::inventory::reconcile`), after the table's feed, and at once after the hands move a weapon:

- Each slot the inventory tracks (the pistols, the two-handed, and the special weapons) holds the weapon the play projection puts there. A weapon that leaves its slot stays alive in the carry set, which now holds a strong reference to every weapon of the table's on the body that no slot holds, and goes out through [`Inventory::RemoveWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-RemoveWeapon), unequipped first when it is wielded; one that comes in goes in through [`Inventory::AddWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-AddWeapon) without an equip. A weapon in a slot that the table does not have on the body (a mission's give before a hand takes it) is left there.
- The primary hand's weapon is wielded: moving the primary unequips the old weapon and equips the new one, with no act, after aborting a draw, holster, or switch clip still playing ([`EventIdSymbolTable::ACT_ABORT_WEAPON_PARTIAL`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/animation::symbol_table::EventIdSymbolTable)), and only while the inventory's permissions allow a draw or a holster. With no weapon in the hands, a weapon of the table's the game wields is unequipped. A weapon the game wields that the table does not have on the body (a grenade, the planted explosive, a mounted gun) is left to the game.
- Every other weapon a hand holds is armed there, and a hand's armed weapon that left it, or that the game now wields, is disarmed.
- A slot's weapon on a socket other than the one the game docks its slot on is placed there by the mod: its holstered attach-bone entries name the socket's bone at the hold's write points, and the socket's offset moves it after the inventory's placement. The slot's single pistol's hip is the one the table has it on.
- Before the game stores its save, and before a mission caches the slots, the tracked slots are given the store projection's content. In play they already hold it, since the two projections differ only in the heavy slot.

The moves the old handling made in the slots are gone: the carry set's slot swaps (taking a slot's weapon onto a socket, a carried weapon into its slot and its occupant onto the socket it left, a carried pistol into the empty pistol slot), the second weapon's promotion and its wait for the hands to be empty, and the carrying of the weapon a pickup replaces. A grab of a socket, a dock, a draw without the game's act, a take of the second weapon, and its put-back and fall are events for the table, and the reconcile does the rest. Before the game's own pickup puts a weapon into a taken slot, the slot's weapon leaves the slot and stays on its socket. The second weapon is the weapon armed in the hand the game does not wield; when the other hand empties, its hand is the primary and it is wielded in place. The divergence readout of the dual-write is gone with the dual-write.

The projection is behind `weapons.inventory.project_slots` (on), in the debug UI's Controllers tab. Off, the table is put back into the game's slots as a store would write them, the hands' armed weapons are disarmed, the table stops, and the game's own one-weapon model runs: the carried weapons left over fall where they are, as with the inventory off, and the hold keeps the game's layout. This is the plan's scheme-switch behaviour; the old slot swaps are deleted, so there is no older path to fall back to.

Decisions taken here:

- The heavy slot stays the game's: the reconcile leaves it alone, so a heavy weapon is wielded from its slot while a hand holds it and dropped by the game when it is put away, as before. Emptying it in play needs the base layer's heavy-weapon drops patched and a removal of a wielded heavy weapon read in the IDB, which phase 5 does.
- The store and cache seams apply the store projection only to the tracked slots, for the same reason.
- A weapon the game holsters or drops on its own (its holster act, a vehicle's entry) still leaves its hand in the table at the next input tick, docked or let go of, as the old handling's; undoing those is phase 5's. A weapon armed in a hand, or one the carry set keeps off the slots, is never taken for such a departure.
- A weapon the game wields while both hands of the table are full is unequipped again, since it can land in no hand.
- Switching the projection off is the scheme switch rather than a mode where the table moves the hands and the game keeps its slots: with the slot swaps gone, the table's moves need the reconcile to happen in the game at all.

## One socket notion

The loadout's sockets are the mod's only notion of a place on the body. The holsters (`GrabVolume::Holster`) and the carried weapons' sockets (`GrabVolume::Socket`, the carry set, the second weapon's home, the single pistols' hips, and the sidecar) all name a `loadout::Socket`. The adapters in `inventory::sockets` give what the old handling needs of one: the bone and the offset a carried weapon is placed by, the name the sidecar keeps it under, and which sockets the carry settings give the carry set and in what order it fills them. The sidecar's names are the carried weapons' sockets' names from before (`back_first` the right shoulder, `back_second` the left, `back_outer`, `hip_left`, `hip_right`, `chest`), so existing sidecars load, and the settings keep their offset fields' names.

The game docks a holstered weapon by its slot, never mirrored for handedness ([`WeaponBase::UpdatePostFinalizePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-UpdatePostFinalizePose)): the two-handed and heavy slots' guns on [`BACK_ATTACH`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::SafeBoneIndex##variant-BACK_ATTACH), on the right of Rico's back, and the special slot's on [`BACK_ATTACH_2`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::SafeBoneIndex##variant-BACK_ATTACH_2), on the left. So the right shoulder is the two-handed and heavy slots' own socket and the left shoulder the special slot's, whatever the dominant hand, and the core's preference and projection take them so. The shoulder holsters follow: the right one draws the two-handed slot and the left one the special slot. For a left-handed player that swaps which shoulder holster draws which slot; with the holsters seated on their guns (`holsters.follow_docks`, on) nothing moves, since each holster already sat on its slot's gun, and with it off each shoulder holster now sits on the side its gun is on. The carry set fills the back from the right shoulder, as it always has, whatever the dominant hand.

## Phases

1. The RE pass. Answers the questions below, and binds what the core and the reconcile need. Done, apart from the open points under "RE answers".
2. The relaxed body. The weapon-wielded condition reads false for the local player's base layer, on its own ahead of the rest, since the combat stance is wrong today whatever the inventory does.
3. The core and the reconcile. The table, its transitions, the slot projection, the per-hand tick and reload, the primary, the readouts; the existing hands and sockets moved onto it. The pure core is done (3a); the reconcile and the move of the existing mechanisms onto it are 3b.
4. The takes. Every source lands in the table and the grabbing hand: the ground, racks and lockers, characters' hands, missions' gives; the game's pickup detoured.
5. Traversal and persistence. Vehicles and seats with armed hands, the wingsuit, the parachute, swimming, the base layer's holster tasks, saves, the checkpoint snapshot, missions and challenges, the spawn reset's re-keying, death and respawn, the lost-weapon return, the scheme switch.
6. The grapple snatch, on top of the takes ([grapple-snatch.md](grapple-snatch.md)): the first feature built on the table, and its end-to-end test.
7. Polish: the per-hand finger poses from the class clips.

Each phase replaces, in the same change, the docs it supersedes: the second weapon, the separate pistols, and the carry set in [physical-weapons.md](physical-weapons.md) and [vr-inventory.md](vr-inventory.md), the dual-wield and two-weapon reloads in [reload.md](reload.md), and the dual-wield parts of [controllers.md](controllers.md). This doc's design sections become the description of what is built as each phase lands, and [vr-inventory.md](vr-inventory.md) is folded into it, keeping only what survives (the despawn protection), so the docs never describe two systems as current.

## RE answers

The phase 1 pass answered the questions it opened with; the findings are in the engine docs and bound in the pyxis defs.

1. A gun ticked outside the selected wield fires fully as the player's, in the holstered state, with the player as its user (see "Firing").
2. The graph's weapon conditions change the base layer's families and stance, not what the player may do (see "Animation"); the base layer's state tasks are what move weapons on their own (see "No automatic moves").
3. Per-hand weapon animation is per class, from one clip for both hands, and feasible at the pose seam (see "Animation").
4. The shooting permissions gate only the game's own fire tick's inputs; they are cleared in vehicle entry and exit, the ragdoll, hit reactions, death, the grapple's fire and reel, the free fall's and the wingsuit's tricks, the parachute's opening, underwater, mounted guns, grenade throws, melee, and the pickup.
5. Each gun keeps its own aim mode; the precision aim and the camera hints are the contention (see "Firing").
6. Moving the primary with `OnUnequip` and `OnEquip` and no act is stable; nothing in the player's graph equips on its own.
7. The game's deferred equip is the AI's only; the player's weapons are equipped, unequipped, selected, given, and removed through a few choke points, which the table routes: `Inventory::AddWeapon`, the give's spawn callback, the removal and despawn, `WeaponBase::OnEquip` and `OnUnequip`, the selection setter, and the auto-pickup's weapon check.
8. The three weapon caches (the mission manager's at each checkpoint, the spawn-weapons node's, and the challenges') compare per slot by type (see "The game's slots are a projection").
9. The spawn reset runs on every load-state entry and replaces the slot weapons (see "The game's slots are a projection").
10. Every seat but the motorcycle's skips the wielded weapon's and the grenade's tick; the boost wingsuit replaces the wielded tick with its own guns; nothing in code holsters for the parachute, swimming, or the grapple's hang, so what does comes from the graph.

Still open: which of the base layer's states own its unequip and hide-in-vehicle tasks; how the seats' poses take armed hands under the IK; the order in which a weapon's own pre-simulation update handles its deferred operations; and the bone lists of the arms', the lower body's, and the recoil's clips.

## Unverified design points

- How arm steering in the wingsuit and the parachute's handles work with a gun in a hand.
- Whether a lost weapon's return reads well, and the speed that counts as travelling fast.
- How a one-handed heavy weapon's lift limit feels.
