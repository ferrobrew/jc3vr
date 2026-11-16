# Breaking glass

A palm or a held gun that strikes a window cracks it the way a bullet does, from outside the car or from inside it, and a hard enough strike, or enough damage, brings the whole pane down. The code is `payload/src/controllers/glass/`, and the engine side is in [window-glass.md](../../engine/rendering/window-glass.md).

## Why the hands needed help

Window glass does not collide with the hand bodies or with a held gun: a hand goes straight through a car window, and a gun can be pushed through one and fired from the other side. Bullets still crack glass, because the glass's shapes answer ray queries with the glass material ([`EGameMaterialId_Glass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GameMaterialId##variant-EGameMaterialId_Glass)) and the bullet's glass handler ([`Bullet::OnBulletHitGlass`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::Bullet##func-OnBulletHitGlass)) adds damage where its ray met it. So the strikes are found the bullet's way, with a ray, and damage the glass through the bullet's own call, so that a punched crack looks like a shot one.

## Strikers and sweeps

Each tick has up to four strikers (`strike::Striker`): each hand's palm while the hand holds nothing (the palm box's centre, as the physical hands place it), and the muzzle end of a gun the hand holds. A long gun's muzzle is the one the gun's geometry measures (the same one the wall collision uses); a gun without measured geometry, such as a pistol, has its muzzle `tuning.pistol_reach_m` ahead of the hand's aim pose. Guns strike only while `glass.weapons` is on.

Each striker's point is kept in the cockpit frame from tick to tick. Between two ticks no further apart than `tuning.max_gap_s`, its speed is the move's length in world metres over the time, and a move at `tuning.min_speed_mps` or faster is swept: a ray from where the striker was to where it is, both placed in the world through this tick's cockpit transform, reaching `tuning.palm_lead_m` (or `tuning.tip_lead_m` for a muzzle) past it so that a striker stopping at the glass's face still meets it. Placing both ends through the current transform makes the sweep relative to the cockpit: in a moving car the sweep is against the car's own glass, not the stretch of road the car covered between the ticks. A hand resting on a window, or pushed slowly through it, sweeps nothing.

The sweep is cast as the bullet's ray is cast: on the query layer ([`PhysicsSystem::QUERY_LAYER`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem)), passing the materials bullets and rays pass, through the player aim's filter that never hits the player, and passing what the striking hand carries. The player's own vehicle is not passed, since its windows are what a hand inside it strikes. A hit counts only when its material is glass.

## Damage

The panes the hit names are looked up as the bullet looks them up ([`GetModelInstancesHit`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::GetModelInstancesHit), then the world's panes around the hit, [`ModelCollection::GetIntersectingDecalModels`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelCollection##func-GetIntersectingDecalModels)). Each shown instance with window metadata takes a damage point at the hit, mapped into its own space, through [`MetaWindow::AddWindowDamagePoint`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::window_damage::MetaWindow##func-AddWindowDamagePoint). The damage grows with the swing's speed from `tuning.min_damage` at the slowest strike to `tuning.max_damage` at `tuning.full_speed_mps` and beyond (`strike::hit_damage`). Light strikes leave small cracks and build up as shots do.

Once a striker has struck glass it waits `tuning.cooldown_s` before it can strike again, so a swing that spends several ticks in a pane strikes it once.

## Bringing a pane down

The game has no broken-window state: the window shader only draws cracks over glass, more opaque the more it is damaged, and nothing in the window path discards or hides a pane ([window-glass.md, "What the shader draws"](../../engine/rendering/window-glass.md#what-the-shader-draws)). So a pane is brought down by hiding its model instance ([`ModelInstanceManager::HideModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstanceManager##func-HideModelInstance)): the glass is gone, as when it has been smashed out. This is the mod's own equivalent, not a game state.

A pane comes down when one strike is at `tuning.shatter_speed_mps` or faster, or when a strike leaves its damage at `tuning.shatter_total`. A pane's damage is the total of its damage points, each counted up to one as the shader draws it, so shots count toward it too; plus what the strikes added past a point already at full strength, since the game's list stops adding to a full point and blows on one spot would otherwise never bring the pane down (`strike::accumulate`). The mod remembers that excess per pane for a while after its last strike.

Hiding takes every render block of the instance's model with it, so a pane is hidden only when its model draws nothing but window blocks; otherwise it stays, cracked, and the log says why. A hidden pane is not struck again: the sweep still meets the glass's ray shapes, but the lookup leaves out instances that are not shown. `glass.shatter` off keeps every pane up.

## Bullets through a pane brought down

Physical-hand queries also recognize hidden panes, independently of the bullet-pass switch. A bounded continuation advances beyond the hidden pane along the same ray and preserves hits on other parts of the same vehicle. Door acquisition rejects ordinary car glass, and generic mechanical palm probes pass glass. An intact aircraft canopy can supply an explicitly recognized door grip or pressure contact, but its hidden panes cannot. See [vehicle-doors.md](vehicle-doors.md#seated-doors-and-palm-pressure).

Hiding a pane leaves its glass shapes, so a bullet would still meet glass where the pane was: it would crack the hidden pane, hit the vehicle, and stop unless its roll against the glass's collide factor let it through ([window-glass.md](../../engine/rendering/window-glass.md#what-a-bullet-does-at-glass)). With `glass.bullets_pass` on (the default, with `glass.shatter`), the bullet's hit handling is detoured (`glass::bullets`): a hit on glass that names a pane the glass brought down, and which is still hidden, is passed as the game's own penetration passes an object. The bullet moves to the hit, its next ray starts there, and the hit object becomes the one object its ray filter ignores; nothing else of the hit runs, so there is no crack, no damage or impulse to the vehicle, no hit effect, and for that one segment no check against creatures or the water surface. A hit on world glass, which has no object to ignore, is left to the game. The handler runs on the bullets' worker threads, looks the hit's panes up as the bullet's glass handling does there, and reads the brought-down panes (the last 256) under a lock of its own. The first glass hit logs the glass material's collide factor (`glass: the glass material's bullet collide factor`), so the log says how often intact glass stops a bullet; the readout shows it and counts the passes (`glass: a bullet passed through a pane brought down`, logged once).

## Feedback

A strike plays a haptic strike on the striking hand, scaled by its damage, and at full strength when the pane comes down. With `glass.sound` on, the player's melee impact for glass plays at the strike, with its effect and its sound ([impact-sounds.md](impact-sounds.md) uses the same lookup for a palm's touch, sound only). The bullet's glass handler plays nothing of its own, so there is no glass-breaking effect of the game's to reuse; whether the melee impact has a glass entry is not known, and the log says so once when it has no sound.

## Threading

The tick runs on the game thread at the start of the game's render update, beside the palms' tick and before the game's own update runs. That is the thread and phase of the ammunition manager's deferred list ([`AmmunitionManagerHandleDeferredQueue`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::AmmunitionManagerHandleDeferredQueue)), which is where the bullets' window damage is added: the bullets defer it because they update on worker threads, and the window's damage list is written from the main thread while the render thread draws it. The tick is already on the main thread, so it adds its damage directly. The panes a hit names are used within the tick, while their metadata is certainly alive.

## Settings, readout, and log

The switches are `controllers.glass.enabled`, `weapons`, `shatter`, and `sound`, all on, and the tuning is `controllers.glass.tuning`. They are in the debug UI's Physical hands feature, below the palms' settings, with a readout: sweeps cast, sweeps that met glass, glass hits that named no pane, strikes, panes brought down, panes refused because their model draws more than glass, the last strike (striker, speed, damage, the pane's damage, and what it did), and the last cast error.

The log has, at info level, `glass: a strike cracked a pane` for each strike, `glass: a strike brought a pane down` for each pane hidden, and `glass: a pane would shatter but stays` with the reason, each with the pane's instance address, its model's name hash, the speed, the damage, and the pane's damage; and once, `glass: the player's melee impact has no sound for glass`.

## Unverified

None of this has been tried in the headset. In particular:

- Whether the sweep meets a car's glass from inside the cabin, or stops first at the interior's own ray shapes or on the player aim filter's rejection of some body kind. The readout's counts tell it apart: sweeps that never meet glass from inside, against strikes from outside.
- Whether a vehicle's window instance draws only glass, so that it can be hidden; a refusal is counted and logged.
- Whether anything shows a hidden pane again (a repair, a respawn of the part, the vehicle's own visibility logic). Nothing restores a hidden pane on the mod's side.
- Whether the speeds suit real punches and gun jabs; the tuning is in the debug UI.

## In the headset

1. On foot by a parked car, punch a side window lightly: a crack at the fist, a haptic strike, `glass: a strike cracked a pane`, and a glass impact sound (or the one-off `no sound for glass` line). Punch the same spot again: the crack grows; after a few punches the pane comes down (`glass: a strike brought a pane down` with `cause=accumulated damage`).
2. Punch another window as hard as you can: it comes down at once (`cause=a hard hit`), or note the speed in the log for the tuning.
3. Rest a hand on a window and push it slowly through: nothing.
4. Jab a window with a rifle's muzzle, and then with a pistol: each cracks it.
5. Sit in a car and punch the windscreen and a side window from inside, parked and then driving: each cracks and can be brought down. If nothing happens, note the readout's sweep and glass counts.
6. Shoot a window a few times, then punch it: it comes down sooner than a clean one.
7. If a pane stays up after `would shatter but stays`, note the vehicle and the reason.
8. Turn `glass.shatter` off: panes only crack. Turn `glass.enabled` off: hands pass through glass as before.
