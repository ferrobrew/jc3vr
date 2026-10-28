# Vehicle grips and stunt positions

A physical handhold stays at the point the hand grips. A grip on a vehicle or helicopter carries the player through the moving climb ([hand-climbing.md](hand-climbing.md#moving-bodies)); it does not enter the native animated helicopter hang or snap onto a roof. The other hand remains available for a gun or an interaction. A rifle can be held in one hand. Only an explicit foregrip grab engages weapon support, and that target keeps priority over the climbing fallback.

The code is `payload/src/controllers/physical_hands/climb/` and `payload/src/controllers/physical_hands/stunt/`. Moving climbing and stunt handoff ship enabled. Their switches, tuning, and readouts stay in the debug UI. Headset tuning remains unverified.

## Native automatic attachment

The climb writes the gripped body as the proxy's support to prevent a fall while the hand carries the player. The game can interpret that support as a vehicle below and call [`Character::AutoAttachToStuntAreaOnVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-AutoAttachToStuntAreaOnVehicleBelow), which changes the interaction graph and snaps the capsule to a stunt area ([engine behavior](../../engine/gameplay/stunt-positions.md)).

A detour suppresses this automatic entry only for a local character with a fresh physical-grip proof. The pre-simulation step validates live body identities before publishing the immutable proof. Input and pre-simulation observations copy the player, proxy, world, body storage, and teleport epoch; a change advances a generation and discards older claims. The worker detour reads only the native caller's character-local flag and proxy member, the immutable proof, the generation, atomic destruction stamps, and the deadline. It performs no singleton lookup or live world, body-storage, property-buffer, or instance read, and takes no climbing lock. Destruction stamps invalidate proofs rather than pinning allocations. An unobserved lifecycle change can cause conservative denial only until the original proof expires; no reader refreshes its deadline. NPCs, ordinary vehicle landings without a physical grip, and unrelated native interactions retain the game behavior. No vehicle-below state flag is cleared or raced against ground-information recomputation.

Body identities include the world and body storage, engine instance, owner, shape, motion, static classification, and a destruction stamp. The destruction hook advances bounded atomic stamps before bodies are destroyed. A hash collision can conservatively drop a grip, but cannot preserve an old grip through destruction. Static geometry can have a null owner when the live body-property metadata identifies its engine instance. A missing instance, removed body, changed owner, or changed lifetime loses the grip without a fling.

## Completed roof mantle

A deliberate mantle captures the exact ledge owner and follows that body's frame. Its start uses the player's hand-relative upward speed rather than the vehicle's upward speed. A later higher grip or release of the original hand cannot change the captured owner. Completion revalidates and reports that captured body's identity, the player and proxy, the teleport epoch, and the feet's position to the input tick; an invalid owner cancels the offer.

The input tick revalidates the landing and checks the nearest stunt area's distance. [`Character::IsThisVehicleBelow`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-IsThisVehicleBelow) must confirm the same vehicle. A short-lived mantle claim prevents automatic attachment from selecting another vehicle before this check. Extra pre-simulation calls with no grips retain that claim without refreshing its deadline. A scoped landing offer keeps it through the deliberate entry attempt and clears it on success or rejection; expiry and lifecycle invalidation also clear it. Only the synchronous, owner-checked deliberate entry receives a thread-scoped bypass of the automatic-entry gate. The below-vehicle entry preserves heading; if it refuses, the nearest-area entry can turn the player to the same vehicle's heading.

After a successful entry, the game controls the character's velocity and attachment. The climb yields without adding a second carry velocity. Standing on a roof needs no grip. A subsequent grip can pin a hand without moving the character while the game holds the stunt position. A native stunt transition discards older grips because the game has moved the character since they were taken.

## Release velocity

The climb smooths total velocity and inherited point velocity with the same coefficient. The release is inherited velocity plus the scaled, capped difference between those smoothed velocities. The fling cap applies only to hand-generated motion. Turning the fling off still preserves inherited vehicle motion. The point velocity includes the body's angular velocity at the gripped point.

The carry average includes every active grip, including zero velocity for a static grip. A changed grip set resets the total and carry filters together. This prevents carry from a released moving hand from remaining in a static grip. Destruction, stale input, a mode change, a teleport, and player replacement discard the grip without generating a release fling.

## Normal helicopter hang

The normal grapple can still enter the game's animated helicopter hang. The physical-grip path never requests that entry. In a native hang, the existing grip tracker waits for a hand to take hold before opening all grips can request release. Its existing jump request remains unchanged. The native input task loads the action from graph data; a replacement action is unverified and no fallback action is added. The game's ordinary controls remain available if the request does not leave the hang.

The animated hang remains an animation-owned state. It is not used to represent a one-handed physical skid grip. A physical helicopter grip uses the moving climb and the actual controller handhold instead.

## Settings and readouts

The debug UI's Controllers tab contains the climbing and stunt settings. Physical-grip-to-hang entry has no policy or tuning fields. Older saved `enter_hang`, `hang_hold_s`, and `hang_below_m` keys are ignored when loading the configuration. `MOVING CLIMBS` reports moving-grip activity. `STUNT POSITIONS` reports a native hang or roof position, successful roof entries, refusals, and native-hang release requests. `ROOF TORSO` reports the procedural torso on the roof idle. The recorder template is `testing/scenarios/stunt_positions.toml`.

## Headset checklist

1. Grip a parked helicopter's skid at several different points, hang below it, and have it lift off. The hand stays at each actual point. There is no animated hang entry or capsule snap. The other hand can aim and fire a pistol or a rifle.
2. Use an interaction with the free hand. Intentionally grab a rifle's foregrip after releasing the climbing hand. The explicit foregrip takes priority; merely holding the rifle does not brace or steal the climb.
3. Grip a translating and turning vehicle. Release the final hand without a pull, then with a pull. Vehicle point velocity remains present in both releases; only the additional hand-generated part reaches the fling cap.
4. Hold a moving body with one hand and static geometry with the other. Release the moving hand first, then the static hand. The second release does not retain the old moving body's carry. Repeat with two different moving bodies.
5. Destroy or stream out the gripped body, respawn the player, switch input modes, and pause long enough to expire input. Each stale grip is lost without a fling or attachment to a recycled body.
6. Climb and deliberately mantle onto a moving car's roof. Only the completed mantle offers roof entry, and only for that vehicle. The game then carries the character with the hands free. Check the roof torso and view heading.
7. Land normally on a vehicle without a physical grip, and observe an NPC stunt entry. The native automatic attachment remains available.
8. Enter a helicopter hang through the normal grapple with open hands. It remains attached until the ordinary game exit or the existing armed grip-release request. Record which native input leaves it if the request is ignored.

These scenarios have not been run in the headset for this change. They remain required for live acceptance and tuning.
