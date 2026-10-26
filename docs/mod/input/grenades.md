# Grenades

## Safe items

Taking a grenade from the chest pouch creates an independent loadout item and an engine model instance. It does not create an explosive, start a fuse, consume ammunition, or add ammunition. Either hand can hold one. Drawing another grenade while one is stowed creates a different item; drawing a stowed grenade keeps the original item and model identity.

A safe grenade can move between hands and the supported hip and chest holsters. Holstered models follow the resolved holster transform. The loadout table checks the expected item view before every move, so an occupied destination or stale move leaves the item unchanged.

Opening the grip without a tracked throwing motion leaves an inert model at its last tracked world pose. It has no native physics body, fuse, collision handler, or detonation callback. A free hand can pick it up again. Losing input or tracking does not arm it. The current inert-drop representation remains suspended where it was released rather than simulating a fall.

## Deliberate throws

Opening the grip during a tracked throwing motion automatically arms and throws the grenade. A separate pin action is not required. Pulling the configured pin input does not create a live grenade while the item is held. Safe grenades cannot be cooked in the hand.

The release captures the item's world transform and its hand-estimated velocity. The controller then calls the existing [`ThrownGrenadeWeapon::PrimeGrenade`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##func-PrimeGrenade), which supplies the engine's ammunition-pool allocation, grenade constructor parameters, ownership, and update registration. Only the ammunition deduction in that specific call is suppressed, on the same thread and for that weapon. Inventory values are never topped up or refunded.

Once the native grenade's physics body exists, the controller invokes [`WeaponBase::Fire_Projectile`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-Fire_Projectile) on the grenade weapon. The native launch handles its effects and ownership metadata. The existing launch hook supplies the captured hand transform and velocity, including the character velocity recorded by the engine. The safe model and table item retire only after that native launch succeeds.

If the pool is full, the character cannot throw, or activation cannot complete promptly, the item stays an inert, re-grabbable model. Cancellation removes only the grenade created by this activation attempt, through the native pool-removal operation; it does not use the ammunition-refunding cancellation wrapper.

## Live grenades

Catching an existing live grenade retains the native grenade and its running fuse. It remains dangerous in the hand, and the existing warning ring and haptics continue to reflect its fuse. Throwing it on does not create a replacement. The actor-free guarantees apply to grenades drawn from the pouch, not to captured explosives.

## Runtime checks

In the game, draw a grenade with no native grenade ammunition, move it into a hip or chest holster, draw another into each free hand, and re-draw the stowed item. Verify that the models remain independent and ammunition does not change. Open a hand slowly or stop controller input: its model must remain inert and be available to re-grab. Then release one with a deliberate throw and verify that only that item becomes a live grenade and leaves its hand. Repeat with both hands, with a native grenade caught in the other hand, and after a gameplay transition.

The native creation and launch paths are verified against the game database. Rendering, physical feel, pool-pressure behavior, and transition timing still require live gameplay testing.
