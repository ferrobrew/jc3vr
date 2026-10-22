# The pouches' models

The grenade, C4, and magazine pouches on Rico's body have no model of their own in the game, so the mod draws the game's own models on them: a grenade on the grenade pouch, the C4 charge as it shows in the hand on the C4 pouch, and the held gun's magazine on the magazine pouch. What the player sees on the body is what a hand reaching there takes ([grip.md](grip.md), [controllers.md, "Holsters, pouches, the foregrip, and the action-runs tier"](controllers.md#holsters-pouches-the-foregrip-and-the-action-runs-tier)). The code is `payload/src/controllers/weapons/pouch_models.rs`, and the setting is `weapons.pouches.models` (on), in the debug UI's weapon handling settings under the pouches.

None of this has been run in the headset.

## The models

Each model is an instance the mod owns, made from a model the game already holds for the item's lifetime, through [`ModelInstanceManager::CreateInstanceFromModel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-CreateInstanceFromModel):

- the grenade: the hand grenade slot's weapon's projectile model ([`ThrownGrenadeWeapon::m_PhysicalProjectileModel`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##field-m_PhysicalProjectileModel));
- the C4 charge: the planted explosive slot's weapon's in-hand model ([`PlantedExplosiveWeapon::m_InactiveModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::PlantedExplosiveWeapon##field-m_InactiveModelInstance), the model despite its name);
- the magazine: the model of the reloaded gun's spare magazine prop ([`WeaponData::m_MagazineObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_MagazineObject), [`RigidObject::m_Model`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::RigidObject##field-m_Model)), which the game respawns a while after each one it drops; meanwhile there is no magazine to draw.

The instance holds its own reference to the model and is added to the world, where the engine's dynamic cull draws it like any other model, casting a shadow, with no physics and no game object behind it. Its clip on distance is cleared, as the game does for the explosives a character holds.

## Where they are

A model is drawn while its pouch has something to give, wherever the pouch is placed and whichever hands are free: the grenade while there is a grenade in stock and the grenade throw is on, the C4 charge while another charge may be placed and the C4 throw is on, and the magazine while the true reload acts on the held weapon and there is a round in reserve. It sits at its pouch's centre, turned with the girdle the pouch rides (the chest for the grenade and the C4, the pelvis for the magazine), moved and turned by its placement (`weapons.pouches.models.grenade`, `.charge`, and `.magazine`: `offset_m` in the girdle's axes and `rotation_deg` about X, then Y, then Z), given for the right side and mirrored across the body's midline for the left. The placements are zero by default and tuned live from the debug UI, which also shows which models are drawn, how many instances were made, and how many frames drew one.

The chest's explosives holster draws nothing: the charge it selects is already drawn on the C4 pouch.

## Riding the body

On the input tick, each model's placement is carried into the character's model space and kept with the last tick's. Once per rendered frame, before the engine's render update culls and draws the frame (from `Game::UpdateRender`, after the late update's snapshot), each shown model's matrix is pushed ([`ModelInstance::UpdateMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##func-UpdateMatrix)) as its placement between the last two ticks', carried by the character's world transform between its last two ticks' ([`Character::m_WorldMatrixT0`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT0) and [`m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##field-m_WorldMatrixT1)), at the frame's interpolation fraction, so it rides the body as the body is drawn ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). A model shown afresh starts at its new place rather than sweeping from its last.

## Lifetime

The instances are made, shown, hidden, and released on the game thread outside the world update, as the engine requires. A model whose pouch has nothing to give is hidden ([`ModelInstanceManager::HideModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-HideModelInstance)) and kept for when it comes back. An instance is released ([`ModelInstanceManager::AddToDeleteQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-AddToDeleteQueue)) when its source model changes (another gun's magazine), when the weapon handling stands down, outside gameplay (where the world the instance lives in may be torn down), with the setting off, and when the payload unloads.

## In the headset

1. Stand with grenades and C4 in stock and look down at the chest: a grenade sits on the gun hand's side and a C4 charge on the other, on the pouches' markers. Turn, crouch, bend, and walk: they stay on the body.
2. Draw a rifle with reserve ammunition and look at the belt: its magazine sits on the free hand's side. Draw a pistol: the magazine changes to the pistol's.
3. Throw every grenade: the grenade on the chest goes, and comes back with a grenade picked up.
4. Open the parachute, fall, and drive: the models stay on the body without lagging it.
5. Tune each model's turn from the debug UI until it sits naturally on its pouch, and note the values.
6. Load a save and quit to the menu and back: the models come back, and the log has no errors.

## Unverified

- Whether the instances are safe across a save load beyond being released outside gameplay, and whether texture streaming covers them.
- Whether the models' own origins put them on the pouches with a zero placement, or need a default offset and turn per model.
- Whether pushing the matrix before the engine's render update is late enough in every frame for the cull to see it.
