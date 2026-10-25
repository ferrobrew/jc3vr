# Body attachment observations

The body mirror consumes copied input-tick observations for weapon sockets and pouch props. It does not read a weapon, a source model, or a model instance on the pose-finalization thread. The existing finalized skeleton and wrist-target paths remain independent.

## Weapon sockets

`controllers::weapons::docks::socket_points` observes the socket weapons under its existing input-thread safety contract. It builds one `attachments::Sample`, returns its `points()` to the existing docking-volume path, and publishes the same sample for the mirror. The sample records the character identity, timestamp, source character transform, socket occupancy, loadout item identity, source object identity, hand-specific usage, and pose or missing reason.

The pose sources are a visible weapon grip, a remembered bone-relative grip, a native socket bone, a configured offset from a socket bone, and an unavailable socket weapon. Object identities are diagnostic values. They neither retain an object nor authorize a grab. The existing loadout and grab rules still determine draw, dock, and no-fit behavior. The occupied-but-unobserved socket fallback remains a no-fit observation with occupied state.

The mirror rebases copied poses through the character's current world transform. This follows character or platform movement without another bone read. It does not refresh the input observation's timestamp or reconstruct the current skeleton pose. Foreign characters, observations older than the bounded age, future timestamps, and invalid source or destination transforms produce no sample. A visible grip remains usable when its bone orientation is unavailable; the orientation line then uses a neutral orientation.

The mirror shows actual hand-specific offers where present. Otherwise, it shows the socket observation. Occupied sockets use amber rings, empty offers use cyan rings, no-fit offers use red rings, and neutral empty-socket estimates use gray rings. A missing pose produces no marker. Missing counts remain available through `controllers::body::mirror::attachment_status`. The mirror shows these socket observations only when holsters and `follow_docks` are enabled. The optional landmark-based interaction mode uses a different target-placement policy and is not represented as grip parity.

## Pouch props

The grenade and C4 pouch centers already use the same volume helpers as their interaction targets. The magazine prop uses the reload provider's stocked pouch. `controllers::weapons::pouch_models::attachments::placement` converts that supplied pouch pose into character model space and applies the configured prop offset and rotation. The model-instance input loop uses the resulting local transform directly and publishes the same copied value for the mirror.

Each prop observation records its kind, side, optional magazine weapon identity, borrowed source-model identity, pouch pose, model transform, and actual instance visibility. An unoffered pouch is distinct from an offered pouch whose source model is unavailable. Instance stand-down clears the observation. Owner and timestamp checks bound its use by the mirror.

The mirror draws a ring at the interaction center. A visible prop also has a ring at its configured model origin and a line from the pouch center to that origin. An offered pouch with a hidden or missing model has a red center ring. The mirror carries the copied model-space placements through the current character transform. It does not reproduce render interpolation or claim that the input-tick pose is a fresh joint observation.

## Boundaries and verification

The shared providers change attachment placement values and diagnostics. They do not change claims, ammunition, model ownership, native inventory writes, reload actions, or current-joint validity. Socket interaction candidates still pass through the existing cockpit-frame conversion and resolver. Pouch models still use the existing instance creation, visibility, interpolation, and release paths.

The socket tests compare the production point adapter with the production mirror renderer across remembered body poses, visible grips, missing bones and grips, native and configured fallbacks, character rebasing, cockpit conversion, owner changes, age expiry, and invalid transforms. The pouch tests compare the production placement and render-matrix helpers with the production mirror renderer for each prop, both sides, body rotations, platform movement, missing models, absent offers, invalid transforms, and age expiry.

The copied attachment status is separate from `MirrorStatus`, so existing status consumers remain source-compatible. The shared status UI does not yet display the attachment counters. Input-tick attachment observations can differ from the later finalized skeleton and interpolated model placement. A render-phase attachment provider would require a separate verified publication contract.
