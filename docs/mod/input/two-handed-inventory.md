# The two-handed inventory

Mostly not built yet: this is the design and the plan of record. The pure core is built (["The core as built"](#the-core-as-built)) but not wired into the game; everything else is design. It replaces the mechanisms described in [vr-inventory.md](vr-inventory.md) and the second weapon and separate pistols of [physical-weapons.md](physical-weapons.md), and the [grapple snatch](grapple-snatch.md) is built on top of it.

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

The existing pieces fold into it: the second weapon's tick becomes every hand's, the carry set's keeping, placement, and end of pass become the table's, the registry and its despawn protection, the true reload, the sidecar, and the store and mission detours carry over, and the second weapon, the carry set as a separate store, the split pistols as an exception, and the pickups' reshuffles go.

Switching the control scheme away from the controllers (or turning the mod's weapon handling off) projects the table back into the game's slots, so nothing is lost; switching back rebuilds the table from the slots and the sidecar. Gamepad play is expected to be rough meanwhile.

## The core as built

The code is `payload/src/controllers/loadout/`. It is pure, apart from the object address an item is bound to, and nothing feeds it yet: the old weapon handling still runs, and moves onto it in phase 3b. Until then the debug UI's Controllers tab has a `Loadout core` section that drives a sandbox table of its own with made-up objects, showing the items, the primary, the slots in play, the last store's sidecar, and the latest transitions and refusals.

### Items and their identity

An item is a weapon (its spawn tag's hash, its slot, and its size class), a prop, a grenade, or a C4 charge. Its identity is the table's own, assigned when it enters the table and never reused in a session. Beside it the table keeps the object the item is now: a live address, or nothing while it waits for one, either for its slot's new object across a spawn reset or for a spawn of its type after a restore. The address is the one place the core meets the game; the hooks and the reconcile turn it into an object.

### The places and their invariants

There are six sockets: each hip, each shoulder (where the game docks its two-handed and special slots' weapons, the two-handed on the dominant side), a third place across the back, and the chest. The size classes are, smallest first, the pistol, the submachine gun, the long gun, and the heavy weapon. The hips and the chest take submachine guns and smaller, and the back's three places take everything, the heavy weapons included. How many places the back and the hips have, and whether there is a chest socket, follow the settings and the dominant hand, as the carried weapons' sockets did; a socket a smaller layout lacks keeps the weapon already on it until the player draws it.

The table's layout carries the invariants. An item is a value that cannot be copied, owned by exactly one container: a hand, a socket, or the world. Each hand and socket holds at most one. A socket's item can only be made through a check that it fits, and only a weapon can lie in the world, so a prop, a grenade, or a charge let go of leaves the table. No two live items share an object, and the primary is never stored: it is the dominant hand when that holds a weapon, otherwise the other hand when it does, otherwise none.

### The transitions

Each transition applies whole and returns its changes (an item entered, moved, left, was bound to a new object, or lost its object), each with its cause for the log, or is refused with a typed error and leaves the table as it was:

- a take into an empty hand, from the ground, a rack, a rack's copy, a locker, a character's hands, a mission's give, the grapple snatch, a pouch, or out of the air; each source brings only its kinds (a pouch a grenade or a charge, a catch a grenade, the ground a weapon or a prop, the rest weapons), and a weapon lying in the world taken again keeps its identity;
- a dock of a hand's weapon on a socket the body has, which is free and fits it; a draw from a socket into an empty hand; a hand-over to an empty other hand;
- a let-go, recorded as at an ordinary pace or at speed;
- a loss of a weapon lying in the world: out of the world whatever its departure, or left behind only when it was let go of at speed. It returns to the first free socket that fits it, or, with none free, to the player's feet, so that the body being full never loses a weapon;
- a game's give, to an empty hand (the dominant first), else a free socket that fits it, else the player's feet;
- a game's removal of one object, one item, or every weapon, from wherever it is, the hands included;
- a restored item's object spawned.

Where a weapon goes when the player does not choose (a give with the hands full, a loss, a respawn, a restore) is the socket the game docks its slot on, then the snuggest socket that fits it, the dominant side first.

### The spawn reset and the caches

At the reset's start every weapon the play projection puts in a slot waits for its slot's new object; a slot already waited for stays as it is. At its end the weapons in the world leave the table, and each new slot object re-binds the item waiting for that slot with the same type, so it keeps its identity and its place. A new object nothing waits for goes onto the body, and an item whose slot the game emptied or gave another type leaves. A respawn also empties the hands: their weapons go onto the body and anything else leaves. A travel keeps the hands, and a load starts the table again from the slots.

At each of the game's caches (a checkpoint, a mission taking the player's weapons, a challenge) the table snapshots the weapons on the body with their places and the slots the store projection gave them. A checkpoint's restore returns the table to the snapshot: what was gained since leaves, and every item comes back to its socket, all taken out before any goes back so that two that swapped places swap back. A mission's or a challenge's restore brings the snapshot's weapons back beside what the player holds since. Either way the hands come back empty, the restored slots' objects re-bind the snapshot's items by slot and type, and a snapshot's weapon whose object is gone waits to be spawned again.

### The projection

Each weapon slot holds one weapon of that slot from the body: the primary hand's, then the other hand's, then the one on the socket the game docks the slot on, then the other sockets' in order, the oldest first among equals. Only a weapon bound to a live object goes into a slot, and one in the world never does. The primary hand's weapon is selected. In play the heavy slot stays empty; for a store or a cache it holds a heavy weapon too, and the sidecar records every weapon on the body with its hand or socket and its slot, or none for one beyond the slots. The consumables' slots stay the game's.

### The events

A hook or the hands send one event per thing that happened: a take, a dock, a draw, a hand-over, a let-go, a loss, a give, a spawn, a removal, a store about to happen, a cache, a restore, a reset beginning, and a reset completing with the slots the game now holds. The fold applies each whole or refuses it, and returns the transitions and, for a store or a cache, the slots and the sidecar the game should be given before it goes on.

## Phases

1. The RE pass. Answers the questions below, and binds what the core and the reconcile need. Done, apart from the open points under "RE answers".
2. The relaxed body. The weapon-wielded condition reads false for the local player's base layer, on its own ahead of the rest, since the combat stance is wrong today whatever the inventory does.
3. The core and the reconcile. The table, its transitions, the slot projection, the per-hand tick and reload, the primary, the readouts; the existing hands and sockets moved onto it. The pure core is done (3a); the reconcile and the move of the existing mechanisms onto it are 3b.
4. The takes. Every source lands in the table and the grabbing hand: the ground, racks and lockers, characters' hands, missions' gives; the game's pickup detoured.
5. Traversal and persistence. Vehicles and seats with armed hands, the wingsuit, the parachute, swimming, the base layer's holster tasks, saves, the checkpoint snapshot, missions and challenges, the spawn reset's re-keying, death and respawn, the lost-weapon return, the scheme switch.
6. The grapple snatch, on top of the takes ([grapple-snatch.md](grapple-snatch.md)): the first feature built on the table, and its end-to-end test.
7. Polish: the per-hand finger poses from the class clips.

Each phase replaces, in the same change, the docs it supersedes: the second weapon, the separate pistols, and the carry set in [physical-weapons.md](physical-weapons.md) and [vr-inventory.md](vr-inventory.md), the dual-wield and two-weapon reloads in [reload.md](reload.md), and the dual-wield parts of [controllers.md](controllers.md). This doc's design sections become the description of what is built as each phase lands, and [vr-inventory.md](vr-inventory.md) is folded into it, keeping only what survives (the registry and its despawn protection), so the docs never describe two systems as current.

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
