# Physical hands

Each empty hand has a palm that collides with the world. Pressing a palm into a wall pushes the body away from it and holds the rendered hand at the surface; a fast hit on a character punches it with a closed fist or slaps it with an open hand; a palm pressed into a loose object shoves it, slowly or hard, and a heavy one resists; and a grip press near a loose object takes hold of it where the hand touched it, or near a weapon lying in the world, collects the weapon. The code is `payload/src/controllers/physical_hands/`; the engine side is in [melee-and-damage.md](../../engine/gameplay/melee-and-damage.md) and [item-pickup.md](../../engine/gameplay/item-pickup.md).

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

## Shoving objects

A palm pressed into a dynamic body shoves it every tick it presses, however slowly the hand moves, so a crate slides, a barrel rolls, and a tall thing on a narrow base tips. The contact is soft, at the velocity level: the body's point under the palm is asked to move into the body at least as fast as the hand moves into it, plus `shove.stiffness_per_s` per metre of press past `shove.deadband_m`, up to `shove.max_speed_mps`. The impulse that asks for it is the point's mass along the surface normal times the speed it lacks, computed from the body's mass, inertia, and velocities ([item-pickup.md](../../engine/gameplay/item-pickup.md#a-body-in-the-havok-world)), so a push off the centre turns the body as well as moving it. It is applied at the contact point ([`hknpWorld::applyBodyImpulseAt`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-applyBodyImpulseAt)) and capped at `shove.max_force_n` over the tick, the hand's strength, so a car takes a push slowly. The palm drags the surface it slides along by friction, up to `shove.friction` times the push.

How much a body gives is measured rather than assumed: each tick compares the speed the point reached with the speed the last tick asked of it, and the share it did not give, or that the strength cap held back, is the resistance, smoothed over `shove.give_smoothing_s`. The push off a surface ([pushing off surfaces](#pushing-off-surfaces)) is scaled by the resistance, so a light crate in the open slides away without pushing the player, while a crate against a wall, a parked car, or a body joined to the world pushes the player away as a wall does. World geometry and bodies that are not dynamic resist fully. The rendered hand is held at the body's surface either way, so it stops at what does not move and follows what does.

A palm meeting an object at `min_object_speed_mps` or faster also knocks it on the first tick of contact, with a share (`object_impulse`) of the player's melee impulse scaled by the hit's strength, on top of the shove: a hit throws a prop harder than a push of the same speed. The engine leaves static and keyframed bodies alone.

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

A held object stays dynamic and is driven, not attached. A hand holds it at the point it touched: at the grab, the point is recorded in the body's frame and in the palm's, and the body's orientation in the palm's frame. Each tick the palm gives where that point should be and how the body should be turned, and the hold is solved as a pair of soft constraints between the palm and the body, the way a physics engine solves its joints ([`hold`](../../../payload/src/controllers/physical_hands/hold.rs)):

- the grab point is asked to move with the palm (the palm's velocity at that point, from its linear and angular velocity) plus `pickup.stiffness_per_s` per metre it is off, up to `pickup.max_speed_mps`;
- the body is asked to turn with the palm plus `pickup.angular_stiffness_per_s` per radian it is off, up to `pickup.max_angular_speed_rps`.

The impulses that give those velocities are found together over a few passes: the pull at the grab point through the point's mass matrix, and the twist through the body's inertia, each accumulated and capped by the hand's strength over the tick, `pickup.hand_force_n` and `pickup.hand_torque_nm`. The solve starts from where gravity (`pickup.gravity_mps2`) will have the body after the next step, so the pull includes holding it up. The result is set as the body's velocities ([`hknpWorld::setBodyVelocity`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpWorld##func-setBodyVelocity)) before the step, which integrates them, resolves the body's contacts, and resolves any joint it hangs from.

What falls out of the strength caps:

- one hand lifts what weighs less than its pull (about 35 kg by default), two hands twice that; a heavier object sinks from the hand and can be dragged along the ground slowly;
- a long object held at one end droops and swings from the hand when its weight's lever outweighs the wrist's twist, and turning the wrist swings its far end like a lever;
- a heavy object comes up to speed slowly, since the pull's cap limits its acceleration.

A hand takes hold of any loose object up to `pickup.max_mass_kg`; the reach ring is red when the hands could not lift it. Holding the rendered hand at the grab point (with `clamp_hand`), the hand stays on the object wherever the object is: it stops at a crate the hand cannot move rather than passing through it. A hand that pulls at near its strength without moving the object (`pickup.resist`) pushes the player toward the grab point, as a wall does, so pushing a held object that will not move pushes the player back, and walking away from one holds the player back. An object that lags more than `pickup.break_distance_m` behind the hand, caught on something, is let go.

### Two hands

Both hands may hold one object, each at its own point. Each hand pulls its own grab point, so a long object hangs between the hands and tilts as one hand rises; the two pulls add, so together they lift twice what one does. The wrists' twists are weakened to `pickup.two_hand_angular_share` of their strength while both hold, so that the two grab points, not two wrists that rarely agree, set the object's line, and the wrists still roll it about that line. Letting go with one hand leaves the object to the other, which holds it if it can.

### Joined objects

A body joined to the world or to another body by a Havok constraint, such as a hinged gate, is held the same way: the hold asks for the velocities that carry the grab point to the hand, and the physics step removes what the joint forbids, so a gate swings about its hinge and a drawer slides along its runner. A placed constraint joins objects found by id, not by type ([item-pickup.md](../../engine/gameplay/item-pickup.md#joined-bodies)), so a joined rigid object is reached and held like any loose prop; a joined object of another type is not a target. A vehicle's loosened door is a part of the vehicle, whose body a query reports as the vehicle, and vehicles are never targets, so it is not held. Which world props carry a joint is unverified.

### The player's own body

The held object's centre is kept out of a cylinder around the player's body (`pickup.keep_out_radius_m`, the player's height tall): a target inside it is moved out horizontally, so a held crate is not driven into the player's capsule, where it would shove the player. The object and the capsule still collide; making the held body ignore the player would need its collision filter word changed on the live world and restored on every release, which the mod does not do.

A holding hand does not probe, push off surfaces, or strike, and the other palm's probes pass through what the hands hold, so a free hand never pushes the body off the carried object. The reach rays do find a held object, so that the second hand can take hold of it. The holding hand still counts as acting for the melee gesture, so a swing with something in the hand never starts the game's melee lunge.

### Letting go and throwing

Releasing the grip lets go. The last hand to let go of an object one hand lifts throws it: the grab point leaves with the palm's velocity at that point times `pickup.throw_scale`, capped at `pickup.max_throw_speed_mps`, and the body keeps its spin, so a flick of the wrist spins it and swings its far end. The palm's velocity is its world-space motion smoothed over a few hundredths of a second, so it includes the player's own movement. A heavier object keeps the velocity the hold gave it.

Every hold is checked each tick before it is driven: the body id must still have a dynamic body, owned by the object it was taken from ([item-pickup.md](../../engine/gameplay/item-pickup.md#a-body-in-the-havok-world)), so a body freed or reused for another object is let go. Holds are dropped without a throw when the palms stand down (a vehicle, a swim, a teleport), when the hand stops being tracked or empty, and when the bindings stop reading the grips; a game menu keeps them.

## Haptics

A palm meeting a surface pulses its hand (`contact_pulse`, weaker for a slower meeting) and a strike pulses it harder (`strike_pulse`, scaled by strength). Taking hold of an object or grabbing a weapon pulses `pickup.grab_pulse`, and letting go `pickup.release_pulse`. While a hand presses or pulls, it pulses `pressure_pulse` every tick in proportion to its load: a shoving palm by the share of its push used (or half its resistance, whichever is more), a holding hand by the share of its pull or twist used, and nothing below a tenth. They are routed through the weapon haptics as a collision, a melee strike, and a pickup, and are behind `haptics` and the haptics master switch.

## Threading

The strikes, the shoves and knocks, and the held objects' velocities change the main thread's engine state. The input tick notes its thread; the palms act only when the aim update runs on that thread, and otherwise stand down and count the tick in the readout. The push is read by the locomotion task's post-call and the clamp by every caller of the active hands, through the physical hands' own lock, never the VR runtime's. Both go stale after a fifth of a second without a tick. The pickup grabs and releases are read on the input tick and acted on at the next aim update, through the same lock. The shoves and holds read and write the Havok motion directly, between physics steps, as the engine's own melee impulse does; they would be unsafe from inside the step or from a Havok callback, and are never made there.

## Settings and readouts

The settings are `physical_hands` in the controller settings, under "Physical hands" in the debug UI's Controllers tab, with the readout (each palm's press, push, speed, and kind, its shove's impulse, load, and resistance, what it reaches or holds with the mass, the lag, and the pull and twist used, the last strike, and the counters) beside the other subsystems. The shove settings are `physical_hands.shove` and the pickup settings `physical_hands.pickup`. `show_boxes` draws the palm boxes in the world-space overlay, coloured by whether they press into the world or an object, with a line along each shove, longer for a harder push and redder for a body that resists. With `pickup.show_targets`, a disc marks each hand's grab point and a line joins it to where the hand would have it, redder the harder the hand pulls.

## Unverified in the headset

- That the aim update runs on the input tick's thread. The readout's off-thread count says.
- That the aim's ray filter hits the characters' hit proxies at palm range, so that a strike lands.
- The strength of the hit reaction for a given impulse, and so the defaults of the impulse shares: the reaction's thresholds are per-character data.
- Which stumble act moves the character which way.
- Whether holding the hand at the surface reads well with the render-time late update.
- How the push feels against the roomscale chase when the player leans into a wall.
- That the aim's ray filter hits loose props and dropped weapons at all, and their bodies' masses and inertias: the strength defaults are guesses.
- That the simulation's gravity is the default `pickup.gravity_mps2`; a mismatch shows as a held object that sags or rises at rest.
- That one physics step runs between two aim updates; several steps per update would integrate the set velocities several times, and a held object would overshoot.
- That the Havok solver respects the set velocities against contacts and joints as expected: a held crate stops at a wall, a hinged gate swings about its hinge.
- Which world props are joined by a placed constraint.
- That a held object kept out of the body's cylinder never shoves the player's capsule, and whether the cylinder's default radius fits the capsule.
- Whether the shove's resistance settles without the body chattering between pushed and not, and whether its strength default moves a car at all against its brakes.
- Which weapon the game picks up when several are in range, and that the use-item click alone starts the pickup.
- Whether the hold's defaults hold a crate steadily at the game's simulation rate, and how a throw feels at `throw_scale` one.

## Headset checklist

1. Lay a palm flat on a wall at arm's length: the hand rests on the surface, does not pass through, and the body does not drift.
2. Push the wall: the body (and the view) moves back, settling with the palm on the wall. Push the floor or a table top: nothing moves.
3. Slap a civilian with an open hand at speed: the readout shows a slap and a small damage, and the hand pulses.
4. Punch a soldier with a closed fist: damage near the game's melee, a stumble at full speed, and a ragdoll for a hard enough hit if the reaction allows.
5. Swing through a character: one strike, not one per tick.
6. Push a crate slowly across the floor: it slides with the hand, the hand stays on its face, the body does not move, and the hand buzzes lightly. Push a barrel at its side: it rolls. Push a tall object high up: it tips.
7. Push the crate into a wall and keep pushing: the readout's resistance climbs, the hand stops at the crate, and the body is pushed back as off a wall. Push a parked car: it barely moves, and the body is pushed back.
8. Hit a crate fast: it is knocked harder than a push.
9. Draw a gun: that hand's palm stops (the readout says not acting) and the melee gesture works with it again; the other, empty hand still collides.
10. In a vehicle, swimming, and under the parachute: no palm acts.
11. With `show_boxes`, the boxes sit over the palms, turn orange against walls and green against objects, and a shove line grows with the push.
12. Reach toward a small crate's corner: a blue ring marks it. Grip: the crate stays where it was in the hand, held by that corner; turn the wrist and it turns about the corner. Carry it into a wall: it stops, and the hand stays on it. Release it while swinging: it flies along the swing, spinning with the flick.
13. Grip a long plank by one end: it droops and swings from the hand. Grip the other end with the other hand: it comes level between the hands; raise one hand and it tilts.
14. Grip a heavy object with one hand: a red ring before the grab; it does not lift but drags along the ground. Grip it with both: it lifts if both hands are strong enough. Push it toward a wall: the body is pushed back.
15. Grip a hinged gate or door if one exists: it swings about its hinge as the hand moves.
16. Hold a crate against the chest: it stays in front of the body rather than shoving the player.
17. Kill a soldier, reach for the dropped weapon: a gold ring. Grip: the pickup animation plays and the weapon is equipped. Walk over another of the same type: the ammunition is collected without a grab.
18. Hold a crate and touch it with the other palm: no push off it, and the other hand can grip it too.
19. Reach toward a holster with a crate beside it: the holster draws. Open the pause menu while holding a crate: it is still held after resuming; enter a vehicle while holding: it drops.
