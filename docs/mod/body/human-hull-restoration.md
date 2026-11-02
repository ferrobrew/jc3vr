# Human hull discovery and restoration

The human hull controller retains the original native shapes and a narrower replacement for each human proxy state. It changes the shape table and live body shapes without changing fake-contact settings, shape user data, body material, collision-filter data, or character state. Quadruped state entries remain unchanged.

## Discovery

Each native pre-simulation pass scans the live bodies in [`hknpWorld::m_bodies`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_bodies). The body property map identifies the physics instance and its owner through [`PfxInstance::m_GameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxInstance##field-m_GameObject). The controller deduplicates owners, checks the native character type, follows the character's physics proxy, and adopts each distinct shared settings table. Owner pointers do not survive the native phase.

A slot counts as a live body only when it has a static or dynamic flag and a valid motion, the engine's own test. Destroying a body clears its type flags and freeing its slot invalidates its motion, but neither clears the slot's shape ([`hknpBody::INVALID_MOTION_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##const-INVALID_MOTION_ID)), so a dead or despawned human's slot keeps naming its narrowed replacement until the slot is reused. Such a slot is skipped: an earlier inventory treated it as a pending body and so refused every restoration, and with it the eject, for as long as any narrowed human's slot stayed unused.

This discovery covers NPCs that exist before injection and NPCs whose bodies stream in later. It does not depend on the local player using the same settings table. Construction hooks cover future settings tables. Destruction hooks remove dead table pointers while retained shape references remain available for any live body that still uses them.

## Clearance and restoration

Disable and shutdown requests retain the narrower shapes until one native pre-simulation pass can restore every affected live body. A disable waits for that pass however long it takes. A shutdown waits a bounded number of attempts (see [Eject](#eject)). The preflight checks each retained original shape at the body's actual transform. Other affected bodies use their final original shapes during the same preflight. The controller does not resize the live body temporarily to obtain a query result.

Actual transformed native AABBs select candidate bodies. The distance test uses the original convex vertices and radius. Convex targets are queried directly. Compressed mesh and compressed heightfield targets enumerate native leaf keys, extract native convex or triangle leaves, and query each potentially overlapping leaf. Enumeration is bounded. An incomplete page, duplicate key, unsupported shape, unsupported scale, invalid transform, invalid output, or exhausted budget defers restoration. An empty unsupported dispatcher result does not establish clearance.

The ordinary native GSK query returns a status separately from its output. Only ordinary separated and ordinary penetrating statuses are accepted. The controller initializes distance outputs to nonfinite values and rejects outputs that remain unwritten. It subtracts both convex radii from the core distance. Heuristic, allocation, unresolved, and unknown statuses defer restoration.

A floor contact can remain during restoration when the current and original shapes produce the same shallow contact depth and a matching near-vertical support normal. This permits restoration on an unchanged flat floor. A wall or ceiling contact cannot use that exception. New support contact, increased penetration, a changed normal, and deep penetration block restoration. A nearby wall still requires clearance from the full original shape.

All clearance checks finish before the first live-body write. The controller applies native shape changes and reads each body back. A rejected write rolls back every attempted change in reverse order and checks the rollback. A rejected rollback permanently refuses restoration for that controller instance. Shape tables return to the originals only after the live-body transaction succeeds. The retained originals are released only after the shutdown participant validates the current restoration generation; the replacements are kept, since a body made from a slot read before the restoration may still name one.

## Eject

The shutdown participant waits a bounded number of shutdown attempts (`RESTORE_ATTEMPTS` in `human_hull/mod.rs`, a second or two of retries) for the native phase to finish a restoration. A restoration in that window commits as above. Otherwise the eject leaves every table and live body with the shapes it has, narrowed or restored, and commits without touching the engine: it keeps every retained reference, original and replacement, and stops adopting, so construction hooks still pending keep the authored geometry. This is safe for the engine because the replacements are the engine's own convex shapes, built by its factory on its heap with the originals' user data and no pointer into the payload, and no shape a slot or a body names can be freed while the payload holds its references; a settings table's destructor releases its slots' own references as for any shape. The cost is a small leak and narrowed humans until their settings tables are freed. Only a settings hook still running payload code holds the eject, for the length of that call.

Each deferred attempt logs the clearance reason when it changes, and the debug UI's physical-hands page shows the adapter state and clearance reason. Each native pass also checks whether the local player's character body has a narrowed shape and logs `human hull: the local player's body shape changed` when that changes, so a session log shows whether the player's own hull took effect.

## Limits

Composite target support is restricted to the verified compressed mesh and compressed heightfield kinds. Other composites defer restoration when their bounds overlap the proposed shape. The controller enumerates all leaves of an overlapping terrain chunk before completion, even when most leaves are distant. Large chunks can exhaust the work budget and defer. A targeted terrain-key query remains a performance improvement; its empty result must not conceal unsupported dispatch or incomplete traversal.

## Runtime acceptance

The following game checks remain unperformed. Unit tests cover native-call adaptation, status rejection, unwritten outputs, contact policy, late distinct settings, and rejected-write rollback. They do not prove the live engine's allocation, callback ordering, or reinjection behavior.

- Inject with nearby NPCs already present. Confirm that distinct settings tables and live human bodies migrate before spawning another character.
- Stream in additional NPCs after activation. Confirm that their live bodies migrate on the next native pre-simulation pass.
- Disable on clear flat ground. Confirm restoration without a jump, teleport, or restart.
- Disable beside a wall that intersects the original width. Confirm that all affected bodies retain the replacement and gameplay remains active. Move to clear ground and confirm that the next complete preflight restores all bodies.
- Eject beside such a wall. Confirm that the eject completes within a few seconds, logs the clearance reason, and leaves the game stable with the narrowed hulls in place.
- Repeat with a second human whose original width would intersect the first human. Confirm that neither body restores independently.
- Exercise standing, crouching, swimming, rotated, and wingsuit states. Confirm unchanged shape metadata and fake-contact settings.
- Eject after restoration and reinject. Confirm that the authored table remains intact and that every newly installed replacement has retained original ownership.
