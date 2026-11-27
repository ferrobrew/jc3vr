# The grip

The grip takes things: a gun from a socket, a rack, or a soldier's hands, a grenade from its pouch, a prop off the ground, the steering wheel, a door's handle, a wall to climb. This page covers how a hand's grip decides what it takes, when, what letting go does, the markers that show it, and the finger poses a held thing gets. The subsystems that act on what was taken describe their own side. The code is `crates/jc3vr-body/src/controllers/grab/` and `fingers/grip/`, tuned by `grab.toml` and `fingers.toml` in `crates/jc3vr-body/tuning/`.

The model is hold to hold everywhere: whatever a hand takes stays only while the hand stays closed, and letting go drops it with the hand's velocity or docks it in the socket under the hand. Of everything a hand can reach, the nearest grab point wins, whichever subsystem placed it, and every target has a marker. An empty grip in the air is a fist and fires no game action; on a surface it climbs. The grapple is on a face button, not the grip.

## Targets

Everything a hand can take is one `GripTarget` (`grab/target.rs`): the `GrabVolume` a squeeze takes (which names its consumer subsystem), the hand allowed to take it, its `Tier`, its reach (a grab point with a radius, or a distance the provider measured), the hand holding it now, and its marker. Providers place their targets every input tick:

- The weapon handling (`controllers::weapons::begin` in the weapons crate): the sockets and pouches (each only while it has something to give), the held gun's parts, held grenades and charges, guns in characters' hands, on racks, and in lockers, grenades and charges in the world, and magazines.
- The vehicle handling (`vehicles::begin`): the driver's controls, the cockpit buttons, the parachute's handles and ripcord, and the mounted gun's handles.
- The physical hands (`physical_hands::targets`): what each empty hand's reach found (an object, a weapon, a door, a character), vehicle door handles, a surface to climb, and the helicopter to hang from.
- The snatch ([grapple-snatch.md](grapple-snatch.md)) and the line grab ([grapple-lock.md](grapple-lock.md#the-free-hand-on-the-line)).

A hand reaches every target as it is drawn: the sockets and pouches ride Rico's skeleton, so what the player sees on the body is where they reach ([rig.md](../body/rig.md#the-bodys-volumes-on-ricos-skeleton)). Each subsystem only provides and consumes. The gameplay crate's binding tick gathers the targets, resolves the hands (`bindings::tick::hands::offers`), steps the grips, and hands each grab to its consumer (`grab::grabs_for`).

To add a kind of target: add the `GrabVolume` variant with its label, category, and consumer; place a `GripTarget` for it in the provider's begin tick (placing a held one first, with `GripTarget::held`); act on it in the consumer's finish tick and read the hand's hold to let go; and add it to `every_kind` in `grab/tests.rs`. Nothing else draws a marker, logs a press, or arbitrates between subsystems.

## Resolving a hand

`grab::resolve` finds each hand's targets in reach and the one its grip would take:

1. A touch target (`Tier::Touch`) is never taken: the hand works it on contact (a cockpit button, a magazine well, the surface a charge sticks to). It is listed only for its marker.
2. A handle target (`Tier::Handle`) in reach wins outright: a mounted gun's handle, which sits on the gun's body, and a held grenade's pin ([grenades.md](grenades.md#the-pin)), which sits on the grenade.
3. Otherwise the nearest grab point wins, except that a character loses to a gun in reach, so a hand by a soldier's gun takes the gun. Ties go to the first placed, and providers place what a hand holds first, so a hand keeps what it holds.
4. With nothing in reach, the nearest fallback (`Tier::Fallback`) wins: a surface to climb, the helicopter, or the reach margin round a pouch or a parachute handle. A surface ranks below everything, because a hand reaching for anything on a surface touches it too.
5. With nothing else, a remote target (`Tier::Remote`), the snatch's, offered only to the empty grapple hand.
6. A hand that finds nothing takes the body slot it was last in reach of within `reach.recall_s`, because a hand often pulls away from a socket as the press lands, and the Index's force press lands a few frames after the fingers close.

A grenade, magazine, or charge on the floor is also reached from the controller once it is well below the drawn hand (`grab::resolve::reach_low`), because Rico does not crouch as low as the player.

## The grip's press

Each hand's grip is a state machine (`grab/latch.rs`, `GripLatch`), stepped every input tick:

- Open: a press takes the target in reach, or with nothing in reach drives the mode's game action on the grip for the whole press, or else makes a fist.
- Fist: takes the first target it comes to within `reach.fist_window_s` of closing, and after that only while the hand moves slower than `reach.slow_hand_mps`, so a punch strikes rather than takes and a fist swung past the hip draws nothing. The grapple hand's fist on a line never takes a socket, since the body swinging under it moves the hand against the room.
- Holding: holds until the hand opens; no game action reads the grip meanwhile.

A hand is closed from a press until it opens (`intent::controls::ClosedHand`). On a Touch it opens when the press ends. The Index presses on force, and a hand wrapped round a handle relaxes that force toward nothing, so there the hand stays closed while the capacitive squeeze stays above `press.squeeze.release` and opens when the fingers come off. Only a press closes the hand. Every hold reads this rule. A press and release between two ticks still counts as a fresh press. While a snatch's line is out, the other free hand's grip is held for the catch ([grapple-snatch.md](grapple-snatch.md#the-interaction)).

## Letting go

What letting go does is the holder's, with one docking rule (`grab::resolve::dock_target`): the nearest target in reach that fits what the hand holds takes it, so a weapon goes onto the socket the loadout table allows ([two-handed-inventory.md](two-handed-inventory.md#the-grabs-against-the-table)), and anything else leaves with the hand's velocity. A grenade or charge is let go when the fingers open, read from the grip value (`opening.closed` and `opening.open`), not when the press ends, because a throwing hand relaxes its squeeze before the fingers leave ([grenades.md](grenades.md)).

## Markers

One renderer draws every target (`grab/markers.rs`) into the overlay, coloured by kind (`grab::Category`, colours in `markers.colours`): white for objects, gold for weapons and gun parts, blue for the body's sockets and pouches, green for vehicle controls, doors, and the helicopter, orange for climbing, violet for characters, and red for explosives. A marker shows always and faintly (the body's slots, the cockpit, the parachute's handles), only with a hand in reach (the world and the gun), or never (a reach margin). It brightens with a hand in reach, is brightest on the target a squeeze would take, and is solid while held; a refusal style marks a slot that will not take what the hand holds. Markers are anchored at render time ([render-time-anchoring.md](../rendering/render-time-anchoring.md)). `controllers.overlay.markers` (the pause menu's `GRIP MARKERS`) switches them all.

## The grab hint

The body's slots are out of sight when they matter, over the shoulder or at the hip, so while a hand is in reach of one, the game's icon of what its grip would take shows on the floating panel's plane below its centre, where the flat game shows its weapon selection (`hud::grab_hint`, published by `controllers::weapons::begin::hint`). A socket shows its docked weapon, a grenade or C4 pouch the weapon in that slot, the magazine pouch the gun in the other hand, and a held weapon over a taken back socket the swap ([weapon-sockets.md](weapon-sockets.md#the-swap)), following the release's own choice. A label names the place. The icons are the game's own ([ui-rendering.md](../../engine/rendering/ui-rendering.md#weapon-icons)); a hint never falls back to text.

## Grip poses

A hand that holds something wraps its fingers round it instead of the generic curl ([hands-and-aiming.md, "Finger curls"](hands-and-aiming.md#finger-curls)): a pistol grip with the finger on the trigger, a magazine pinched, a grenade in the palm, a hook over a ledge, a palm flat on a crate, a wrap sized to a rim or a rope, a door handle, a fistful of collar.

Each system declares what a hand holds on each of its ticks, with `fingers::grip::declare(hand, Grip::new(source))`. The `GripSource` is a constant in the owning system's `grip_sources` module naming its default pose and firmness. A declaration expires after a few sim ticks, so a hold that ends lets the hand go without a release call, and the declarations sit behind a leaf lock any thread may take. When two sources declare for one hand, the earlier in `GRIP_ORDER` (`controllers::register_hands`) wins: what the fingers hold over what the hand holds on to, and a palm on a surface last. A held source (a weapon, a vehicle control, the grapple line) takes the whole pose; a grip source (the physical hands, climbing) closes onto the pose as the squeeze grows.

Each pose (`catalogue.rs`, values in the tuning's `poses`) gives each finger a curl, splay, and hook (the bend moved between the knuckle and the outer joints) and the thumb an opposition. A bar's pose follows its diameter. A hand on a long gun takes the pose of the part it holds (`GunRegion::finger_pose`, [physical-guns.md](physical-guns.md#grabbing-a-gun-anywhere)). The pistol grip's index finger follows the trigger while on it and the measured curl when lifted, and the thumb on a bar keeps the measured curl under hand tracking. The pose values are judged by eye. The shape blends from the measured curls toward the pose (`blend.rs`), so taking hold closes the fingers and letting go opens them.

While holding a body (an object, a door, a ragdoll), the contact fit (`contact.rs`) sweeps each finger from straight toward a fist with casts along its joints' chords, and stops it where it meets the held body, ignoring other hits.

## Readouts and logs

The Status page's `Grip` section has `Left grip` and `Right grip`: what the hand holds, what a squeeze would take and how far, or that it is a fist. The grab view ([hand-views.md](hand-views.md#the-grab-view)) draws the same around each hand in the headset. The `Fingers` section has `Left grip pose` and `Right grip pose`.

Each press is logged at info (target `controllers`, `grip: pressed`), as is a fist's take, with what the hand could reach, what it took, and why each other target lost. A change of a hand's grip source logs `fingers: the hand's grip source changed`, throttled per hand.
