# Breaking glass

A palm or a held gun that strikes a window cracks it the way a bullet does, from outside a car or inside it, and a hard enough strike, or enough damage, brings the pane down. The code is `crates/jc3vr-physical/src/controllers/glass/`, tuned in `glass.toml`; the engine side is in [window-glass.md](../../engine/rendering/window-glass.md).

## Why the hands need help

Window glass does not collide with the hand bodies or a held gun, so a hand goes straight through a car window. Bullets crack it because its shapes answer ray queries with the glass material ([`EGameMaterialId_Glass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GameMaterialId##variant-EGameMaterialId_Glass)) and the bullet's handler ([`Bullet::OnBulletHitGlass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##func-OnBulletHitGlass)) adds damage there. So strikes are found the bullet's way, with a ray, and damage the glass through the bullet's own path, so a punched crack looks like a shot one.

## Strikes

Each tick has up to four strikers (`strike::Striker`): each empty palm and each held gun's muzzle (measured for a long gun, a fixed reach ahead of the hand for a pistol). A striker's point is kept in the cockpit frame, and a fast enough move between two close ticks is swept as a ray from where it was to where it is, slightly past, both ends placed through this tick's cockpit transform, so in a moving car the sweep is against the car's own glass, not the road covered. A hand resting on or pushed slowly through a window sweeps nothing. The ray is cast as a bullet's ([`PhysicsSystem::QUERY_LAYER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem), the aim's filter), without passing the player's own vehicle, whose windows a hand inside it strikes, and counts only glass hits.

The panes a hit names are found as the bullet finds them ([`GetModelInstancesHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GetModelInstancesHit), [`ModelCollection::GetIntersectingDecalModels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelCollection##func-GetIntersectingDecalModels)), and each takes a damage point through [`MetaWindow::AddWindowDamagePoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaWindow##func-AddWindowDamagePoint), growing with the swing's speed (`strike::hit_damage`), so light strikes build up as shots do. A striker that hit glass waits a moment, so one swing strikes a pane once.

## Bringing a pane down

The game has no broken-window state: the shader only draws cracks, and nothing hides a pane ([window-glass.md, "What the shader draws"](../../engine/rendering/window-glass.md#what-the-shader-draws)). So the mod hides the pane's model instance ([`ModelInstanceManager::HideModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-HideModelInstance)) after one fast enough strike or once its damage reaches a total. A pane's damage is its damage points, each counted up to full as the shader draws it (so shots count), plus what strikes added to points already full, since the game stops adding to a full point and blows on one spot would otherwise never bring it down (`strike::accumulate`). Hiding takes the whole model with it, so a pane is hidden only when its model draws nothing but window blocks; otherwise it stays cracked and the log says why. Nothing on the mod's side shows a hidden pane again, and the sweep steps past hidden panes along the same ray to reach an intact one farther on.

## Bullets through a pane brought down

A hidden pane keeps its glass shapes, so a bullet would still crack it and stop unless its roll against the glass's collide factor let it through ([window-glass.md](../../engine/rendering/window-glass.md#what-a-bullet-does-at-glass)). With `controllers.glass.bullets_pass` on (the default), the bullet's hit handling is detoured (`glass::bullets`): a glass hit naming a pane the mod brought down, still hidden, is passed as the game's own penetration passes an object, the bullet moving on with that object as its ray filter's ignored object and nothing else of the hit running. World glass, which has no object to ignore, is left to the game. The handler runs on the bullets' worker threads under a lock of its own.

The physical hands' own queries step past hidden panes regardless of that switch.

## Feedback

A strike plays `Event::HandStrike` scaled by its damage, full when the pane comes down, and the player's glass melee impact, effect and sound ([impacts.md](impacts.md#impact-sounds) uses the same lookup, sound only), since the bullet's handler plays nothing of its own to reuse.

## Threading and readouts

The tick runs on the game thread at the start of the render update, beside the palms' tick: the thread and phase of the ammunition manager's deferred list ([`AmmunitionManagerHandleDeferredQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::AmmunitionManagerHandleDeferredQueue)), where bullets add their window damage, because the damage list is written from the main thread while the render thread draws it. The tick adds damage directly, using the panes a hit names within the tick while their metadata is alive. The debug UI's Physical hands feature holds the `bullets_pass` switch and the glass readout (sweeps, glass hits, strikes, panes brought down or refused, bullets passed), and the log records each crack, shatter, and refusal under `glass:`.

## Outstanding checks

- Whether vehicle window instances draw only glass, so they can be hidden.
- Whether anything in the game shows a hidden pane again.
