# The grip

The grip takes things: a gun from a socket, a rack, or a soldier's hands, a grenade from its pouch, a prop off the ground, the steering wheel, a door's handle, a wall to climb. This doc describes how a hand's grip decides what it takes, when it takes it, what letting go does, and the markers that show it. The subsystems that act on what was taken describe their own side ([physical-weapons.md](physical-weapons.md), [physical-hands.md](physical-hands.md), [steering-wheel.md](steering-wheel.md), and the others linked below). The code is `crates/jc3vr-body/src/controllers/grab/`, with its tuning in `crates/jc3vr-body/tuning/grab.toml` ([tuning.md](../tuning.md)).

## Decisions

- Hold to hold, everywhere. Whatever the hand takes stays in it only while the hand stays closed ([the grip's press](#the-grips-press)). Letting go drops it with the hand's velocity, or docks it in the socket that fits it when the hand is over one.
- One targeting rule. Of everything a hand is within reach of, the target whose grab point is nearest the hand wins, whatever subsystem placed it.
- Targeting runs continuously while the grip is held, so a hand closed in the air and then laid on a handle takes the handle.
- An empty grip in the air is a fist and fires no game action. A grip on a surface the hand touches climbs it. The grapple is on a face button, not the grip, and in a vehicle or the wingsuit the grips fire nothing either.
- Every target has a marker in its kind's colour, and the one a squeeze would take is highlighted before the squeeze.

## Targets

Everything a hand can take is one `GripTarget` (`grab/target.rs`): what a squeeze takes (a `GrabVolume`, which also names the subsystem that acts on it, `GrabVolume::consumer`), the one hand that may take it or either, its `Tier`, its reach (a grab point in the cockpit frame with a radius, or a distance the provider measured itself), the hand that holds it now, and its marker.

A hand reaches every target as it is drawn. The sockets and pouches ride Rico's skeleton, the body the player sees, through a crouch, a lean, a fall, or the parachute, so what the player sees on the body is where they reach ([rig.md](../body/rig.md#the-bodys-volumes-on-ricos-skeleton)). Where Rico's body and the player's disagree, the rig closes the gap rather than the volumes following the player.

The providers place their targets every input tick:

| Provider | Targets | Consumer |
| --- | --- | --- |
| The weapon handling (the weapons crate's `controllers::weapons::begin`) | The sockets and the grenade, C4, and magazine pouches (each only while it has something to give, and each with a reach margin), the held gun's grip (for the other hand to take it over), foregrip, action, inserted magazine, and well, the held grenade and its pin, the held charge, a grenade display at a locker, the guns in characters' hands, on racks, and in lockers, a live or dropped grenade, the surface a held charge would stick to, a placed charge, and a magazine in the world | `Weapons` |
| The vehicle handling (`vehicles::begin`) | The driver's controls, the cockpit's buttons, the parachute's handles (with a reach margin), the ripcord, and the mounted gun's handles | `Vehicles` |
| The physical hands (`physical_hands::targets`) | What each empty hand's reach found (a loose or hinged object, a weapon, a door, a character), every vehicle door near the hand by its handle, a surface to climb, and the helicopter the player hangs from | `Pickup`, `Climb`, `Stunt` |
| The snatch (the grapple crate's `controllers::snatch`) | The thing out of reach under the grapple's aim ([grapple-snatch.md](grapple-snatch.md)) | `Snatch` |
| The free hand on the line (`grapple::line_grab`) | The held lock's or the swinging snatch's line ahead of the grapple hand ([grapple-line-grab.md](grapple-line-grab.md)) | `Line` |

Each subsystem only provides and consumes. The gameplay crate's binding tick gathers the targets and resolves the hands among them (`bindings::tick::hands::offers`), steps the grips, and hands each grab to the subsystem its target names (`grab::grabs_for`, in `bindings::tick::acts`).

## Resolving a hand

`grab::resolve` finds, for each hand, every target it may take and is within reach of, and the one its grip would take:

1. A touch target (`Tier::Touch`) is never taken. The hand works it without a squeeze (a cockpit button pressed by the palm, a magazine well a round goes into on contact, the surface a charge sticks to), and it is in the list only for its marker.
2. A handle target (`Tier::Handle`) in reach wins over every other target. These are a mounted gun's handle, which sits on the gun's own body so that the nearest grab point would hand the grip to the body by a few millimetres, and a held grenade's pin, which sits on the grenade the other hand would otherwise take ([grenades.md, "The pin"](grenades.md#the-pin)).
3. Otherwise, of the targets in reach, the nearest grab point wins. A character loses to a gun in reach however much nearer the character is, so a hand closing by a gun in a soldier's hands takes the gun. Equally near targets go to the first placed, and each provider places what a hand holds first, so a hand keeps what it holds.
4. With no target in reach, the nearest fallback (`Tier::Fallback`) wins: a surface to climb, the helicopter to hang from, or the reach margin around a pouch or a parachute handle. A hand reaching for anything on a surface touches that surface too, so a surface ranks below everything a hand reaches for on purpose; and a hand feeling for a pouch lands a little off it more often than not.
5. With nothing at all in reach, a remote target (`Tier::Remote`) wins: the snatch's target, offered only to the empty grapple hand on foot with the hook free. A fist never takes it; only a press aimed at it arms the snatch.
6. A hand with nothing in reach, or only a remote target, takes the slot on the body it was last in reach of within `reach.recall_s`, if that slot is still offered to it (`grab::resolve::recall`). A hand reaching for a socket or a pouch often starts to pull away as the press lands, and on a force-sensing grip the press lands a few frames after the fingers close. The press's log notes `recalled the ... the hand left ... ms ago`.

One exception reaches from the controller rather than the drawn hand (`grab::resolve::reach_low`): a grenade, a magazine, or a placed charge lying on the floor, offered to one hand, is also reached from that hand's controller grip once the controller is more than `reach.low_reach_m` below the drawn grip and the drawn grip is out of reach. Rico's crouch does not go as low as the player's, so a player crouched with the controller at the floor would otherwise miss.

The physical hands' reach is measured by their own casts on the simulation tick, so its targets come with their distance and are in reach whenever they are offered. A surface to climb is offered only where the reach found one: a hand that merely presses into something the reach did not find offers nothing, since a grip there would have nothing to hold.

## The grip's press

Each hand's grip runs a small state machine (`grab/latch.rs`, `GripLatch`), stepped every input tick:

- Open. A press takes the target the hand would take. With nothing in reach, a press on a grip that a game action of the mode reads drives that action for the whole press. Otherwise the hand closes into a fist.
- Fist. The fist takes the first target the hand comes to within `reach.fist_window_s` of closing, however fast the hand moves, so a hand that closes a moment before it reaches a handle still takes it. After that it takes only while the hand moves slower than `reach.slow_hand_mps`. A fist that stays closed and moves fast stays a fist: a punch does not take what it strikes (the hand bodies deal it as a blow), and a fist swung past the hip draws nothing. The grapple hand's slow fist, while its line is out or reeling, never takes a socket or a pouch: the hand's speed is measured against the room, and the body swinging under a fist closed on the line would otherwise draw a holstered gun.
- Holding. The press took a target and holds it until the hand opens. While it holds, no game action reads the grip.
- Open again. The hand opening lets go of whatever the press took.

A hand is closed from a grip press until it opens (`intent::controls::ClosedHand`, read as `HandControls::hold`). On a Touch controller the hand opens when the press ends. The Index controller's grip presses on its force, a deliberate squeeze, and a hand wrapped round a handle relaxes that force toward nothing while it holds on. So on a force-sensing grip the hand stays closed while the grip's squeeze value (the fingers' capacitive sense) stays at or above the value grips release at (the view tuning's `press.squeeze.release`), and opens when the fingers come off. Only a press closes the hand, so fingers resting round the handle keep a hold but never start one, and a fist or a game action on the grip still ends with the press. Every hold reads this rule: the grab latch, the guns and their parts, the pickups, the climb and stunt holds, the snatch's catch, the vehicle controls, the handles, the ripcord, and the magazine pull.

A press and release between two ticks is a fresh press even while the last one is still latched, so a quick tap reaches the target in reach at that moment.

While a grapple snatch's line is out, the free, empty hand other than the grapple hand has its grip held for the catch: a press closes it into a fist whatever is in reach, drives no game action, and takes nothing it comes to ([grapple-snatch.md, "The interaction"](grapple-snatch.md#the-interaction)).

## Letting go

What letting go does is the holder's, with one shared rule for docking (`grab::resolve::dock_target`): of the targets the opening hand is within reach of, the nearest that fits what it holds takes it. A weapon goes onto the socket under the hand that fits it, as the loadout table decides ([two-handed-inventory.md](two-handed-inventory.md#the-grabs-against-the-table)); anything let go of elsewhere leaves with the hand's velocity. A hand over both a pouch and a socket puts the gun on the socket, because a pouch does not fit a gun.

A grenade or a charge is let go when the fingers open, not when the press ends (the weapons crate's `controllers::weapons::opening`): a throwing hand relaxes its squeeze before the fingers leave the handle, so a release read from the press drops the grenade mid-throw. The fingers are read from the grip's value (the Index controller's capacitive sense, the Touch controller's pull), closed above the handling tuning's `opening.closed` and open below `opening.open`; a controller without a value falls back to the press. The throw itself is [grenades.md](grenades.md)'s and [physical-weapons.md](physical-weapons.md#c4-pressed-onto-a-surface)'s.

## Markers

One renderer draws every target (`grab/markers.rs`), submitted to the overlay at the input tick's rate. Each kind has its colour (`grab::Category`, with the colours in the tuning's `markers.colours`):

| Kind | Colour | Targets |
| --- | --- | --- |
| Object | white | loose and hinged objects |
| Weapon | gold | a weapon in the world, on a rack, or in a character's hands, and the held gun's parts |
| Body | blue | the sockets and the pouches |
| Vehicle | green | the driver's controls, the cockpit's buttons, the doors, the handles, and the helicopter to hang from |
| Climb | orange | a surface to climb |
| Character | violet | a character |
| Explosive | red | a live or dropped grenade, a held grenade's pin, the surface a held charge would stick to |

A marker shows as its target says (`Shown`): always, faintly while no hand is within reach (the slots on the body, the cockpit's controls and buttons, the parachute's handles); only while a hand is within reach (everything in the world and on a gun); or never (a reach margin, which its pouch or handle already marks). It is brighter while a hand is within reach of it, brightest and a little larger on the target a squeeze would take, and solid while a hand holds it. Its styles (`MarkStyle`) are a ring around the grab point, a handle (a dot, ringed once a hand is near), a button (a disc on its face, ringed while a hand is in front of it), and two refusal styles in the no-fit colour: a small disc on a slot of the body that will not take what the hand holds, and a ring on a place such as a magazine well that still has a round in it. A marker with a label (the cockpit's nitrous button, a grenade's pin) has it drawn above it by the HUD while a hand is at it (`hud::mark_labels`). A marker on something in the world lies on its surface where the reach knows the surface's normal.

The markers are anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)): a marker in the cockpit frame to the render-time head, one on a held gun to its hand's render-time grip, and only what lies in the world to the world.

`controllers.overlay.markers` (on; the pause menu's `GRIP MARKERS`) is the one switch for every marker.

## The grab hint

The markers on the body's slots are out of sight when they matter most: a hand reaching over the shoulder or to the hip cannot see the socket or what is docked there. So while a hand is within reach of a slot on the body, the game's own icon of what its grip would take shows in front of the player, on the floating panel's plane below its centre, where the flat game shows its weapon selection (`hud::grab_hint`). The weapon handling's input tick publishes each hand's hint from the grip resolution (the weapons crate's `controllers::weapons::begin::hint`):

- an empty hand within reach of a socket shows the icon of the weapon docked there;
- a hand within reach of the grenade pouch or the C4 pouch shows the icon of the weapon in the player's grenade or planted explosive slot;
- a hand within reach of the belt's magazine pouch shows the icon of the gun in the other hand, the gun the magazine is for;
- a hand holding a weapon inside the attraction of a taken socket on the back, where letting go swaps ([physical-weapons.md](physical-weapons.md#attraction)), shows the docked weapon's icon dimmed, an arrow, and the held weapon's icon. The hint follows the release's own choice, so a hand that is also inside a free socket's attraction, which the release takes first, shows no swap.

The icons are the game's weapon icons, read from its archives and decoded by the mod ([ui-rendering.md](../../engine/rendering/ui-rendering.md#weapon-icons), `hud::game_icons`). They have no card or backing, only a soft dark halo, so the white icons read against a bright sky or sand. Above the middle element a small label names the place, in sentence case: "Left hip", "Right hip", "Left back", "Right back", "Grenade pouch", "C4 pouch", or "Magazine pouch" (`Socket::display_name` and the weapons crate's `hud_feed::grab_hint::Place`). A hint whose icons have not loaded shows nothing; it never falls back to text. Each hand has its own hint: a lone hint is centred, and two move apart toward their own hands' sides. A hint fades in and out briefly and goes when the input tick stops publishing.

## Readouts and logs

The `Grip` section of the debug UI's Status page has a row per hand (`Left grip`, `Right grip`), saying what the hand holds, what a squeeze would take and how far its grab point is, or that it is a fist. The grab view ([grab-view.md](grab-view.md)) draws the same in the headset around each hand.

Each grip press is logged at info (target `controllers`, `grip: pressed`), as is what a fist takes (`grip: the closed hand came to a target and took it`). The line has what the hand was within reach of, what it took, why each other target lost (farther by how much, a fallback, a handle in reach, a character beside a gun, or worked by the palm), the nearest target it missed when it took nothing, whether the hand was slow, and the physical hands' notes on what their reach found and why it was not taken. Its `palms` field says what the palms' last tick did for that hand, which explains a grab the notes leave empty.

## Adding a grab target

A new kind of target is a provider and a consumer:

1. Add the `GrabVolume` variant, with its label, its category (the marker's colour), and its consumer. A new subsystem that acts on grabs adds a `Consumer`, and the binding tick hands it its grabs with `grabs_for`.
2. In the providing subsystem's begin tick, place a `GripTarget` for it each input tick: a sphere around its grab point (`GripTarget::sphere`, or `Candidate::target` from a placement), or a measured distance (`GripTarget::measured`), with the marker it shows (`Mark`). Place what a hand holds as held (`GripTarget::held`), first. A reach margin is the same target with a larger radius, ranked a fallback and never shown.
3. In the consumer's finish tick, act on a grab of the variant, and read the hand's hold to let go.
4. Add the variant to `every_kind` in `grab/tests.rs`, which checks that every kind goes through the resolver and the renderer.

Nothing else draws a marker, logs a press, or decides between two subsystems' targets.

## Tuning

The fist's window and speed, the recall, the low reach, and the markers' sizes, opacities, and colours are the grip model's tuning (`grab.toml`). The pouches' reach margin is the hold tuning's `volumes.pouches.reach_margin_m`, and the parachute handles' is the vehicles tuning's `parachute_handles.reach_m`.

## In the headset

1. Look down: the sockets and pouches show faint blue rings on Rico's hips, shoulders, chest, and belt. Move a hand toward one: it brightens, the one a squeeze would take is brightest, and the Status page names it in `Left grip` or `Right grip`.
2. Close a hand in the air, then lay it on the grenade pouch: the grenade comes into the hand. Swing a closed fist through a crate: the crate is struck, not taken.
3. Draw a rifle from the back and let go over the back: it docks. Let go in front of the body: it falls.
4. Reach for a crate on the ground with the palm on the ground beside it: the crate's white marker is highlighted, not the ground's orange one. Lay the palm on a bare wall: the orange marker is highlighted, and a squeeze climbs.
5. Sit in a car: green markers sit on the wheel's rim near each hand. Grip the rim: the marker goes solid. Stand beside a parked car: a green marker sits on each door's handle once a hand comes near, and a squeeze takes the door.
6. Lean over a table, open the parachute, and step back half a metre so Rico is drawn ahead of you: the markers stay on Rico's body each time, and the drawn hand reaches them there.
7. Take a gun from a soldier's hands and one from a locker: the gold marker sits on the gun where the hand would close.
8. Take a grenade and throw it overhand, relaxing the squeeze through the swing: it leaves the hand only as the fingers open.
9. Drop a grenade with its pin in and crouch as far as you can, the controller at the floor: a squeeze at the grenade takes it.
10. With a rifle on the right shoulder, reach the empty right hand over it without looking: the rifle's icon shows below the middle of the view with "Right back" above it while the hand is within reach. Do the same at a hip, the grenade pouch, the C4 pouch, and, with a gun in the other hand and its magazine out, the magazine pouch.
11. Holding a second rifle, bring it over the taken shoulder: the swap hint shows, and letting go matches it. Reach both hands to two slots at once: the two hints sit apart, each toward its own hand's side, and neither covers the reticle.
12. On Index controllers, take the steering wheel, a rifle, and a crate, and hold each with the hand wrapped round the controller but relaxed: each stays held while the fingers stay round the handle, and opening the hand lets go at once. A relaxed hand laid on something without a squeeze takes nothing.
13. Reach for a socket or the C4 pouch and squeeze as the hand starts to pull away: it still takes, and the press's log notes `recalled the ...`.
