# The collider view

The collider view draws the physics world's bodies near the player in the world-space overlay: each body's box and, where it is cheap to read, its shape, coloured by what the body is, with the mod's joints, the contacts the hand bodies and held guns start, and the player's ground. It is a diagnostic for seeing what the hand bodies ([physical-hands.md](physical-hands.md)), the held guns' bodies ([physical-guns.md](physical-guns.md)), and the player's capsule meet. It is off by default. For why a hand does or does not take something, the grab view ([grab-view.md](grab-view.md)) draws the reach's rays, the grip targets, and the candidates around the hands, and borrows this view's read of the world for its candidates. The code is `payload/src/controllers/physical_hands/collider_view/`; the engine side is in [havok-bodies.md](../../engine/physics/havok-bodies.md).

## Using it

Ctrl+Numpad0 (the director keymap's `ToggleColliderView`, [trailer-tooling.md](../trailer-tooling.md)) shows and hides it, as does the button at the top of "Collider view" in the debug UI's Controllers tab, where the readout is. Its settings are `controllers.physical_hands.collider_view`, under "Collider view" in the tab's settings: the radius (6 m), the most bodies (128) and primitives (1536) drawn, and switches for the boxes, the shapes, the static bodies, the contacts and ground, the joints, and the gaze. The view draws through the overlay, so it needs `controllers.overlay.enabled`, and it acts only in gameplay. The `VR: STATUS` page has a `COLLIDER VIEW` row under `HAND BODIES`: `OFF` with the key that shows it, `WAITING` until the first read after a physics step, `FAILED` when the world could not be read (the error is in the page's log line), and otherwise the bodies drawn out of those in range, the primitives, and how many bodies the budget left out.

The colours:

| Kind | Colour | What it is |
| --- | --- | --- |
| Static | grey | a static body: the terrain, buildings, and fixed props |
| Dynamic | cyan | a dynamic body no other kind claims: a loose prop or debris |
| Keyframed | violet | a keyframed body |
| Hand body | orange | one of the mod's hand bodies |
| Gun body | yellow | one of the mod's held guns' bodies |
| Player capsule | green | the player's character proxy |
| Character | magenta | another character's proxy |
| Ragdoll part | red | a ragdoll's part or proxy |
| Vehicle | blue | a vehicle's body or wheel collision |

A body's box is drawn faint and its shape bright, both shown faintly through the scene in front of them. A grip joint is a lime line between the two bodies' origins, a gun's grip joint yellow, and its brace orange. A contact flashes as a growing white ring where the hand's or gun's body was before the step, sized by the contact's impulse, with the other body's box in white, for a little under half a second. The player's ground is its box in green, with a line along the surface's normal from the bottom of the player's box. The body under the gaze has a white box, and the readout names it: its id, kind, motion, layer and system group, mass, shape, and distance. The overlay has no text, so the labels are in the readout rather than in the world.

## Reading the world

The world is read on the game thread at the top of the render update (the [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) hook, before the VR frame begins): after the game's update has stepped the physics, outside the step, where the hand bodies' own reads of the world are safe too. It is read once per physics step, not once per frame. A pre-simulation callback of the view's own ([`PfxPreSimulationCallback`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxPreSimulationCallback)) counts the steps; it is registered from the frame tick ahead of the game's update while the view is on, removed there when it is switched off, and removed by the shutdown cleanup. The render update reads the world when the count has moved since its last read.

A read goes through every body slot of the world's body array ([`hknpWorld::m_bodies`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_bodies)) up to its peak body id ([`m_peakBodyIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_peakBodyIndex)). A slot holds a body in the simulation when it has a motion, its flags mark it static or dynamic, and it has a broad-phase entry ([`hknpBody::m_broadPhaseId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_broadPhaseId)). Its box is the broad phase's ([`m_aabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_aabb)), quantized into the world's integer space and restored through it ([`hknpWorld::m_intSpaceUtil`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_intSpaceUtil)) as the engine's [`getBodyAabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-getBodyAabb) does, then shifted into game space. The box's distance from the head picks the bodies in range, so a large static body whose origin is far away still counts when its box reaches the player. The nearest of them are kept, up to the most bodies, and only those are read further:

- the frame ([`hknpBody::m_transform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_transform)) in game space;
- the motion: static, keyframed, or dynamic, from the flags;
- the kind: one of the mod's bodies by id (from the hand bodies' and the guns' readouts), the player's proxy by id ([`PfxCharacterInstance::GetCharacterRigidBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-GetCharacterRigidBodyId)), then the collision layer ([`PfxLayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer)) for characters, ragdolls, and vehicles, then the motion;
- the mass, the inverse of the motion's inverse mass ([`hknpMotion::m_inverseInertia`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpMotion##field-m_inverseInertia));
- the shape, by the shape's own kind ([`hknpShape::getType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpShape##vfunc-getType), [`hknpShapeType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpShapeType)): a sphere's centre and convex radius, a capsule's segment ([`hknpCapsuleShape::m_a`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpCapsuleShape##field-m_a)) and radius, a convex polytope's vertices and the edges of its faces ([`hknpConvexPolytopeShape::m_faces`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpConvexPolytopeShape##field-m_faces)), and a plain convex shape's vertices. Compounds, meshes, and height fields, which are most of the static world, are drawn by their boxes alone.

The read also takes the mod's joints, each from the world's constraint slot for its id ([`hknpWorld::m_constraintManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_constraintManager)) while the slot holds an enabled constraint, and the player's ground from its proxy's last support check ([`PfxCharacterInstance::m_GroundBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundBodyId), [`m_GroundInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundInformation)). That is the cheap view of the capsule's contacts: what it stands on. A static ground body is read even with the static bodies switched off.

The contacts come from the hand bodies' contact queue: as the frame tick deals the blows, each contact the hand bodies and the held guns started is noted with where its body was before the step, the other body, and the impulse. A gun's contact with the player's own bodies, which the blows skip, is not noted.

## Drawing at render time

The primitives are built every rendered frame in the same render update, at the frame's interpolation fraction (the render context's `m_Dtf`) between the last two reads, and submitted to the overlay's own slot, as the render-time anchoring rule asks ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). The engine draws each object between its last two ticks' matrices by that fraction, and the reads are one per step, so a body's frame interpolated between its last two reads is where its object is drawn: a collider stays on its object at frame rate however fast either moves, including the player's capsule in a car. Every point is a world anchor at that interpolated position. A body the earlier read did not have, or had under the same id with another shape, is drawn at its last read. A box is carried with its body's interpolated position. While the physics is paused there are no new reads, and the view holds still.

## Cost and caps

The read costs one pass over every body slot up to the peak id: per slot a few flag checks and a box restored from sixteen-bit integers, and per kept body a shape read. It runs once per physics step while the view is on, and the readout shows the slots it went through and how long it took. The drawing is capped twice: the most bodies kept (the nearest first), and the most primitives submitted, with whole bodies kept the nearest first and the rest counted as over budget. The joints, the contacts, the ground, and the gazed body are drawn before the bodies, so they survive the budget. A hull draws at most 96 edges and a convex shape without faces at most 32 vertices. The view's primitive cap is held under a ceiling of 2048 whatever its setting, half of what the overlay draws in a frame, and the view's slot is the overlay's last, so when the overlay is full the view gives way first.

## Threading

The view's own state is a lock taken on the game thread only, by the render update; the overlay store's lock and the readout's are taken under it, briefly, and both are leaves. The contacts are noted under a leaf lock of their own on the game thread, and taken in the render update before the view's state is locked. The status page's row reads the readout's lock on the UI capture thread. Nothing here takes the VR runtime lock ([threading.md](../threading.md)).

## Unverified

- The cost of a read in a dense part of the map: the peak body id there, and the time the readout shows.
- Whether a body's broad-phase box can lag its frame for a body moved outside the step, such as a keyframed body or a hand body placed by the pre-simulation callback; the box would then sit off the shape for a step.
- That character proxies in ghost mode and ragdoll proxies carry the layers the kinds read; a body classified by its motion instead would be drawn as dynamic or keyframed.

## Headset checklist

- Switch the view on with Ctrl+Numpad0 and check that the player's capsule (green) stands around Rico, that the ground's box is green under it, and that the normal line points up off flat ground.
- Walk up to a crate and a car and check that their boxes and shapes sit on them, and that the gaze readout names the one looked at, with a sensible mass.
- Reach into a wall and swing at a crate: the hand bodies (orange) should stay at the wall, and each hit should flash a ring at the hand and the crate's box.
- Pick up a prop with joint grabs on, and hold a gun: a lime line should join the hand to the prop, and a yellow one the hand to the gun, with an orange one for a bracing hand.
- Drive fast and fall: the colliders should stay on the car and on Rico without trailing.
- Watch the readout's read time in a town, and turn the radius up to see where the budget starts leaving bodies out.
