# Window glass

How the game damages and draws window glass: the metadata a window carries, how bullets crack it, what the window shader does with the damage, and what the game does not have. The mod's use of it is in [breaking-glass.md](../../mod/input/breaking-glass.md).

## Glass is drawn, not simulated

In play, bodies pass through windows: a hand body or a held gun goes straight through car glass. Which collision filter keeps them apart was not traced. What a window does have is shapes that answer the engine's ray queries with the glass material, [`EGameMaterialId_Glass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GameMaterialId##variant-EGameMaterialId_Glass). A bullet's ray meets them; that is the only way the game finds a window to damage.

The glass itself is a model instance whose model is drawn with the window render block, [`RenderBlockWindow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockWindow). A vehicle's windows are parts with their own model instances; the world's panes are static model instances in the model collections.

## The window metadata

A model instance's render info starts with a tagged metadata word, [`ModelInstance::m_MetaFlags`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##field-m_MetaFlags): the address of a [`MetaData`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaData) block, directly or through a shared record when the indirect tag bit is set (`META_INDIRECT`, `META_INDIRECT_OFFSET`). The block's first word is its kind. A window's block is a [`MetaWindow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaWindow) (kind `MetaDataType::Window`), which holds the window's damage as a [`WindowContactPointList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::WindowContactPointList).

The list keeps two fixed arrays of contacts, one per [`WindowDamageKind`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::WindowDamageKind); only the `Point` list is drawn. Each contact is a position in the window instance's own space and a strength ([`WindowContactDamage`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::WindowContactDamage)). [`WindowContactPointList::AddDamage`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::WindowContactPointList##func-AddDamage) adds a contact, or, when one already lies close enough (the merge radius scales with the existing contact's strength), adds to that contact's strength, clamped to one. A merged contact never grows past full strength, so repeated hits on one spot stop adding damage there. A full list grows its nearest contact instead.

Writes come from the main thread while the render thread reads: a new contact is written before the list's count is raised atomically. Metadata blocks are reference counted and destroyed from a deletion list on a later frame, so a block found through a live instance stays valid for the frame.

## How bullets crack glass

A bullet sweeps a ray from its last position to its new one on the query layer ([`PhysicsSystem::QUERY_LAYER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem)), with a filter that passes the materials bullets and rays pass. When the hit's material is glass, its hit handler calls [`Bullet::OnBulletHitGlass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##func-OnBulletHitGlass) before handling the hit object:

1. [`GetModelInstancesHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GetModelInstancesHit) asks the hit object for the model instance of the hit shape, from the cast info's shape key and body ([`CastRayInfoBase::m_ShapeKey`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfoBase##field-m_ShapeKey)). It fills a [`ModelInstanceArray`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceArray) the caller points at a stack buffer.
2. Finding none, it looks up the static world's panes in a centimetre box around the bullet ([`ModelCollection::GetIntersectingDecalModels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelCollection##func-GetIntersectingDecalModels)).
3. For each instance with window metadata, it maps the bullet's position into the instance's space through its matrix (the rigid inverse of [`ModelInstance::GetMatrix`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##func-GetMatrix)) and queues a call that adds a damage point there with the bullet's fixed strength ([`MetaWindow::AddWindowDamagePoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaWindow##func-AddWindowDamagePoint)).

The call is queued because bullets update on worker threads. The ammunition manager's render update runs the queue first ([`AmmunitionManagerHandleDeferredQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::AmmunitionManagerHandleDeferredQueue)), on the main thread, so the point is added there. The glass handler plays no effect or sound; it only adds damage.

The vehicle part code adds points to a part's window through the same function.

## What the shader draws

[`RenderBlockWindow::Draw`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::render_block::RenderBlockWindow##func-Draw) draws nothing while the instance's colour alpha ([`ModelInstance::m_FadeAlpha`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##field-m_FadeAlpha)) is zero. Otherwise it passes the window shader the alpha and the damage weights in its material constants, and the `Point` contacts, moved into world space through the instance's matrix, in the `WindowContactPoints` constant buffer (`NumPoints`, then each contact's position and strength).

The `windowdamage` fragment shader loops over the contacts. For each, it projects the pixel's offset from the contact onto the glass's tangent and bitangent, plus one half, to sample a crack texture (`DamagePointPropertyMap` and `DamagePointNormalMap`), so a contact's cracks span about a metre around it. Each fourth contact mirrors the texture differently, so neighbouring contacts do not repeat. A pixel whose crack mask, weighted by the damage weights, falls under the contact's strength (saturated to one) is cracked: it takes the crack normal, a brightened colour, and an alpha of at least 0.6. The output alpha is the larger of that and the glass's own, times the material alpha.

So damage only ever makes glass more opaque. The shader has no discard and no transparent state. There is no broken-window state in the window metadata, and nothing found in the window path hides a pane. However much damage a window takes, it is drawn cracked over, not open.

## What a pane can be hidden with

A model instance is hidden through [`ModelInstanceManager::HideModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-HideModelInstance): it clears the instance's `VISIBLE` and `FLAG_ENABLED` bits and removes it from its culling context, and the instance keeps its context so that [`ShowModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-ShowModelInstance) brings it back. Hiding takes every render block of the model with it. A model's render blocks are listed per LOD by [`Model::GetRenderBlocks`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::Model##vfunc-GetRenderBlocks) over [`Model::CountLods`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::Model##vfunc-CountLods), and a window block is told apart by its virtual table (`RenderBlockWindow::VFTABLE`).

Hiding does not touch the glass's ray shapes: bullets and rays still meet the glass material where the pane was.

## What a bullet does at glass

[`Bullet::HandleHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##func-HandleHit) moves the bullet to the hit and, for a hit on the glass material, cracks the pane first. It then handles the hit as a hit on the object the glass belongs to, a vehicle for a car's windows: the vehicle takes the bullet's damage and impulse. Whether the bullet goes on is a roll: it passes through when a uniform random number exceeds the hit material's [`GameMaterialDesc::m_BulletCollideFactor`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GameMaterialDesc##field-m_BulletCollideFactor), and otherwise it stops there. A bullet that passes ([`Bullet::m_HasPenetratedOneObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##field-m_HasPenetratedOneObject)) starts its next ray at the hit and ignores the whole object it passed for the rest of its flight ([`Bullet::m_RayIgnoreList`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##field-m_RayIgnoreList)), so it can go on to hit the vehicle's occupants, which are objects of their own. Passing reduces nothing of the bullet's damage. The collide factor is material data, one unless the data sets it; its value for glass is not in the code.

## Open questions

- Whether the vehicles' window models carry only window render blocks. Unverified: it is checked at run time before a pane is hidden.
- Whether anything shows a hidden part's model instance again (a repair, a LOD change, or the vehicle's own visibility logic). Unverified.
- Whether the player's melee impact usage has an entry for glass. The usage is data and was not surveyed.
