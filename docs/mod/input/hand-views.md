# Grab and collider views

Two overlay diagnostics show, in the headset, what the hands reach and what the physics world holds near the player. Both draw through the world-space overlay, so they need `controllers.overlay.enabled`, are switched in the debug UI's visualization drawer and at the top of their readouts under the Physical hands page's Diagnostics, have no pause menu entry ([settings.md](../settings.md)), and take their reach and budgets from `crates/jc3vr-physical/tuning/views.toml`. The overlay has no text, so every state is a colour or a shape, and the readouts name what the overlay cannot.

## The grab view

The grab view (`physical_hands/grab_view/`, on by default) answers why a weapon on the ground, a loose object, or a car door is not taken: whether it is a candidate, whether a ray reaches it, whether the reach takes what the ray hit, whether the resolver offers it, and whether the subsystem acting on the grab refused it. The grip press log ([grip.md](grip.md#readouts-and-logs)) says the same in text. It draws:

- each acting palm's five reach rays ([physical-hands.md](physical-hands.md#picking-things-up)) to their hit, grey for a miss, orange for something a hand does not take, green for something offered;
- the door handles near a free palm as rings, green when offered and red when no body was found at the handle ([vehicle-doors.md](vehicle-doors.md));
- a line from each hand's grip to every grip target it may take, coloured by state (out of reach, in reach, chosen, held) and ending at the target's marker ([grip.md](grip.md#markers)), nearest first under a cap;
- a red ring flashing where a chosen grab was refused, at the reach's hit for the physical hands or the target's marker for the weapon handling;
- the candidates near the hands as boxes: dropped weapons gold, liftable objects white, too heavy red, vehicles and loosened parts green; something the reach never finds, or a weapon with no gold box, is where to look when a grab does nothing;
- on a gun held in one hand with the other free, the zones that decide what the free hand takes on it ([reload.md](reload.md)).

The palms' tick records the rays and handles only while the view is on, the candidates come a few times a second from the collider view's world read, and the gun's zones through the body crate's held-item seam (`held::reload_views`), so the physical hands do not name the weapon handling. Everything is anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)): the hand ends follow the drawn hands, the rays ride with the head that cast them, and the handles, boxes, and refusals are world anchors, so a box on something moving lags by up to one read.

## The collider view

The collider view (`physical_hands/collider_view/`, off by default, also on F3 through the director keymap's `ToggleColliderView`, [trailer-tooling.md](../trailer-tooling.md)) draws the physics bodies near the player as boxes and, where cheap, shapes, coloured by kind (static, dynamic, keyframed, the mod's hand and gun bodies, the player's capsule, other characters, ragdoll parts, and vehicles), with the mod's joints, the contacts the hand and gun bodies start, the player's ground, and a white box on the body under the gaze, which the readout describes. The engine side is in [havok-bodies.md](../../engine/physics/havok-bodies.md).

### Reading the world

The world is read on the game thread at the top of the render update ([`Game::UpdateRender`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::Game##func-UpdateRender)), outside the step, once per physics step on the mod's step clock ([physical-hands.md](physical-hands.md#threading)). A read walks every slot of [`hknpWorld::m_bodies`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_bodies) up to [`m_peakBodyIndex`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_peakBodyIndex); a slot holds a live body when it has a motion, static or dynamic flags, and a broad-phase entry ([`hknpBody::m_broadPhaseId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_broadPhaseId)). Its box is the broad phase's [`m_aabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_aabb), restored from the world's integer space ([`m_intSpaceUtil`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_intSpaceUtil)) as [`getBodyAabb`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-getBodyAabb) does; the box's distance picks the bodies in range, so a large static body counts when its box reaches the player. For the nearest bodies it reads:

- the frame ([`m_transform`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_transform));
- the kind: the mod's bodies by id, the player's proxy by [`PfxCharacterInstance::GetCharacterRigidBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##func-GetCharacterRigidBodyId), then the [`PfxLayer`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer) for characters, ragdolls, and vehicles;
- the mass from [`hknpMotion::m_inverseInertia`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpMotion##field-m_inverseInertia);
- the shape by [`hknpShape::getType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpShape##vfunc-getType): a sphere, a capsule ([`hknpCapsuleShape::m_a`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpCapsuleShape##field-m_a)), a convex polytope's vertices and edges ([`m_faces`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/havok::hknpConvexPolytopeShape##field-m_faces)), or a plain convex's vertices; compounds, meshes, and height fields, most of the static world, draw as boxes.

It also takes the mod's joints from their constraint slots ([`m_constraintManager`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##field-m_constraintManager)), the player's ground from the proxy's last support check ([`m_GroundBodyId`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundBodyId), [`m_GroundInformation`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxCharacterInstance##field-m_GroundInformation)), and the contacts from the hand bodies' queue.

### Drawing

The primitives are built every rendered frame at the frame's interpolation fraction (the render context's `m_Dtf`) between the last two reads, as the engine draws each object between its last two ticks, so a collider stays on its object at frame rate however fast it moves ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). While the physics is paused the view holds still. The drawing is capped, nearest bodies first, with joints, contacts, ground, and the gazed body drawn first so they survive the budget; its overlay slot is the last.

## Outstanding checks

- The cost of a world read in a dense part of the map.
- Whether a body moved outside the step (a keyframed body, or a placed hand body) has a broad-phase box that lags its frame for a step.
- Whether ghost-mode character proxies and ragdoll proxies carry the layers the kinds read.
