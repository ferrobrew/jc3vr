# The grip

The grip takes things: a gun from a holster or a rack, a grenade from its pouch, a prop off the ground, the steering wheel, a door's handle, a wall to climb. This doc is the one description of how a hand's grip decides what it takes, when it takes it, what letting go does, and the markers that show it. The subsystems that act on what was taken describe their own side ([physical-weapons.md](physical-weapons.md), [physical-hands.md](physical-hands.md), [steering-wheel.md](steering-wheel.md), and the others linked below). The code is `payload/src/controllers/grab/`.

## Decisions

- Hold to hold, everywhere. A gun, a prop, a grenade, a control, or anything else stays in the hand only while the grip is held. Letting go drops it with the hand's velocity, or docks it in the socket that fits it when the hand is over one (a rifle on the back, a pistol on the hip).
- One targeting rule. Of everything a hand is within reach of, the target whose grab point is nearest the hand wins, whatever subsystem placed it: the sockets and pouches on the body, objects in the world, weapons (in the world, on racks and in lockers, in characters' hands, docked on the body), vehicle controls, doors, characters, and surfaces to climb.
- Targeting runs continuously while the grip is held. A hand closed in the air and then laid on a handle takes the handle.
- An empty grip in the air is a fist and fires no game action. A grip on a surface the hand touches climbs it. The grapple is on the off-hand's upper face button, not the grip ([controllers-and-roomscale.md, "On foot"](controllers-and-roomscale.md#on-foot)).
- One exception, for now: in a vehicle and in the wingsuit the grips fire the vehicle's and the suit's weapons, which have no other input yet. A grip there fires its weapon only when the press takes nothing, so a hand on the wheel or a handle never fires, and nothing becomes unreachable. Where those weapons go is to be decided.
- Every target has a marker in its kind's colour, and the one a squeeze would take is highlighted before the squeeze. The markers are anchored at render time.

## Targets

Everything a hand can take is one `GripTarget` (`grab/target.rs`):

- what a squeeze takes, a `GrabVolume`, which also names the subsystem that acts on the grab (`GrabVolume::consumer`);
- the one hand that may take it, or either;
- its tier: a target, a fallback, or a touch target (below);
- its reach: a grab point in the cockpit frame with a radius around it, reached by one of the hand's two points, or a distance the providing subsystem measured itself;
- the hand that holds it now, if one does;
- its marker: where it is drawn, the surface it lies on, its size, its style, and when it shows.

A hand has two points. The sockets and pouches on the player's own body are reached by the player's own grip, before the body-scale remap, so a hand on the player's own belt finds the pouch on it whatever the character's proportions. Everything else (a held gun's parts, what lies on the character, the world, the cockpit) is reached by the hand as it is drawn.

The player's body is where the headset puts it, not where the character's animated head is: the cockpit frame's origin follows that head, which sits ahead of the player whenever the animation leans or the roomscale chase trails. The shoulders, the chest, and their sockets and pouches hang from the neck pivot (from the torso estimate, or below and behind the eyes by the head's own pose without one), facing the estimated torso; the hips and the belt's pouch hang over the feet's estimate, which stays put through a lean or a reach, so they do not swing out with a lean ([controllers.md](controllers.md#holsters-pouches-the-foregrip-and-the-action-runs-tier), [alignment.md](../body/alignment.md)).

The providers place their targets on every input tick:

| Provider | Targets | Consumer |
| --- | --- | --- |
| The weapon handling (`weapons/begin/`) | The holsters, the grenade, C4, and reload pouches (each with a reach margin, and only while it has something to give: a grenade, room for another charge, or a round in reserve), a long gun docked on a shoulder where the hand touches it, the carried weapons' sockets, the held gun's grip (to take it over), foregrip, action, and magazine well, a braced foregrip as held, the guns in characters' hands, on racks, and in lockers (each at the point of the gun nearest the hand), the held grenade's pin, a live grenade, the surface a held charge would stick to, and a magazine in the world | `weapons::finish_tick` |
| The vehicle handling (`vehicles/begin.rs`) | The driver's controls (one grab point per hand on each), the cockpit's buttons, the parachute's handles (each with a reach margin), and the mounted gun's handles, with what a hand holds as held | `vehicles::finish_tick` |
| The physical hands (`physical_hands/targets.rs`) | What each empty hand's reach found (an object, a hinged object, a weapon, a door, a character), every door near the hand by its handle, what a hand holds, a surface to climb, and the helicopter the player hangs from | `pickup::finish_tick`, `climb::finish_tick`, `stunt::finish_tick` |

Each subsystem only provides and consumes. The input tick (`bindings::input_tick`) gathers the targets, resolves the hands among them, steps the grips, and hands each grab to the subsystem its target names.

## Resolving a hand

`grab::resolve` finds, for each hand, every target it may take and is within reach of, and the one its grip would take:

1. A touch target is never taken. It is worked by the hand without a squeeze (a cockpit button pressed by the palm, a gun's magazine well that a round goes into on contact), and is in the list only for its marker.
2. Of the targets in reach, the nearest grab point wins. Equally near ones go to the first placed, and each provider places what a hand holds first, so a hand keeps what it holds.
3. With no target in reach, the nearest fallback wins. The fallbacks are a surface to climb, the helicopter to hang from, and the reach margins around the pouches and the parachute's handles. A hand that reaches for anything resting on a surface touches that surface too, so a surface ranks below everything a hand reaches for on purpose; and a hand feeling for a pouch lands a little off it more often than not, so a press just outside a pouch still takes from it when nothing else is near.

The physical hands' reach is measured by their own casts on the simulation tick, so its targets come with their distance and are in reach whenever they are offered. A surface to climb is offered only where the reach found one: a hand that merely presses into something the reach did not find offers nothing, since a grip there would have nothing to hold.

## The grip's press

Each hand's grip runs a small state machine (`grab/latch.rs`, `GripLatch`), stepped by the binding tables every input tick:

- Open. A press takes the target the hand would take. With nothing in reach, a press on a grip that a game action of the mode reads drives that action for the whole press. Otherwise the hand closes into a fist.
- Fist. The fist takes the first target the hand comes to within 0.2 s of closing, however fast the hand moves, so a hand that closes a moment before it reaches a handle still takes it; after that, only while the hand moves slower than 1.2 m/s. A fist that stays closed and moves fast stays a fist: a punch does not take what it strikes (the hand bodies deal it as a blow), and a fist swung past the hip draws nothing.
- Holding. The press took a target, and holds it until the grip opens. While it holds, no game action reads the grip.
- Open again. The grip's release lets go of whatever the press took: each holder reads the grip's release itself.

A press and release between two ticks is a fresh press even while the last one is still latched, so a quick tap reaches the target in reach at that moment.

## Letting go

What letting go does is the holder's, with one shared rule for docking (`grab::resolve::dock_target`): of the targets the opening hand is within reach of, the nearest that fits what it holds takes it. A gun goes onto a free socket that fits it or its own holster, whichever is nearer under the hand; the second weapon goes onto any socket or back to its own holster; anything else falls with the hand's velocity. A hand over both a pouch and the gun's own holster puts the gun in the holster, because a pouch does not fit a gun.

A grenade or a charge is let go when the fingers open, not when the press ends (`weapons/opening.rs`): a throwing hand relaxes its squeeze before the fingers leave the handle, so a release read from the press drops the grenade mid-throw. The fingers are read from the grip's value, the Index controller's capacitive sense or the Touch controller's pull, closed above 0.6 and open below 0.35, and a controller without a value falls back to the press. The throw's velocity comes from the fastest moment of the last quarter second before the fingers opened ([controllers.md, "Grenades, aim assist, and reloads"](controllers.md#grenades-aim-assist-and-reloads)), and the release is logged at info level with its speed. While a hand holds a grenade or a charge, the fire input is held back unless the other hand holds a gun, since with nothing wielded it would draw one into the hand with the explosive.

A charge is the same whether it came from the chest pouch or the weapon wheel: choosing the explosives on the wheel puts the game in its planting mode, which the mod leaves at once for a charge in a free hand. Held near a surface, a red ring marks where it would stick; pressed onto the surface it sticks there, and let go in the air it is thrown ([physical-weapons.md, "C4 pressed onto a surface"](physical-weapons.md#c4-pressed-onto-a-surface)).

## Markers

One renderer draws every target (`grab/markers.rs`), submitted to the overlay at the input tick's rate:

| Kind | Colour | Targets |
| --- | --- | --- |
| Object | white | loose and hinged objects, a magazine in the world |
| Weapon | gold | a weapon in the world, on a rack, or in a character's hands, and the held gun's grip, foregrip, action, and well |
| Body | blue | the holsters, the sockets, and the pouches |
| Vehicle | green | the driver's controls, the cockpit's buttons, the doors, the parachute's and the mounted gun's handles, and the helicopter to hang from |
| Climb | orange | a surface to climb |
| Character | violet | a character |
| Explosive | red | a live grenade, the held grenade's pin, the surface a held charge would stick to |

A marker shows as its target says: always, faintly while no hand is within reach (the sockets and pouches on the body, the vehicle's controls and buttons, the parachute's handles); only while a hand is within reach (everything in the world and on a gun); or never (a reach margin, which its pouch or handle already marks). It is brighter while a hand is within reach of it, brightest and a little larger on the target a squeeze would take, and solid while a hand holds it. Its styles are a ring around the grab point, a handle (a dot, ringed once a hand is near), and a button (a disc on its face, ringed while a hand is in front of it). A marker on something in the world lies on its surface where the reach knows the surface's normal, and one on a weapon in the world sits where the hand would take it.

The markers are anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)): a marker in the cockpit frame (the body's sockets, the vehicle's controls) to the render-time head, one on a held gun to its hand's render-time grip, and only what lies in the world to the world. A socket on the player's own body is drawn shifted by the remap of the hand that reaches for it, so the drawn hand meets the drawn marker where the grab happens.

`overlay.markers` (on; the pause menu's `GRIP MARKERS`) is the one switch for every marker. It replaces the separate highlight switches the weapon, vehicle, and pickup handling had.

## Readouts and logs

The `VR: STATUS` page's `GRIP` section has a row per hand, saying what the hand holds, or what a squeeze would take and how far its grab point is, or that it is a fist; and a row for the markers. The grab view ([grab-view.md](grab-view.md)) draws the same in the headset, around each hand: the reach's rays and what they hit, a line to each target near the hand coloured by whether it is out of reach, in reach, chosen, or held, the palms' state on the wrist, a flash where a grab was refused, and the things near the hands a hand might take. Each grip press is logged at info (target `controllers`, `grip: pressed`), as is what a fist takes (`grip: the closed hand came to a target and took it`), with what the hand was within reach of, what it took, why each other target lost (farther by how much, a fallback, or worked by the palm), the nearest target it was out of reach of when it took nothing, whether the hand was slow, and the physical hands' notes on what their reach found: each body it hit, the kind of object (with its class's virtual table for anything the reach does not know), and why it was not taken (the player stands on it, it is static, it is too heavy, a weapon that is not lying dropped). The line's `palms` field says what the palms' last tick did for that hand: why the palms stood down, or how long ago they ticked, whether the hand was free, driven by its body and whether that body's pose was fresh, had a palm, and reached anything, with the tick's casts and the last cast failure; a grab that the notes leave empty is explained by it.

## Adding a grab target

A new kind of target is a provider and a consumer:

1. Add the `GrabVolume` variant, with its label, its category (the marker's colour), and its consumer. A new subsystem that acts on grabs adds a `Consumer` and is handed its grabs in `bindings::input_tick` with `grabs_for`.
2. In the providing subsystem's begin tick, place a `GripTarget` for it each input tick: a sphere around its grab point (`GripTarget::sphere`, or `Candidate::target` from a placement), or a measured distance (`GripTarget::measured`), with the marker it shows (`Mark`). Place what a hand holds as held (`GripTarget::held`), first. A reach margin is the same target with a larger radius, ranked a fallback and never shown.
3. In the consumer's finish tick, act on a grab of the variant, and read the grip's release to let go.
4. Add the variant to `every_kind` in `grab/tests.rs`, which checks that every kind goes through the resolver and the renderer.

Nothing else draws a marker, logs a press, or decides between two subsystems' targets.

## Settings

`overlay.markers` switches the markers. The reach margins are `weapons.pouches.reach_margin_m` and `vehicles.parachute_handles.reach_m`. The fist's speed is fixed (`grab::tick::SLOW_HAND_MPS`).

## In the headset

1. Look down with `GRIP MARKERS` on: the holsters and pouches show faint blue rings on the body, at the hips, shoulders, chest, and belt, where the hands reach for them. Move a hand toward one: it brightens, and the one a squeeze would take is brightest. `VR: STATUS` names it in `LEFT GRIP` or `RIGHT GRIP`.
2. Close a hand in the air, then lay it on the grenade pouch: the grenade comes into the hand. Swing a closed fist through a crate: the crate is struck, not taken.
3. Hold a hand over a pouch and the hip holster at once: the nearer marker is the brightest, and a squeeze takes that one.
4. Draw a rifle from the back and let go over the back: it docks. Let go in front of the body: it falls.
5. Reach for a crate on the ground with the palm on the ground beside it: the crate's white marker is highlighted, not the ground's orange one. Lay the palm on a bare wall: the wall's orange marker is highlighted, and a squeeze climbs.
6. Sit in a car: green markers sit on the wheel's rim near each hand and on the nitrous button. Grip the rim: the marker goes solid. Stand beside a parked car: a green marker sits on each door's handle once a hand comes within 15 cm of it, and a squeeze takes the door.
7. Press a grip near a barrel and read the log's `grip: pressed` line: its notes say what the reach hit and why it did or did not take it.
8. Lean over a table without stepping: the hip holsters and the belt's pouch stay at the hips, and the chest's pouches lean with the shoulders. Stand half a metre behind where Rico's head is drawn (the chase trailing): the markers sit on the player's own body.
9. Throw every grenade: the grenade pouch's marker goes, and comes back with a grenade picked up. Take a gun from a soldier's hands, and one from a locker: the gold marker sits on the gun where the hand would close.
10. Take a grenade and throw it overhand, relaxing the squeeze through the swing: it leaves the hand only as the fingers open, and the log's `grenade: the fingers opened` line shows a `speed_mps` near the hand's peak. Pull the trigger of the hand holding it with no gun drawn: no pistol comes out.
11. Choose C4 on the weapon wheel with a hand empty: the planting mode ends and a charge is in the dominant hand. Bring it near a wall: a red ring lies on the wall where it would stick; press it on: it sticks. Punch a civilian solidly: they stagger; swing hard: they fall.
