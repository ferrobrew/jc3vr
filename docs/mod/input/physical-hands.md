# Physical hands

Each empty hand has a palm that collides with the world. Pressing a palm into a wall pushes the body away from it and holds the rendered hand at the surface; a fast hit on a character punches it with a closed fist or slaps it with an open hand; a fast hit on a loose object knocks it. The code is `payload/src/controllers/physical_hands/`; the engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md).

## Probes, not bodies

The palms are probed with ray casts rather than simulated as bodies. A keyframed box per palm would need the Havok shape and body construction, the game's collision filter, and a drive before each physics step, none of which the mod binds, and a contact would still need translating into the game's damage and reactions. The casts reuse the player aim's filtered ray query ([`PhysicsSystem::CastRay`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-CastRay) through the aim's filter, as in [the aim raycast](../../../payload/src/controllers/aim/raycast.rs)), which never hits the player and already names the hit object and body. The game's own melee uses a box overlap query ([`PhysicsSystem::CollideBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsSystem##func-CollideBox)) for objects; it is bound but not used, because its all-hits collector owns Havok heap memory the mod would have to manage.

## The palm

The palm is a box in the wrist frame, sized from the player's hand (`palm_half_extents_m`, about 9 by 4 by 10 cm) and centred toward the fingers (`palm_offset_m`), scaled by the VR world scale. It is probed from six points: its centre, the middle of the palm's face, and the four corners of the face toward the fingers, which lead a punch and a slap's follow-through.

A palm acts on foot on the ground, for a tracked hand the controllers drive (its tier weight is above zero) that holds nothing: no gun, not the foregrip, no grenade or charge being thrown. The hands are taken before the two-handed pose, recoil, and the anchored grapple hand are applied, so the probes see where the player's hand is.

## Contacts

Each tick, from the player aim control's post-simulation update, each probe point is cast from where it last was in the open to where it is now, as the head probe does. A hit on world geometry or an object means the point crossed the surface: it is pressed in by its depth behind the surface along the normal, and its open position stays put until the hand comes back out. A cast from the shoulder forgives a stale open position, so that a hand carried around an obstacle counts as clear again, and a teleport forgets the open positions. A hit on a character is a strike: characters are never pressed against.

The hit object's kind comes from the engine's run-time type test ([`GameObject::IsType`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/game::GameObject##vfunc-IsType) with [`Character::TYPE_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::character::Character##func-TYPE_ID)) on the hit's physics game object.

## Pushing off surfaces

A palm pressed past a small deadband (`push_deadband_m`) into a surface whose normal is mostly horizontal (`min_push_horizontal`) asks for a push along the normal, at `push_stiffness_per_s` per metre of press, up to `max_push_speed_mps` for both hands together. Pressing down on a table or the floor does not push. As the body moves away the press shallows, so the push settles with the palm resting on the surface.

The push is added to the on-foot locomotion task's wanted velocity from the roomscale simulation half, before the chase reads the task's velocity, so it counts as the task's own movement: the view moves with the body as it does for the stick, and the cockpit shift does not take it back out. Only the on-foot task's ticks carry it, so a palm in a vehicle, a swim, or a fall never pushes.

With `clamp_hand`, the hand's cockpit poses are moved out of the surface by the deepest press, which the IK targets and the rendered hand follow. The probes use the unclamped hands, so the press, and with it the push, still measures where the real hand is.

## Strikes

A probe point that crosses into a character strikes it, through a per-hand gate: one strike per swing (the hand must slow below `rearm_speed_mps`) and `strike_cooldown_s` between strikes. The strike's speed is the hand's peak speed since the last tick (from the controllers' motion history), taken into the surface; below `min_strike_speed_mps` it does nothing, and it reaches full strength at `full_strike_speed_mps`.

A hand whose four fingers' mean curl reaches `fist_curl` punches; an open hand slaps. The curl is the finger curls' when they drive the hand, and the grip's squeeze otherwise. Each kind scales the player's own melee damage and impulse ([`Player::m_MeleeDamage`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::player::Player##field-m_MeleeDamage) and [`Player::m_MeleeImpulse`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::player::Player##field-m_MeleeImpulse)) by its share and the strike's strength, so the hammer upgrades carry over: by default a full punch deals the game's melee damage and a full slap a fifth of it.

The strike is the game's melee hit made by hand: a melee damage message ([`DMG_TYPE_MELEE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/damage::DamageFlags##flag-DMG_TYPE_MELEE)) with the player as the damager, the player's faction and state, the contact as its position, and the hand's direction, sent to the character's damage virtual ([`PhysicsGameObject::OnDamage`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PhysicsGameObject##vfunc-OnDamage)). Unlike the game's melee, the message carries an impulse, so the character's own hit reaction decides from it whether it stumbles or ragdolls. A strike past the kind's `stumble_strength` also queues one of the game's melee stumbles on the character, front or behind from its facing and left or right from the side the blow travels toward.

With `replace_melee_gesture`, a hand whose palm acts does not start the game's melee attack through [the physical-melee gesture](controllers-and-roomscale.md#melee): the palm's strike replaces the lunge. A hand holding a weapon keeps the gesture.

## Objects

A palm meeting a solid surface owned by an object at `min_object_speed_mps` or faster pushes the hit body along the hand's direction with a share (`object_impulse`) of the player's melee impulse, scaled by the hit's strength ([`hknpWorld::applyBodyImpulseAt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-applyBodyImpulseAt)). The engine leaves static and keyframed bodies alone, so a hit on a fixed object only pushes the player.

## Haptics

A palm meeting a surface pulses its hand (`contact_pulse`, weaker for a slower meeting) and a strike pulses it harder (`strike_pulse`, scaled by strength). They are routed through the weapon haptics as a collision and a melee strike, and are behind `haptics` and the haptics master switch.

## Threading

The strikes and the object pushes change the main thread's engine state. The input tick notes its thread; the palms act only when the aim update runs on that thread, and otherwise stand down and count the tick in the readout. The push is read by the locomotion task's post-call and the clamp by every caller of the active hands, through the physical hands' own lock, never the VR runtime's. Both go stale after a fifth of a second without a tick.

## Settings and readouts

The settings are `physical_hands` in the controller settings, under "Physical hands" in the debug UI's Controllers tab, with the readout (each palm's press, push, speed, and kind, the last strike, and the counters) beside the other subsystems. `show_boxes` draws the palm boxes in the world-space overlay, coloured by whether they press into the world or an object.

## Unverified in the headset

- That the aim update runs on the input tick's thread. The readout's off-thread count says.
- That the aim's ray filter hits the characters' hit proxies at palm range, so that a strike lands.
- The strength of the hit reaction for a given impulse, and so the defaults of the impulse shares: the reaction's thresholds are per-character data.
- Which stumble act moves the character which way.
- Whether holding the hand at the surface reads well with the render-time late update.
- How the push feels against the roomscale chase when the player leans into a wall.

## Headset checklist

1. Lay a palm flat on a wall at arm's length: the hand rests on the surface, does not pass through, and the body does not drift.
2. Push the wall: the body (and the view) moves back, settling with the palm on the wall. Push the floor or a table top: nothing moves.
3. Slap a civilian with an open hand at speed: the readout shows a slap and a small damage, and the hand pulses.
4. Punch a soldier with a closed fist: damage near the game's melee, a stumble at full speed, and a ragdoll for a hard enough hit if the reaction allows.
5. Swing through a character: one strike, not one per tick.
6. Hit a crate or a barrel: it is knocked along the swing. Hit a parked car: no visible effect beyond the push off it.
7. Draw a gun: that hand's palm stops (the readout says not acting) and the melee gesture works with it again; the other, empty hand still collides.
8. In a vehicle, swimming, and under the parachute: no palm acts.
9. With `show_boxes`, the boxes sit over the palms, turn orange against walls and green against objects.
