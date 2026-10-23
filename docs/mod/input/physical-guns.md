# Physical guns

With the hand bodies on ([physical-hands.md](physical-hands.md#hand-bodies)), a gun in a hand is a body of its own in the physics world, joined to the hand bodies that hold it. Its weight hangs from the hands and drags them by its mass, it collides along its whole length, a two-handed hold is two joints, and swung into a character it strikes as the hands do. The drawn gun and the drawn hands follow the physics: each hand is drawn at its body, and the gun at its body. With `physical_hands.bodies.guns.enabled` off (it is on by default; the debug UI's Controllers tab, under "Hand bodies"), or with the hand bodies off, a held gun has no body, its hands' bodies leave the world as before, and the kinematic weight ([`heft`](../../../payload/src/controllers/weapons/heft/mod.rs)), recoil offset, two-handed pose, and wall probe ([physical-weapons.md, "Guns against walls"](physical-weapons.md#guns-against-walls)) act instead.

The code is `payload/src/controllers/physical_hands/physics/guns/`. The engine side is in [havok-bodies.md](../../engine/physics/havok-bodies.md) and [weapon-equip-and-drop.md](../../engine/gameplay/weapon-equip-and-drop.md#one-object-for-its-whole-life). None of this has been run in the headset.

## Which body

Each gun has a body of its own ([`WeaponData::m_PfxInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_PfxInstance)), in the world only while the weapon lies dropped or is mounted. Reusing it while the gun is held would mean fighting the weapon's state changes, which add it to and take it out of the world, and joining the hands to a body the game destroys with the weapon whenever it likes; the world does not end a constraint when one of its bodies is destroyed ([havok-bodies.md](../../engine/physics/havok-bodies.md#joining-bodies-with-a-fixed-constraint)). So the gun body is the mod's own, made the way the hand bodies are, with the gun's own shape ([`hknpBody::m_shape`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_shape), which the new body takes a reference of its own to, so the two share it) and the gun's own mass (its own body's motion's inverse mass). That mass is the weapon's pickup's, far heavier than the gun: a rifle's own body weighs ten kilograms, and each pistol of the dual-wield pair five. Against a three-kilogram hand, a gun that heavy swung the hand rather than the other way round, so the gun flailed. The dual-wield pair's two guns share their weapon's mass, and the mass is held between `min_mass_kg` and the weight class's `max_mass_by_class_kg` (a kilogram for a sidearm, 2.5 for a submachine gun, four for a rifle, and six for a heavy weapon). A gun whose own body gives no finite mass takes its weight class's `fallback_mass_kg`. Its gravity stays on.

The design target is a gun near-rigid in the hand: it follows the hand within a degree or two, and its mass shows only as a slight lag when it is swung fast.

It is on the weapons layer in the player's collision group, as the hand bodies are: it meets the static world, terrain, props, debris, character capsules, ragdolls, and vehicles, and not the player, the hands, other weapons-layer bodies, or the aim's ray query, so it never blocks a shot. Its joints join two bodies of the mod's own, so nothing the mod holds names a body the game can free.

## The seat and the joints

The weapon handling's input tick publishes each hand's gun: the weapon, the gun, its own body's id and wrapper, and the weight class. A gun counts only while the rigid grip pinned it to that hand lately and no slide (a draw, a holster, a take) is moving it. The hand holding a gun then acts, so its body is driven toward its controller with the hand's full strength, against the hand's mass and the gun body's together. A hand lifting a loose object has a smaller budget (`pickup.hand_force_n` and `pickup.hand_torque_nm`), and the hands on guns had it too, with the twist weakened further while both hands held guns; a rifle then outweighed what the hand could turn, and the gun dragged the hand about.

The next pre-simulation step checks that the gun's own body still belongs to the gun (its slot's engine instance is the gun's wrapper, which the gun's destruction would have cleared) and is out of the world, reads the gun's last placement ([`WeaponData::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_WorldMatrixT1); the game places the gun's own body at that matrix, so its shape is in that frame), and takes the gun's seat on the hand from it: the placement in the frame of the controller's palm, which the hand's body is driven to. The placement is the attach bone's, which the rigid grip pins to the hand, so the seat is where the grip puts the gun in the palm, a few centimetres for a gun held by its grip. A gun whose seat is further out than a gun held by its grip sits ([`MAX_GRIP_SEAT_M`](../../../payload/src/controllers/physical_hands/physics/guns/math.rs)), or than its muzzle for one carried by another part, is not in the hand yet and waits. So does a gun whose hand's body is not on its palm yet (within five centimetres and about eleven degrees): the seat was once taken from where the hand's body was when the gun body was made, and a body still catching up with its palm then held the gun as far off the hand as it lagged for as long as it was held (the log showed seats of 13 to 36 cm against a usual 4 cm). The gun body is made on its seat on the hand's body, moving with it, and joined to it there with a fixed constraint. It is checked to be in the player's collision system group, so it never meets the player's body or the hands; one that is not is destroyed and the gun waits.

While the other hand holds the gun's foregrip ([physical-weapons.md](physical-weapons.md) and `weapons::foregrip`), that hand's body acts too and is joined to the gun body where it holds it, a second fixed constraint: the brace. The gun hangs between the two hands' bodies, their pulls add, and the line between the two grips, not either wrist alone, sets where it points. The brace ends when the hand lets go of the foregrip or its body is put somewhere, and is made again where the hand is.

Each step gives the gun body the velocity it has as part of the holding hand's body (the hand's velocity at the gun's centre of mass, and the hand's turn), so the two move as one rigid body and the joint holds the gun on its seat rather than catching it up a step late. What the gun meets still stops it within the step, and a shot's kick is given after. Each step also measures how far the gun body strays from its seat (the deviation, in the gun's own frame). A gun body that strays past `stray_distance_m` (5 cm) or `stray_turn_deg` (8°), or whose hand's body was put somewhere rather than driven there (added, put back on its controller, or carried by a snap turn), is put back on its seat, which the readout counts. The limits were 40 cm and 46°, past which a flailing gun was left to flail.

## Drawing the gun and the hands

The hands are drawn at their bodies as every hand body's is ([physical-hands.md, "Drawing the hand at its body"](physical-hands.md#drawing-the-hand-at-its-body)), with no turn deadband for a hand on a gun, so a gun turned by the other hand's pull is drawn turned. The rigid grip ([`crate::controllers::hands::grip`](../../../payload/src/controllers/hands/grip.rs)) pins the gun's attach bone to the drawn wrist at the canonical grip as before, then carries it by the gun body's deviation from its seat, held to `drawn_max_shift_m` (1.5 cm) and `drawn_max_turn_deg` (2°), so the drawn gun is where the gun body is and never more than a couple of degrees off the hand's grip. Everything that derives from the gun's placement follows it: the muzzle effects and the shot origin, the gun aim's sight line, which is the gun's transform carried to the controller's latest pose ([controllers.md, "Aiming"](controllers.md#aiming)), the reload's magazine wells, and the render-time late update.

A hand on a gun body takes none of the kinematic poses a held gun otherwise gives the hand: the two-handed pose, the weight's sag, lag, and sway, the recoil offset, and the wall probe's push are taken back out of that hand's poses (`hands::active_hands`). Aiming down the sights still turns the drawn gun onto the eye; it is a correction of a few centimetres on top of where the bodies have the gun, and the sight line follows the drawn gun, so what the eye sees down the sights is where the shot goes. The weight's wider one-handed spread still acts.

## Weight, collision, and strikes

The gun's weight hangs from the joints and its inertia rides the hands: the hand driving it carries the gun's mass on its own, so a heavier gun lags a fast swing a little more, and a braced gun is held up by both hands. Pushed into a wall, the gun stops along its whole length, a side brushing a wall included, and the hand behind it lags, which pushes the player away as a hand pressed into a wall does. A gun pressed into a crate shoves it.

With `bodies.hits.enabled`, the gun body's contacts are listened for as a hand body's are ([physical-hands.md, "Hits"](physical-hands.md#hits)), and each counts as its holding hand's. A contact with a body in the player's collision group (the player's own, a hand's, or the other gun's) is none: the gun body never collides with them, and a hand touching the gun it holds is not a strike. A contact pulses the hand by the arm's mass alone at the hand's speed, not the gun's, and at most once every `contact_pulse_interval_s` (0.2 s) per hand, since a gun rubbing along a wall touches it on every step and the gun's mass saturated the pulse into a constant buzz. A gun swung into a character is a blow, its impulse the momentum of the arm (`hits.blow_mass_kg`) and the gun together at the gun's speed into the character, so a rifle butt strikes harder than a fist at the same speed. One swing through a character is one blow for the hand and its gun together.

## Shots

Each shot of a gun whose body the hands hold kicks that body: an impulse back along the barrel of `recoil_impulse_ns` for its weight class, and `muzzle_rise` of that upward across the barrel, given at the muzzle, so the muzzle rises about the gun's centre of mass. The barrel is the fire point's axis and the up is the world's up across it as the gun was held when the body was made. The kick pulls the hands with it within their strength, and a braced gun kicks less. The shots are queued from the per-shot seam (`weapons::fire`) and given at the next step.

## The lifecycle

The gun body is made, kept, and destroyed only in the pre-simulation step, on the game thread outside the physics step, like the hand bodies. Its joints always go before either body they join:

- The gun body goes, its brace and its holder's joint first, when the gun leaves the hand (holstered, docked, dropped, handed over, moved between slots or onto a socket, sliding, or no longer published), when the hand starts or stops holding it by a part other than its grip ([below](#grabbing-a-gun-anywhere)), when its own body is back in the world (a drop or a mount since the input tick), when the gun is gone, when its joint went away, when the player's collision group changes, or when the hits are switched on or off.
- Every path that stops a hand's body first destroys the gun hanging from it and ends the brace it holds: the hand parked or held in place (a vehicle, a swim, the parachute, a lost controller), its body forgotten or destroyed (the game leaving gameplay for a load or the menu, the hand bodies switched off, a new character after a death), and the shutdown cleanup.

The weapon itself is never touched by the gun body: if the game destroys the weapon before the next step finds it gone, the gun body is a body of the mod's own with the mod's own joints, destroyed at that step.

A heavy weapon's own body stays in the world while it is wielded, so such a gun gets no gun body and keeps the kinematic weight and recoil.

## Grabbing a gun anywhere

With `weapons.regions.enabled` (on; the debug UI's Controllers tab, under "Weapons"), a long gun (the two-handed, special, and heavy slots) is taken wherever the hand closes on it, in the hands, on a shoulder, or lying in the world, with or without the gun bodies above. A pistol keeps its grip. Each hand carries its own gun, the wielded weapon or one armed beside it, so a long gun drawn by its barrel into one hand while the other holds a pistol stays carried there; a brace, a touch along the gun, and a grip taken from a carrying hand need the other hand free, so they act on the long gun held in one hand while the other is free ([two-handed-inventory.md, "The gun in either hand"](two-handed-inventory.md#the-gun-in-either-hand)). The code is `payload/src/controllers/weapons/regions/`.

### The regions

Each gun's regions are worked out in its own frame, the frame of [`WeaponData::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_WorldMatrixT1) (+Y up, -Z toward the muzzle, its origin the attach point in the palm of the hand on the pistol grip), from:

- the model's box: the first gun's model instance ([`WeaponData::m_BaseModelInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_BaseModelInstance)), whose model space is that frame, gives it ([`ModelInstance::GetModelSpaceBoundingBox`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/graphics_engine::model_instance::ModelInstance##func-GetModelSpaceBoundingBox));
- the muzzle: the first gun's current fire point;
- the foregrip: the other hand's grip point ([`WeaponBase::GetGripPosition`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-GetGripPosition)), when the gun has one;
- the magazine well and the action's handle: the wells table's ([reload.md](reload.md)), when its entry is trusted.

A hand touches the gun when its grip point is within `reach_m` (0.05 m) of the box. It is then in the nearest of the parts with a point of their own, within each one's size: the pistol grip (within `grip_radius_m`, 0.07 m, of the line from the attach point down the grip), the action (a sphere at its handle), and on a long gun the magazine (the length a magazine hangs below its well). Otherwise it is in a region along the bore: the stock behind the grip, the handguard (the receiver and on to past the foregrip, or most of the way to the muzzle without one), and the barrel out to the muzzle. The point the hand closes on, the anchor, is the part's own point (the attach point for the grip), or for the stock, the handguard, and the barrel, the point nearest the hand on the line the region runs along: back from the grip, through the foregrip, and through the muzzle, never past the gun's ends.

### What each hand does

- The hand on the grip holds the gun as before, and it is the only hand that fires it: the fire inputs are held back while the gun is carried by any other part.
- A second hand on any other part braces the gun there: the foregrip hold ([physical-weapons.md](physical-weapons.md)) takes the anchor in place of the gun's own foregrip point, so the two-handed pose puts the hand where it closed on the gun, the brace joint of a gun body joins it there, and the fingers take that part's pose. A brace ahead of the grip (the handguard, the barrel, the magazine, or the action) steers the gun as the foregrip does; a hand on the stock holds the gun without turning it. While the hand stays on the handguard or the barrel, the brace slides along them with it, except while the reload has a pump out, which sets where the hand is.
- A lone hand on any part but the grip carries the gun: the rigid grip pins it so that the anchor sits in the palm, where the grip's attach point sits, turned on the wrist as the hand took it (`regions::carry_offset`), so it does not jump when taken. It is not aimed: its trigger fires nothing, and the sights do not come onto the eye.
- A hand that takes the grip of a carried gun (the handover volume on the gun's grip, wherever the gun is), or a bracing hand that slides onto it, becomes the firing hand, and the hand it took the gun from braces it where it carried it, while its grip stays held. The brace waits for the gun to be placed on its new hand and measured there, for up to `rebrace_s` (1 s).
- With `grip_to_hold`, the holding hand letting go while the other braces leaves the gun carried by the bracing hand at its brace, rather than dropping it. At the gun's own holster, it holsters as before.

A gun is taken by a part other than its grip from the world and from the shoulder too:

- a direct pickup ([physical-weapons.md, "Picking a weapon up into the hand"](physical-weapons.md#picking-a-weapon-up-into-the-hand)) classifies where the hand reached the gun, with the gun's pose before the pickup and the hand's wrist rotation, and with the take's slide the gun slides from where it lay to its anchor in the palm;
- a long gun docked on a shoulder has a volume on whatever part a free hand touches, as both are drawn, which draws it as the shoulder's holster does, and the gun comes into the hand at that part (a draw waits up to `draw_wait_s`, 3 s, for the gun to arrive).

A gun taken out of a character's hands or off a rack comes into the hand at its grip.

The carry ends when the gun leaves its hand, goes to the other hand, or another gun is in that hand, and with the regions switched off or out of gameplay; the gun is then at its grip again. With the gun bodies on, every change of the holding hand remakes the gun body on its new seat, so a carried gun's body is joined where the hand holds it.

The action's region is also where the reload places the action's handle while the chamber waits on it (`regions::action_volume`): a grip press there works the action, and anywhere else on the action it braces the gun.

The fingers take each region's pose ([grip-poses.md](grip-poses.md)): the pistol grip on the grip, the foregrip's wrap on the handguard, a wrap round a thin bar on the barrel and a thicker one on the stock, a cup under the magazine, and a pinch on the action.

### Its settings, readout, and log

The settings are `controllers.weapons.regions` (`enabled`, `reach_m`, `grip_radius_m`, `rebrace_s`, `draw_wait_s`), under the hold's in the debug UI's Controllers tab, with a readout of each hand's region and role, the counts, and the wielded long gun's shape (its length and which known points it has). The `WEAPONS IN HAND` section of the debug UI's Status tab has `LEFT HAND ON GUN` and `RIGHT HAND ON GUN`: the region, `FIRING`, `BRACING`, or `CARRYING, CANNOT FIRE`, and the counts, with the gun's length and known points in the log's note; `NOT ON A GUN` while waiting, and `GUN'S SHAPE NOT READ` fails. The recorder's signals are `regions.carries`, `regions.braces`, `regions.grips_taken`, `regions.let_go`, and `regions.carrying`, and the template is `testing/scenarios/grab_anywhere.toml`.

The log has, at info level, `regions: the left hand took the gun by its barrel` (or right, and the region) with the cause (`pickup`, `draw`, or `the holding hand let go`) and the anchor; `regions: the gun is no longer carried by its barrel` with the reason; `regions: the left hand braced the gun at its handguard`; and `regions: the left hand took the gun's grip; the other braces it at its stock`.

## Settings and readout

The settings are `physical_hands.bodies.guns`, under "Hand bodies" in the debug UI's Controllers tab, with the readout beside the hand bodies': the gun bodies made, destroyed, and put back, the shots given, each hand's gun contacts, and for each hand's gun body its id, whether it is in the world, its mass and where it came from, its joints, how far it is off its seat, and its speed. The switch is `physical_hands.bodies.guns.enabled`.

The `HAND BODIES` status section has `LEFT GUN BODY` and `RIGHT GUN BODY`: `NO GUN BODY` while waiting, or the body's id and mass, `HELD` or `HELD+BRACED`, and the contacts it started, with its mass's source and its deviation in the log's note; `NOT JOINED` or `NOT IN WORLD` fail. The recorder's signals are `guns.made`, `guns.held`, `guns.braced`, `guns.shots`, `guns.contacts`, `guns.reseats`, and `guns.error`, and the template is `testing/scenarios/physical_guns.toml`.

The log has, at info level, `held guns: made a gun body and joined it to its hand` with the hand, weapon, class, body, constraint, mass and its source, the seat's distance (`seat_m`, a few centimetres for a grip), how far the made joint's seat is from it (`seat_error_m`, near zero), and how far the hand's body was off its palm (`hand_off_palm_m`); `held guns: joined the bracing hand to the gun`; `held guns: ended a gun's joint` with its role (`holder` or `brace`) and the reason; `held guns: destroyed a gun body` with the reason; `held guns: could not make a gun body` once per gun with the error; and a hand body's blow says `with_gun`. At debug level, `held guns: no gun body yet` for a gun not yet in the hand or whose hand's body is not on its palm yet, and `held guns: put a gun body back on its seat` with the reason.

## In the headset

With the hand bodies and `bodies.guns.enabled` on:

1. Draw a pistol: `held guns: made a gun body and joined it to its hand`, with `mass_from="its own body"`, a mass of a kilogram at most, and `seat_m` of a few centimetres. `RIGHT GUN BODY` reads `OK, #..., HELD`. The pistol sits in the hand as before, and the hand's body stays in the world (no `removed a hand body from the world`).
2. Wave it quickly: it follows without visible lag; the readout's distance off its seat stays within a few millimetres, the drawn pistol stays within a couple of degrees of the hand, and the hand does not buzz.
3. Draw a rifle and hold it at the grip only, arm out: its mass is four kilograms at most, it stays in the hand, and lags a fast swing only slightly more than the pistol. Dual-wield pistols: each body is a kilogram at most. Brace it on the foregrip with the other hand: `held guns: joined the bracing hand to the gun`, `HELD+BRACED`, and it steadies and points along the line between the hands. Let go of the foregrip: `ended a gun's joint` with `role="brace"`.
4. Walk up to a wall with the rifle raised, then brush its side along the wall: it stops at the wall along its whole length, and the body is pushed back as it is with a hand. Point it at a crate and push: the crate slides.
5. Fire a rifle on full auto braced and one-handed: the muzzle climbs and the gun kicks back into the hands, less when braced; `guns.shots` climbs. The shots land where the sights point, and the muzzle flash and casings come from the drawn gun.
6. Aim down the sights with the rifle: the sights still come onto the eye.
7. Swing the rifle's butt into a soldier: `a hand struck a character` with `with_gun=true`, harder than a punch at the same speed, and the gun row's hits climb.
8. Reload the rifle from the pouch: the magazine well is on the drawn gun.
9. Dock the rifle at the shoulder: `destroyed a gun body` with `the gun left the hand` before the slide, and the rifle slides to the back. Draw it again: a new gun body once it is in the hand.
10. Drop the rifle (open the grip away from the body with `grip_to_hold`): the gun body goes (`the gun left the hand` or `the gun's own body is back in the world`), and the rifle falls with the hand's swing.
11. Hand a pistol to the other hand at its grip: the old hand's gun body goes and the new hand's is made.
12. Enter a vehicle, swim, open the parachute, die, and load a save with a gun in hand: each time `destroyed a gun body` before the hand's body is removed or destroyed, and no crash.
13. Switch `bodies.guns.enabled` off with a rifle in hand: the gun body goes, the hand's body leaves the world, and the rifle sags and sways as before.
14. Hold a rocket launcher: no gun body (`NO GUN BODY`), and its kinematic weight and recoil act.

With `weapons.regions.enabled` on, once with `bodies.guns.enabled` off and once with it on:

15. Draw a rifle and look at `RIGHT HAND ON GUN`: `GRIP, FIRING`, with the gun's length and its known points in the note. A rifle without a foregrip, a magazine well, or an action in the note still has its handguard and barrel.
16. Take the rifle's handguard with the left hand behind the native foregrip, then well ahead of it near the muzzle: each time `regions: the left hand braced the gun at its handguard` (then `barrel`), the hand stays where it closed on the gun rather than jumping to the foregrip, the rifle points along the line between the hands, and `LEFT HAND ON GUN` reads `HANDGUARD, BRACING` (then `BARREL`). The fingers wrap the handguard, then close tighter round the barrel.
17. Slide the left hand along the handguard and the barrel while bracing: the drawn hand follows along the gun, staying on it. With `bodies.guns.enabled` on, the drawn hand stays where the brace joint holds it.
18. Brace on the magazine and on the stock: `MAGAZINE, BRACING` with a cupped hand, and `STOCK, BRACING` with the rifle not turning toward the left hand.
19. With `grip_to_hold` on, brace the barrel and let go with the right hand: `regions: the left hand took the gun by its barrel` with `cause="the holding hand let go"`, the rifle hangs from the left hand where it held it without jumping, `LEFT HAND ON GUN` reads `BARREL, CARRYING, CANNOT FIRE`, and the left trigger fires nothing. The muzzle effects and the sight line follow the carried rifle.
20. Take the carried rifle's pistol grip with the right hand: `regions: the right hand took the gun's grip; the other braces it at its barrel`, then `braced the gun at its barrel`; the right trigger fires again, and the left hand stays on the barrel where it was.
21. Carry the rifle by the barrel in the left hand, take the barrel with the right, and slide the right hand back onto the pistol grip: it becomes the firing hand the same way.
22. Kill a soldier and pick his rifle up off the ground by the barrel: `took the gun by its barrel` with `cause="pickup"`; the rifle slides from the ground with the barrel into the palm, and cannot fire until the other hand takes the grip. Pick another up by its grip: no carry, and it fires.
23. With a rifle on the back, reach over the shoulder and grab its barrel or stock rather than the grip: the rifle draws into the hand at that part (`cause="draw"`). Grab it at the grip: it draws at the grip.
24. With the gun bodies on, carry a rifle by the barrel and swing its stock into a soldier: the blow counts with the gun, and `RIGHT GUN BODY` (or left) shows the carrying hand's gun body.
25. Holster and drop a carried rifle: `the gun is no longer carried` with `the gun left the hands`; draw it again at the grip: `GRIP, FIRING`.
26. Empty a rifle that needs its charging handle worked, then grab the handle with the free hand while the chamber waits: the action is worked as before, not braced. Grab the same handle with the chamber loaded: `braced the gun at its action`, with a pinch.
27. Pump a shotgun braced on its handguard: the pump moves with the hand as before, and the brace does not slide while the pump is out.
28. Hold a pistol: `GRIP, FIRING`, and no part of it but the grip takes it.
29. Switch `weapons.regions.enabled` off while carrying a rifle by the barrel: it goes back to the grip, and the foregrip braces only at its own point again.

## Unverified

- That the gun's own body's motion keeps a real mass while the weapon is held, and what masses the game gives its guns; the readout's mass source says, and the class fallbacks are guesses.
- That the gun's own shape, given in the gun's model frame, lines up with the drawn gun: the game places the body at the gun's world matrix when it adds it, which says it does, but nothing in the headset has shown it.
- Whether moving the gun body with the hand's body each step keeps it steady through the fixed joint, whether the gun jitters against the world, and whether two fixed joints on one gun (the holder's and the brace) fight each other when the hands disagree.
- Whether the hand's body settles on its palm often enough for a gun body to be made at once after a draw; a body held back by a wall keeps the gun without a body until it is free.
- Whether the impulse defaults feel like recoil, and whether a full-auto burst walks the gun off the hands; the gun is put back on its seat if it strays.
- Whether the engine's listeners do anything with a weapons-layer body of no owner meeting a character, as for the hand bodies.
- Whether the fire point's axis is the barrel for every gun (a gun whose fire point is turned kicks off its barrel).
- Whether the model's box fits each gun closely enough for its regions: a box that takes in a scope, a bipod, or a launcher under the barrel moves the handguard and the barrel's line, and the region sizes are judged, not measured.
- Whether the attach point sits in the palm closely enough that a carried gun's anchor lands in the hand, and whether a gun turned on the wrist as the hand took it reads as held rather than stuck to the hand at an odd angle.
- Whether the animation graph and the gun aim cope with a long gun carried by its barrel or stock on the hand's attach bone: the weapon is still wielded, so the upper body may hold an aiming pose with the gun pointing elsewhere.
- Whether a brace sliding along the barrel reads as sliding with the gun bodies' brace joint holding the hand where it took hold.
- Whether a pump worked from a brace on the barrel rather than the handguard behaves, since the reload takes any brace for the hand on the pump.
