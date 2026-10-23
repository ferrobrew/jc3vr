# Grapple snatch

Not built yet: this is the design and the plan of record. It is built after the [two-handed inventory](two-handed-inventory.md), whose takes it ends in: the snatch's arrival lands the item in the loadout table and the grabbing hand, never through the game's equip. It is the first feature built on the inventory, and its test: a snatch takes an item from anywhere into the grabbing hand while the other hand holds whatever it holds, which exercises the table, the hands' independence, and the reconcile end to end.

The grapple hand pulls things to itself. The grapple button keeps doing what it does in the game (a tap fires, a hold tethers to and between targets), and the grip pulls: with the empty grapple hand pointed at something out of reach, a squeeze and a yank back toward the body fire the game's hook at it and snatch it from wherever it is (the ground, a rack or a locker, a character's hands, a holster on a character's body) into the hand, in the held position. From there it is an ordinary held thing: hand it to the other hand, holster it, carry it, or drop it. Weapons are the core of the feature. Props and car doors follow, the doors pulled open on the line rather than into the hand.

The game offers nothing like it: its grapple never hooks a weapon, has no pull of a light object to the player, and cannot reach into a character's hands. The snatch keeps the game's own grapple wherever it can, so it reads as the same device doing one thing more: the game's aim and target highlight, the game's hook, line, and flight from Rico's left forearm, and the game's own pickup and steal rules at the end. The mod adds the trigger, the pull, and the arrival.

## Why the game's grapple ignores weapons

Verified against the release IDB unless noted.

- The player's aim ray (`PlayerAimControl::UpdateDirectAim`) and the hook's own in-flight cast (`CWireEnd::TryCollide`) both query the `RAYCAST_EVERYTHING_PHYSICAL` layer, and the `WEAPONS` layer does not collide with it ([engine: havok-bodies.md](../../engine/physics/havok-bodies.md)). A dropped weapon's body is on `WEAPONS`, so both rays pass through it.
- Everything after the ray supports weapons. A weapon's grappling mode is "reel in, no attach", not "ungrapplable", and `CWireEnd::SetupProxyForAttachment` has an explicit weapon branch that builds the same object attachment proxy as for any loose object.
- The release has no usable pull of a light object to the player. The symbol dump's `CGrapplingHook::HandleGrapplePull` pulls one only behind two developer menu flags, and the release function bearing that name does not match it.
- A weapon in a character's hands has no body in the world, and the dump's `PlayerAimControl::CheckDirectTargets` resolves a hit on a held weapon to its holder (dump only).

So the game was built to hook weapons, and only its rays keep it from finding one.

## Targeting: the game's aim, scoped to the player's grapple

The snatch aims with the grapple's own aim: the off-hand's ray that the mod already feeds to the game's grapple ([controllers-and-roomscale.md, "The grapple"](controllers-and-roomscale.md#the-grapple)), with the game's own target highlight on what it hits. To make that aim see weapons, the local player's grapple aim cast also tests the `WEAPONS` layer: a patch scoped to that one cast (its call site or a detour of the grapple aim), never the physics system's layer table, which every query on the layer shares (the gun's aim among them, which would then stop on guns lying on the ground). Other characters' grapples and every other query are untouched.

With the scoped patch, a gun on the ground is a grapple target like any other, highlighted by the game. The grapple button treats it as the game treats any light object, so a hold tethers a gun to something else, or two guns together, through the game's own tethers; a tap at a gun must not zip Rico to it, since a weapon's grappling mode is "reel in, no attach", and phase 1 settles what the tap does instead. A character holding a gun is already one; for the snatch, an armed character under the aim is a disarm target (the gun in its hands). What the scoped patch cannot reach (a rack's weapon, which has no body; a locker's, unverified) falls back to the mod's own selection: a cone of about 6 degrees around the same ray, to about 20 m (the hook's line length caps it), with a line-of-sight ray on the grapple's query layer, and a mod marker in the target's colour in place of the game's highlight.

In the grip's one targeting rule ([grip.md](grip.md)) the snatch is a new lowest tier, `Tier::Remote`, taken only when nothing at all is in reach (not even a surface to climb or a reach margin), so anything within real reach always wins. It is offered only to the grapple hand, empty, on foot, with the hook free (no active wire) and the grapple button up.

The grapple hand is the off-hand of the acting hand. The game's grapple device is on Rico's left forearm, so with a left-handed setup the line would leave the arm opposite the snatching hand; that is deferred with left-handedness project-wide.

## The interaction

1. Aim: point the empty grapple hand at the target. The game highlights it (or the mod's marker shows it).
2. Grip: squeezing arms the snatch on the target under the aim. Nothing flies yet, and the target is held while the grip is.
3. Yank: a pull of the hand back toward the body, faster than a threshold, while the grip is held, fires it. The yank is the confirmation, as with the gravity gloves of other games, and the motion matches what it does. Opening the grip before the yank disarms it, and a punch or a swing that closes the hand does not snatch, since the grip comes first. The grapple's line already uses a yank to zip ([physical-grapple.md](physical-grapple.md)); the snatch's yank is told apart by the grip being held on a remote target with no line out.
4. Flight: the hook flies from the left forearm to the target, attaches, and the target arcs into the hand.
5. Arrival: the target lands in the hand in its held pose.

Hold to hold holds: opening the grip at any point cancels. Mid-flight, the line comes away and the target keeps its momentum and falls, rather than snapping back to where it came from.

Haptics: the grapple's fire and attach cues on the snatching hand, the line's tension while it pulls, and the take's cue on arrival; a refused or failed snatch plays the refusal cue ([haptics.md](haptics.md)).

## How a weapon is snatched

1. A weapon without a body is first freed into the world at its source. A gun in a character's hands goes through the steal's existing drop and stagger ([physical-weapons.md, "Taking a gun from a character's hands"](physical-weapons.md#taking-a-gun-from-a-characters-hands)), which leaves it a dropped weapon with bodies; a holstered gun on a character the same way without a selection change (phase 3).
2. The game's hook is fired at the weapon through `CGrapplingHook::FireHookCustom`, the entry point the game's own vehicle grapple and stunt-position grapple use, with a target built on the weapon's game object at the gun's grip (`HookTargetInfo::SetupTargetFromGameObject`) and the ghost-wire flag on. A ghost wire flies to the target, and when its casts miss, as they do on a weapon, attaches to the target object itself (`CWireEnd::ForceAttachToTarget`). The player sees the game's hook and line.
3. The mod pulls: the physical hands' hold solve drives the weapon's body toward the palm, started with the grab point metres away and its own speed and acceleration caps, easing in the last metre. Gravity is in the solve, so the gun arcs. The game's line follows the gun, since the hook is attached to it.
4. Within about 0.4 m of the palm (outside Rico's collision proxy, which a weapon's body collides with), the mod cancels the wire (`CGrapplingHook::CancelActiveWire`) and takes the weapon through the direct pickup (`pickup::take`) with its short slide into the hand. Every rule of the pickup applies: the weapon is still dropped, is not a grenade or an unlimited-stock copy, and the animation allows a pickup. It lands in its slot or on a socket by the inventory's rules ([vr-inventory.md](vr-inventory.md)).

The mod holds a strong reference to the weapon for the whole snatch, so the despawn never takes it mid-flight. A stall (the body moving less than a set distance over half a second, snagged on the world) or a line that would run out fails the snatch: the wire is cancelled and the weapon is left where it is. A refused take at arrival leaves the weapon falling, logged with the reason.

The hook's state machine serialises snatches: `FireHookCustom` refuses while a line is out or the hook is busy, and the mod holds the grapple button back from the game for the snatch's duration and always ends the custom wire itself, on every exit, stand-down, and load.

## Per target kind

| Kind | Freeing | Hook | Pull | Finish | Phase |
| --- | --- | --- | --- | --- | --- |
| Weapon on the ground | none | custom ghost fire at the weapon | the hold solve | `pickup::take` | 1 |
| Weapon in a character's hands | the steal's drop and stagger | custom ghost fire once it has bodies | the hold solve | `pickup::take` | 1 |
| Weapon on a rack | open: the rack hands its weapon to a pickup, not to the world | open | open | the rack take, or `pickup::take` | 2 |
| Rack with unlimited stock | none: the copy spawns into the inventory | a mod-drawn line to the rack's weapon | the copy flies in on a stretched, arced slide | the rack's copy path | 2 |
| Weapon in a locker | none if it lies dropped with bodies (unverified) | as on the ground | as on the ground | `pickup::take` | 2 |
| Weapon holstered on a character | the slot's weapon dropped as the steal drops a held one | as on the ground | as on the ground | `pickup::take` | 3 |
| Loose prop | none | custom ghost fire at the object | the hold solve | the hand's ordinary hold continues | 3 |
| Car door | the doors' unlatch once the hook is on ([vehicle-doors.md](vehicle-doors.md)) | fire at the vehicle near the handle | an outward impulse on the door's body, not into the hand | the wire cancelled; the door swings | 4 |

A disarmed character staggers and reacts as to the steal in reach: it draws another weapon, fights unarmed, or goes for one lying in the world. How the AI reacts to a disarm from range is to be watched in the headset.

## Settings

`weapons.snatch` (on by default), in the debug UI only, with the range, the cone, the yank's speed and window, the pull's caps, and the capture radius. The Status tab gains a `SNATCH` row: the chosen target, its distance, the snatch's state, and the last refusal.

## Phases

### Phase 1: weapons on the ground and in characters' hands

- Engine: the scoped patch that lets the player's grapple aim see weapons, with the RE of the aim cast it patches and of what the grapple's tap and hold do with a weapon under the aim (a tap at a weapon must not zip Rico to it); bindings for `FireHookCustom`, `CancelActiveWire`, the active wire's far end and its attached state, and the weapon's grappling mode; a "Weapons and the grapple" section in [engine: grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md).
- Payload: the snatch module (armed, freeing, hook out, pulling, arriving, done or failed), `Tier::Remote`, the snatch grab volume, the yank detection, the grapple button held back during a snatch, the steal's character notice widened to the snatch's range, the setting, and the Status row.
- Tests: the yank detection, the tier's place in the resolver, and the capture and stall rules, as pure functions.

In the headset:

1. Point the empty grapple hand at a gun on the ground 10 m away: the game highlights it. Move the hand onto a crate within reach: the crate's marker wins.
2. Squeeze, then yank: the hook flies from the left forearm, takes hold of the gun, the gun arcs into the hand, the line comes away, and the gun sits in the held pose. Holster it, hand it over, drop it.
3. Squeeze without yanking, then open the hand: nothing flies. Hold the grapple button on a gun, aim at a car, and let go: the gun is tethered to the car.
4. Yank, then open the hand mid-flight: the line goes and the gun falls near you.
5. Snatch a gun from a soldier at 10 m: the soldier staggers, and the gun flies in.
6. Point at a gun behind a wall: no highlight. Tap the grapple button at a gun on the ground: the game's grapple does what it does at any light object, and does not zip Rico to it.
7. With tethers attached, try a snatch: it works with the tethers untouched, or is refused cleanly.
8. Swing a closed fist past a highlighted gun: no snatch.
9. Watch the frame time with many weapons and characters near.

### Phase 2: racks and lockers

The RE of a rack's pickup and of finding distant racks; a physical flight for a rack weapon that can be dropped into the world; the stretched, arced slide and a mod-drawn line for the unlimited-stock copy. Missions that hand the player a rack's weapon keep working through the game's interaction.

### Phase 3: holstered weapons on characters, and props

Noting characters' holstered weapons and dropping a slot the character does not wield; for a prop, the remote pull handing over to the hand's ordinary hold on arrival, within the hand's strength.

### Phase 4: car doors

The door as a remote target at its handle; once the hook is on near the handle, the doors' unlatch and an outward impulse on the door's own body, then the wire cancelled. A moving car's door is refused or handled safely.

## Open questions

- Whether `FireHookCustom` is accepted while dual tethers are attached, and whether the hook acts on the game's grapple input in its custom state.
- Whether a ghost wire passes through walls (inferred); the line-of-sight check guards it either way.
- Whether the AI reserves a dropped weapon it walks toward, and what it does when the weapon leaves.
- Whether the custom fire plays the hook's fire sound; the force-attach skips the impact effect, which the mod can play itself.
- Whether moving the incoming weapon into the player's collision group would let it come all the way to the palm rather than stopping at the capture radius.

Two corrections to record with the phase 1 bindings: the pyxis doc of `GHS_CUSTOM_ACTIVE_WIRE` calls it a tether state, and it is the state `FireHookCustom` sets for a scripted wire; and in the release `CWireEnd::Update`, the call the IDB names `CDebugRenderer::DebugLineTTL` is `CPhysicsSystem::CastSphere`.
