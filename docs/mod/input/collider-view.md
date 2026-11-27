# The collider view

The collider view draws the physics world's bodies near the player in the world-space overlay: each body's box and, where it is cheap to read, its shape, coloured by what the body is, with the mod's joints, the contacts the hand bodies and held guns start, and the player's ground. It is a diagnostic for seeing what the hand bodies ([physical-hands.md](physical-hands.md)), the held guns' bodies ([physical-guns.md](physical-guns.md)), and the player's capsule meet. For why a hand does or does not take something, the grab view ([grab-view.md](grab-view.md)) draws the reach and the grip targets, and borrows this view's read of the world for its candidates. The code is `crates/jc3vr-physical/src/controllers/physical_hands/collider_view/`; the engine side is in [havok-bodies.md](../../engine/physics/havok-bodies.md).

## Using it

The view is off by default (`controllers.physical_hands.collider_view.enabled`). F3 (the director keymap's `ToggleColliderView`, [trailer-tooling.md](../trailer-tooling.md)) shows and hides it, as do the Collider view switch in the debug UI's visualization drawer and the button at the top of its readout under the Physical hands page's Diagnostics. It draws through the overlay, so it needs `controllers.overlay.enabled`, and it acts only in gameplay. Its reach and budgets are the `collider_view` section of `crates/jc3vr-physical/tuning/views.toml` ([tuning.md](../tuning.md)). The readout shows the steps, the reads with the last read's slots and time, the bodies drawn by kind, and the body under the gaze.

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

A body's box is drawn faint and its shape bright, both shown faintly through the scene. A grip joint is a lime line between the two bodies' origins, a gun's grip joint yellow, and its brace orange. A contact flashes as a growing white ring where the hand's or gun's body was before the step, sized by its impulse, with the other body's box in white, for `collider_view.draw.flash_s`. The player's ground is its box in green with a line along the surface's normal. The body under the gaze has a white box, and the readout names its id, kind, motion, layer and system group, mass, shape, and distance, since the overlay has no text.

## Reading the world

The world is read on the game thread at the top of the render update (the [`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender) hook, before the VR frame begins), after the game's update has stepped the physics and outside the step. It is read once per physics step, when the mod's physics-step clock (`jc3vr_core::published::Step`, begun by the hand bodies' pre-simulation callback, [physical-hands.md](physical-hands.md#threading)) has moved since the last read.

A read goes through every body slot of the world's body array ([`hknpWorld::m_bodies`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_bodies)) up to its peak body id ([`m_peakBodyIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_peakBodyIndex)). A slot holds a body in the simulation when it has a motion, its flags mark it static or dynamic, and it has a broad-phase entry ([`hknpBody::m_broadPhaseId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_broadPhaseId)). Its box is the broad phase's ([`m_aabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_aabb)), restored from the world's integer space ([`hknpWorld::m_intSpaceUtil`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_intSpaceUtil)) as the engine's [`getBodyAabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-getBodyAabb) does, then shifted into game space. The box's distance from the head, within `collider_view.radius_m`, picks the bodies in range, so a large static body whose origin is far away still counts when its box reaches the player. The nearest `collider_view.max_bodies` are kept and read further:

- the frame ([`hknpBody::m_transform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_transform)) and the motion (static, keyframed, or dynamic, from the flags);
- the kind: one of the mod's bodies by id (from the hand bodies' and the guns' readouts), the player's proxy by id ([`PfxCharacterInstance::GetCharacterRigidBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-GetCharacterRigidBodyId)), then the collision layer ([`PfxLayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer)) for characters, ragdolls, and vehicles, then the motion;
- the mass, from the motion's inverse mass ([`hknpMotion::m_inverseInertia`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpMotion##field-m_inverseInertia));
- the shape, by its own kind ([`hknpShape::getType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpShape##vfunc-getType), [`hknpShapeType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpShapeType)): a sphere, a capsule's segment ([`hknpCapsuleShape::m_a`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpCapsuleShape##field-m_a)) and radius, a convex polytope's vertices and face edges ([`hknpConvexPolytopeShape::m_faces`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpConvexPolytopeShape##field-m_faces)), and a plain convex shape's vertices. Compounds, meshes, and height fields, most of the static world, are drawn by their boxes alone.

The read also takes the mod's joints, each from the world's constraint slot for its id ([`hknpWorld::m_constraintManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_constraintManager)) while the slot holds an enabled constraint, and the player's ground from its proxy's last support check ([`PfxCharacterInstance::m_GroundBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundBodyId), [`m_GroundInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundInformation)): the cheap view of the capsule's contacts. The contacts come from the hand bodies' contact queue as the frame tick deals the blows, each with where its body was before the step, the other body, and the impulse; a gun's contact with the player's own bodies is not noted.

## Drawing at render time

The primitives are built every rendered frame in the same render update, at the frame's interpolation fraction (the render context's `m_Dtf`) between the last two reads, as the render-time anchoring rule asks ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). The engine draws each object between its last two ticks' matrices by that fraction, and the reads are one per step, so a collider interpolated the same way stays on its object at frame rate however fast either moves, the player's capsule in a car included. A body the earlier read did not have, or had with another shape, is drawn at its last read. While the physics is paused there are no new reads, and the view holds still.

## Cost and threading

A read is one pass over every body slot up to the peak id (a few flag checks and a box restored from sixteen-bit integers per slot, and a shape read per kept body), once per physics step while the view is on, with its slots and time in the readout. The drawing is capped at `collider_view.max_primitives`, held under a ceiling (`PRIMITIVE_CEILING` in `collider_view::draw`), with whole bodies kept nearest first; the joints, contacts, ground, and gazed body are drawn first so they survive the budget. A hull draws at most `collider_view.draw.max_hull_edges` edges and a faceless convex shape at most `collider_view.draw.max_points` vertices. The view's slot is the overlay's last, so when the overlay is full the view gives way first.

The view's own state is a lock taken on the game thread only, by the render update; the overlay store's and the readout's leaf locks are taken briefly under it. The contacts are noted under a leaf lock of their own and taken before the view's state is locked. Nothing here takes the VR runtime lock ([threading.md](../threading.md)).

## Outstanding checks

- The cost of a read in a dense part of the map.
- Whether a body moved outside the step (a keyframed body, or a hand body the pre-simulation callback places) has a broad-phase box that lags its frame for a step.
- Whether character proxies in ghost mode and ragdoll proxies carry the layers the kinds read.
