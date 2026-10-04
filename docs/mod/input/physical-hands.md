# Physical hands

Each empty hand has a palm that collides with the world. Pressing a palm into a wall pushes the body away from it and holds the rendered hand at the surface; a fast hit on a character punches it with a closed fist or slaps it with an open hand; a fast hit on a loose object knocks it; and a grip press near a loose object picks it up, or near a weapon lying in the world, collects the weapon. The code is `payload/src/controllers/physical_hands/`; the engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md) and [item-pickup.md](../../engine/gameplay/item-pickup.md).

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

## Picking things up

An acting palm that holds nothing looks, every tick, for what its grip would take: five short rays from the back of the hand through the palm, out of the palm, along the fingers, between the two, and out of either edge, reaching `pickup.reach_m` past the palm, and the surface the palm presses into. The nearest hit that a hand takes is the hand's target, shown with `pickup.show_targets` as a ring in the world-space overlay: blue for an object one hand lifts, red for one it does not, gold for a weapon. A hand takes two kinds of thing, told apart by the engine's run-time type test:

- a weapon lying loose in the world ([`WeaponBase::TYPE_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-TYPE_ID), in the [`E_WS_DROPPED`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponState##variant-E_WS_DROPPED) state);
- a rigid object ([`RigidObject::TYPE_ID`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::RigidObject##func-TYPE_ID)) whose body is dynamic and of finite mass: a crate, a barrel, a gas canister, debris.

Characters, ragdolls, and vehicles are never targets. Collectibles have no body, so the rays never find them, and they are left to the game's use input.

### The grip

The target reaches the grip as a grab volume, the pickup volume ([`GrabVolume::Pickup`](../../../payload/src/controllers/bindings/grab.rs)), reported for a hand with a target that holds nothing. Every placed volume wins over it: a holster, a pouch, the foregrip, a vehicle control, and a parachute handle are where the hand goes on purpose, and a crate beside the hip must not block a draw. As with every grab volume, the grip press is latched at its edge, so it neither grapples nor reaches the game for the whole press.

A grab of a weapon clicks the game's use-item action ([`USE_ITEM`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/input::input_action_map::Action##variant-USE_ITEM)), the action the Y button already carries, so the game picks the weapon up its own way: its pickup animation, its inventory rules, its HUD, and its choice among the weapons in range (see [item-pickup.md](../../engine/gameplay/item-pickup.md#picking-up-a-weapon)). The hand does not hold the weapon; the game equips it. Ammunition and grenades need no grab, since the game collects them as the player walks over them. `pickup.weapons` turns the weapon grab off.

A grab of an object asks the next simulation tick to take hold of it, and releasing the grip asks it to let go.

### Holding

A held object stays dynamic and is driven, not attached. At the grab its body's centre of mass and orientation are recorded in the palm's frame; each tick the hand's palm gives the place the body should be, and the body is given the velocities that carry it there ([`hknpWorld::setBodyVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-setBodyVelocity)): the palm's own velocity plus `pickup.stiffness_per_s` per metre it is off, up to `pickup.max_speed_mps`, and a turn of `pickup.angular_stiffness_per_s` per radian, up to `pickup.max_angular_speed_rps`. A heavy object takes its time to come up to that velocity (`pickup.heavy_response_s` at the one-hand limit, lighter ones in proportion), so it feels heavier than a light one. Because the body is still simulated, it collides, a wall stops it, and nothing has to be restored when it is let go. Keyframing the body instead would make it push everything with infinite mass and would have to be undone on every exit path.

One hand lifts up to `pickup.one_hand_mass_kg` (25 kg by default) and both hands on one object up to `pickup.two_hand_mass_kg` (60 kg). With both hands on it, the object is held between them: at the midpoint of the two hands' places and halfway between their rotations. A hand that holds what it cannot lift keeps hold but moves nothing (a red disc marks it) until the other hand takes hold too. An object that lags more than `pickup.break_distance_m` behind the hand, caught on something, is let go.

A holding hand does not probe, push off surfaces, or strike, and the other palm's probes pass through what the hands hold, so a free hand never pushes the body off the carried object. The reach rays do find a held object, so that the second hand can take hold of it. The holding hand still counts as acting for the melee gesture, so a swing with something in the hand never starts the game's melee lunge.

### Letting go and throwing

Releasing the grip lets go. The last hand to let go of an object it lifts throws it: the body's linear velocity is set to the hand's velocity times `pickup.throw_scale`, capped at `pickup.max_throw_speed_mps`, and its spin is left as it was. The hand's velocity is its palm's world-space motion smoothed over a few hundredths of a second, so it includes the player's own movement. An object only two hands lift keeps the velocity the hold gave it.

Every hold is checked each tick before it is driven: the body id must still have a dynamic body, owned by the object it was taken from ([item-pickup.md](../../engine/gameplay/item-pickup.md#a-body-in-the-havok-world)), so a body freed or reused for another object is let go. Holds are dropped without a throw when the palms stand down (a vehicle, a swim, a teleport), when the hand stops being tracked or empty, and when the bindings stop reading the grips; a game menu keeps them.

## Haptics

A palm meeting a surface pulses its hand (`contact_pulse`, weaker for a slower meeting) and a strike pulses it harder (`strike_pulse`, scaled by strength). Taking hold of an object or grabbing a weapon pulses `pickup.grab_pulse`, and letting go `pickup.release_pulse`. They are routed through the weapon haptics as a collision, a melee strike, and a pickup, and are behind `haptics` and the haptics master switch.

## Threading

The strikes, the object pushes, and the held objects' velocities change the main thread's engine state. The input tick notes its thread; the palms act only when the aim update runs on that thread, and otherwise stand down and count the tick in the readout. The push is read by the locomotion task's post-call and the clamp by every caller of the active hands, through the physical hands' own lock, never the VR runtime's. Both go stale after a fifth of a second without a tick. The pickup grabs and releases are read on the input tick and acted on at the next aim update, through the same lock.

## Settings and readouts

The settings are `physical_hands` in the controller settings, under "Physical hands" in the debug UI's Controllers tab, with the readout (each palm's press, push, speed, and kind, what it reaches or holds with the mass and the lag, the last strike, and the counters) beside the other subsystems. The pickup settings are `physical_hands.pickup`. `show_boxes` draws the palm boxes in the world-space overlay, coloured by whether they press into the world or an object.

## Unverified in the headset

- That the aim update runs on the input tick's thread. The readout's off-thread count says.
- That the aim's ray filter hits the characters' hit proxies at palm range, so that a strike lands.
- The strength of the hit reaction for a given impulse, and so the defaults of the impulse shares: the reaction's thresholds are per-character data.
- Which stumble act moves the character which way.
- Whether holding the hand at the surface reads well with the render-time late update.
- How the push feels against the roomscale chase when the player leans into a wall.
- That the aim's ray filter hits loose props and dropped weapons at all, and their bodies' masses: the mass limits are guesses.
- That a held object pressed into the player's own capsule does not shove the player; nothing stops the two colliding.
- Which weapon the game picks up when several are in range, and that the use-item click alone starts the pickup.
- Whether the follow's defaults hold a crate steadily at a 60 Hz simulation, and how a throw feels at `throw_scale` one.

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
10. Reach toward a small crate or a barrel: a blue ring marks it. Grip: it follows the hand, turns with the wrist, and stops against a wall rather than passing through. Release it while swinging: it flies along the swing.
11. Grip a heavy object with one hand: a red disc, nothing moves. Grip it with the other hand too: it lifts between the hands. Let go of one hand: it drops.
12. Kill a soldier, reach for the dropped weapon: a gold ring. Grip: the pickup animation plays and the weapon is equipped. Walk over another of the same type: the ammunition is collected without a grab.
13. Hold a crate and touch it with the other palm: no push off it, and the other hand can grip it too.
14. Reach toward a holster with a crate beside it: the holster draws. Open the pause menu while holding a crate: it is still held after resuming; enter a vehicle while holding: it drops.
