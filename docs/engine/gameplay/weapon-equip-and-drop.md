# Weapon equip, pickup, and drop

How a weapon object moves between the world, the inventory, the hands, and the body: what completing a pickup does, what equipping and unequipping do, and how a weapon is dropped. Picking a weapon up through the use input is in [item-pickup.md](item-pickup.md#picking-up-a-weapon), the magazine and the reserve in [weapon-ammo-and-reload.md](weapon-ammo-and-reload.md), and the attach bones in [hands-locomotion-and-state.md](../character/hands-locomotion-and-state.md).

## One object for its whole life

A weapon is one [`WeaponBase`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase) whether it lies in the world, is held, or is holstered. The character's inventory holds one weapon per slot ([`Inventory::m_Weapons`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##field-m_Weapons)), and the weapon's state ([`WeaponBase::m_State`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_State)) says where it is. The magazine ([`m_MagazineAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_MagazineAmmo)) belongs to the object and travels with it; the reserve belongs to the character.

The weapon's bodies are in the physics world only in the dropped, mounted, and turret states (and a heavy weapon's while wielded). Every other state, holstered and wielded included, takes them out, and dropped also wakes them.

## Completing a pickup

[`WeaponBase::PickupItem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupItem) takes the character's game-object subobject ([`Character::GAME_OBJECT_BASE_OFFSET`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##const-GAME_OBJECT_BASE_OFFSET)) and ignores any other activator. It:

1. unequips the wielded weapon, which goes to its holster;
2. puts this weapon into the inventory with the equip mode that equips it. The inventory's add runs the weapon's pickup virtual ([`WeaponBase::OnPickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnPickup): the holder is set, the state becomes holstered, so its bodies leave the physics world, the pickup interaction is disabled, and the HUD shows the pickup) and then its equip virtual;
3. selects the weapon's slot and adds its stored ammunition ([`m_StoredNonMagazineAmmo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_StoredNonMagazineAmmo)) to the reserve, and plays the pickup vocal;
4. clears the stored ammunition.

It does not refill the magazine, and it plays no animation act. It accepts a weapon in the dropped state with its bodies in the world: a swimming character's pickup calls it on such a weapon directly, with no prior [`PickupStart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupStart), after dropping the weapon already in that slot, and then queues `ACT_PICKUP_WEAPON_PARTIAL` (see [`Inventory::UpdateWeaponHandling`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateWeaponHandling)). On land, `PickupStart` first puts the weapon in the picking-up state with its bodies out of the world, and the pickup track message calls `PickupItem` later.

Two kinds of weapon are not put into the inventory by it. A hand grenade is left to `PickupStart`, which adds its rounds to the reserve and despawns it. A weapon with [`m_InfinitePickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_InfinitePickup) set gives the character a copy from `PickupStart` (spawned from the weapon's spawn tag), and `PickupItem` adds nothing. Both still lose their stored ammunition to `PickupItem`.

## The inventory's add

The inventory's add reads the incoming weapon's slot ([`m_EquipSlot`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_EquipSlot)). When the slot already holds another weapon, that weapon is dropped when the inventory's per-slot drop flag is set, and otherwise removed, which despawns it (a mounted gun is only released). Then the new weapon is stored and picked up, and with the equipping mode the wielded weapon is unequipped and the new one equipped. The grenade, planted-explosive, and beacon slots are put into use at once. The add is unbound.

## Equip and unequip

The equip virtual ([`WeaponBase::OnEquip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnEquip)) shows the weapon, runs its components' equip, selects its slot, starts using it (its user becomes the character), puts the player's avatar into combat, and sets the state to wielding, or to dual-wielding for the player's dual-wield slot. It queues no act: the draw animation comes from the character's draw and the slot-select path, not from the equip. So a weapon equipped through it alone is in the hands at once, with the character's animation never having played a draw.

The unequip virtual ([`WeaponBase::OnUnequip`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnUnequip)) cancels the holder's reload, sets the state to holstered (a heavy weapon is dropped through the inventory instead, and a mounted gun keeps its state), and stops using the weapon. It plays no holster act either.

Whether the animation allows a draw or a holster is the inventory's own permission ([`Inventory::GetAnimationAllowsDrawing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsDrawing) and [`GetAnimationAllowsHolstering`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsHolstering)); the virtuals do not check it.

## Start using

[`WeaponBase::StartUsing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-StartUsing) makes a character the weapon's user ([`m_User`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_User), with [`m_IsUserLocalPlayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_IsUserLocalPlayer) and [`m_IsUserPlayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_IsUserPlayer)), adds the weapon's camera hint for the locally controlled character, and hands the player's aim mode to the weapon. It does not change the weapon's state. The equip calls it, and the inventory's add calls it for the hand grenade, the planted explosive, and the beacon, which are in use while holstered. The weapon's per-tick update from its user ([`UpdateFromUserPostSim_ParallelBase`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-UpdateFromUserPostSim_ParallelBase)) checks no state either: it stores the aim point, updates the gun at the weapon's next gun with the inputs it is handed, and updates every gun's attached effects.

## Stop using

The stop-using virtual ([`WeaponBase::StopUsing`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-StopUsing)) ends the weapon's fire and effects, clears its user, and, when the weapon is wielding or dual-wielding and not in the heavy slot, sets it holstered. A mounted gun stopped this way also leaves the inventory.

## Dropping

The inventory's drop of a slot ([`Inventory::DropWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-DropWeapon)) clears the slot, stops using the weapon when it is wielding or dual-wielding, and defers the drop to the weapon's next pre-simulation update; with its second argument set it then selects another weapon when the dropped slot was selected. The weapon's drop virtual ([`WeaponBase::OnDrop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnDrop)), which the deferred drop runs, unequips a weapon still wielded by a user (leaving the player's precision aim), sets the dropped state, adds the bodies to the world and wakes them, gives them a velocity, clears the holder, and enables the pickup interaction again. The velocity depends on the state the weapon was in once unequipped:

- holstered, with a character holder: the holder's velocity plus its forward direction scaled by the slot (forward for the two-handed slot, backward for the special slot, nothing for the others);
- still wielding or dual-wielding (a heavy weapon, which stopping does not holster): the first gun's measured velocity and angular velocity, set on its body ([`WeaponData::m_PfxInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_PfxInstance)) ([`WeaponData::m_MeasuredWorldVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_MeasuredWorldVelocity) and `m_MeasuredAngularVelocity`).

Since the inventory's drop stops using the weapon first, a held weapon other than a heavy one is dropped as if from its holster, with the holder's motion and not the hand's. A swimming character's pickup calls the drop virtual on the slot's occupant directly, which drops it at once with the slot still holding it. The deferral does nothing to a weapon already in the dropped state, so a weapon dropped that way and then removed through the inventory's drop is dropped once. The deferral is unbound.
