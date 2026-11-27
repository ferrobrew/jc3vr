# Breaking glass

A palm or a held gun that strikes a window cracks it the way a bullet does, from outside the car or from inside it, and a hard enough strike, or enough damage, brings the whole pane down. The code is `crates/jc3vr-physical/src/controllers/glass/`, its tuning is `crates/jc3vr-physical/tuning/glass.toml` ([tuning.md](../tuning.md)), and the engine side is in [window-glass.md](../../engine/rendering/window-glass.md). It is always on under the controller scheme.

## Why the hands need help

Window glass does not collide with the hand bodies or a held gun: a hand goes straight through a car window. Bullets still crack glass, because the glass's shapes answer ray queries with the glass material ([`EGameMaterialId_Glass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GameMaterialId##variant-EGameMaterialId_Glass)) and the bullet's glass handler ([`Bullet::OnBulletHitGlass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##func-OnBulletHitGlass)) adds damage where its ray met it. So the strikes are found the bullet's way, with a ray, and damage the glass through the bullet's own call, so a punched crack looks like a shot one.

## Strikers and sweeps

Each tick has up to four strikers (`strike::Striker`): each empty hand's palm (the palm box's centre) and the muzzle end of each held gun. A long gun's muzzle is the one its geometry measures (the same one the wall collision uses); a gun without measured geometry, such as a pistol, has its muzzle `strike.pistol_reach_m` ahead of the hand's aim pose.

Each striker's point is kept in the cockpit frame from tick to tick. Between two ticks no further apart than `strike.max_gap_s`, a move at `strike.min_speed_mps` or faster is swept: a ray from where the striker was to where it is, both placed in the world through this tick's cockpit transform, reaching `strike.palm_lead_m` (or `strike.tip_lead_m` for a muzzle) past it, so a striker stopping at the glass's face still meets it. Placing both ends through the current transform makes the sweep relative to the cockpit, so in a moving car it is against the car's own glass, not the road covered between ticks. A hand resting on a window, or pushed slowly through it, sweeps nothing.

The sweep is cast as the bullet's ray is: on the query layer ([`PhysicsSystem::QUERY_LAYER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem)), passing the materials bullets pass, through the player aim's filter, and passing what the striking hand carries. The player's own vehicle is not passed, since its windows are what a hand inside it strikes. A hit counts only when its material is glass.

## Damage

The panes the hit names are looked up as the bullet looks them up ([`GetModelInstancesHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GetModelInstancesHit), then the world's panes around the hit, [`ModelCollection::GetIntersectingDecalModels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelCollection##func-GetIntersectingDecalModels)). Each shown instance with window metadata takes a damage point at the hit, mapped into its own space, through [`MetaWindow::AddWindowDamagePoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaWindow##func-AddWindowDamagePoint). The damage grows with the swing's speed from `strike.min_damage` to `strike.max_damage` at `strike.full_speed_mps` (`strike::hit_damage`), so light strikes leave small cracks and build up as shots do. A striker that has struck glass waits `strike.cooldown_s` before it strikes again, so a swing that spends several ticks in a pane strikes it once.

## Bringing a pane down

The game has no broken-window state: the window shader only draws cracks over glass, and nothing in the window path hides a pane ([window-glass.md, "What the shader draws"](../../engine/rendering/window-glass.md#what-the-shader-draws)). So the mod brings a pane down by hiding its model instance ([`ModelInstanceManager::HideModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-HideModelInstance)).

A pane comes down when one strike is at `strike.shatter_speed_mps` or faster, or when a strike leaves its damage at `strike.shatter_total`. A pane's damage is the total of its damage points, each counted up to one as the shader draws it, so shots count toward it; plus what the strikes added past a point already at full strength, since the game's list stops adding to a full point and blows on one spot would otherwise never bring the pane down (`strike::accumulate`). That excess is remembered per pane for `panes.memory_s` after its last strike, for at most `panes.max_records` panes.

Hiding takes every render block of the instance's model with it, so a pane is hidden only when its model draws nothing but window blocks; otherwise it stays, cracked, and the log says why. Nothing on the mod's side shows a hidden pane again. A hidden pane is not struck again: the sweep continues past positively identified hidden panes along the same ray, within a fixed cast budget, and can reach an intact pane farther along, while a solid obstruction ends it.

## Bullets through a pane brought down

Hiding a pane leaves its glass shapes, so a bullet would still meet glass where the pane was, crack the hidden pane, hit the vehicle, and stop unless its roll against the glass's collide factor let it through ([window-glass.md](../../engine/rendering/window-glass.md#what-a-bullet-does-at-glass)). With `controllers.glass.bullets_pass` on (the default), the bullet's hit handling is detoured (`glass::bullets`): a hit on glass that names a pane the glass brought down, and which is still hidden, is passed as the game's own penetration passes an object. The bullet moves to the hit, its next ray starts there, and the hit object becomes the one object its ray filter ignores; nothing else of the hit runs, so there is no crack, damage, impulse, or hit effect, and for that one segment no check against creatures or the water surface. A hit on world glass, which has no object to ignore, is left to the game. The handler runs on the bullets' worker threads and reads the most recently brought-down panes under a lock of its own.

The physical hands' own queries recognize hidden panes too, independently of that switch: a bounded continuation steps past the hidden pane along the same ray and keeps hits on other parts of the same vehicle. Door acquisition rejects ordinary car glass, and the palm probes pass glass; an intact aircraft canopy can supply a door grip or pressure contact, but its hidden panes cannot ([vehicle-doors.md](vehicle-doors.md#seated-doors-and-palm-pressure)). A hand that came in through a brought-down pane reaches the occupant behind it ([character-grabs.md](character-grabs.md#through-a-broken-window)).

## Feedback

A strike plays `Event::HandStrike` on the striking hand, scaled by its damage, and at full strength when the pane comes down. The player's melee impact for glass plays at the strike, effect and sound ([impacts.md](impacts.md#impact-sounds) uses the same lookup, sound only). The bullet's glass handler plays nothing of its own, so there is no glass-breaking effect of the game's to reuse; when the melee impact has no glass sound, the log says so once.

## Threading

The tick runs on the game thread at the start of the game's render update, beside the palms' tick and before the game's own update. That is the thread and phase of the ammunition manager's deferred list ([`AmmunitionManagerHandleDeferredQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::AmmunitionManagerHandleDeferredQueue)), where the bullets' window damage is added: the bullets defer it because they update on worker threads, and the window's damage list is written from the main thread while the render thread draws it. The tick adds its damage directly, and uses the panes a hit names within the tick, while their metadata is alive.

## Settings, readout, and log

The one setting, `controllers.glass.bullets_pass`, sits in the debug UI's Physical hands feature with a readout: sweeps cast, sweeps that met glass, glass hits that named no pane, strikes, panes brought down, panes refused because their model draws more than glass, the bullets passed and the glass's collide factor, the last strike, and the last cast error.

The log has, at info level, `glass: a strike cracked a pane`, `glass: a strike brought a pane down` with the cause, and `glass: a pane would shatter but stays` with the reason, each with the pane's instance address, its model's name hash, the speed, and the damage; and once each, `glass: the player's melee impact has no sound for glass`, `glass: the glass material's bullet collide factor`, and `glass: a bullet passed through a pane brought down`.

## Outstanding checks

- Whether the sweep meets a car's glass from inside the cabin, or stops first at the interior's ray shapes; the readout's sweep and glass counts tell inside strikes from outside ones.
- Whether vehicle window instances draw only glass, so they can be hidden.
- Whether anything in the game shows a hidden pane again (a repair, a respawn of the part).
