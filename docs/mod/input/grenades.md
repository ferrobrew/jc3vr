# Grenades

## Safe items

Taking a grenade from the chest pouch creates an independent loadout item and an engine model instance. It does not create an explosive, start a fuse, consume ammunition, or add ammunition. Either hand can hold one. Drawing another grenade while one is stowed creates a different item; drawing a stowed grenade keeps the original item and model identity.

A safe grenade can move between hands and the supported hip and chest holsters. Holstered models follow the resolved holster transform. The loadout table checks the expected item view before every move, so an occupied destination or stale move leaves the item unchanged.

Opening the grip without a tracked throwing motion leaves an inert model at its last tracked world pose. It has no native physics body, fuse, collision handler, or detonation callback. A free hand can pick it up again. Losing input or tracking does not arm it. The current inert-drop representation remains suspended where it was released rather than simulating a fall.

## Locker sources

A free hand can draw a safe grenade from a visible native grenade weapon display. Read-only observations after [`WeaponBase::SetState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-SetState), [`ThrownGrenadeWeapon::UpdateAttachmentTransforms`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##func-UpdateAttachmentTransforms), and the world weapon render pass register the actual weapon's weak reference. The existing interaction-entry walk can also seed the same registry. Registration requires the hand-grenade slot and a dropped or rack state. It never calls a native pickup predicate, starts native pickup, or changes native eligibility. A full native ammunition count therefore does not suppress a source observed during placement or rendering.

The bounded registry retains weak references rather than keeping native weapons alive. Each input tick locks registered weapons briefly, prunes freed or changed world sources, and reads each visible [`WeaponData::m_BaseModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_BaseModelInstance) at its current weapon-data transform. Each free hand receives its nearest qualifying display within reach. Completion checks the same registration, native weapon and control block, weapon-data record, visible model, and current distance. A missing or changed source refuses the grab instead of selecting another source. Leaving gameplay or losing input clears offers; shutdown releases the weak registry. A successful grab calls the same safe-item draw used by the pouch. It leaves the display, native ammunition, the other hand, and holstered items unchanged, so either hand can draw repeatedly from the same display. A nearby gun rack, the player position, and the body pouch do not create a locker target.

Source coverage includes native grenade weapon displays observed in world placement or rendering, independently of valid interaction entries and native ammunition capacity. The shipped grenade entity contains a native grenade weapon and pickup graph, but the recorded locker's displayed grenade group has not been correlated with a live native weapon. Static grenade decorations and locker meshes without a grenade weapon source remain unsupported. Registration and successful acquisition log the native weapon, registration, and displayed model so the runtime check can correlate that group without a generic model scan. This is an active implementation attempt for native grenade displays, not game/headset acceptance of the recorded locker.

## Deliberate throws

Opening the grip during a tracked throwing motion automatically arms and throws the grenade. A separate pin action is not required. Pulling the configured pin input does not create a live grenade while the item is held. Safe grenades cannot be cooked in the hand.

The release captures the item's world transform and its hand-estimated velocity. The controller then calls the existing [`ThrownGrenadeWeapon::PrimeGrenade`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::ThrownGrenadeWeapon##func-PrimeGrenade), which supplies the engine's ammunition-pool allocation, grenade constructor parameters, ownership, and update registration. Only the ammunition deduction in that specific call is suppressed, on the same thread and for that weapon. Inventory values are never topped up or refunded.

Once the native grenade's physics body exists, the controller invokes [`WeaponBase::Fire_Projectile`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-Fire_Projectile) on the grenade weapon. The native launch handles its effects and ownership metadata. The existing launch hook supplies the captured hand transform and velocity, including the character velocity recorded by the engine. The safe model and table item retire only after that native launch succeeds.

If the pool is full, the character cannot throw, or activation cannot complete promptly, the item stays an inert, re-grabbable model. Cancellation removes only the grenade created by this activation attempt, through the native pool-removal operation; it does not use the ammunition-refunding cancellation wrapper.

## Live grenades

Catching an existing live grenade retains the native grenade and its running fuse. It remains dangerous in the hand, and the existing warning ring and haptics continue to reflect its fuse. Throwing it on does not create a replacement. The actor-free guarantees apply to grenades drawn from the pouch, not to captured explosives.

## Runtime checks

In the game, draw a grenade with no native grenade ammunition, move it into a hip or chest holster, draw another into each free hand, and re-draw the stowed item. Verify that the models remain independent and ammunition does not change. Open a hand slowly or stop controller input: its model must remain inert and be available to re-grab. Then release one with a deliberate throw and verify that only that item becomes a live grenade and leaves its hand. Repeat with both hands, with a native grenade caught in the other hand, and after a gameplay transition.

At the locker, repeat the draw with native grenade stock at zero, below capacity, and full. Test both free hands independently and together, with an occupied opposite hand and with stored safe items. Confirm that the visible grenade display is the target, that stock and existing items do not change, and that leaving the source or resetting gameplay cannot reuse a stale offer. Check slow release, re-grab, deliberate throw, and repeated draws. Native grenade displays must remain available at full stock even when the native pickup prompt is absent. Correlate the source-registration and successful-acquisition logs with the visible grenade group. A missing target at a static decoration remains a source-coverage failure, not evidence of successful locker acquisition.

The native creation and launch paths are verified against the game database. The locker adapter's mapped provider and exact-source completion have unit tests, but this change has not run those tests or compiled independently; the coordinator performs the integrated checks. Rendering, source publication, physical feel, pool-pressure behavior, and transition timing still require live gameplay testing.
