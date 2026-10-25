# Direct weapon acquisition

Direct weapon offers in `controllers/weapons/hold/world.rs` cover guns in characters' hands and weapons offered by racks or lockers. They do not classify dropped physics bodies or remote grapple targets. Each offer uses a grip-to-grip segment, and each eligible free hand receives the nearest point on that segment. Acquisition and completion use the same distance boundary.

`weapons.hold.take_radius_m` defaults to 0.30 player-scale metres. `weapons.hold.handle_radius_m` defaults to 0.14 player-scale metres for transfer around the pistol grip. Existing saved values remain unchanged. The shared candidate resolver retains direct-target priority and the separate constraints of support, reload, action, and socket targets. A broad transfer offer does not replace an engaged foregrip or a touch on another gun region.

## Placement provenance

The native hand-placement hook records the primary gun's grip relative to the solved holding wrist after placement. The record contains the placement generation, weapon, primary gun, holder, weighted wrist, and pinning state. The current weighted wrist carries the measured relationship forward rather than treating the wrist origin as the pistol grip.

A new placement invalidates the previous record. Loadout hand changes and hold teardown invalidate it before publishing ownership. Missing, stale, incompatible, or invalid records use the holding wrist fallback. The record does not authorize engine access or retain a weapon lifetime.

## Validation limits

Pure tests cover nearest-point geometry, equal acquisition/completion boundaries, scale, occupied hands, transfer exclusions, placement generations, and stale records. This contract does not establish a fix for remote-snatch refusal or dropped-floor-body detection. Live clustered acquisition, physical handover, and selected-target feedback still require a headset check.
