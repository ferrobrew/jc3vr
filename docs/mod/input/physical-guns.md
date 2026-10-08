# Physical guns

With the hand bodies on ([physical-hands.md](physical-hands.md#hand-bodies)), a gun in a hand is a body of its own in the physics world, joined to the hand bodies that hold it. Its weight hangs from the hands and drags them by its mass, it collides along its whole length, a two-handed hold is two joints, and swung into a character it strikes as the hands do. The drawn gun and the drawn hands follow the physics: each hand is drawn at its body, and the gun at its body. With `PHYSICAL GUNS` off (`physical_hands.bodies.guns.enabled`, on the pause menu's `VR: CONTROLS` page under `PHYSICAL HANDS`), or with the hand bodies off, a held gun has no body, its hands' bodies leave the world as before, and the kinematic weight ([`heft`](../../../payload/src/controllers/weapons/heft/mod.rs)), recoil offset, two-handed pose, and wall probe ([physical-weapons.md, "Guns against walls"](physical-weapons.md#guns-against-walls)) act instead.

The code is `payload/src/controllers/physical_hands/physics/guns/`. The engine side is in [havok-bodies.md](../../engine/physics/havok-bodies.md) and [weapon-equip-and-drop.md](../../engine/gameplay/weapon-equip-and-drop.md#one-object-for-its-whole-life). None of this has been run in the headset.

## Which body

Each gun has a body of its own ([`WeaponData::m_PfxInstance`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_PfxInstance)), in the world only while the weapon lies dropped or is mounted. Reusing it while the gun is held would mean fighting the weapon's state changes, which add it to and take it out of the world, and joining the hands to a body the game destroys with the weapon whenever it likes; the world does not end a constraint when one of its bodies is destroyed ([havok-bodies.md](../../engine/physics/havok-bodies.md#joining-bodies-with-a-fixed-constraint)). So the gun body is the mod's own, made the way the hand bodies are, with the gun's own shape ([`hknpBody::m_shape`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::hknpBody##field-m_shape), which the new body takes a reference of its own to, so the two share it) and the gun's own mass (its own body's motion's inverse mass). A gun whose own body gives no finite mass takes its weight class's `fallback_mass_kg`, and either is held between `min_mass_kg` and `max_mass_kg`. Its gravity stays on.

It is on the weapons layer in the player's collision group, as the hand bodies are: it meets the static world, terrain, props, debris, character capsules, ragdolls, and vehicles, and not the player, the hands, other weapons-layer bodies, or the aim's ray query, so it never blocks a shot. Its joints join two bodies of the mod's own, so nothing the mod holds names a body the game can free.

## The seat and the joints

The weapon handling's input tick publishes each hand's gun: the weapon, the gun, its own body's id and wrapper, and the weight class. A gun counts only while the rigid grip pinned it to that hand lately and no slide (a draw, a holster, a take) is moving it. The hand holding a gun then acts, so its body is driven toward its controller with a holding hand's strength (`pickup.hand_force_n` and `pickup.hand_torque_nm`, the same budget a hand holding a crate through a joint has), and its twist is weakened to `pickup.two_hand_angular_share` while both hands are on guns.

The next pre-simulation step checks that the gun's own body still belongs to the gun (its slot's engine instance is the gun's wrapper, which the gun's destruction would have cleared) and is out of the world, reads the gun's last placement ([`WeaponData::m_WorldMatrixT1`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponData##field-m_WorldMatrixT1); the game places the gun's own body at that matrix, so its shape is in that frame), and refuses a gun further than `break_distance_m` from the hand body, which is not in the hand yet. It makes the gun body there, moving with the hand, and joins it to the hand's body with a fixed constraint in the pose the two are in. That pose is the gun's seat on the hand.

While the other hand holds the gun's foregrip ([physical-weapons.md](physical-weapons.md) and `weapons::foregrip`), that hand's body acts too and is joined to the gun body where it holds it, a second fixed constraint: the brace. The gun hangs between the two hands' bodies, their pulls add, and the line between the two grips, not either wrist alone, sets where it points. The brace ends when the hand lets go of the foregrip or its body is put somewhere, and is made again where the hand is.

Each step measures how far the gun body strays from its seat (the deviation, in the gun's own frame). A gun body that strays past `break_distance_m` or a large turn, or whose hand's body was put somewhere rather than driven there (added, put back on its controller, or carried by a snap turn), is put back on its seat at the hand's velocity, which the readout counts.

## Drawing the gun and the hands

The hands are drawn at their bodies as every hand body's is ([physical-hands.md, "Drawing the hand at its body"](physical-hands.md#drawing-the-hand-at-its-body)), with no turn deadband for a hand on a gun, so a gun turned by the other hand's pull is drawn turned. The rigid grip ([`crate::controllers::hands::grip`](../../../payload/src/controllers/hands/grip.rs)) pins the gun's attach bone to the drawn wrist at the canonical grip as before, then carries it by the gun body's deviation from its seat, so the drawn gun is where the gun body is. Everything that derives from the gun's placement follows it: the muzzle effects and the shot origin, the gun aim's sight line, which is the gun's transform carried to the controller's latest pose ([controllers.md, "Aiming"](controllers.md#aiming)), the reload's magazine wells, and the render-time late update.

A hand on a gun body takes none of the kinematic poses the body now gives: the two-handed pose, the weight's sag, lag, and sway, the recoil offset, and the wall probe's push are taken back out of that hand's poses (`hands::active_hands`). Aiming down the sights still turns the drawn gun onto the eye; it is a correction of a few centimetres on top of where the bodies have the gun, and the sight line follows the drawn gun, so what the eye sees down the sights is where the shot goes. The weight's wider one-handed spread still acts.

## Weight, collision, and strikes

The gun's weight hangs from the joints and its inertia rides the hands: a heavy gun lags a fast swing and droops when held at one end past the wrist's twist, and a braced gun is held up by both hands. Pushed into a wall, the gun stops along its whole length, a side brushing a wall included, and the hand behind it lags, which pushes the player away as a hand pressed into a wall does. A gun pressed into a crate shoves it.

With `bodies.hits.enabled`, the gun body's contacts are listened for as a hand body's are ([physical-hands.md, "Hits"](physical-hands.md#hits)), and each counts as its holding hand's: it pulses that hand, and a gun swung into a character is a blow, its impulse the momentum of the arm (`hits.blow_mass_kg`) and the gun together at the gun's speed into the character, so a rifle butt strikes harder than a fist at the same speed. One swing through a character is one blow for the hand and its gun together.

## Shots

Each shot of a gun whose body the hands hold kicks that body: an impulse back along the barrel of `recoil_impulse_ns` for its weight class, and `muzzle_rise` of that upward across the barrel, given at the muzzle, so the muzzle rises about the gun's centre of mass. The barrel is the fire point's axis and the up is the world's up across it as the gun was held when the body was made. The kick pulls the hands with it within their strength, and a braced gun kicks less. The shots are queued from the per-shot seam (`weapons::fire`) and given at the next step.

## The lifecycle

The gun body is made, kept, and destroyed only in the pre-simulation step, on the game thread outside the physics step, like the hand bodies. Its joints always go before either body they join:

- The gun body goes, its brace and its holder's joint first, when the gun leaves the hand (holstered, docked, dropped, handed over, moved between slots or onto a socket, sliding, or no longer published), when its own body is back in the world (a drop or a mount since the input tick), when the gun is gone, when its joint went away, when the player's collision group changes, or when the hits are switched on or off.
- Every path that stops a hand's body first destroys the gun hanging from it and ends the brace it holds: the hand parked or held in place (a vehicle, a swim, the parachute, a lost controller), its body forgotten or destroyed (the game leaving gameplay for a load or the menu, the hand bodies switched off, a new character after a death), and the shutdown cleanup.

The weapon itself is never touched by the gun body: if the game destroys the weapon before the next step finds it gone, the gun body is a body of the mod's own with the mod's own joints, destroyed at that step.

A heavy weapon's own body stays in the world while it is wielded, so such a gun gets no gun body and keeps the kinematic weight and recoil.

## Settings and readout

The settings are `physical_hands.bodies.guns`, under "Hand bodies" in the debug UI's Controllers tab, with the readout beside the hand bodies': the gun bodies made, destroyed, and put back, the shots given, each hand's gun contacts, and for each hand's gun body its id, whether it is in the world, its mass and where it came from, its joints, how far it is off its seat, and its speed. The pause menu's switch is `PHYSICAL GUNS`.

The `VR: STATUS` page's `HAND BODIES` section has `LEFT GUN BODY` and `RIGHT GUN BODY`: `NO GUN BODY` while waiting, or the body's id and mass, `HELD` or `HELD+BRACED`, and the contacts it started, with its mass's source and its deviation in the log's note; `NOT JOINED` or `NOT IN WORLD` fail. The recorder's signals are `guns.made`, `guns.held`, `guns.braced`, `guns.shots`, `guns.contacts`, `guns.reseats`, and `guns.error`, and the template is `testing/scenarios/physical_guns.toml`.

The log has, at info level, `held guns: made a gun body and joined it to its hand` with the hand, weapon, class, body, constraint, mass and its source, and the seat's distance; `held guns: joined the bracing hand to the gun`; `held guns: ended a gun's joint` with its role (`holder` or `brace`) and the reason; `held guns: destroyed a gun body` with the reason; `held guns: could not make a gun body` once per gun with the error; and a hand body's blow says `with_gun`. At debug level, `held guns: no gun body yet` for a gun not yet in the hand, and `held guns: put a gun body back on its seat` with the reason.

## In the headset

With the hand bodies and `PHYSICAL GUNS` on:

1. Draw a pistol: `held guns: made a gun body and joined it to its hand`, with `mass_from="its own body"` and a mass of a kilogram or so. `RIGHT GUN BODY` reads `OK, #..., HELD`. The pistol sits in the hand as before, and the hand's body stays in the world (no `removed a hand body from the world`).
2. Wave it quickly: it follows without visible lag; the readout's distance off its seat stays within a few millimetres.
3. Draw a rifle and hold it at the grip only, arm out: it droops a little and lags a fast swing more than the pistol. Brace it on the foregrip with the other hand: `held guns: joined the bracing hand to the gun`, `HELD+BRACED`, and it steadies and points along the line between the hands. Let go of the foregrip: `ended a gun's joint` with `role="brace"`.
4. Walk up to a wall with the rifle raised, then brush its side along the wall: it stops at the wall along its whole length, and the body is pushed back as it is with a hand. Point it at a crate and push: the crate slides.
5. Fire a rifle on full auto braced and one-handed: the muzzle climbs and the gun kicks back into the hands, less when braced; `guns.shots` climbs. The shots land where the sights point, and the muzzle flash and casings come from the drawn gun.
6. Aim down the sights with the rifle: the sights still come onto the eye.
7. Swing the rifle's butt into a soldier: `a hand struck a character` with `with_gun=true`, harder than a punch at the same speed, and the gun row's hits climb.
8. Reload the rifle from the pouch: the magazine well is on the drawn gun.
9. Holster the rifle at the shoulder, with and without `instant_dock`: `destroyed a gun body` with `the gun left the hand` before the slide, and the rifle slides to the back. Draw it again: a new gun body once it is in the hand.
10. Drop the rifle (open the grip away from the body with `grip_to_hold`): the gun body goes (`the gun left the hand` or `the gun's own body is back in the world`), and the rifle falls with the hand's swing.
11. Hand a pistol to the other hand at its grip: the old hand's gun body goes and the new hand's is made.
12. Enter a vehicle, swim, open the parachute, die, and load a save with a gun in hand: each time `destroyed a gun body` before the hand's body is removed or destroyed, and no crash.
13. Switch `PHYSICAL GUNS` off with a rifle in hand and resume: the gun body goes, the hand's body leaves the world, and the rifle sags and sways as before.
14. Hold a rocket launcher: no gun body (`NO GUN BODY`), and its kinematic weight and recoil act.

## Unverified

- That the gun's own body's motion keeps a real mass while the weapon is held, and what masses the game gives its guns; the readout's mass source says, and the class fallbacks are guesses.
- That the gun's own shape, given in the gun's model frame, lines up with the drawn gun: the game places the body at the gun's world matrix when it adds it, which says it does, but nothing in the headset has shown it.
- Whether a three-kilogram hand holds a heavy gun steadily through a fixed joint at the holding hand's strength, whether the gun jitters against the world, and whether two fixed joints on one gun (the holder's and the brace) fight each other when the hands disagree.
- Whether the impulse defaults feel like recoil, and whether a full-auto burst walks the gun off the hands; the gun is put back on its seat if it strays.
- Whether the engine's listeners do anything with a weapons-layer body of no owner meeting a character, as for the hand bodies.
- Whether the fire point's axis is the barrel for every gun (a gun whose fire point is turned kicks off its barrel).
