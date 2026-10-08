# Physical weapons

The aim is that a weapon behaves as one physical object: it is held in whichever hand took it, it moves between hands, it is picked up off the ground by the hand that touches it, and it goes onto the body and comes off it as the same object. The game keeps owning which weapons the player has (one per inventory slot), the reserve, the saves and loadouts, and the HUD of the selected weapon; the mod owns where each gun is and which hand fires it. The engine side is in [weapon-equip-and-drop.md](../../engine/gameplay/weapon-equip-and-drop.md) and [item-pickup.md](../../engine/gameplay/item-pickup.md), and the attach bones in [hands-locomotion-and-state.md](../../engine/character/hands-locomotion-and-state.md).

None of this has been run in the headset. Everything acts only while the bindings inject, under the controller scheme with a controller active. The code is `payload/src/controllers/weapons/hold/`.

## Either hand

With `weapons.hold.either_hand` (on; the pause menu's `HOLD WEAPONS IN EITHER HAND`), the wielded weapon rides the hand that holds it rather than always the right hand. The holding hand is decided by a small state machine (`hold::holder`), fed once per input tick with the address of the local player's wielded weapon:

- A holster grab's draw leaves the grabbing hand pending, and the next weapon the game wields takes it. A draw plays after the current weapon's holster or switch, so the pending hand waits up to three seconds before it lapses.
- A direct pickup (below) leaves the grabbing hand pending for that one weapon, so another weapon the game wields first does not take it.
- A weapon the game wields on its own (the weapon wheel, a switch, an automatic draw) goes to the acting hand (`controllers.acting_hand()`).
- A handover moves the wielded weapon to the other hand at once.
- A weapon that leaves the hands forgets its hand.

The log line `hold: the weapon is in the left hand` (or right) names the hand each time a weapon comes into the hands, with the cause (`draw`, `pickup`, or `the game's draw`).

The dual-wield pair is not moved this way: it keeps its own layout, one pistol per hand ([controllers.md, "Recoil, haptics, dual-wield, and the grapple hand"](controllers.md#recoil-haptics-dual-wield-and-the-grapple-hand)). A pair taken off the ground with independent pistol draws on brings out only the grabbing hand's pistol, as a hip grab's draw does.

## The owned attach bones

A gun is placed at the world matrix of the bone its attach-bone table ([`WeaponData::m_lBoneAttachementPerWeaponState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_lBoneAttachementPerWeaponState)) names for the weapon's state. The game rewrites the tables at the end of every pose-finalization pass ([`WeaponBase::UpdatePostFinalizePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-UpdatePostFinalizePose)) and its state tasks move guns between bones within the pass, so the mod writes the holding hand's bones at the same three points the dual-wield layout does: before each placement ([`Inventory::UpdateAttachmentTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateAttachmentTransforms)), after the end-of-pass rewrite ([`Inventory::UpdateInventoryPostFinalizePose`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-UpdateInventoryPostFinalizePose)), and at the weapon's own per-tick update, before it fires. Both detours live in `hold/mod.rs` and serve both layouts.

Only a gun on the other hand's attach bone is moved, to its counterpart on the holding hand (the right hand's primary or secondary attach bone to the left hand's primary or secondary, and back). The game's own moves within a hand, and a gun it puts on a holster or back bone, are kept. Both the wielding and the dual-wielding entries are written, and the weapon's state is never changed. A gun whose hand changed since its last placement skips its render interpolation for that placement ([`WeaponBase::SkipRenderInterpolationForOneFrame`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-SkipRenderInterpolationForOneFrame)), so a handover moves the gun in one frame rather than streaking across.

Everything that derives a gun's hand from its attach bone follows without change: the rigid grip pins the gun to the holding wrist, the gun aim casts the holding hand's sight line, the shot's hand drives the recoil, the heft, and the fire haptics, the reload works with the free hand, and the render-time late update corrects the gun by the holding hand. The magazine pouch is laid out on the side away from the holding hand, since it follows the gun's hand (`volumes::gun_side`). The character-level aim point is the dominant hand's while it holds a gun, and the other hand's otherwise.

## Firing

Only the holding hand's trigger fires a gun. With a gun in the off-hand, that hand's trigger presses the off-hand fire action (`fire_off_hand_pistol`), and the tether retract moves to the off-hand's stick click, exactly as while the off-hand holds a pistol of the pair (`Controls::dual_wield` is set for any gun in the off-hand, in a mode whose triggers fire the hand weapons). Both triggers drive the one fire action, so while the gun is in the off-hand and only the dominant trigger is pulled, the weapon handling holds the fire inputs back (`weapons::dominant_trigger_unarmed`). The dual-wield split's per-gun pin is unchanged.

## Handing a gun over

With `weapons.hold.handover` (on; `HAND GUNS OVER AT THE GRIP`), a grab volume (`GrabVolume::Handle`) sits on the held gun's grip, at the holding hand's grip point in its weighted pose (the foregrip and the heft applied), with radius `weapons.hold.handle_radius_m` (0.07 m), for the free hand only. A grip press there hands the gun to the free hand: the holder changes at once, the tables follow on the next placement, and a foregrip hold ends. The grab pulses the grab cue. The volume is placed only while the hold owns the weapon's bones (not for the dual-wield pair, whose hands each hold their own pistol), and the deepest volume wins where it overlaps the weapon's action or the foregrip, which sit further along the gun. The log line is `hold: handed the weapon to the left hand` (or right).

## Picking a weapon up into the hand

With `weapons.hold.direct_pickup` (on; `PICK WEAPONS UP INTO THE HAND`), a grab of a weapon lying in the world ([physical-hands.md](physical-hands.md)) takes that weapon straight into the grabbing hand instead of clicking the use-item action. On the input tick, with the hand's reach from the last simulation tick, it checks that the reached body still belongs to the weapon (a weapon's body leaves the world when it is picked up or despawned, so this proves the object is alive), that the weapon is still dropped, that it is not a hand grenade and does not give a copy ([`WeaponBase::m_InfinitePickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_InfinitePickup)), and that the animation allows a pickup ([`Inventory::GetAnimationAllowsPickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-GetAnimationAllowsPickup)). It then leaves the hand pending for that weapon and calls [`WeaponBase::PickupItem`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupItem) with the character's game-object subobject, the call a swimming character's pickup makes: the weapon goes into the inventory and is equipped there and then, its magazine as it was, with no pickup animation. A weapon already in its slot is dropped or despawned by the game's own inventory add, as with the game's pickup. The log line is `hold: picked the weapon up into the left hand` (or right).

A refused pickup falls back to the use-item click, so the game picks a weapon up its own way, and the readout counts it with the refusal. The one refusal that does not fall back is a pickup that ran but left the weapon out of the hands, since the call already happened.

## Settings and readout

The settings are `controllers.weapons.hold`: `either_hand`, `handover`, `handle_radius_m`, and `direct_pickup`, all on by default. Each call they make is the game's own, read in the IDB: the attach-bone table writes and the interpolation skip are the dual-wield layout's, and `PickupItem` on a dropped weapon is the swimming pickup's path. None writes the inventory directly. The debug UI's Controllers tab shows the holding hand, whether the bones are owned, a pending hand and its cause, the handovers, the table writes and snaps, and the direct pickups, the fallbacks, and the last refusal.

## In the headset

1. Draw a rifle from the shoulder with the left hand: `hold: the weapon is in the left hand` with `cause=draw`. It sits in the left hand, the left trigger fires it from its own muzzle along its own sight line, and the right trigger fires nothing.
2. Grab its grip with the right hand: `hold: handed the weapon to the right hand`. It moves to the right hand in one frame without streaking.
3. Brace it on the foregrip with each hand in turn.
4. Kill a soldier and grab the dropped weapon with the left hand: `hold: picked the weapon up into the left hand`. It appears in the left hand at once with no pickup animation, and the HUD shows its magazine as it was.
5. Watch `grip_watch` for a long gun turning over in the left hand, and the upper body for a left-handed animation state.
6. Reload a left-held rifle from the pouch with the right hand.

## Unverified

- How the animation graph treats a non-pistol gun on the left hand's attach bone: the game's `CWeaponBase::IsWieldedInLeftHand` (the first gun on the left attach bone) is read by an animation condition and the vertical aim samplers, so a left-held rifle may select left-handed or broken upper-body states.
- Whether the left hand's canonical grip (its attach bone's bind pose on the wrist) holds a long gun the right way round: it is right for a pistol, and untested for rifles.
- Whether the swimming pickup's call behaves the same on land: the act the swimming path queues afterwards is not played.
