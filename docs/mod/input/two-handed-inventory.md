# The two-handed inventory

Not built yet: this is the design and the plan of record. It replaces the mechanisms described in [vr-inventory.md](vr-inventory.md) and the second weapon and separate pistols of [physical-weapons.md](physical-weapons.md), and the [grapple snatch](grapple-snatch.md) is built on top of it.

The local player's two hands are independent. Each holds its own weapon or object, items pass between the hands, weapons dock on the body's sockets and are drawn from them, and the player carries what the body holds rather than what the game's slots allow. Boneworks and Half-Life: Alyx are the guiding light: the player does everything with their own body, even where that adds friction, and nothing docks, draws, or switches by itself.

## Why the game's inventory is replaced

The game wields exactly one weapon by construction ([engine: weapon-ownership-and-despawn.md](../../engine/gameplay/weapon-ownership-and-despawn.md), [engine: item-pickup.md](../../engine/gameplay/item-pickup.md)). Its eight slots are a fixed count, `Character::GetWieldingWeapon` returns the selected slot's weapon, and nearly every consumer reads that one weapon: the fire tick, the reload paths, the track messages, the animation graph, aim, the HUD, the AI's threat rating, the camera, the vehicles' and traversal's holstering, the saves, and the missions. The mod has so far kept that model and worked around it (a second weapon fired by the mod, a carry set beside the slots, pistols split from their pair, and takes that reshuffle the slots after the game's own pickup), and every workaround meets every other: a take with the free hand goes through the game's pickup, whose equip takes the other hand's weapon away, and the second weapon lacks the foregrip, heft, sights, scope, wall collision, and handover that the wielded one has.

So the mod owns weapon handling for the local player. The game keeps three jobs: owning the weapon objects (their spawn, despawn, and references), simulating the weapons themselves (projectiles, effects, damage), and holding the ammunition reserves. Everything else that reads the wielded weapon is replaced by the mod, patched to work per weapon, or fed with the primary hand's weapon.

## The model

### The loadout table

One table holds every item the player has: each is in exactly one place, the left hand, the right hand, a body socket, or the world (kept near the player). Items are weapons, the props a hand picks up, and a grenade or a C4 charge while a hand holds one. Both hands are the same type: a gun in either hand has the foregrip, the heft, the sights, the scope, the wall collision, the reload, and the pips, and a heavy weapon held in one hand is simply very heavy and lifts only so far.

The primary hand is the dominant hand when it holds a weapon, otherwise the other hand. The primary follows from the hands and is never set on its own.

### The game's slots are a projection

The game's slots are made to match the table, never the other way round:

- the primary hand's weapon is in its slot, selected, and wielded, for the consumers the mod feeds rather than patches;
- before the game stores its save, and before a mission or challenge caches the player's weapons, the slots are rewritten to a sensible loadout of what the table holds, and the mod's sidecar records the rest: which hand or socket each weapon is in, and the weapons beyond the slots ([settings.md](../settings.md), the inventory sidecar).

The game's word on ownership is final. A mission or challenge that removes the player's weapons removes them from the table, the hands' included; a weapon the game gives goes to an empty hand if one is free, otherwise to a socket that fits it. On death and respawn the game's rules decide what the player owns, and the hands come back empty with everything else on its socket.

### No automatic moves

Nothing moves an item but the player:

- Every take (from the ground, a rack or locker, a character's hands, a mission's give, the grapple snatch) lands in the table and then in the grabbing hand. The game's equip never runs, so a take never touches the other hand.
- Entering a vehicle, gliding, opening the parachute, falling, and swimming leave the hands as they are. A hand holding a gun cannot take the steering wheel or a control, so the player drives one-handed or docks first; a gun in a hand in a seat fires, since the mod ticks it rather than the game's wielded tick, which skips non-motorcycle seats.
- The game's weapon cycling, the slot selects, the weapon wheel, and the automatic switch away from an empty weapon become requests on the table or go away.
- Ammunition pickups credit the reserve for any weapon in the table.

### Letting go

A weapon a hand lets go of stays where it falls, protected from the despawn while the player is near. It returns to a free socket that fits it only when it is lost rather than dropped:

- out of the world: it fell through the terrain or below the world's floor, or sank out of reach in water;
- left behind at speed: it left the hand while the player travelled fast (the wingsuit, a vehicle, the parachute, or a fall), so the player never chose to leave it there.

Swapping a weapon on the ground for another, or walking away from one, never brings it back. Retrieving a weapon left behind is the grapple snatch's job.

## Slots

The sockets are today's ([vr-inventory.md, "Carrying more than the slots"](vr-inventory.md#carrying-more-than-the-slots)), on Rico's skeleton ([body/rig.md](../body/rig.md#the-bodys-volumes-on-ricos-skeleton)): the hips for pistols, three on the back for long guns and heavy weapons, the chest, and the pouches. Each socket takes its size class or smaller, mirrored for handedness. A held weapon shows before the hand opens whether the socket under it fits, and docking confirms with a haptic cue. Props have no socket and no backpack: letting go of one drops it. Grenades and C4 stay consumables drawn from their pouches.

## Ammunition and reload

One reserve per ammunition type, shared by both hands: two pistols of one calibre draw on one stock. The universal belt pouch hands out what the gun in the other hand takes ([reload.md](reload.md)). Every reload is the mod's true reload, per weapon: the game's reload paths are refused for the local player, and each gun's magazine, chamber, and action are its own.

## The HUD

Each hand shows its own weapon's state; the per-hand ammunition pips do this today, and a fuller per-hand HUD is the goal. Until then, the game's ammunition counter shows the primary's.

## Animation

The arms are the mod's IK, and the game's weapon acts (the draw, the holster, the switch, the reload) are cut. A weapon's own hand animation, its hold and fire layers on the hand and wrist, should apply to the hand that holds it and nothing else, if the game's clips can be sampled per hand (an RE question below); otherwise the grip poses, the recoil, and the reload's parts cover the hand.

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
| Firing as the player from a hand (heat, the gunshot stimulus, stealth, player-fired damage) | Patched or verified for a weapon that is not the selected one |
| The shooting permissions (`GetAnimationAllowsShooting` and the inventory's) | Bypassed for the mod's tick |
| The aim mode with two weapons in use (`WeaponBase::StartUsing`, `Player::SetAimMode`) | Patched so both have a user without contention |
| Vehicles' and traversal's holstering and tick swaps | Patched out for the player: the table's rules apply |
| The animation graph's armed and weapon-type states | Fed by the primary, as far as they still drive locomotion and stance |
| The AI's threat rating, Rico's lines, the level of detail, the button hints, the store | Fed by the primary |
| The save, the mission caches and restores, the challenges | Fed by the slot projection and the sidecar |

## The architecture

The table is the one source of truth for the player's items, owned by one module, and nothing else holds any of that state.

- The core is pure: the table, its places, and its transitions (take, dock, draw, hand over, let go, lose, a game-side give or removal), with its invariants in the types: each item is in exactly one place, a hand holds at most one item, and the primary follows from the hands. It is tested without the game.
- The hooks are thin adapters. A detour decides nothing: it turns what the game wanted (a pickup, a give, a removal, a save about to be stored, a mission cache) into an event for the table, or reads the table's published snapshot. One reconcile step per tick makes the game match the table: the slot projection, each weapon's state and user, its placement and visibility, and its tick.
- The readouts are one: a Status row and a debug UI section show the whole table, and the log records every transition with its cause, so a bug is either the table being wrong or the reconcile not applying it.

The existing pieces fold into it: the second weapon's tick becomes every hand's, the carry set's keeping, placement, and end of pass become the table's, the registry and its despawn protection, the true reload, the sidecar, and the store and mission detours carry over, and the second weapon, the carry set as a separate store, the split pistols as an exception, and the pickups' reshuffles go.

Switching the control scheme away from the controllers (or turning the mod's weapon handling off) projects the table back into the game's slots, so nothing is lost; switching back rebuilds the table from the slots and the sidecar. Gamepad play is expected to be rough meanwhile.

## Phases

1. The RE pass. Answers the questions below, and binds what the core and the reconcile need.
2. The core and the reconcile. The table, its transitions, the slot projection, the per-hand tick and reload, the primary, the readouts; the existing hands and sockets moved onto it.
3. The takes. Every source lands in the table and the grabbing hand: the ground, racks and lockers, characters' hands, missions' gives; the game's pickup detoured.
4. Traversal and persistence. Vehicles and seats with armed hands, the wingsuit, the parachute, swimming, saves, missions and challenges, death and respawn, the lost-weapon return, the scheme switch.
5. The grapple snatch, on top of the takes ([grapple-snatch.md](grapple-snatch.md)).

Each phase replaces, in the same change, the docs it supersedes: the second weapon, the separate pistols, and the carry set in [physical-weapons.md](physical-weapons.md) and [vr-inventory.md](vr-inventory.md), the dual-wield and two-weapon reloads in [reload.md](reload.md), and the dual-wield parts of [controllers.md](controllers.md). This doc's design sections become the description of what is built as each phase lands, and [vr-inventory.md](vr-inventory.md) is folded into it, keeping only what survives (the registry and its despawn protection), so the docs never describe two systems as current.

## RE questions

1. Does a weapon ticked by the mod outside the selected wield fire fully as the player's: heat, the gunshot stimulus the AI hears, stealth, and player-fired damage? If not, which state it needs (wielding without being selected, for example).
2. What the animation graph does with the wielded weapon's type and hand once the arms are IK'd: which conditions and modifiers change locomotion or block actions (sprint, vault, the grapple, stunts), and what `IsWieldedInLeftHand` selects.
3. Whether a weapon's hold and fire clips can be sampled as layers and applied to one hand's bones only.
4. The shooting permissions: which animation states clear them.
5. Two users of the aim mode: what `Player::SetAimMode` hands a weapon, and whether a second user breaks the first's.
6. Moving the primary without acts: whether a slot select and equip with no draw act leave the game stable.
7. The weapon's own deferred equip and unequip (`CWeaponBase::UpdatePreSim`): whether the game can equip a weapon behind the table's back.
8. Missions and challenges: which use the weapon cache, how challenges strip and restore weapons, and what the restore does to slots the table rewrote.
9. Which transitions pass through the spawn reset (death, fast travel, the frontend).
10. The vehicles' entry and exit paths: which states holster or hide the wielded weapon, and how the seats' poses take armed hands under the IK.

## Unverified design points

- How arm steering in the wingsuit and the parachute's handles work with a gun in a hand.
- Whether a lost weapon's return reads well, and the speed that counts as travelling fast.
- How a one-handed heavy weapon's lift limit feels.
