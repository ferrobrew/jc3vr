# Grapple snatch

This is the plan of record and the description of what is built. Phase 1 is built: weapons on the ground and in characters' hands ([Phases](#phases)). Phases 2 to 4 (racks and lockers, holstered guns and props, car doors) are not built yet, and the parts of the design that wait on them say so. None of it has been run in the headset. The code is `payload/src/controllers/snatch/`, with the aim patch in `payload/src/controllers/aim/grapple_weapons.rs`; the engine side is "Weapons and the grapple" in [grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md#weapons-and-the-grapple).

It is built on the [two-handed inventory](two-handed-inventory.md), whose takes it ends in: the snatch's arrival lands the item in the loadout table and the grabbing hand, never through the game's equip. It is the first feature built on the inventory, and its test: a snatch takes an item from anywhere into the grabbing hand while the other hand holds whatever it holds, which exercises the table, the hands' independence, and the reconcile end to end.

The grapple hand pulls things to itself. The grapple button keeps doing what it does in the game (a tap fires, a hold tethers to and between targets), and the grip pulls: with the empty grapple hand pointed at something out of reach, a squeeze and a yank back toward the body fire the game's hook at it and snatch it from wherever it is (the ground, a rack or a locker, a character's hands, a holster on a character's body) into the hand, in the held position. From there it is an ordinary held thing: hand it to the other hand, holster it, carry it, or drop it. Weapons are the core of the feature. Props and car doors follow, the doors pulled open on the line rather than into the hand.

The game offers nothing like it: its grapple never hooks a weapon, has no pull of a light object to the player, and cannot reach into a character's hands. The snatch keeps the game's own grapple wherever it can, so it reads as the same device doing one thing more: the game's aim and target highlight, the game's hook, line, and flight from Rico's left forearm, and the game's own pickup and steal rules at the end. The mod adds the trigger, the pull, and the arrival.

## Why the game's grapple ignores weapons

Verified against the release IDB unless noted.

- The player's aim ray (`PlayerAimControl::UpdateDirectAim`) and the hook's own in-flight cast (`CWireEnd::TryCollide`) both query the `RAYCAST_EVERYTHING_PHYSICAL` layer, and the `WEAPONS` layer does not collide with it ([engine: havok-bodies.md](../../engine/physics/havok-bodies.md)). A dropped weapon's body is on `WEAPONS`, so both rays pass through it.
- Everything after the ray supports weapons. A weapon's grappling mode is "reel in, no attach", not "ungrapplable", and `CWireEnd::SetupProxyForAttachment` has an explicit weapon branch that builds the same object attachment proxy as for any loose object.
- The release has no usable pull of a light object to the player. The symbol dump's `CGrapplingHook::HandleGrapplePull` pulls one only behind two developer menu flags, and the release function bearing that name does not match it.
- A weapon in a character's hands has no body in the world, and `PlayerAimControl::CheckDirectTargets` resolves a hit on a held weapon to its holder.

So the game was built to hook weapons, and only its rays keep it from finding one.

## Targeting: the game's aim, scoped to the player's grapple

The snatch aims with the grapple's own aim: the off-hand's ray that the mod already feeds to the game's grapple ([controllers-and-roomscale.md, "The grapple"](controllers-and-roomscale.md#the-grapple)). To make that aim see weapons, the patch is scoped to the grapple's pass of the direct-aim raycast, never the physics system's layer table, which every query on the layer shares (the gun's aim among them, which would then stop on guns lying on the ground). Other characters' grapples, the camera's pass, and every other query are untouched.

As built (`aim::grapple_weapons`): inside the grapple's pass, a detour of [`PlayerAimControl::CheckDirectTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-CheckDirectTargets), the distribution of the ray's hits, first casts again along the same ray on [`VEHICLE_RAYCAST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer##variant-VEHICLE_RAYCAST), which meets weapons, out to the ray's closest hit, with the aim's own filter ignoring the player and its vehicle. It steps past what that layer meets and the aim's does not (a character's capsule, a projectile, a vehicle's collision body) a few times. A weapon lying in the world that it finds is written into the ray's closest hit ([`CastRayInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfo)), and a holey hit beyond it is dropped from the [collector](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimRaycastCollector##field-m_HasHoleyHit), so the game does everything it does with a hit with the weapon: the slot's game object, the grapplable flag, the last-valid grapple cache, and the latch of a fire. The weapon found is published for the snatch. Because the line is the aim's own cast, a weapon behind a wall is never found.

With the patch, a gun on the ground is a grapple target like any other ([The grapple button at a gun](#the-grapple-button-at-a-gun)). For the snatch, a character holding a gun is a disarm target: the guns in characters' hands are the steal's ([physical-weapons.md](physical-weapons.md#taking-a-gun-from-a-characters-hands)), noted out to the snatch's range while the snatch and the steal are on, and the one whose holding hand's grip is nearest the grapple's ray within the cone (`weapons.snatch.cone_deg`) and the range (`weapons.snatch.range_m`) is chosen, with a line-of-sight cast to it on the aim's layer that ignores the player and the character. The weapon on the ground under the aim, at the ray itself, wins over a gun in hands off it. The chosen target shows one marker, a ring in the weapon colour around it, which stands in for the game's highlight: the game's reticle is hidden under hand aim. What the patch cannot reach (a rack's weapon, which has no body; a locker's, unverified) waits on phase 2, with the mod's own selection around the same ray.

In the grip's one targeting rule ([grip.md](grip.md)) the snatch is the lowest tier, `Tier::Remote`, taken only when nothing at all is in reach (not even a surface to climb or a reach margin), so anything within real reach always wins. It is offered only to the grapple hand, empty (no gun, foregrip, grenade, charge, or prop), on foot with no traversal, with the hook free (no active or attachment wire, no fire pending, the hook inactive) and the grapple button up. A fist never takes it: only a press aimed at it arms the snatch.

The grapple hand is the off-hand of the acting hand. The game's grapple device is on Rico's left forearm, so with a left-handed setup the line would leave the arm opposite the snatching hand; that is deferred with left-handedness project-wide.

## The interaction

1. Aim: point the empty grapple hand at the target. The marker shows it.
2. Grip: squeezing arms the snatch on the target under the aim. Nothing flies yet, and the target is held while the grip is.
3. Yank: a pull of the hand back toward the body, faster than a threshold, while the grip is held, fires it. The yank is the confirmation, as with the gravity gloves of other games, and the motion matches what it does. Opening the grip before the yank disarms it, and a punch or a swing that closes the hand does not snatch, since the grip comes first. The grapple's line already uses a yank to zip ([physical-grapple.md](physical-grapple.md)); the snatch's yank is its own detector (`weapons.snatch.yank`, the bindings' yank gesture with its own thresholds), armed only while the grip holds a remote target with no line out.
4. Flight: the hook flies from the left forearm to the target, attaches, and the target arcs into the hand.
5. Arrival: the target lands in the hand in its held pose.

Hold to hold holds: opening the grip at any point cancels. Mid-flight, the line comes away and the target keeps its momentum and falls, rather than snapping back to where it came from.

Haptics: `Event::SnatchArm` when the grip arms it, the grapple's fire and attach cues on the snatching hand, the line's tension (`Sustain::GrappleTension`, by the pull's share of its strongest pull) while it pulls, and the take's cue on arrival; a refused or failed snatch plays `Event::SnatchRefused` ([haptics.md](haptics.md)).

Leaving the feet (a vehicle, a fall, a ragdoll, a reel) or turning the snatch off cancels it as opening the grip does.

## The grapple button at a gun

With the aim seeing weapons, the grapple button acts on a gun as the game acts on any light object, with one difference (`snatch::hooks`):

- A hook fired at a weapon flies as a ghost wire: a detour of [`WireEnd::SetExtend`]({V}equipment::wire_end::WireEnd##func-SetExtend) sets the ghost flag when the latched object is a weapon. The hook's own casts never hit a weapon's body, so a plain hook would attach to whatever lies under or behind the gun and zip Rico there; a ghost hook attaches to the gun itself ([`WireEnd::ForceAttachToTarget`]({V}equipment::wire_end::WireEnd##func-ForceAttachToTarget)). It covers the first end of a tap or a hold and the second end of a tether, so a hold tethers a gun to a car, or two guns together, through the game's own tethers.
- A tap at a gun reels the gun in, not Rico. A weapon's grappling mode is "reel in, no attach", and the release has no pull of a light object, so where the game would start its reel on an attached line to a weapon (in [`GrapplingHook::OnPreSimUpdate`]({V}equipment::grappling_hook::GrapplingHook##func-OnPreSimUpdate), the local player's hook inactive, the button up, no tether pending), a detour ends the wire ([`GrapplingHook::CancelActiveWire`]({V}equipment::grappling_hook::GrapplingHook##func-CancelActiveWire)) and the next input tick tosses the gun on a ballistic arc to land a little in front of the player. The tap is the grapple's way of fetching a gun for a hand that is not free, or for the gamepad's habits; the snatch is the hand's.

Both act only while the snatch's switch is on and the bindings inject, since the aim sees weapons only then.

## How a weapon is snatched

1. A weapon without a body is first freed into the world at its source. A gun in a character's hands goes through the steal's drop and stagger ([physical-weapons.md, "Taking a gun from a character's hands"](physical-weapons.md#taking-a-gun-from-a-characters-hands)), which leaves it a dropped weapon with bodies (`steal::release`, shared with the steal's take); the check that the hand can take it comes first, so a refused take leaves the gun in the character's hands. A holstered gun on a character goes the same way without a selection change (phase 3).
2. The game's hook is fired at the weapon through [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom), the entry point the game's own vehicle grapple and stunt-position grapple use, with a target info built on the weapon's game object ([`HookTargetInfo::SetupTargetFromGameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::HookTargetInfo##func-SetupTargetFromGameObject), latched at the weapon's origin, with the world position, the in-range flag, and no bone set by the mod) and the ghost-wire flag on. A ghost wire flies to the target and, when its last casts miss, as they do on a weapon, attaches to the target object itself. The player sees the game's hook and line. Before a weapon on the ground is fired at, the pickup's rules are checked (not a grenade or an unlimited-stock copy, the animation allows a pickup, and the table can take it into the hand), so a snatch the hand could not finish never flies. A freed gun is fired at on the next tick its body can be read, within `weapons.snatch.free_timeout_s`.
3. When the wire's hook end is attached ([`WireEnd::m_WireEndState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##field-m_WireEndState) and its proxy), the mod pulls: the physical hands' hold solve drives the weapon's body toward the hand's wrist, its grab point the body's centre of mass, with the snatch's own stiffness, top speed, and acceleration (the hand's strength is the weapon's mass times `weapons.snatch.max_accel_mps2`) and no twist. The solve's target is the wrist raised by `weapons.snatch.arc` of the distance still to go, so a far gun rises and falls into the hand and a near one comes straight in, and gravity is in the solve. The game's line follows the gun, since the hook is attached to it. A hook that flies longer than `weapons.snatch.attach_timeout_s`, or whose wire the game ends, fails the snatch.
4. Within `weapons.snatch.capture_radius_m` of the wrist (outside Rico's collision proxy, which a weapon's body collides with), the mod cancels the wire and takes the weapon through the direct pickup (`pickup::take`, as `Cause::Snatch`, whose table source is `TakeSource::Snatch`) with its short slide into the hand. Every rule of the pickup applies again, and the weapon lands in the grabbing hand through the table, the other hand untouched.

The mod holds a strong reference to the weapon from the fire to the end, so the despawn never takes it mid-flight. A stall (the body staying within `weapons.snatch.stall_distance_m` over `weapons.snatch.stall_window_s`, snagged on the world) or a weapon farther from the hand than the wire's [`m_MaxLength`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##field-m_MaxLength) fails the snatch: the wire is cancelled and the weapon is left where it is. A refused take at arrival leaves the weapon falling, logged with the reason.

The hook's state machine serialises snatches: `FireHookCustom` refuses while a line is out or the hook is busy, and the mod refuses before it while a normal fire is pending, which `FireHookCustom` does not test. While the snatch's line is out the bindings hold the grapple button back from the game, and the snatch ends its custom wire itself on every exit and every stand-down of the bindings, checking first that the wire is still the hook's active one.

## Per target kind

| Kind | Freeing | Hook | Pull | Finish | Phase |
| --- | --- | --- | --- | --- | --- |
| Weapon on the ground | none | custom ghost fire at the weapon | the hold solve | `pickup::take` | 1, built |
| Weapon in a character's hands | the steal's drop and stagger | custom ghost fire once it has bodies | the hold solve | `pickup::take` | 1, built |
| Weapon on a rack | open: the rack hands its weapon to a pickup, not to the world | open | open | the rack take, or `pickup::take` | 2 |
| Rack with unlimited stock | none: the copy spawns into the inventory | a mod-drawn line to the rack's weapon | the copy flies in on a stretched, arced slide | the rack's copy path | 2 |
| Weapon in a locker | none if it lies dropped with bodies (unverified) | as on the ground | as on the ground | `pickup::take` | 2 |
| Weapon holstered on a character | the slot's weapon dropped as the steal drops a held one | as on the ground | as on the ground | `pickup::take` | 3 |
| Loose prop | none | custom ghost fire at the object | the hold solve | the hand's ordinary hold continues | 3 |
| Car door | the doors' unlatch once the hook is on ([vehicle-doors.md](vehicle-doors.md)) | fire at the vehicle near the handle | an outward impulse on the door's body, not into the hand | the wire cancelled; the door swings | 4 |

A disarmed character staggers and reacts as to the steal in reach: it draws another weapon, fights unarmed, or goes for one lying in the world. How the AI reacts to a disarm from range is to be watched in the headset.

## Settings

`weapons.snatch.enabled` (on by default) switches the snatch, the aim's patch, and the grapple button's ghost fire and tap at a gun together. It and its tuning are in the debug UI only (Controllers, under the weapon settings): the range, the cone for guns in hands, the yank's speed, length, and window, the pull's top speed, acceleration, stiffness, and arc, the capture radius, the stall rule, and the hook's flight limit. A gun in a character's hands needs the steal's own switches too (`weapons.hold.steal`, with `direct_pickup` and `either_hand`).

The Status tab's `GRAPPLE` section has a `SNATCH` row: the snatch's state (idle, armed, freeing, hook out, pulling), the target offered and its distance or why none is offered (not on foot, the hand not empty, the button held, a line out, the hook busy), and the snatches taken of those fired, with the last refusal as a note. The weapons readout has a line with the counts and the tosses. The log, at info under `controllers`, has `snatch: armed; a yank fires`, `snatch: yanked; firing` with the yank, `snatch: the character let go of its gun`, `snatch: the hook holds the weapon; pulling`, `snatch: the weapon came into the … hand` with the slot, `snatch: failed` with the reason, `snatch: cancelled` with why, `snatch: a tap of the grapple reeled a weapon in`, and `snatch: tossed a weapon to the player's feet`; each grip press's log notes the remote target offered.

## Phases

### Phase 1: weapons on the ground and in characters' hands

Built, as described above, except the headset checks below.

- Engine: the scoped patch that lets the player's grapple aim see weapons, with the RE of the aim cast it patches and of what the grapple's tap and hold do with a weapon under the aim (a tap at a weapon must not zip Rico to it); bindings for `FireHookCustom`, `CancelActiveWire`, the active wire's far end and its attached state, and the weapon's grappling mode; a "Weapons and the grapple" section in [engine: grapple-pipeline.md](../../engine/gameplay/grapple-pipeline.md).
- Payload: the snatch module (armed, freeing, hook out, pulling; the arrival is the pull's last tick, and done or failed returns it to idle), `Tier::Remote`, the snatch grab volume, the yank detection, the grapple button held back during a snatch, the steal's character notice widened to the snatch's range, the setting, and the Status row.
- Tests: the yank detection, the tier's place in the resolver, and the capture and stall rules, as pure functions.

In the headset:

1. Point the empty grapple hand at a gun on the ground 10 m away: the marker shows on it. Move the hand onto a crate within reach: the crate's marker wins.
2. Squeeze, then yank: the hook flies from the left forearm, takes hold of the gun, the gun arcs into the hand, the line comes away, and the gun sits in the held pose. Holster it, hand it over, drop it.
3. Squeeze without yanking, then open the hand: nothing flies. Hold the grapple button on a gun, aim at a car, and let go: the gun is tethered to the car.
4. Yank, then open the hand mid-flight: the line goes and the gun falls near you.
5. Snatch a gun from a soldier at 10 m: the soldier staggers, and the gun flies in.
6. Point at a gun behind a wall: no marker. Tap the grapple button at a gun on the ground: the game's grapple does what it does at any light object, and does not zip Rico to it: the gun lands at your feet.
7. With tethers attached, try a snatch: it works with the tethers untouched, or is refused cleanly.
8. Swing a closed fist past a highlighted gun: no snatch.
9. Watch the frame time with many weapons and characters near.

### Phase 2: racks and lockers

The RE of a rack's pickup and of finding distant racks; a physical flight for a rack weapon that can be dropped into the world; the stretched, arced slide and a mod-drawn line for the unlimited-stock copy. Missions that hand the player a rack's weapon keep working through the game's interaction.

### Phase 3: holstered weapons on characters, and props

Noting characters' holstered weapons and dropping a slot the character does not wield; for a prop, the remote pull handing over to the hand's ordinary hold on arrival, within the hand's strength.

### Phase 4: car doors

The door as a remote target at its handle; once the hook is on near the handle, the doors' unlatch and an outward impulse on the door's own body, then the wire cancelled. A moving car's door is refused or handled safely.

## Decisions

- A tap of the grapple button at a gun tosses the gun to the player's feet ([The grapple button at a gun](#the-grapple-button-at-a-gun)): the game's "reel in, no attach" read as the object reeled in, not the player, and the hook's line ends there, so nothing zips and the gun comes within a step.
- The hook latches the weapon at its origin rather than at its grip: a dropped weapon's grip has no cheap object-space point, and the pull and the capture work on the body's centre of mass either way.
- The pull is the hold solve with the snatch's own tuning and no twist: the gun keeps its own spin in flight, and the take's slide turns it into the held pose.
- The snatch's yank has its own thresholds, a little gentler than the zip's, since the grip is already the arming and the yank only confirms.
- The game's impact effect is not played at the attach: [`WireEnd::PlayImpactEffect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##func-PlayImpactEffect) wants a cast hit's material, which a hand-built hit lacks; the attach cue on the hand stands in.
- The switch is in the debug UI only, as the plan says, and not on the pause menu's VR pages.

## Open questions

- Whether the AI reserves a dropped weapon it walks toward, and what it does when the weapon leaves.
- Whether the custom fire plays the hook's fire sound (it plays no fire animation, so the animation's sound is skipped).
- Whether moving the incoming weapon into the player's collision group would let it come all the way to the palm rather than stopping at the capture radius.
- Whether the hook's state is ever left parked off inactive with no wire, which would refuse the custom fire ("the hook is busy" in the `SNATCH` row).

Settled by the phase 1 RE: the custom fire is accepted while dual tethers are attached (they are not the active wire), and the hook ignores the grapple input in its custom state; a ghost wire passes through walls, so the line of sight is the aim's own cast for a weapon on the ground and the snatch's cast for a gun in hands.
