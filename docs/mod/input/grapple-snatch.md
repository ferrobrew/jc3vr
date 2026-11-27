# Grapple snatch

The grapple hand pulls things to itself. With the empty grapple hand pointed at something out of reach, a squeeze of the grip and a yank back fire the game's hook at it and pull it into a hand, from the ground, a rack or a locker, a character's hands, or a holster on a character's body. Weapons are the core of the feature; loose objects, grenades, and placed charges come the same way, car doors are pulled open, and a heavy object or a ragdolling body is swung on the line instead. The grapple button keeps doing what it does in the game.

The snatch keeps the game's grapple wherever it can, so it reads as the same device doing one thing more: the game's aim, its hook, line, and flight from Rico's left forearm, and its pickup and steal rules at the end. The mod adds the trigger, the pull, and the arrival. The code is `controllers::snatch` in `jc3vr-grapple`, with the aim patch in `jc3vr_body::controllers::aim::grapple_weapons`; the tuning is `crates/jc3vr-grapple/tuning/snatch.toml` (`SnatchTuning`, [tuning.md](../tuning.md)). The engine side, including why the game's own grapple never hooks a weapon, is in [grapple-pipeline.md, "Weapons and the grapple"](../../engine/gameplay/grapple-pipeline.md#weapons-and-the-grapple). An arrival lands the item through the [two-handed inventory](two-handed-inventory.md)'s takes, never the game's equip. The snatch has no switch.

## Targeting

The snatch aims with the grapple's own aim, the off-hand's ray ([hands-and-aiming.md](hands-and-aiming.md#grapple-aim)). The game's aim ray passes through weapons, so a detour of [`PlayerAimControl::CheckDirectTargets`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/aim::aim::PlayerAimControl##func-CheckDirectTargets) recasts the grapple's pass alone on [`VEHICLE_RAYCAST`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::PfxLayer##variant-VEHICLE_RAYCAST), which meets weapons, stepping past what that layer meets and the aim's does not (capsules, projectiles, vehicle collision bodies). A weapon it finds is written into the ray's closest hit ([`CastRayInfo`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/physics::CastRayInfo)) and a holey hit beyond it is dropped, so the game treats the weapon as any hit: the slot's object, the grapplable flag, and the latch of a fire. The physics layer table is never patched, since every query on the layer (the gun's aim among them) shares it. Because the line is the aim's own cast, nothing behind a wall is found.

The candidates (`snatch::target`):

- guns in characters' hands and holstered on their bodies, which the steal notes out to the snatch's range ([physical-weapons.md](physical-weapons.md#taking-a-gun-from-a-character)), within `cone_deg` and `range_m`;
- weapons, loose objects, grenades, and placed charges lying in the world, found by their broad-phase boxes near the ray (`physical_hands::remote::dynamic_near`) within a cone that widens with distance (`rules::floor_cone`), since a gun lying flat at range subtends less than a tracked hand can hold on it;
- weapons a rack or a locker holds out ([Racks and lockers](#racks-and-lockers));
- what the aim hits itself: a weapon, a loose object, or a car door's handle.

Each off-ray candidate needs a line-of-sight cast. The smallest angle wins, so a direct hit beats one off the ray. The chosen target shows a ring in its kind's colour, standing in for the game's highlight, which is hidden under hand aim. A gun still on its mount is never offered.

In the grip's targeting rule ([grip.md](grip.md)) the snatch is the lowest tier, `Tier::Remote`, taken only when nothing at all is in reach. It is offered only to the grapple hand, empty, on foot with no traversal, with the hook free and the grapple button up. The grapple device is on Rico's left forearm, so in a left-handed setup the snatch still leaves the opposite arm.

## The interaction

1. Aim: the ring shows the target.
2. Grip: squeezing arms the snatch. Nothing flies yet; a cyan latch dot and a line from the hand to the target show while it is armed (`snatch::latch`).
3. Yank: a pull of the hand back along its aim, faster than a threshold, while the grip is held, fires it. The pull is measured along the aim in the cockpit frame (`controllers::yank::reach_along`), not as the hand nearing the body, because a seated player's hand pulls back past its side and nears neither chest nor head. The detector (the tuning's `yank`) is gentler than the zip's ([physical-grapple.md](physical-grapple.md#the-yank)), since the grip already armed it. A punch never snatches, since the grip comes first.
4. Flight: the hook flies from the forearm and bites, and the line yanks the target toward a hand, a real body with real velocity and spin that collides with what is in the way. A gun turns in the air to arrive in the pose the catching hand holds it in ([The aligned landing](#the-aligned-landing)).
5. Catch: the line steers the target to whichever free, empty hand is nearest its path, so the player can pull with the grapple hand and hold out the other. A couple of ticks out the line goes slack, and a hand catches it when its grip closes on it in the catch zone or a moment after. Otherwise it flies past and drops.

Hold to hold: opening the grip cancels, and mid-flight the target keeps its momentum and falls. Leaving the feet cancels too. While the line is out, the other hand's grip is reserved for the catch (`snatch::reserved_hands`): a press closes it on nothing, takes nothing in reach, and drives no game action. The one exception is the line itself ([grapple-lock.md, "The free hand on the line"](grapple-lock.md#the-free-hand-on-the-line)).

Haptics: `Event::SnatchArm`, `Event::SnatchFire`, `Event::SnatchHit` at the bite, `Sustain::GrappleTension` while the line pulls, `Event::SnatchCatch` on the catching hand, and `Event::SnatchRefused` for any refusal or miss ([haptics.md](haptics.md)).

## The pull and the catch

A weapon without a body is first freed at its source through the hands' take from a source ([physical-weapons.md, "Taking a weapon from a source"](physical-weapons.md#taking-a-weapon-from-a-source)); the check that the hand can take it comes first, so a refused take leaves it where it was. Before anything flies, the pickup's rules are checked (not a grenade or an unlimited-stock copy, the animation allows a pickup, and the table can take it), so a snatch the hand could not finish never fires.

The hook is fired through [`GrapplingHook::FireHookCustom`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-FireHookCustom), the entry the game's vehicle and stunt-position grapples use, with a target built by [`HookTargetInfo::SetupTargetFromGameObject`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::HookTargetInfo##func-SetupTargetFromGameObject) at the object's origin and the ghost-wire flag on. A ghost wire whose last casts miss, as they do on a weapon, attaches to the target object itself. The origin rather than the grip is latched because a dropped weapon's grip has no cheap object-space point, and the pull works on the centre of mass either way.

When the hook end is attached ([`WireEnd::m_WireEndState`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##field-m_WireEndState)), `pull::bite` notes the hook's point in the body's frame and takes the native rope off the line (`sling_hooks::exclude`). Then `pull::pull` drives the body each tick through the physical hands' hold solve, toward the receiving wrist with a strength of its mass times `line_accel_mps2` and no twist. Far out it pulls at the hook's point with an arc above the wrist so a far target clears the ground, which spins it; within `approach_m` the speed falls toward `arrival_speed_mps` and the pull moves to the centre, so the target keeps its spin but its centre flies at the wrist. A target heavier than `heavy_mass_kg` gets a wider catch zone and a slower line (`rules::HeavyCatch`). The line never brakes to a stop: a pull that slowed to nothing read as placed rather than thrown, so the target arrives hard, as a yanked thing does.

The receiver (`catch::receiver`) is the free, empty hand whose wrist is nearest the segment from the target to the eye, with hysteresis, re-chosen each tick while the line pulls. When the target is within the distance it flies in `coast_s` (`rules::slack_radius`), the line goes slack and the target coasts on its momentum. The catch is swept along the path since the last tick (`catch::CatchWatch`) so a fast target is not stepped over: a hand catches when its grip is closed while the target is within `catch_zone_m` of its wrist, or up to `catch_grace_s` after. The receiver and the free hand other than the grapple hand may catch; the grapple hand, whose grip holds the line, catches only as the receiver. A weapon is then taken through the direct pickup (`pickup::take` with its source and `Delivery::Remote`), whose rules all apply again. Nothing draws a slack target toward a hand, so a hand that moves away misses.

A slack target that recedes from every hand, stops, or flies longer than `max_slack_s` is a miss. One caught but refused by the table is brought to rest at the hand and drops there. The snatch also fails on a stall while the line pulls (`stall_distance_m` over `stall_window_s`), a target beyond the wire's [`m_MaxLength`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHookWire##field-m_MaxLength), a hook flying longer than `attach_timeout_s`, or a wire the game ends. Targets are held by a strong reference so the despawn never takes one mid-flight, except a pooled C4 charge ([Placed charges](#placed-charges)).

The hook's state machine serialises snatches: `FireHookCustom` refuses while a line is out or the hook is busy, and the mod refuses before it while a normal fire is pending, which `FireHookCustom` does not test. The custom fire is accepted with dual tethers attached, since they are not the active wire. While the line is out the bindings hold the grapple button back from the game, and the snatch ends its own custom wire on every exit and stand-down, after checking it is still the hook's active one.

### The aligned landing

A gun lands ready to shoot (`snatch::landing` for the math, `snatch::arrival` for the engine side). At the bite the landing reads the body's frame in the gun's grip frame. Each tick it knows where the receiver will hold the gun: the hand's IK wrist target extrapolated to the catch, composed with the rigid grip's canonical grip (`grip::canonical_on_wrist_frame`). The body's angular velocity is steered toward the turn still to make over the time to go (`landing::turn_drive`), through the velocity setter the line uses, with its own caps above the line's spin because most snatches fly only a few ticks. The turn keeps running after the line goes slack; the position is corrected only while it pulls, across the line and within `landing.lateral_window_m` (`landing::aim_shift`). The take's slide covers what is left. The log reports how far the gun arrived from the hold, and for `watch_ms` after a catch `arrival::ArrivalWatch` logs whether the gun is drawn, since a catch can show a frame with no gun between the take and the first placement.

## Targets

| Kind | Freeing | Finish |
| --- | --- | --- |
| Weapon on the ground | none | `pickup::take` |
| Weapon in a character's hands | the steal's drop and stagger | `pickup::take` |
| Weapon holstered on a character | the slot's weapon dropped | `pickup::take` |
| Gun on a rack or in a locker | dropped where it hangs | `pickup::take` |
| Loose object a hand can carry | turned loose at the bite if placed | the hand's ordinary hold |
| Grenade lying in the world | none | taken into the hand through the loadout |
| Placed C4 charge | its body put into the simulation | the hand's take of a placed charge |
| Heavy loose object or ragdoll | none | swung on the line |
| Car door | unlatched once the hook holds | an outward impulse; the door swings |

### Guns on characters

A gun in a character's hands goes through the steal's selection change and stagger; a disarmed character reacts as to the steal in reach. A holstered gun is dropped as the player's own replaced weapon is ([`WeaponBase::OnDrop`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##vfunc-OnDrop), then [`Inventory::DropWeapon`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/character::inventory::Inventory##func-DropWeapon)) with no selection change, so the character keeps what it wields. A freed gun is fired at on the first tick its body can be read, within `free_timeout_s`. A gun torn off its mount is snatched as any weapon.

### Racks and lockers

A rack's weapon has no body, the aim never hits it, and the game offers it only through interaction entries within reach. So `hold::rack::remote` in `jc3vr-weapons` notes, from the weapon render detour, the weapons a placement rule holds out, by weak reference: a weapon in the rack state, and one lying where its rule set it down (a locker's), known by [`WeaponBase::m_SpawnedFromRule`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_SpawnedFromRule) with no holder. The render pass only notes; everything that changes a weapon runs in the input tick. Firing frees the weapon through the same take a hand makes ([physical-weapons.md, "Taking a weapon off a rack"](physical-weapons.md#taking-a-weapon-off-a-rack)), differing only in delivery:

- A finite rack's weapon gets [`WeaponBase::PickupStart`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##func-PickupStart), which has no distance test and tells the rack, and then drops where it hangs. A locker's drops the same way.
- A rack with unlimited stock ([`WeaponBase::m_InfinitePickup`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::weapon::WeaponBase##field-m_InfinitePickup)) is asked for a copy (`hold::rack::copy`), which spawns a few frames later where the rack's weapon hangs and is dropped there; the snatch waits for it (the phase reads freeing, up to `hold.rack.copy_wait_ms`). A locker weapon with unlimited stock is not offered, since its copy would drop inside the bodies of the weapon still lying there.
- A rack's pistol pair gives up the pistol aimed at ([physical-weapons.md, "Paired rack pistols"](physical-weapons.md#paired-rack-pistols)), claimed from the pair and dropped alone. The claim is settled at once, so a missed catch leaves a loose single pistol holding its share of the ammunition.

### Loose objects

A loose object is offered by the hand's own classification ([physical-hands.md, "Picking things up"](physical-hands.md#picking-things-up), `physical_hands::remote::loose_mass`): not a character, a vehicle or a part still on it, or a weapon, with a finite mass. One the other hand holds is not. A placed object at rest has no dynamic body for the box search, so it is offered only under the aim's exact hit and turned loose at the bite as the game turns one loose (`pull::loosen`, [havok-bodies.md](../../engine/physics/havok-bodies.md#placed-bodies-becoming-dynamic)). On the catch it is handed to the hand's ordinary hold through `held::handover`, so the hold's own solve absorbs its momentum and the grip's release throws it.

### Grenades

An inert grenade of the loadout lying in the world (`OfferKind::Grenade`, `grenade::inert_lying`) is anchored by its own body, since a sight cast rarely meets so small a body, and is taken into the hand through the loadout on the next input tick ([grenades.md, "Dropped grenades"](grenades.md#dropped-grenades)). A finite native grenade pickup (`OfferKind::WorldGrenade`) is yanked by its own body and taken by `grenade::take_world_pickup`, as a hand closing on it would. A live grenade and one on a display are not targets.

### Placed charges

A C4 charge the player placed (`OfferKind::PlacedCharge`, from `c4::placed::charges`) has its native body put into the simulation by [`PlantedExplosive::DropExplosive`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::explosive::PlantedExplosive##func-DropExplosive) at the yank. That body comes off its surface slowly, so its stall window is scaled by `charge_stall_scale`. The charge stays on the weapon's active list in flight, still counted and detonatable, and a miss leaves it live in the world. Only a catch requests the grip's take of a placed charge (`c4::placed::take`), which primes a replacement in the hand before removing the original. Flight holds only the pool slot and allocation identity, validated against the live list before removal, so a charge recycled in flight is refused cleanly.

### Car doors

A closed door of the vehicle under the aim, with its handle within `door_reach_m` of the aim's point, is found by the doors' own reach ([vehicle-doors.md](vehicle-doors.md)); a vehicle faster than `door_max_vehicle_speed_mps` is refused. The hook is fired at the vehicle at the handle; once it holds, the door is unlatched as a hand's pull does, pushed outward by its mass times `door_swing_mps`, and the wire ends.

## Swinging on the line

A heavy object or a ragdolling body is never pulled into the hand: once the hook bites, the held line swings it from the grapple hand, to be dragged, whirled, or lifted and let go with its velocity by opening the grip. The game's physics and damage do the rest. The code is `snatch::sling`, `snatch::brace`, and `snatch::sling_hooks`, tuned by the `swing` section. Three things swing:

- A loose object too heavy for one hand (`remote::carry_max_kg`) and no heavier than `swing.ceiling_kg` (`rules::prop_handling`); above the ceiling nothing is offered. Vehicles, standing characters, and parts still on a vehicle are never offered, since the hook's attachment would select the vehicle's body.
- A ragdolling character whose whole ragdoll is under the ceiling (`remote::ragdoll_target`), hooked at the bone nearest the hit ([character-grabs.md](character-grabs.md#reaching-a-character)). The pull is an impulse split between the hooked body (`remote.ragdoll_lead_share`, so it comes limb first) and every body by mass (`remote::spread_pull`), since the ragdoll's constraints cannot pass a whole body's momentum down the chain within one step. The line reels the body in to arm's reach, easing as it nears, where the free hand can take it ([character-grabs.md](character-grabs.md)).
- A weapon or light object while B (the grapple button) is held. Releasing B resumes the pull to the hand.

The line is the rope the grapple lock uses (`grapple::rope`, [grapple-lock.md](grapple-lock.md#the-swing)) with the hand as its kinematic end: a tension-only spring-damper on the target's velocity along the line, capped at the line's strength, which scales with the target's mass (`sling::line_strength_n`) so a barrel takes a real swing and a rifle swings briskly. It writes velocity through the path every hold uses, never placing the target, and the target collides with everything, Rico included. The held length reels in toward `swing_length_m` but never faster than the target comes, and never pays out. The anchor is the tracked wrist, and the native device proxy's visual getters get that frame so the game draws the line from the tracked forearm. Overstretch, an anchor jump, tracking loss, a lost line, an occupied hand, or a menu ends it; a stationary swing has no stall timeout.

The native constraint and tension callbacks are excluded from the fire on (`sling_hooks`), so the target does not take a second rope force and Rico is not pulled toward a heavy one. The hooks only copy the published anchor and compare identities; target access stays on the input tick.

The grapple button is owned only after the grip arms a remote target, until the catch or miss, so a B press under an armed heavy snatch does nothing rather than firing the game's grapple; the press is consumed until neutral. B before the grip is an ordinary grapple.

While the line swings, the other hand can brace the grapple hand by holding its (already reserved) grip with its wrist within `brace_take_m`: the line's strength rises by `brace_factor` and the anchor moves to the hands' average. It holds until the grip opens, a wrist loses tracking, or they drift `brace_break_m` apart. The other hand can instead take the line ahead of the wrist ([grapple-lock.md, "The free hand on the line"](grapple-lock.md#the-free-hand-on-the-line)). The latch draws the swinging line with colour and width following tension, a ring where the held length ends, and a dot between the hands while braced.

## Sounds

The snatch plays the grapple's own sounds and effects from the hook's effect container (`snatch::juice`; [grapple-pipeline.md, "The hook's sounds and effects"](../../engine/gameplay/grapple-pipeline.md#the-hooks-sounds-and-effects)). The custom fire plays no fire animation, so only the wire's flight loop plays by itself. The mod plays the fire's [`GRAPPLE_FIRE_USAGE`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##const-GRAPPLE_FIRE_USAGE), the bite's [`WireEnd::PlayImpactEffect`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##func-PlayImpactEffect) fed by a cast through the hook's end on the weapon layer, the reel's loop held on while the line pulls by a detour of [`GrapplingHook::UpdateLoopingEffects`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-UpdateLoopingEffects), and the retract's one-off when the line comes away. The pre-fire sound is not played: which point of the game's fire it belongs to is unverified.

## The grapple button at a gun

With the aim seeing weapons, the grapple button acts on a gun as the game acts on any light object, with one difference (`snatch::hooks`), and only while the bindings inject:

- A detour of [`WireEnd::SetExtend`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::wire_end::WireEnd##func-SetExtend) makes a hook fired at a weapon a ghost wire. The hook's casts never hit a weapon's body, so a plain hook would attach behind the gun and zip Rico there. This covers both ends of a tether, so a gun can be tethered to a car.
- A tap tosses the gun to the player's feet rather than Rico to the gun. A weapon's grappling mode is "reel in, no attach", and the game has no pull of a light object, so where [`GrapplingHook::OnPreSimUpdate`](https://ferrobrew.github.io/pyxis/#/JustCause3_Steam_20206564/item/equipment::grappling_hook::GrapplingHook##func-OnPreSimUpdate) would start a reel on a weapon, a detour ends the wire and the next input tick tosses the gun on a ballistic arc (the tuning's `toss`). The toss serves a hand that is not free and the gamepad's habits.

## Readouts

The Status page's `Grapple` section has a `Snatch` row: the state (idle, armed, freeing, hook out, pulling, inbound, swinging), the offered target with its mass and distance or why none is offered, the swing's length, tension, and brace, and counts of failures and tosses with the last refusal. The log, at info level under `controllers`, follows each snatch from `snatch: armed; a yank fires` to the catch, `snatch: failed` with the reason, or `snatch: cancelled`.

## Open questions

- How the AI reacts to a disarm or a rack weapon taken from range, and whether it reserves a dropped weapon it walks toward.
- Whether a target caught close to the body hits Rico's collision proxy first.
- Whether a swung barrel's impact is hard enough to explode it, and whether the player counts as its instigator.
- Whether the hook is ever parked off inactive with no wire, which would refuse the custom fire.
